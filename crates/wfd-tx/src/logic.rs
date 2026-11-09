//! Pure pieces of wfd-tx (no I/O, no clock reads): input framing, audio PTS planning, the
//! constant-rate padding budget and the 27 MHz clock arithmetic. Time comes in as arguments, so each
//! piece is unit-tested.

use std::time::Duration;

use crate::ts::TS;

/// 27 MHz (PCR) ticks per 90 kHz (PTS) tick.
pub const PCR_PER_PTS: u64 = 300;
/// 27 MHz ticks per millisecond.
pub const TICKS27_PER_MS: f64 = 27_000.0;
/// 90 kHz (PTS) ticks per millisecond.
const PTS_PER_MS: u64 = 90;

/// LPCM as WFD carries it (Table 106): 48 kHz, 16-bit, stereo, in 10 ms chunks (one PES each).
pub const AUDIO_RATE: usize = 48_000;
const AUDIO_FRAME_BYTES: usize = 2 * 2; // 16 bit x 2 channels
pub const AUDIO_CHUNK_MS: u64 = 10;
/// One LPCM chunk in bytes (1920).
pub const LPCM_CHUNK: usize = AUDIO_RATE / 1000 * AUDIO_CHUNK_MS as usize * AUDIO_FRAME_BYTES;
/// One LPCM chunk (10 ms) in 90 kHz ticks (900).
pub const AUDIO_PTS_STEP: i64 = (PTS_PER_MS * AUDIO_CHUNK_MS) as i64;
/// One LPCM chunk as a Duration (the audio pacing tick).
pub const AUDIO_TICK: Duration = Duration::from_millis(AUDIO_CHUNK_MS);
/// Audio PTS may run this far (40 ms, 90 kHz) from the clock before it is re-anchored.
pub const AUDIO_RESYNC_TICKS: i64 = 3600;
/// Framed input: "WFD1" + u32 BE length + AU (wfd-media --framed).
pub const MAGIC: [u8; 4] = *b"WFD1";
const HEADER: usize = MAGIC.len() + 4;
/// A length above this is not a real header (a 1080p IDR is well under 1 MiB).
pub const MAX_AU: usize = 16 << 20;
/// kb/s -> bytes per second.
const BYTES_PER_KBIT: f64 = 125.0;
/// Padding added after one AU covers at most this long (one frame at 30 fps), so a pause is not
/// followed by a burst.
const PAD_MAX_S: f64 = 1.0 / 30.0;
/// Surplus carried over after busy content (video above the target rate).
const PAD_CARRY_S: f64 = 0.2;
/// Behind by more than this: restart the budget instead of catching up.
const PAD_RESTART_S: f64 = 1.0;

/// 27 MHz ticks in a Duration.
pub fn ticks27(d: Duration) -> u64 {
    (d.as_nanos() * 27 / 1000) as u64
}

/// A span of 27 MHz ticks as a Duration.
pub fn ticks27_to_duration(t: u64) -> Duration {
    Duration::from_nanos(t * 1000 / 27)
}

/// One decimal, for log values that were `{:.1}` in hand-written lines (a number stays a number).
pub fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// Two decimals (`{:.2}`).
pub fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// Milliseconds (live tuning files) as 27 MHz ticks, truncated like the PTS arithmetic.
pub fn ms_to_ticks27(ms: f64) -> f64 {
    ms * TICKS27_PER_MS
}

/// One step of reading framed input.
#[derive(Debug, PartialEq)]
pub enum Framed {
    /// A complete AU, moved from the buffer into `au`.
    Au,
    /// Bytes before the next magic were dropped (a writer died mid-AU, or garbage).
    Skipped(usize),
    /// Not enough bytes for the next AU yet.
    NeedMore,
}

/// Takes the next AU (or a resync step) off the front of `buf`. The AU is copied into `au` (cleared
/// first; a recycled Vec, so no allocation per AU once it has grown).
pub fn next_framed_au(buf: &mut Vec<u8>, au: &mut Vec<u8>) -> Framed {
    if buf.len() >= 4 && buf[..4] != MAGIC {
        // keep the last 3 bytes: they may be the start of a magic split across reads
        let skip = buf.windows(4).position(|w| w == MAGIC).unwrap_or(buf.len() - 3);
        buf.drain(..skip);
        return Framed::Skipped(skip);
    }
    if buf.len() < HEADER {
        return Framed::NeedMore;
    }
    let len = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
    if len > MAX_AU {
        buf.drain(..4); // not a real header: look for the next magic
        return Framed::Skipped(4);
    }
    if buf.len() < HEADER + len {
        return Framed::NeedMore;
    }
    au.clear();
    au.extend_from_slice(&buf[HEADER..HEADER + len]);
    // usually nothing follows (one AU per write), so this moves no bytes
    buf.drain(..HEADER + len);
    Framed::Au
}

/// How to timestamp a series of `n` audio chunks read at the same instant.
#[derive(Debug, PartialEq)]
pub struct PtsPlan {
    /// PTS (90 kHz) of the first chunk that is sent.
    pub start: u64,
    /// Leading chunks to drop (source clock ahead of ours).
    pub skip: usize,
    /// Last chunk's PTS minus the clock before any correction (90 kHz).
    pub drift: i64,
    /// `Some("jump")` / `Some("drop")` when a correction was made (log it).
    pub action: Option<&'static str>,
}

/// PTS for the whole series at once (a burst after a stall holds several chunks): continuous
/// (+10 ms per chunk) while the series' last chunk stays within 40 ms of the clock `now90`.
/// Behind (gap, slow source clock): re-anchor the last chunk on the clock (PTS jumps forward,
/// the sink fills the gap). Ahead (fast source clock): drop leading chunks instead of moving PTS
/// back, so audio PTS never goes backwards. `prev` = PTS the next chunk would get (None after
/// silence or a delay change).
pub fn audio_pts_plan(prev: Option<u64>, n: usize, now90: i64) -> PtsPlan {
    let n = n as i64;
    let anchored = (now90 - AUDIO_PTS_STEP * (n - 1)).max(0) as u64;
    let Some(p) = prev else {
        return PtsPlan { start: anchored, skip: 0, drift: 0, action: None };
    };
    let drift = p as i64 + AUDIO_PTS_STEP * (n - 1) - now90;
    if drift < -AUDIO_RESYNC_TICKS {
        PtsPlan { start: anchored, skip: 0, drift, action: Some("jump") }
    } else if drift > AUDIO_RESYNC_TICKS {
        let skip = ((drift + AUDIO_PTS_STEP - 1) / AUDIO_PTS_STEP).min(n) as usize;
        PtsPlan { start: p, skip, drift, action: Some("drop") }
    } else {
        PtsPlan { start: p, skip: 0, drift, action: None }
    }
}

/// Constant TS rate: null packets top the stream up to `kbps`. Time in seconds since any fixed start.
pub struct PadBudget {
    pub kbps: f64,
    t0: f64,
    sent: u64,
}

impl PadBudget {
    pub fn new(kbps: f64, now: f64) -> Self {
        PadBudget { kbps, t0: now, sent: 0 }
    }

    /// New target rate (SIGHUP) or a fresh start.
    pub fn reset(&mut self, kbps: f64, now: f64) {
        *self = PadBudget::new(kbps, now);
    }

    /// The rate from the tuning file: restarts the budget only when it changed (SIGHUP also comes
    /// for the other tuning files; a restart drops the carried surplus). True when it changed.
    pub fn set_kbps(&mut self, kbps: f64, now: f64) -> bool {
        if kbps == self.kbps {
            return false;
        }
        self.reset(kbps, now);
        true
    }

    /// Packets sent outside AUs (audio, PCR-only) count toward the rate.
    pub fn add(&mut self, bytes: usize) {
        self.sent += bytes as u64;
    }

    /// Null packets to append after an AU whose TS bytes are `out_len`; updates the budget.
    pub fn after_au(&mut self, out_len: usize, now: f64) -> usize {
        if self.kbps <= 0.0 {
            return 0;
        }
        let rate = self.kbps * BYTES_PER_KBIT;
        let due = ((now - self.t0) * rate) as u64;
        let have = self.sent + out_len as u64;
        if have >= due {
            // only a bounded surplus is carried over: unbounded, the budget stays ahead for minutes
            // after busy content and the stream goes thin with no padding at all (Samsung Q10: thin
            // TS = uneven audio); none at all turns the target into a per-frame floor (measured
            // 19 Mb/s on air with 12 Mb/s video)
            self.t0 = now;
            self.sent = (have - due).min((rate * PAD_CARRY_S) as u64);
            return 0;
        }
        let npk = ((due - have).min((rate * PAD_MAX_S) as u64) / TS as u64) as usize;
        if due > have + (rate * PAD_RESTART_S) as u64 {
            // far behind (pause, reopen): restart the budget instead of bursting
            self.t0 = now;
            self.sent = 0;
        }
        self.sent += (out_len + npk * TS) as u64;
        npk
    }
}

/// Upper bound for the video hold ("film" mode): more is a typo, not a sink buffer.
pub const MAX_HOLD_MS: f64 = 2000.0;

/// True when decoding can restart at this Annex B access unit after frames were dropped: an IDR slice
/// (NAL 5) or an SPS (NAL 7). A forced key unit from VA-API (wfd-media SIGUSR1) is an I frame with
/// SPS/PPS, not always NAL 5; waiting for a real IDR alone meant up to one GOP (2 s) without picture.
pub fn is_keyframe(au: &[u8]) -> bool {
    au.windows(4).any(|w| w[..3] == [0, 0, 1] && matches!(w[3] & 0x1f, 5 | 7))
}

/// What to do with the held video queue after the hold changed (or was checked).
#[derive(Debug, PartialEq)]
pub struct HoldPlan {
    /// leading AUs to drop
    pub drop: usize,
    /// after the drop, send nothing until an IDR comes
    pub wait_idr: bool,
}

/// "Film" mode holds AUs `hold27` after arrival (the sink ignores video PTS, so delay must be real).
/// When the hold gets shorter, the AUs already past the new deadline would go out in one burst and
/// the sink would buffer them (more delay, not less): drop them and restart on the next IDR.
/// `arrivals` = arrival clocks (27 MHz) of the queued AUs, oldest first.
pub fn shorten_hold(arrivals: &[u64], new_hold27: u64, now27: u64) -> HoldPlan {
    let late = arrivals.iter().take_while(|&&c| c + new_hold27 < now27).count();
    // one late frame is the normal case (it is due right now), more is a burst
    if late <= 1 {
        HoldPlan { drop: 0, wait_idr: false }
    } else {
        HoldPlan { drop: late, wait_idr: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(len: usize, b: u8) -> Vec<u8> {
        let mut v = MAGIC.to_vec();
        v.extend_from_slice(&(len as u32).to_be_bytes());
        v.extend(std::iter::repeat_n(b, len));
        v
    }

    fn drain_all(buf: &mut Vec<u8>, aus: &mut Vec<Vec<u8>>) -> usize {
        let mut skipped = 0;
        let mut a = vec![0xee; 3]; // stale content must not leak into the next AU
        loop {
            match next_framed_au(buf, &mut a) {
                Framed::Au => aus.push(a.clone()),
                Framed::Skipped(n) => skipped += n,
                Framed::NeedMore => return skipped,
            }
        }
    }

    #[test]
    fn framed_split_anywhere_gives_the_same_aus() {
        let stream: Vec<u8> = [frame(5, 1), frame(0, 2), frame(300, 3)].concat();
        for cut in 0..=stream.len() {
            let (mut buf, mut aus) = (Vec::new(), Vec::new());
            buf.extend_from_slice(&stream[..cut]);
            let mut skipped = drain_all(&mut buf, &mut aus);
            buf.extend_from_slice(&stream[cut..]);
            skipped += drain_all(&mut buf, &mut aus);
            assert_eq!(skipped, 0, "cut {cut}");
            assert_eq!(aus, vec![vec![1; 5], vec![], vec![3; 300]], "cut {cut}");
            assert!(buf.is_empty());
        }
    }

    #[test]
    fn framed_resyncs_after_garbage_and_bad_length() {
        let mut buf = b"junk!".to_vec();
        buf.extend(frame(3, 7));
        let mut aus = Vec::new();
        assert_eq!(drain_all(&mut buf, &mut aus), 5);
        assert_eq!(aus, vec![vec![7; 3]]);
        // a header with an impossible length is skipped, the next real AU is found
        let mut buf = MAGIC.to_vec();
        buf.extend_from_slice(&u32::MAX.to_be_bytes());
        buf.extend(frame(2, 9));
        let mut aus = Vec::new();
        drain_all(&mut buf, &mut aus);
        assert_eq!(aus, vec![vec![9; 2]]);
    }

    #[test]
    fn framed_keeps_a_magic_split_across_reads() {
        let f = frame(4, 5);
        let mut buf = b"xx".to_vec();
        buf.extend_from_slice(&f[..2]); // "xxWF"
        let mut aus = Vec::new();
        drain_all(&mut buf, &mut aus);
        buf.extend_from_slice(&f[2..]);
        drain_all(&mut buf, &mut aus);
        assert_eq!(aus, vec![vec![5; 4]]);
    }

    #[test]
    fn audio_pts_is_continuous_near_the_clock() {
        let p = audio_pts_plan(Some(90_000), 1, 90_000 + 1000);
        assert_eq!((p.start, p.skip, p.action), (90_000, 0, None));
        // first series after silence: last chunk anchored on the clock
        let p = audio_pts_plan(None, 3, 90_000);
        assert_eq!(p.start, 90_000 - 1800);
    }

    #[test]
    fn audio_pts_jumps_forward_when_behind_and_drops_when_ahead() {
        // a 100 ms burst after a stall, series 60 ms behind the clock -> jump
        let p = audio_pts_plan(Some(100_000), 10, 100_000 + 8100 + 5400);
        assert_eq!(p.action, Some("jump"));
        assert_eq!(p.start as i64 + 9 * AUDIO_PTS_STEP, 100_000 + 8100 + 5400);
        // 50 ms ahead -> drop: ceil(4500/900) = 5 chunks wanted, only the 1 present is dropped
        let p = audio_pts_plan(Some(100_000), 1, 100_000 - 4500);
        assert_eq!((p.action, p.skip, p.start), (Some("drop"), 1, 100_000));
    }

    #[test]
    fn audio_pts_never_goes_backwards() {
        // pseudo-random series sizes and clock jitter; the next PTS must always be >= the last sent + step
        let (mut prev, mut last_sent): (Option<u64>, Option<u64>) = (None, None);
        let mut clock: i64 = 1_000_000;
        let mut seed: u64 = 12345;
        for _ in 0..20_000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let n = 1 + (seed >> 60) as usize % 10; // 1..=10 chunks
            let jitter = ((seed >> 40) % 9000) as i64 - 4500; // +-50 ms
            clock += AUDIO_PTS_STEP * n as i64 + jitter / 10;
            let plan = audio_pts_plan(prev, n, clock + jitter);
            let mut pts = plan.start;
            for _ in plan.skip..n {
                if let Some(l) = last_sent {
                    assert!(pts >= l + AUDIO_PTS_STEP as u64, "PTS went back: {pts} after {l}");
                }
                last_sent = Some(pts);
                pts += AUDIO_PTS_STEP as u64;
            }
            prev = Some(pts);
        }
    }

    #[test]
    fn padding_fills_a_thin_stream_to_the_target_rate() {
        let mut b = PadBudget::new(8000.0, 0.0);
        let mut total = 0usize;
        for i in 1..=600 {
            // 60 fps, 1 KiB AUs (static screen)
            let t = i as f64 / 60.0;
            total += 1024 + b.after_au(1024, t) * TS;
        }
        let rate_kbps = total as f64 * 8.0 / 10.0 / 1000.0;
        assert!((7800.0..=8100.0).contains(&rate_kbps), "{rate_kbps} kb/s");
    }

    #[test]
    fn padding_resumes_within_200ms_after_busy_content() {
        let mut b = PadBudget::new(8000.0, 0.0);
        let mut t = 0.0;
        for _ in 0..600 {
            // 10 s of 12 Mb/s video: always ahead, surplus capped
            t += 1.0 / 60.0;
            assert_eq!(b.after_au(25_000, t), 0);
        }
        // static screen again: padding must start within the carried 200 ms (+ one frame)
        let mut first_pad = None;
        for i in 1..=60 {
            if b.after_au(1024, t + i as f64 / 60.0) > 0 {
                first_pad = Some(i);
                break;
            }
        }
        let frames = first_pad.expect("padding never resumed");
        assert!(frames as f64 / 60.0 <= PAD_CARRY_S + 1.0 / 60.0 + 1e-9, "after {frames} frames");
    }

    #[test]
    fn same_rate_on_sighup_keeps_the_budget() {
        // busy content leaves a carried surplus; then a SIGHUP for another tuning file re-reads the same
        // mux-kbps: the surplus must stay (a reset would pad as if nothing had been sent)
        let run = |sighup: &dyn Fn(&mut PadBudget)| {
            let mut b = PadBudget::new(0.0, 0.0);
            assert!(b.set_kbps(8000.0, 0.0));
            assert_eq!(b.after_au(25_000, 0.01), 0);
            sighup(&mut b);
            b.after_au(1024, 0.05)
        };
        let untouched = run(&|_| {});
        let same = run(&|b| assert!(!b.set_kbps(8000.0, 0.02)));
        let reset = run(&|b| b.reset(8000.0, 0.02));
        assert_eq!(same, untouched);
        assert_ne!(reset, untouched, "the test must tell a reset apart");
        let mut b = PadBudget::new(8000.0, 0.0);
        assert!(b.set_kbps(6000.0, 0.06));
        assert_eq!(b.kbps, 6000.0);
    }

    #[test]
    fn audio_constants_come_from_one_definition() {
        assert_eq!(LPCM_CHUNK, 1920);
        assert_eq!(AUDIO_PTS_STEP, 900);
        assert_eq!(AUDIO_TICK, Duration::from_millis(10));
    }

    #[test]
    fn ticks27_round_trip() {
        assert_eq!(ticks27(Duration::from_millis(40)), 1_080_000);
        assert_eq!(ticks27_to_duration(1_080_000), Duration::from_millis(40));
        assert_eq!(ticks27_to_duration(27), Duration::from_micros(1));
        assert_eq!(ms_to_ticks27(1.5) as u64, 40_500);
    }

    #[test]
    fn padding_off_when_rate_is_zero() {
        let mut b = PadBudget::new(0.0, 0.0);
        assert_eq!(b.after_au(100, 10.0), 0);
    }

    #[test]
    fn keyframe_found_in_annex_b() {
        let idr = [0, 0, 0, 1, 0x09, 0xf0, 0, 0, 1, 0x65, 9, 9];
        assert!(is_keyframe(&idr));
        // forced key unit: SPS + PPS + non-IDR I slice
        let forced = [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x68, 3, 0, 0, 1, 0x21, 9];
        assert!(is_keyframe(&forced));
        let p = [0, 0, 0, 1, 0x41, 0x9a, 0, 0, 1, 0x06, 5];
        assert!(!is_keyframe(&p));
        assert!(!is_keyframe(&[]));
    }

    #[test]
    fn shorter_hold_drops_the_burst_and_waits_for_idr() {
        let ms = |m: u64| m * 27_000;
        // 60 fps, 21 AUs held 350 ms; now = newest + 0
        let now = ms(10_000);
        let arr: Vec<u64> = (0..21).map(|i| now - ms(350) + ms(i * 50 / 3)).collect();
        // off: everything is late
        assert_eq!(shorten_hold(&arr, 0, now), HoldPlan { drop: 21, wait_idr: true });
        // a few ms shorter: at most one is due, nothing to drop
        assert_eq!(shorten_hold(&arr, ms(345), now), HoldPlan { drop: 0, wait_idr: false });
        assert_eq!(shorten_hold(&[], 0, now), HoldPlan { drop: 0, wait_idr: false });
    }
}
