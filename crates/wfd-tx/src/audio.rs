//! Audio side: 10 ms LPCM chunks from the FIFO, paced by the source clock, PTS never backwards.

use std::io::Read as _;
use std::time::{Duration, Instant};

use crate::logic::{self, AUDIO_PTS_STEP, AUDIO_TICK, LPCM_CHUNK, PCR_PER_PTS};
use crate::ts::TS;
use crate::{Clock, Stream};

/// Silence is sent only this long after the last real audio (PipeWire suspends idle sinks).
pub const SILENCE_AFTER: Duration = Duration::from_millis(60);
/// A burst after a stall keeps only the newest 100 ms of audio.
const AUDIO_MAX_BACKLOG: usize = 10 * LPCM_CHUNK;
/// One 48 kHz s16 stereo sample frame: the backlog is trimmed on frame boundaries.
const FRAME_BYTES: usize = 4;
/// Stats line interval (not with --quiet).
const STATS_EVERY: Duration = Duration::from_secs(2);
/// FIFO read size.
const READ_CHUNK: usize = 1 << 16;
/// TS bytes of one LPCM chunk (PES of 1940 bytes in 11 packets), rounded up.
const OUT_CAP: usize = (LPCM_CHUNK / TS + 2) * TS;
static SILENCE: [u8; LPCM_CHUNK] = [0; LPCM_CHUNK];

pub struct Audio {
    pub fifo: std::fs::File,
    /// bytes read, not yet sent
    buf: Vec<u8>,
    /// read scratch
    tmp: Vec<u8>,
    /// TS scratch for one chunk
    out: Vec<u8>,
    /// PTS the next chunk gets (None: re-anchor on the clock)
    pts: Option<u64>,
    /// from --audio-delay-file (27 MHz ticks, may be negative)
    pub delay27: i64,
    pub next_tick: Instant,
    pub last_real: Instant,
    last_emit: Instant,
    last_stats: Instant,
    real: u64,
    silence: u64,
    bursts: u64,
    max_gap: Duration,
}

impl Audio {
    pub fn new(fifo: std::fs::File, now: Instant) -> Audio {
        Audio {
            fifo,
            buf: Vec::with_capacity(AUDIO_MAX_BACKLOG + READ_CHUNK),
            tmp: vec![0u8; READ_CHUNK],
            out: Vec::with_capacity(OUT_CAP),
            pts: None,
            delay27: 0,
            next_tick: now + AUDIO_TICK,
            last_real: now,
            last_emit: now,
            last_stats: now,
            real: 0,
            silence: 0,
            bursts: 0,
            max_gap: Duration::ZERO,
        }
    }

    /// New delay from the tuning file. Re-anchors only on a real change: every PTS jump forward grows
    /// the sink's audio buffer for good (sinks/ Q11), and SIGHUP also comes for the other tuning files.
    pub fn set_delay_ms(&mut self, ms: f64) {
        let d = logic::ms_to_ticks27(ms) as i64;
        if d != self.delay27 {
            self.delay27 = d;
            self.pts = None;
            wfd_util::event!("audio_delay", "ms" => ms);
        }
    }

    /// When the loop must wake for audio: the silence filler is due only SILENCE_AFTER after the last
    /// real audio; waking at next_tick before that would poll(0) in a loop until then.
    pub fn deadline(&self, now: Instant) -> Instant {
        if now < self.last_real + SILENCE_AFTER {
            self.next_tick.max(self.last_real + SILENCE_AFTER + Duration::from_millis(1))
        } else {
            self.next_tick
        }
    }

    pub fn read(&mut self) {
        loop {
            match self.fifo.read(&mut self.tmp) {
                Ok(0) => break,
                Ok(n) => self.buf.extend_from_slice(&self.tmp[..n]),
                Err(_) => break, // WouldBlock
            }
        }
        if self.buf.len() > AUDIO_MAX_BACKLOG {
            // a burst after a stall: keep the newest 100 ms (frame aligned)
            let drop = (self.buf.len() - AUDIO_MAX_BACKLOG) / FRAME_BYTES * FRAME_BYTES;
            self.buf.drain(..drop);
        }
    }

    /// Paced by the source clock: send whenever 10 ms of samples are there (no drift against our clock,
    /// no inserted gaps). Silence only when the source has really stopped: then keep the 10 ms cadence
    /// so the sink stays fed.
    pub fn send(&mut self, s: &mut Stream, clk: &Clock, quiet: bool) {
        let n_real = self.buf.len() / LPCM_CHUNK;
        if n_real > 0 {
            self.real += n_real as u64;
            self.last_real = Instant::now();
        }
        let silence = n_real == 0 && self.last_real.elapsed() > SILENCE_AFTER && Instant::now() >= self.next_tick;
        if silence {
            self.silence += 1;
            self.pts = None; // restart the PTS run after a gap
        }
        let n = if silence { 1 } else { n_real };
        if n == 0 {
            self.log_stats(quiet);
            return;
        }
        self.max_gap = self.max_gap.max(self.last_emit.elapsed());
        if n > 1 {
            self.bursts += 1;
        }
        self.last_emit = Instant::now();
        // PTS for the whole series at once, never backwards (logic::audio_pts_plan)
        let now90 = (clk.now27() as i64 + self.delay27).max(0) / PCR_PER_PTS as i64;
        let plan = logic::audio_pts_plan(self.pts, n, now90);
        if let Some(action) = plan.action {
            wfd_util::event!(
                "audio_resync",
                "drift_ms" => logic::round1(plan.drift as f64 / 90.0),
                "n" => n,
                "action" => action,
                "dropped" => plan.skip,
            );
        }
        let bytes = if silence { &SILENCE[..] } else { &self.buf[..n * LPCM_CHUNK] };
        let mut pts90 = plan.start;
        for chunk in bytes.as_chunks::<LPCM_CHUNK>().0.iter().skip(plan.skip) {
            self.out.clear();
            let pcr = clk.now27();
            s.mux.write_lpcm(chunk, pts90, &mut self.out);
            pts90 += AUDIO_PTS_STEP as u64;
            s.pad.add(self.out.len());
            s.send(&self.out, pcr);
        }
        if !silence {
            self.buf.drain(..n * LPCM_CHUNK);
        }
        self.pts = Some(pts90);
        self.next_tick = Instant::now() + AUDIO_TICK;
        self.log_stats(quiet);
    }

    fn log_stats(&mut self, quiet: bool) {
        if !quiet && self.last_stats.elapsed() >= STATS_EVERY {
            wfd_util::event!(
                "audio",
                "real" => self.real,
                "silence" => self.silence,
                "max_gap_ms" => logic::round1(self.max_gap.as_secs_f64() * 1000.0),
                "bursts" => self.bursts,
            );
            (self.real, self.silence, self.bursts, self.max_gap) = (0, 0, 0, Duration::ZERO);
            self.last_stats = Instant::now();
        }
    }
}
