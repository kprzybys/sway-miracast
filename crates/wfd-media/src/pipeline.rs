//! GStreamer side: upload (raw -> VA NV12) and encode (VA -> H.264) pipelines, the grid clock that feeds the
//! encoder, AU output on stdout, forced IDR and the stall watchdog. Without a VA H.264 encoder (NVIDIA, no VA
//! driver) the same two pipelines run in software: videoconvert (raw -> I420) and x264enc.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::opts::Opts;
use crate::wl::CaptureFmt;

/// SIGUSR1 (sink `wfd_idr_request`): next AU is an IDR with SPS/PPS.
pub static IDR: AtomicBool = AtomicBool::new(false);
/// Encoded AUs since start (monotonic, never reset): the stall watchdog's progress and the stats' "encoded".
pub static AUS: AtomicU64 = AtomicU64::new(0);

/// The grid clock resyncs instead of bursting when it is this many periods late.
const GRID_RESYNC_PERIODS: u32 = 4;

/// Upload (raw -> VA surface, NV12) and encode (VA surface -> H.264) pipelines. A static screen is repeated
/// by re-feeding the last VA surface to the encoder: no CPU copy, no re-upload.
pub struct Pipes {
    pub up: gst::Pipeline,
    pub enc: gst::Pipeline,
    /// raw frames in
    pub src: gst_app::AppSrc,
    /// VA surfaces into the encoder
    pub vasrc: gst_app::AppSrc,
    /// newest uploaded surface (repeated by the grid thread)
    last_va: Arc<Mutex<Option<gst::Buffer>>>,
}

/// Re-feed a VA surface to the encoder (the copy shares the surface; timestamps come from do-timestamp).
fn feed(vasrc: &gst_app::AppSrc, b: &gst::Buffer) {
    let mut c = b.copy();
    if let Some(cm) = c.get_mut() {
        // a fresh copy is writable
        cm.set_pts(gst::ClockTime::NONE);
        cm.set_dts(gst::ClockTime::NONE);
    }
    let _ = vasrc.push_buffer(c);
}

/// SIGUSR1 (sink `wfd_idr_request`): next AU is an IDR with SPS/PPS.
pub fn force_idr_if_asked(vasrc: &gst_app::AppSrc) {
    if IDR.swap(false, Ordering::Relaxed) {
        vasrc.send_event(gstreamer_video::DownstreamForceKeyUnitEvent::builder().all_headers(true).build());
        wfd_util::event!("idr_forced");
    }
}

/// The H.264 encoder: a VA-API element (GPU, frames stay in VAMemory) or software x264.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoder {
    Va(&'static str),
    X264,
}

impl Encoder {
    pub fn element(self) -> &'static str {
        match self {
            Encoder::Va(e) => e,
            Encoder::X264 => "x264enc",
        }
    }

    /// Frames live in VA surfaces (dmabuf capture and the shared VA display need this).
    pub fn is_va(self) -> bool {
        matches!(self, Encoder::Va(_))
    }
}

/// The encoder for `--lp` given which GStreamer elements exist; Some(reason) when it is not the one asked for.
/// VA: vah264lpenc with --lp, else vah264enc, each standing in for the other when only one exists (some GPUs
/// have only the low-power one). No VA H.264 encoder or no vapostproc: x264enc. Nothing at all: vah264enc,
/// whose pipeline then fails with a clear gst_error.
pub fn choose_encoder(lp: bool, has: impl Fn(&str) -> bool) -> (Encoder, Option<&'static str>) {
    let (want, other) = if lp { ("vah264lpenc", "vah264enc") } else { ("vah264enc", "vah264lpenc") };
    let va = has("vapostproc");
    if va && has(want) {
        (Encoder::Va(want), None)
    } else if va && has(other) {
        (Encoder::Va(other), Some(if lp { "vah264lpenc missing" } else { "vah264enc missing" }))
    } else if has("x264enc") {
        let why = if va { "no VA H.264 encoder (vah264enc/vah264lpenc)" } else { "no VA video (vapostproc)" };
        (Encoder::X264, Some(why))
    } else {
        (Encoder::Va("vah264enc"), None)
    }
}

/// `choose_encoder` with the installed GStreamer elements; the fallback goes to the log (`encoder_fallback`).
pub fn pick_encoder(lp: bool) -> Encoder {
    let ok = gst::init().is_ok();
    let (enc, why) = choose_encoder(lp, |e| ok && gst::ElementFactory::find(e).is_some());
    if let Some(why) = why {
        wfd_util::event!("encoder_fallback", "element" => enc.element(), "reason" => why);
    }
    enc
}

/// gst-launch descriptions of the upload and encode pipelines.
fn pipeline_descs(o: &Opts, cf: &CaptureFmt, ow: u32, oh: u32, enc: Encoder) -> (String, String) {
    let Encoder::Va(enc) = enc else { return x264_descs(o, cf, ow, oh) };
    // CPB of ~2 frames (cpb-size is in kbit): keeps the encoder's VBV, and so the TV's buffering, small
    let cpb = o.kbps * 2 / o.fps.max(1);
    let up_desc = format!(
        "appsrc name=src is-live=true format=time do-timestamp=true max-buffers=1 leaky-type=downstream \
           caps=video/x-raw,format={gst_fmt},width={cw},height={ch},framerate={fps}/1 \
         ! vapostproc ! video/x-raw(memory:VAMemory),format=NV12,width={ow},height={oh} \
         ! appsink name=vasink sync=false max-buffers=1 drop=true",
        gst_fmt = cf.gst_fmt,
        cw = cf.w,
        ch = cf.h,
        fps = o.fps
    );
    let enc_desc = format!(
        "appsrc name=vasrc is-live=true format=time do-timestamp=true max-buffers=1 leaky-type=downstream \
           caps=video/x-raw(memory:VAMemory),format=NV12,width={ow},height={oh},framerate={fps}/1 \
         ! {enc} rate-control=cbr bitrate={kbps} cpb-size={cpb} key-int-max={gop} b-frames=0 ref-frames=1 num-slices=1 \
           cabac=false target-usage=7 \
         ! video/x-h264,profile=constrained-baseline \
         ! h264parse config-interval=-1 ! video/x-h264,stream-format=byte-stream,alignment=au \
         ! appsink name=sink sync=false max-buffers=2 drop=true",
        fps = o.fps,
        kbps = o.kbps,
        gop = o.gop,
    );
    (up_desc, enc_desc)
}

/// Software fallback with the same shape and element names as the VA pipelines: the upload pipeline converts
/// and scales to I420 in system memory, the encoder is x264 with the VA settings' equivalents (CBR, VBV of
/// ~2 frames, GOP, no B-frames, one reference, constrained baseline = no CABAC, zerolatency = no lookahead).
fn x264_descs(o: &Opts, cf: &CaptureFmt, ow: u32, oh: u32) -> (String, String) {
    let vbv_ms = (2000 / o.fps.max(1)).max(1);
    let up_desc = format!(
        "appsrc name=src is-live=true format=time do-timestamp=true max-buffers=1 leaky-type=downstream \
           caps=video/x-raw,format={gst_fmt},width={cw},height={ch},framerate={fps}/1 \
         ! videoconvert n-threads=0 ! videoscale ! video/x-raw,format=I420,width={ow},height={oh} \
         ! appsink name=vasink sync=false max-buffers=1 drop=true",
        gst_fmt = cf.gst_fmt,
        cw = cf.w,
        ch = cf.h,
        fps = o.fps
    );
    let enc_desc = format!(
        "appsrc name=vasrc is-live=true format=time do-timestamp=true max-buffers=1 leaky-type=downstream \
           caps=video/x-raw,format=I420,width={ow},height={oh},framerate={fps}/1 \
         ! x264enc pass=cbr bitrate={kbps} vbv-buf-capacity={vbv_ms} key-int-max={gop} bframes=0 ref=1 \
           tune=zerolatency speed-preset=ultrafast byte-stream=true \
         ! video/x-h264,profile=constrained-baseline \
         ! h264parse config-interval=-1 ! video/x-h264,stream-format=byte-stream,alignment=au \
         ! appsink name=sink sync=false max-buffers=2 drop=true",
        fps = o.fps,
        kbps = o.kbps,
        gop = o.gop,
    );
    (up_desc, enc_desc)
}

/// Build both pipelines (startup: failures panic, the supervisor restarts us).
pub fn build_pipelines(o: &Opts, cf: &CaptureFmt, ow: u32, oh: u32, encoder: Encoder) -> Pipes {
    gst::init().expect("gst init");
    let (up_desc, enc_desc) = pipeline_descs(o, cf, ow, oh, encoder);
    let up = gst::parse::launch(&up_desc).expect("upload pipeline").downcast::<gst::Pipeline>().unwrap();
    let enc = gst::parse::launch(&enc_desc).expect("encode pipeline").downcast::<gst::Pipeline>().unwrap();
    // share the VA display: capture the uploader's context and give it to the encoder
    let va_ctx: Arc<Mutex<Option<gst::Context>>> = Default::default();
    {
        let va_ctx = va_ctx.clone();
        up.bus().unwrap().set_sync_handler(move |_, msg| {
            if let gst::MessageView::HaveContext(hc) = msg.view() {
                *va_ctx.lock().unwrap_or_else(|e| e.into_inner()) = Some(hc.context());
            }
            gst::BusSyncReply::Pass
        });
    }
    up.set_state(gst::State::Ready).expect("upload ready");
    match va_ctx.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        Some(c) => enc.set_context(&c),
        None if encoder.is_va() => wfd_util::event!("warn", "msg" => "no VA display context from uploader"),
        None => {} // x264: system memory, nothing to share
    }
    // element names come from the descriptions above
    let src = up.by_name("src").unwrap().downcast::<gst_app::AppSrc>().unwrap();
    let vasink = up.by_name("vasink").unwrap().downcast::<gst_app::AppSink>().unwrap();
    let vasrc = enc.by_name("vasrc").unwrap().downcast::<gst_app::AppSrc>().unwrap();
    let sink = enc.by_name("sink").unwrap().downcast::<gst_app::AppSink>().unwrap();
    let last_va: Arc<Mutex<Option<gst::Buffer>>> = Default::default();
    {
        let last_va = last_va.clone();
        vasink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |s| {
                    let sample = s.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let buf = sample.buffer_owned().ok_or(gst::FlowError::Error)?;
                    // the grid thread feeds it to the encoder on the next tick
                    *last_va.lock().unwrap_or_else(|e| e.into_inner()) = Some(buf);
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
    }
    let (framed, quiet) = (o.framed, o.quiet);
    let mut scratch: Vec<u8> = Vec::new(); // --framed: header + AU, reused (grows to the largest AU once)
    sink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_sample(move |s| {
                let sample = s.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                let buf = sample.buffer().ok_or(gst::FlowError::Error)?;
                let map = buf.map_readable().map_err(|_| gst::FlowError::Error)?;
                if write_au(&mut std::io::stdout().lock(), &mut scratch, &map, framed).is_err() {
                    std::process::exit(0); // reader gone: the session ends, nothing to restart
                }
                let key = !buf.flags().contains(gst::BufferFlags::DELTA_UNIT);
                let au_n = AUS.fetch_add(1, Ordering::Relaxed);
                if !quiet {
                    eprintln!(
                        "{{\"au\":{au_n},\"bytes\":{},\"pts_us\":{},\"t_out\":{},\"key\":{key}}}",
                        map.len(),
                        buf.pts().map(|p| p.useconds() as i64).unwrap_or(-1),
                        wfd_util::unix_us()
                    );
                }
                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );
    for bus in [up.bus().unwrap(), enc.bus().unwrap()] {
        std::thread::spawn(move || {
            for msg in bus.iter_timed(gst::ClockTime::NONE) {
                if let gst::MessageView::Error(e) = msg.view() {
                    let dbg = e.debug().map(|d| d.to_string()).unwrap_or_default();
                    wfd_util::event!("gst_error", "error" => &e.error().to_string(), "debug" => &dbg);
                    std::process::exit(1);
                }
            }
        });
    }
    Pipes { up, enc, src, vasrc, last_va }
}

/// --framed header: "WFD1" + u32 BE length (wfd-tx logic::next_framed_au reads it).
fn frame_header(len: usize) -> [u8; 8] {
    let mut h = [0u8; 8];
    h[..4].copy_from_slice(b"WFD1");
    h[4..].copy_from_slice(&(len as u32).to_be_bytes());
    h
}

/// One AU to `out` in a single write (--framed: "WFD1" + u32 BE length first, assembled in `scratch`).
fn write_au(out: &mut impl Write, scratch: &mut Vec<u8>, au: &[u8], framed: bool) -> std::io::Result<()> {
    if framed {
        scratch.clear();
        scratch.extend_from_slice(&frame_header(au.len()));
        scratch.extend_from_slice(au);
        out.write_all(scratch)?;
    } else {
        out.write_all(au)?;
    }
    out.flush()
}

/// The grid tick after `prev`: one period later, or one period from `now` when far behind (a stall),
/// so a late grid resyncs instead of bursting the frames it missed.
fn next_grid_tick(prev: Instant, now: Instant, period: Duration) -> Instant {
    let next = prev + period;
    if now > next + period * GRID_RESYNC_PERIODS {
        now + period
    } else {
        next
    }
}

/// Feed the encoder on a strict 1/fps clock with the newest surface (or a repeat of it on a static screen).
pub fn spawn_grid(p: &Pipes, period: Duration) {
    let (last_va, vasrc) = (p.last_va.clone(), p.vasrc.clone());
    std::thread::spawn(move || {
        let mut next = Instant::now() + period;
        loop {
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            }
            next = next_grid_tick(next, Instant::now(), period);
            if let Some(b) = last_va.lock().unwrap_or_else(|e| e.into_inner()).clone() {
                feed(&vasrc, &b);
            }
        }
    });
}

/// The encoder emits an AU every frame period (the grid thread repeats a static screen). No AU for
/// STALL_AFTER (after the first one) means GStreamer hangs without an error on the bus: exit, so wfd-source
/// restarts the encoder in about a second instead of the TV ending the session after ~25 s.
pub fn spawn_stall_watchdog() {
    const STALL_AFTER: Duration = Duration::from_secs(3);
    const FIRST_AU_WITHIN: Duration = Duration::from_secs(15);
    std::thread::spawn(|| {
        let (start, mut last_n, mut last_change) = (Instant::now(), 0u64, Instant::now());
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let n = AUS.load(Ordering::Relaxed);
            if n != last_n {
                (last_n, last_change) = (n, Instant::now());
                continue;
            }
            if stalled(n, start.elapsed(), last_change.elapsed(), STALL_AFTER, FIRST_AU_WITHIN) {
                wfd_util::event!("encoder_stalled", "aus" => n);
                std::process::exit(1);
            }
        }
    });
}

/// No progress for too long: `since_start` before the first AU, `since_change` after it.
fn stalled(aus: u64, since_start: Duration, since_change: Duration, after: Duration, first_within: Duration) -> bool {
    if aus == 0 {
        since_start >= first_within
    } else {
        since_change >= after
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wayland_client::protocol::wl_shm;

    #[test]
    fn frame_header_is_magic_and_big_endian_length() {
        assert_eq!(frame_header(0x0102_0304), *b"WFD1\x01\x02\x03\x04");
    }

    #[test]
    fn write_au_frames_and_reuses_the_scratch_buffer() {
        let (mut out, mut scratch) = (Vec::new(), Vec::new());
        write_au(&mut out, &mut scratch, b"\0\0\0\x01AB", true).unwrap();
        write_au(&mut out, &mut scratch, b"C", true).unwrap();
        write_au(&mut out, &mut scratch, b"D", false).unwrap();
        assert_eq!(out, b"WFD1\0\0\0\x06\0\0\0\x01ABWFD1\0\0\0\x01CD");
        assert!(scratch.capacity() >= 14);
    }

    #[test]
    fn grid_ticks_evenly_and_resyncs_instead_of_bursting() {
        let p = Duration::from_millis(16);
        let t0 = Instant::now();
        // on time or slightly late: strictly one period apart (catches up without drifting)
        assert_eq!(next_grid_tick(t0, t0 + Duration::from_millis(5), p), t0 + p);
        assert_eq!(next_grid_tick(t0, t0 + p * 3, p), t0 + p);
        // stalled for > GRID_RESYNC_PERIODS: next tick one period from now, not a burst of missed ones
        let now = t0 + p * (GRID_RESYNC_PERIODS + 3);
        assert_eq!(next_grid_tick(t0, now, p), now + p);
    }

    #[test]
    fn watchdog_gives_the_first_au_longer() {
        let s = Duration::from_secs;
        assert!(!stalled(0, s(10), s(10), s(3), s(15)));
        assert!(stalled(0, s(15), s(15), s(3), s(15)));
        assert!(!stalled(5, s(100), s(2), s(3), s(15)));
        assert!(stalled(5, s(100), s(3), s(3), s(15)));
    }

    #[test]
    fn pipelines_use_low_latency_settings() {
        let o = Opts { fps: 60, gop: 120, quiet: true, framed: true, ..Opts::default() };
        let cf = CaptureFmt {
            fmt: wl_shm::Format::Xbgr8888,
            w: 1920,
            h: 1080,
            stride: 7680,
            gst_fmt: "RGBx",
            dmabuf_offer: None,
        };
        let (up, enc) = pipeline_descs(&o, &cf, 1920, 1080, Encoder::Va("vah264enc"));
        assert!(up.contains("format=RGBx,width=1920,height=1080,framerate=60/1"), "{up}");
        assert!(up.contains("max-buffers=1 drop=true"), "upload never queues: {up}");
        // CPB of 2 frames at 8 Mb/s, 60 fps = 266 kbit; GOP 2 s; no B-frames; constrained baseline
        for want in [
            "bitrate=8000",
            "cpb-size=266",
            "key-int-max=120",
            "b-frames=0",
            "num-slices=1",
            "profile=constrained-baseline",
        ] {
            assert!(enc.contains(want), "{want} missing: {enc}");
        }
        assert!(enc.contains("! vah264enc "), "{enc}");
        let (_, lp) = pipeline_descs(&o, &cf, 1920, 1080, Encoder::Va("vah264lpenc"));
        assert_eq!(lp, enc.replace("! vah264enc ", "! vah264lpenc "));
    }

    fn cf_1080() -> CaptureFmt {
        CaptureFmt {
            fmt: wl_shm::Format::Xrgb8888,
            w: 1920,
            h: 1080,
            stride: 7680,
            gst_fmt: "BGRx",
            dmabuf_offer: None,
        }
    }

    #[test]
    fn va_pipelines_are_unchanged() {
        // the strings verified on the TV (2026-11-09); a change here is a change of the live encoder
        let o = Opts { fps: 60, gop: 120, kbps: 12000, ..Opts::default() };
        let (up, enc) = pipeline_descs(&o, &cf_1080(), 1920, 1080, Encoder::Va("vah264lpenc"));
        let squash = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(
            squash(&up),
            "appsrc name=src is-live=true format=time do-timestamp=true max-buffers=1 leaky-type=downstream \
             caps=video/x-raw,format=BGRx,width=1920,height=1080,framerate=60/1 ! vapostproc ! \
             video/x-raw(memory:VAMemory),format=NV12,width=1920,height=1080 ! appsink name=vasink sync=false \
             max-buffers=1 drop=true"
        );
        assert_eq!(
            squash(&enc),
            "appsrc name=vasrc is-live=true format=time do-timestamp=true max-buffers=1 leaky-type=downstream \
             caps=video/x-raw(memory:VAMemory),format=NV12,width=1920,height=1080,framerate=60/1 ! vah264lpenc \
             rate-control=cbr bitrate=12000 cpb-size=400 key-int-max=120 b-frames=0 ref-frames=1 num-slices=1 \
             cabac=false target-usage=7 ! video/x-h264,profile=constrained-baseline ! h264parse config-interval=-1 \
             ! video/x-h264,stream-format=byte-stream,alignment=au ! appsink name=sink sync=false max-buffers=2 drop=true"
        );
    }

    #[test]
    fn encoder_choice_follows_the_installed_elements() {
        let with = |list: &'static [&'static str]| move |e: &str| list.contains(&e);
        let all = with(&["vapostproc", "vah264enc", "vah264lpenc", "x264enc"]);
        assert_eq!(choose_encoder(true, all), (Encoder::Va("vah264lpenc"), None));
        assert_eq!(choose_encoder(false, all), (Encoder::Va("vah264enc"), None));
        // --lp on a GPU without the low-power encoder, and a GPU with only the low-power one
        let full_only = with(&["vapostproc", "vah264enc", "x264enc"]);
        assert_eq!(choose_encoder(true, full_only).0, Encoder::Va("vah264enc"));
        let lp_only = with(&["vapostproc", "vah264lpenc"]);
        assert_eq!(choose_encoder(false, lp_only).0, Encoder::Va("vah264lpenc"));
        // NVIDIA without a VA driver: VA video may exist without an H.264 encoder, or not at all
        let (e, why) = choose_encoder(true, with(&["vapostproc", "x264enc"]));
        assert_eq!(e, Encoder::X264);
        assert!(why.unwrap().contains("no VA H.264 encoder"));
        let (e, why) = choose_encoder(true, with(&["x264enc"]));
        assert_eq!((e, why), (Encoder::X264, Some("no VA video (vapostproc)")));
        assert!(choose_encoder(true, with(&["vah264enc", "x264enc"])).0 == Encoder::X264);
        // nothing usable: keep the VA element, its pipeline fails with a clear error
        assert_eq!(choose_encoder(true, with(&[])), (Encoder::Va("vah264enc"), None));
    }

    #[test]
    fn x264_fallback_matches_the_va_settings() {
        let o = Opts { fps: 60, gop: 120, kbps: 8000, ..Opts::default() };
        let (up, enc) = pipeline_descs(&o, &cf_1080(), 1280, 720, Encoder::X264);
        assert!(up.contains("format=BGRx,width=1920,height=1080,framerate=60/1"), "{up}");
        assert!(up.contains("! videoconvert n-threads=0 ! videoscale ! video/x-raw,format=I420,width=1280,height=720"));
        assert!(!up.contains("vapostproc") && !enc.contains("VAMemory"), "{up} {enc}");
        assert!(up.contains("appsink name=vasink sync=false max-buffers=1 drop=true"), "{up}");
        assert!(enc.contains("caps=video/x-raw,format=I420,width=1280,height=720,framerate=60/1"), "{enc}");
        for want in [
            "! x264enc pass=cbr",
            "bitrate=8000",
            "vbv-buf-capacity=33",
            "key-int-max=120",
            "bframes=0",
            "ref=1",
            "tune=zerolatency",
            "speed-preset=ultrafast",
            "profile=constrained-baseline",
            "stream-format=byte-stream,alignment=au",
            "appsink name=sink",
        ] {
            assert!(enc.contains(want), "{want} missing: {enc}");
        }
    }
}
