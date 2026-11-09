//! The capture loop: screencopy (damage-driven) -> upload pipeline, paced to --fps, plus the 10 s stats.

use gstreamer_allocators as gst_alloc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use wayland_client::protocol::wl_shm;
use wfd_util::{event, mono_us, unix_us};

use crate::dmabuf::{dmabuf_buffer, Dma};
use crate::opts::Opts;
use crate::pipeline::{force_idr_if_asked, Pipes, AUS};
use crate::shm::{free_shm_slot, shm_buffer, Shm, RING};
use crate::wl::{CaptureFmt, FrameState, Wl, DISPATCH_MAX};

/// `capture_failed` (e.g. DPMS off) and skipped dmabuf frames are logged once every this many in --quiet mode
/// (~10 s at 60 fps).
const LOG_EVERY: u64 = 600;
const STATS_EVERY: Duration = Duration::from_secs(10);

/// What the ring buffers were made for. The compositor offering anything else (output resized, mode or
/// format changed) means the buffers no longer fit: exit, and the supervisor restarts with a new probe.
#[derive(Debug, Clone, Copy, PartialEq)]
enum RingGeom {
    Shm { fmt: wl_shm::Format, w: u32, h: u32, stride: u32 },
    Dma { fourcc: u32, w: u32, h: u32 },
}

impl RingGeom {
    fn of(cf: &CaptureFmt, dring: &[Dma]) -> Self {
        match dring.first() {
            Some(d) => RingGeom::Dma { fourcc: d.fourcc, w: d.w, h: d.h },
            None => RingGeom::Shm { fmt: cf.fmt, w: cf.w, h: cf.h, stride: cf.stride },
        }
    }

    /// The geometry the compositor offers for this frame, of the same kind (None: not offered).
    fn offered(&self, f: &FrameState) -> Option<Self> {
        match self {
            RingGeom::Shm { .. } => f.fmt.map(|(fmt, w, h, stride)| RingGeom::Shm { fmt, w, h, stride }),
            RingGeom::Dma { .. } => f.dmabuf.map(|(fourcc, w, h)| RingGeom::Dma { fourcc, w, h }),
        }
    }
}

/// Capture -> upload, paced to --fps, forever. The grid thread feeds the encoder (and repeats a static screen).
pub fn capture_loop(
    o: &Opts,
    wl: &mut Wl,
    cf: &CaptureFmt,
    ring: &[Shm],
    dring: &[Dma],
    p: &Pipes,
    period: Duration,
) -> ! {
    let use_dma = !dring.is_empty();
    let dma_alloc = gst_alloc::DmaBufAllocator::new();
    let mono_to_unix = unix_us() as i64 - mono_us();
    let geom = RingGeom::of(cf, dring);
    let quiet = o.quiet;
    let (mut ri, mut cap_n, mut fail_n, mut skip_n, mut first) = (0usize, 0u64, 0u64, 0u64, true);
    let mut stats = Stats::new(Instant::now(), AUS.load(Ordering::Relaxed));
    loop {
        let t_req = Instant::now();
        if !use_dma {
            ri = free_shm_slot(ri);
        }
        let fr = wl.capture(o.cursor);
        wl.wait_buffer_params();
        if !wl.st.f.failed {
            let got = geom.offered(&wl.st.f);
            if got != Some(geom) {
                event!("capture_resized", "ring" => &format!("{geom:?}"), "offered" => &format!("{got:?}"));
                std::process::exit(1);
            }
        }
        let target = if use_dma { &dring[ri].wl } else { &ring[ri].buffer };
        if first {
            fr.copy(target);
            first = false;
        } else {
            fr.copy_with_damage(target);
        }
        // copy_with_damage: ready only when the screen changes, so no deadline here; the grid thread keeps
        // the encoder fed meanwhile. Wake up at least every DISPATCH_MAX for the IDR flag and the stats.
        while wl.st.f.ready_mono_us.is_none() && !wl.st.f.failed {
            force_idr_if_asked(&p.vasrc);
            stats.tick(Instant::now(), AUS.load(Ordering::Relaxed));
            wl.dispatch(period.min(DISPATCH_MAX));
        }
        fr.destroy();
        if wl.st.f.failed {
            // e.g. output powered off (DPMS): the grid thread repeats the last frame; retry capture
            if !quiet || fail_n.is_multiple_of(LOG_EVERY) {
                event!("capture_failed", "n" => fail_n);
            }
            fail_n += 1;
            std::thread::sleep(period);
            stats.tick(Instant::now(), AUS.load(Ordering::Relaxed));
            continue;
        }
        fail_n = 0;
        let t_ready = unix_us() as i64;
        let t_present = wl.st.f.ready_mono_us.unwrap_or_default() + mono_to_unix; // Some: loop above ended on it
        let b = if use_dma { dmabuf_buffer(&dma_alloc, &dring[ri].bo, cf) } else { Some(shm_buffer(ring, ri)) };
        ri = (ri + 1) % RING;
        force_idr_if_asked(&p.vasrc);
        match b {
            Some(b) => {
                let _ = p.src.push_buffer(b);
                let t_push = unix_us() as i64;
                if !quiet {
                    eprintln!(
                        "{{\"cap\":{cap_n},\"t_present\":{t_present},\"t_ready\":{t_ready},\"t_push\":{t_push},\"y_invert\":{}}}",
                        wl.st.f.y_invert
                    );
                }
                cap_n += 1;
                stats.captured(t_push - t_present);
            }
            None => {
                // fd limit or import error: drop this frame (the grid repeats the previous one), keep capturing
                if !quiet || skip_n.is_multiple_of(LOG_EVERY) {
                    event!("dmabuf_frame_skipped", "n" => skip_n);
                }
                skip_n += 1;
            }
        }
        stats.tick(Instant::now(), AUS.load(Ordering::Relaxed));
        // pace to --fps: the next copy is requested no earlier than one period after this one
        let el = t_req.elapsed();
        if el < period {
            std::thread::sleep(period - el);
        }
    }
}

/// The 10 s stats line (also in --quiet): captured and encoded frames, median present->push.
struct Stats {
    since: Instant,
    captured: u64,
    /// AUS at `since`
    aus_at: u64,
    /// present->push of each captured frame; cleared, not reallocated, every period
    lat_us: Vec<i64>,
}

/// One stats period.
#[derive(Debug, PartialEq)]
struct StatsLine {
    secs: f64,
    captured: u64,
    encoded: u64,
    present_to_push_med_us: i64,
}

impl Stats {
    fn new(now: Instant, aus: u64) -> Self {
        Stats { since: now, captured: 0, aus_at: aus, lat_us: Vec::with_capacity(1024) }
    }

    /// One captured frame pushed `lat_us` after the compositor presented it.
    fn captured(&mut self, lat_us: i64) {
        self.captured += 1;
        self.lat_us.push(lat_us);
    }

    /// Log the line when STATS_EVERY has passed (`aus`: the AU counter now).
    fn tick(&mut self, now: Instant, aus: u64) {
        if let Some(l) = self.take(now, aus) {
            event!(
                "stats",
                "secs" => (l.secs * 10.0).round() / 10.0,
                "captured" => l.captured,
                "encoded" => l.encoded,
                "present_to_push_med_us" => l.present_to_push_med_us,
            );
        }
    }

    /// The finished period's numbers (and start a new one), or None before STATS_EVERY.
    fn take(&mut self, now: Instant, aus: u64) -> Option<StatsLine> {
        let secs = now.saturating_duration_since(self.since);
        if secs < STATS_EVERY {
            return None;
        }
        self.lat_us.sort_unstable();
        let line = StatsLine {
            secs: secs.as_secs_f64(),
            captured: self.captured,
            encoded: aus.wrapping_sub(self.aus_at),
            present_to_push_med_us: self.lat_us.get(self.lat_us.len() / 2).copied().unwrap_or(-1),
        };
        (self.since, self.captured, self.aus_at) = (now, 0, aus);
        self.lat_us.clear();
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_count_deltas_and_report_on_a_static_screen() {
        let t0 = Instant::now();
        let mut s = Stats::new(t0, 100);
        for l in [30, 10, 20] {
            s.captured(l);
        }
        assert_eq!(s.take(t0 + Duration::from_secs(9), 500), None);
        let l = s.take(t0 + STATS_EVERY, 700).unwrap();
        assert_eq!(l, StatsLine { secs: 10.0, captured: 3, encoded: 600, present_to_push_med_us: 20 });
        // static screen: nothing captured, the grid still encodes; the line still comes
        let l = s.take(t0 + STATS_EVERY * 2, 1300).unwrap();
        assert_eq!((l.captured, l.encoded, l.present_to_push_med_us), (0, 600, -1));
    }

    #[test]
    fn ring_geometry_change_is_detected() {
        let cf = CaptureFmt {
            fmt: wl_shm::Format::Xbgr8888,
            w: 1920,
            h: 1080,
            stride: 7680,
            gst_fmt: "RGBx",
            dmabuf_offer: None,
        };
        let g = RingGeom::of(&cf, &[]);
        let mut f = FrameState { fmt: Some((wl_shm::Format::Xbgr8888, 1920, 1080, 7680)), ..Default::default() };
        assert_eq!(g.offered(&f), Some(g));
        f.fmt = Some((wl_shm::Format::Xbgr8888, 1280, 720, 5120));
        assert_ne!(g.offered(&f), Some(g));
        f.fmt = None;
        assert_eq!(g.offered(&f), None);

        let d = RingGeom::Dma { fourcc: u32::from_le_bytes(*b"XR24"), w: 1920, h: 1080 };
        let mut f = FrameState { dmabuf: Some((u32::from_le_bytes(*b"XR24"), 1920, 1080)), ..Default::default() };
        assert_eq!(d.offered(&f), Some(d));
        f.dmabuf = Some((u32::from_le_bytes(*b"XR24"), 2560, 1440));
        assert_ne!(d.offered(&f), Some(d));
    }
}
