//! Wayland side: registry, screencopy frame events, bounded dispatch, the capture format probe.

use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};
use wayland_client::protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool};
use wayland_client::{delegate_noop, Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1 as dbparams, zwp_linux_dmabuf_v1 as dbmgr,
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1 as frame, zwlr_screencopy_manager_v1 as scmgr,
};
use wfd_util::die;

use crate::opts::Opts;

/// Upper bound for one wait on the Wayland socket, so the IDR flag is checked often enough.
pub const DISPATCH_MAX: Duration = Duration::from_millis(50);
/// A frame's buffer parameters (or a one-shot copy) not there within this: the compositor hangs. Exit, so the
/// supervisor restarts the encoder.
pub const COMPOSITOR_TIMEOUT: Duration = Duration::from_secs(5);

/// Events of the current screencopy frame.
#[derive(Default, Debug, Clone)]
pub struct FrameState {
    /// shm buffer offer: format, width, height, stride
    pub fmt: Option<(wl_shm::Format, u32, u32, u32)>,
    pub buffer_done: bool,
    pub ready_mono_us: Option<i64>,
    pub failed: bool,
    pub y_invert: bool,
    /// dmabuf offer: DRM fourcc, width, height
    pub dmabuf: Option<(u32, u32, u32)>,
}

pub struct State {
    outputs: Vec<(wl_output::WlOutput, Option<String>)>,
    pub shm: Option<wl_shm::WlShm>,
    mgr: Option<scmgr::ZwlrScreencopyManagerV1>,
    pub dmabuf: Option<dbmgr::ZwpLinuxDmabufV1>,
    pub f: FrameState,
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        st: &mut Self,
        reg: &wl_registry::WlRegistry,
        ev: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = ev {
            match interface.as_str() {
                "wl_output" => st.outputs.push((reg.bind(name, version.min(4), qh, ()), None)),
                "wl_shm" => st.shm = Some(reg.bind(name, 1, qh, ())),
                "zwlr_screencopy_manager_v1" => st.mgr = Some(reg.bind(name, version.min(3), qh, ())),
                "zwp_linux_dmabuf_v1" if version >= 3 => st.dmabuf = Some(reg.bind(name, 3, qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for State {
    fn event(
        st: &mut Self,
        out: &wl_output::WlOutput,
        ev: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = ev {
            if let Some(o) = st.outputs.iter_mut().find(|(o, _)| o == out) {
                o.1 = Some(name);
            }
        }
    }
}

impl Dispatch<frame::ZwlrScreencopyFrameV1, ()> for State {
    fn event(
        st: &mut Self,
        _: &frame::ZwlrScreencopyFrameV1,
        ev: frame::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            frame::Event::Buffer { format: WEnum::Value(f), width, height, stride } => {
                st.f.fmt = Some((f, width, height, stride))
            }
            frame::Event::LinuxDmabuf { format, width, height } => st.f.dmabuf = Some((format, width, height)),
            frame::Event::BufferDone => st.f.buffer_done = true,
            frame::Event::Flags { flags: WEnum::Value(fl) } => st.f.y_invert = fl.contains(frame::Flags::YInvert),
            frame::Event::Ready { tv_sec_hi, tv_sec_lo, tv_nsec } => {
                let sec = ((tv_sec_hi as i64) << 32) | tv_sec_lo as i64;
                st.f.ready_mono_us = Some(sec * 1_000_000 + tv_nsec as i64 / 1000);
            }
            frame::Event::Failed => st.f.failed = true,
            _ => {}
        }
    }
}

delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wl_buffer::WlBuffer);
delegate_noop!(State: ignore scmgr::ZwlrScreencopyManagerV1);
delegate_noop!(State: ignore dbmgr::ZwpLinuxDmabufV1);
delegate_noop!(State: ignore dbparams::ZwpLinuxBufferParamsV1);

/// Dispatch Wayland events, waiting at most `timeout` for new ones.
fn dispatch_timeout(q: &mut EventQueue<State>, st: &mut State, timeout: Duration) -> Result<(), String> {
    q.dispatch_pending(st).map_err(|e| e.to_string())?;
    q.flush().map_err(|e| e.to_string())?;
    if let Some(guard) = q.prepare_read() {
        let mut pfd = libc::pollfd { fd: guard.connection_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: `pfd` is one valid pollfd on the stack for the duration of the call.
        let r = unsafe { libc::poll(&mut pfd, 1, timeout.as_millis().min(i32::MAX as u128) as i32) };
        if r > 0 {
            guard.read().map_err(|e| e.to_string())?;
        }
    }
    q.dispatch_pending(st).map_err(|e| e.to_string())?;
    Ok(())
}

/// Wayland connection with the screencopy manager and the chosen output.
pub struct Wl {
    q: EventQueue<State>,
    pub qh: QueueHandle<State>,
    pub st: State,
    mgr: scmgr::ZwlrScreencopyManagerV1,
    output: wl_output::WlOutput,
}

impl Wl {
    /// Dispatch, waiting at most `timeout`. A broken connection (compositor gone) is fatal.
    pub fn dispatch(&mut self, timeout: Duration) {
        if let Err(e) = dispatch_timeout(&mut self.q, &mut self.st, timeout) {
            die("wayland_error", &e);
        }
    }

    /// Dispatch until `done(frame state)`, at most COMPOSITOR_TIMEOUT; then exit with event `on_timeout`.
    fn wait_until(&mut self, done: impl Fn(&FrameState) -> bool, on_timeout: &str) {
        let deadline = Instant::now() + COMPOSITOR_TIMEOUT;
        while !done(&self.st.f) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                die(on_timeout, "compositor did not answer within 5 s");
            }
            self.dispatch(left.min(DISPATCH_MAX));
        }
    }

    /// Dispatch until the frame has told us its buffer parameters (or failed).
    pub fn wait_buffer_params(&mut self) {
        self.wait_until(|f| f.buffer_done || f.failed, "buffer_params_timeout");
    }

    /// Dispatch until a one-shot (not damage-driven) copy is ready (or failed).
    pub fn wait_copied(&mut self) {
        self.wait_until(|f| f.ready_mono_us.is_some() || f.failed, "copy_timeout");
    }

    pub fn capture(&mut self, cursor: bool) -> frame::ZwlrScreencopyFrameV1 {
        self.st.f = FrameState::default();
        self.mgr.capture_output(cursor as i32, &self.output, &self.qh, ())
    }
}

pub fn wayland_connect(o: &Opts) -> Wl {
    let conn = Connection::connect_to_env().unwrap_or_else(|e| die("wayland_error", &e.to_string()));
    let mut q = conn.new_event_queue();
    let qh = q.handle();
    conn.display().get_registry(&qh, ());
    let mut st = State { outputs: vec![], shm: None, mgr: None, dmabuf: None, f: FrameState::default() };
    for _ in 0..2 {
        if let Err(e) = q.roundtrip(&mut st) {
            die("wayland_error", &e.to_string());
        }
    }
    let Some(mgr) = st.mgr.clone() else { die("wayland_error", "compositor lacks zwlr_screencopy_manager_v1") };
    let names: Vec<Option<&str>> = st.outputs.iter().map(|(_, n)| n.as_deref()).collect();
    let i = pick_output(o.output.as_deref(), &names).unwrap_or_else(|| {
        let have: Vec<&str> = names.iter().flatten().copied().collect();
        die("output_not_found", &format!("{:?} not found; have {have:?}", o.output.as_deref().unwrap_or("")))
    });
    if o.output.as_deref() == Some(AUTO_OUTPUT) {
        wfd_util::event!("output_auto", "output" => names[i].unwrap_or_default());
    }
    let output = st.outputs[i].0.clone();
    Wl { q, qh, st, mgr, output }
}

/// `--output auto`: the laptop's built-in panel.
pub const AUTO_OUTPUT: &str = "auto";
/// Connector name prefixes of built-in panels (DRM: embedded DisplayPort, LVDS, MIPI DSI).
const BUILTIN_PREFIXES: [&str; 3] = ["eDP", "LVDS", "DSI"];

/// Index of the output to capture among the outputs' names (in registry order; None: no name event, a
/// wl_output older than v4): `want` by name; `auto` = the first built-in panel, else the first output; no
/// --output = the first output. None: nothing matches.
fn pick_output(want: Option<&str>, names: &[Option<&str>]) -> Option<usize> {
    match want {
        Some(AUTO_OUTPUT) => names
            .iter()
            .position(|n| n.is_some_and(|n| BUILTIN_PREFIXES.iter().any(|p| n.starts_with(p))))
            .or((!names.is_empty()).then_some(0)),
        Some(w) => names.iter().position(|n| *n == Some(w)),
        None => (!names.is_empty()).then_some(0),
    }
}

/// What the compositor captures into: shm format and size, plus its dmabuf offer.
#[derive(Debug, Clone)]
pub struct CaptureFmt {
    pub fmt: wl_shm::Format,
    pub w: u32,
    pub h: u32,
    pub stride: u32,
    /// GStreamer raw format with the same byte order.
    pub gst_fmt: &'static str,
    pub dmabuf_offer: Option<String>,
}

/// One capture request (not copied) to learn the buffer format.
pub fn probe_format(wl: &mut Wl, cursor: bool) -> CaptureFmt {
    let fr = wl.capture(cursor);
    wl.wait_buffer_params();
    fr.destroy();
    let Some((fmt, w, h, stride)) = wl.st.f.fmt else { die("unsupported_format", "no shm buffer format offered") };
    let dmabuf_offer = wl.st.f.dmabuf.map(|(f, w, h)| {
        let cc: String = f.to_le_bytes().iter().map(|&b| b as char).collect();
        format!("{cc} {w}x{h}")
    });
    let gst_fmt = match fmt {
        wl_shm::Format::Xrgb8888 | wl_shm::Format::Argb8888 => "BGRx",
        wl_shm::Format::Xbgr8888 | wl_shm::Format::Abgr8888 => "RGBx",
        f => die("unsupported_format", &format!("unsupported shm format {f:?}")),
    };
    CaptureFmt { fmt, w, h, stride, gst_fmt, dmabuf_offer }
}

/// GStreamer raw format with the byte order of a DRM fourcc (little-endian packed: XR24 = B,G,R,X in memory).
pub fn drm_gst_fmt(fourcc: u32) -> Option<&'static str> {
    match &fourcc.to_le_bytes() {
        b"XR24" | b"AR24" => Some("BGRx"),
        b"XB24" | b"AB24" => Some("RGBx"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_output_is_the_builtin_panel() {
        let hdmi_first = [Some("HDMI-A-1"), Some("eDP-1"), Some("HEADLESS-1")];
        assert_eq!(pick_output(Some("auto"), &hdmi_first), Some(1));
        assert_eq!(pick_output(Some("auto"), &[Some("DP-3"), Some("LVDS-1")]), Some(1));
        assert_eq!(pick_output(Some("auto"), &[None, Some("DSI-1")]), Some(1));
        // desktop without a panel: the first output
        assert_eq!(pick_output(Some("auto"), &[Some("DP-1"), Some("HDMI-A-1")]), Some(0));
        assert_eq!(pick_output(Some("auto"), &[]), None);
        assert_eq!(pick_output(Some("HEADLESS-1"), &hdmi_first), Some(2));
        assert_eq!(pick_output(Some("eDP-2"), &hdmi_first), None);
        assert_eq!(pick_output(None, &hdmi_first), Some(0));
        assert_eq!(pick_output(None, &[]), None);
    }

    #[test]
    fn dmabuf_byte_order_follows_the_drm_fourcc() {
        let cc = |s: &[u8; 4]| u32::from_le_bytes(*s);
        // Sway offers XR24 for dmabuf while shm is XB24: the two must not share one raw format
        assert_eq!(drm_gst_fmt(cc(b"XR24")), Some("BGRx"));
        assert_eq!(drm_gst_fmt(cc(b"XB24")), Some("RGBx"));
        assert_eq!(drm_gst_fmt(cc(b"NV12")), None);
    }
}
