//! wfd-media: capture one wlroots output -> VA-API H.264 (Constrained Baseline) -> Annex B on stdout.
//!
//! One job: produce the video elementary stream for wfd-tx. Capture is wlr-screencopy v3 into a shm
//! buffer (`copy_with_damage`: a frame is delivered only when the screen changes, at most `--fps`);
//! encoding is GStreamer (appsrc -> vapostproc -> vah264enc -> appsink). Each access unit is written to
//! stdout with a single write. Nothing is queued: appsrc and appsink keep at most 1-2 buffers and drop.
//!
//! Constant frame rate (grid): the encoder is fed on a strict 1/fps clock with the newest frame, or a repeat
//! of it when the screen did not change — regular cadence whatever the screen does (+~half a period of
//! latency). The TV buffers a fixed number of frames (~2-3), not a fixed time: at 3 fps its latency was
//! ~600 ms, at 30 fps ~100 ms. It also ends the session after ~25 s without video.
//! SIGUSR1 forces an IDR with SPS/PPS (sink `wfd_idr_request`).
//!
//! Log (JSON lines, stderr): {"cap":n,"t_present","t_ready","t_push","y_invert"} per pushed frame,
//! {"au":n,"bytes","pts_us","t_out","key"} per encoded access unit (both not with --quiet); events
//! {"event":...,"t"} otherwise. Times are Unix µs.
//!
//! Usage: wfd-media [--output auto|NAME] [--size 1920x1080] [--fps 30] [--kbps 8000] [--gop 30]
//!                  [--no-cursor] [--quiet] [--dmabuf] [--render-node PATH] [--lp] [--framed]
//! --output: wl_output name; `auto` = the built-in panel (first output named eDP*, LVDS* or DSI*), else the
//! first output, logged as {"event":"output_auto","output":NAME}. Without --output: the first output.
//! --size: encoded size (even), default the capture size. --fps >= 1, --kbps > 0, --gop >= 1 (frames).
//! --dmabuf: capture GPU->GPU (dmabuf, LINEAR, no CPU copy), falls back to shm. The appsrc caps are plain
//! video/x-raw, so the buffer must be linear: a Y-tiled one was read row by row and came out shredded
//! (2026-11-09). Tiled would need DMA_DRM caps, and GStreamer-VA 1.24 has no XR24 there (its BA24 happens to
//! work, a mislabel that a fixed GStreamer would undo). The raw format comes from the dmabuf fourcc (Sway
//! offers XR24 = BGRx), not from the shm format (XB24 = RGBx): mixing them up swapped red and blue.
//! --render-node PATH: GBM device for --dmabuf (default /dev/dri/renderD128, else the first renderD* that opens).
//! --lp: the GPU's low-power encoder (vah264lpenc, VDEnc fixed-function: less power, same settings); falls
//! back to vah264enc when the element is missing (and vah264enc to vah264lpenc).
//! Software fallback: without a VA H.264 encoder or vapostproc (NVIDIA, no VA driver) the encoder is x264enc
//! (CBR, zerolatency, ultrafast, constrained baseline, same GOP and ~2-frame VBV) after videoconvert, and
//! capture uses shm (--dmabuf is ignored: it needs VA); logged as {"event":"encoder_fallback","element","reason"}.
//! --framed: "WFD1" + u32 BE length before each AU (for wfd-tx --framed).
//! --quiet: no per-frame lines; a stats line every 10 s (always).
//! Exit 1 (the supervisor restarts the encoder): no AU for 3 s (15 s for the first) {"event":"encoder_stalled"},
//! GStreamer error {"event":"gst_error"}, compositor silent for 5 s {"event":"buffer_params_timeout"}, capture
//! size/format changed {"event":"capture_resized"}, Wayland connection lost {"event":"wayland_error"}.
//! Exit 0: stdout reader gone. Exit 2: bad command line.

mod capture;
mod dmabuf;
mod opts;
mod pipeline;
mod shm;
mod wl;

use gstreamer as gst;
use gstreamer::prelude::*;
use std::time::Duration;

fn main() {
    let o = opts::parse_opts().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2)
    });
    wfd_util::install_panic_hook("wfd-media");
    wfd_util::signal_flag(libc::SIGUSR1, &pipeline::IDR);

    let encoder = pipeline::pick_encoder(o.lp);
    let mut wl = wl::wayland_connect(&o);
    let mut cf = wl::probe_format(&mut wl, o.cursor);
    let (ow, oh) = o.size.unwrap_or((cf.w, cf.h));
    let ring: Vec<shm::Shm> =
        (0..shm::RING).map(|_| shm::make_shm(&wl.st, &wl.qh, cf.fmt, cf.w, cf.h, cf.stride)).collect();
    if o.dmabuf && !encoder.is_va() {
        wfd_util::event!("dmabuf_unavailable", "why" => &format!("{} needs system memory", encoder.element()));
    }
    let mut dring = if o.dmabuf && encoder.is_va() { dmabuf::dmabuf_ring(&mut wl, &o) } else { Vec::new() };
    // the upload caps must describe the dmabuf's byte order, not the shm one
    if let Some(d) = dring.first() {
        match wl::drm_gst_fmt(d.fourcc) {
            Some(f) => cf.gst_fmt = f,
            None => {
                wfd_util::event!("dmabuf_unavailable", "why" => &format!("unsupported fourcc {:#x}", d.fourcc));
                dring.clear();
            }
        }
    }

    let p = pipeline::build_pipelines(&o, &cf, ow, oh, encoder);
    p.up.set_state(gst::State::Playing).expect("upload playing");
    let period = Duration::from_micros(1_000_000 / o.fps as u64);
    pipeline::spawn_grid(&p, period);
    p.enc.set_state(gst::State::Playing).expect("playing");
    pipeline::spawn_stall_watchdog();
    wfd_util::event!(
        "start",
        "path" => if dring.is_empty() { "shm" } else { "dmabuf" },
        "capture" => &format!("{}x{} {:?}", cf.w, cf.h, cf.fmt),
        "dmabuf_offer" => cf.dmabuf_offer.as_deref().unwrap_or_default(),
        "encode" => &format!("{ow}x{oh}p{} {} kbps {}", o.fps, o.kbps, encoder.element()),
    );
    capture::capture_loop(&o, &mut wl, &cf, &ring, &dring, &p, period)
}
