//! Video side: framed AUs from the FIFO ("WFD1" + length + AU), the "film" hold queue, TS out with
//! tables, PCR and null-packet padding, and PCR-only packets while no AU comes.

use std::collections::VecDeque;
use std::io::Read as _;
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use wfd_util::unix_us;

use crate::logic::{self, Framed, PCR_PER_PTS, TICKS27_PER_MS};
use crate::ts;
use crate::{Clock, Stream};

/// A partial framed AU older than this is a dead writer's leftover.
const STALE: Duration = Duration::from_millis(200);
/// H.222.0 wants a PCR at least every 100 ms; send one alone when no AU came for this long.
pub const PCR_EVERY: Duration = Duration::from_millis(40);
/// PAT + PMT before every this many AUs.
const TABLES_EVERY: u64 = 3;
/// FIFO read size.
const READ_CHUNK: usize = 1 << 16;
/// Initial capacity of the input and TS buffers (a 1080p IDR is well under this).
const AU_BUF: usize = 1 << 20;
/// Sent AU buffers kept for reuse (no allocation per AU once they have grown).
const SPARE_MAX: usize = 4;
/// PCR-only packets in a row before the next AU is logged as "video_resumed".
const RESUMED_AFTER_PCR_ONLY: u64 = 3;

/// A complete AU waiting for its send time.
struct Held {
    au: Vec<u8>,
    /// arrival clock (27 MHz) and Unix time
    c_in: u64,
    t_in: u64,
}

pub struct Video {
    input: std::fs::File,
    /// input is stdin (blocking, may end) rather than a FIFO opened read-write
    stdin: bool,
    eof: bool,
    /// bytes not yet cut into AUs
    buf: Vec<u8>,
    /// read scratch
    tmp: Vec<u8>,
    /// TS scratch for one AU (or one PCR-only packet)
    out: Vec<u8>,
    t_in: u64,
    c_in: u64,
    last_byte: Instant,
    ready: VecDeque<Held>,
    spare: Vec<Vec<u8>>,
    /// "film" mode: AUs leave `hold27` after they arrived (0: at once)
    hold27: u64,
    /// AUs were dropped (hold shortened): send nothing until an IDR
    wait_idr: bool,
    frame: u64,
    last_vid: Instant,
    /// PCR-only packets sent since the last AU
    pcr_gap: u64,
}

impl Video {
    pub fn new(input: std::fs::File, stdin: bool, now: Instant) -> Video {
        Video {
            input,
            stdin,
            eof: false,
            buf: Vec::with_capacity(AU_BUF),
            tmp: vec![0u8; READ_CHUNK],
            out: Vec::with_capacity(AU_BUF),
            t_in: 0,
            c_in: 0,
            last_byte: now,
            ready: VecDeque::new(),
            spare: Vec::with_capacity(SPARE_MAX),
            hold27: 0,
            wait_idr: false,
            frame: 0,
            last_vid: now,
            pcr_gap: 0,
        }
    }

    /// stdin ended and everything read has been sent (a FIFO opened read-write never reports EOF).
    pub fn finished(&self) -> bool {
        self.eof && self.buf.is_empty() && self.ready.is_empty()
    }

    /// The fd to poll (-1 after EOF).
    pub fn poll_fd(&self) -> libc::c_int {
        if self.eof {
            -1
        } else {
            self.input.as_raw_fd()
        }
    }

    /// Wake-ups the loop owes the video side: dropping a stale partial AU, the next PCR-only packet,
    /// the oldest held AU.
    pub fn deadlines(&self, now: Instant, clk: &Clock, mut wake: impl FnMut(Instant)) {
        if !self.buf.is_empty() {
            wake(self.last_byte + STALE);
        }
        if self.frame > 0 {
            wake(self.last_vid + PCR_EVERY);
        }
        if let Some(h) = self.ready.front() {
            // the oldest held AU is due at arrival + hold
            let left = (h.c_in + self.hold27).saturating_sub(clk.now27());
            wake(now + logic::ticks27_to_duration(left));
        }
    }

    pub fn read(&mut self, clk: &Clock) {
        loop {
            match self.input.read(&mut self.tmp) {
                Ok(0) => {
                    self.eof = self.stdin;
                    break;
                }
                Ok(n) => {
                    if self.buf.is_empty() {
                        self.t_in = unix_us();
                        self.c_in = clk.now27();
                    }
                    self.buf.extend_from_slice(&self.tmp[..n]);
                    self.last_byte = Instant::now();
                    if self.stdin {
                        break; // stdin is blocking: one read per wake-up
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    wfd_util::event!("input_error", "error" => e.to_string().as_str());
                    self.eof = true;
                    break;
                }
            }
        }
    }

    /// Complete AUs from the input buffer into the hold queue: "WFD1" + u32 BE length + AU
    /// (logic::next_framed_au); resync on the magic after a writer died mid-AU.
    pub fn cut_aus(&mut self, clk: &Clock) {
        loop {
            let mut au = self.spare.pop().unwrap_or_default();
            match logic::next_framed_au(&mut self.buf, &mut au) {
                Framed::Au => {
                    self.ready.push_back(Held { au, c_in: self.c_in, t_in: self.t_in });
                    if !self.buf.is_empty() {
                        // the rest came with the same read
                        self.t_in = unix_us();
                        self.c_in = clk.now27();
                    }
                }
                Framed::Skipped(n) => {
                    self.spare.push(au);
                    wfd_util::event!("input_resync", "skipped" => n);
                }
                Framed::NeedMore => {
                    self.spare.push(au);
                    break;
                }
            }
        }
        if !self.buf.is_empty() && self.last_byte.elapsed() >= STALE {
            // the encoder died mid-AU (restart): a new one starts with a fresh header
            wfd_util::event!("partial_au_dropped", "bytes" => self.buf.len());
            self.buf.clear();
        }
    }

    /// New "film" hold. Longer: the picture pauses once for the difference. Shorter: the AUs now late
    /// are dropped (logic::shorten_hold) and sending resumes on the next IDR.
    pub fn set_hold(&mut self, ms: f64, clk: &Clock) {
        let new = logic::ms_to_ticks27(ms) as u64;
        let mut dropped = 0;
        if new < self.hold27 {
            let arrivals: Vec<u64> = self.ready.iter().map(|h| h.c_in).collect();
            let plan = logic::shorten_hold(&arrivals, new, clk.now27());
            for h in self.ready.drain(..plan.drop) {
                recycle(&mut self.spare, h.au);
            }
            self.wait_idr |= plan.wait_idr;
            dropped = plan.drop;
        }
        self.hold27 = new;
        wfd_util::event!("video_hold", "ms" => ms, "dropped" => dropped, "wait_idr" => self.wait_idr);
    }

    /// Sends the queued AUs whose hold has passed; while waiting for an IDR, drops the others.
    pub fn send_due(&mut self, s: &mut Stream, clk: &Clock, quiet: bool) {
        while let Some(h) = self.ready.front() {
            if h.c_in + self.hold27 > clk.now27() {
                break;
            }
            let Some(h) = self.ready.pop_front() else { break };
            if self.wait_idr {
                if !logic::is_keyframe(&h.au) {
                    recycle(&mut self.spare, h.au);
                    continue;
                }
                self.wait_idr = false;
                wfd_util::event!("video_idr_resume");
            }
            self.send_au(&h, s, clk, quiet);
            recycle(&mut self.spare, h.au);
        }
    }

    fn send_au(&mut self, h: &Held, s: &mut Stream, clk: &Clock, quiet: bool) {
        self.out.clear();
        if self.frame.is_multiple_of(TABLES_EVERY) {
            s.mux.write_tables(&mut self.out);
        }
        let pcr = clk.now27();
        // PTS from the send time (arrival + hold), so pts - pcr stays the same in film mode too
        let pts90 = (h.c_in + self.hold27) / PCR_PER_PTS;
        s.mux.write_au(&h.au, pts90, pcr / PCR_PER_PTS, (pcr % PCR_PER_PTS) as u16, &mut self.out);
        // constant mux rate: null packets top the stream up to the target (logic::PadBudget)
        let npk = s.pad.after_au(self.out.len(), clk.secs());
        ts::write_null(npk, &mut self.out);
        let pkts = s.send(&self.out, pcr);
        if !quiet {
            // hand-written: the per-frame line has no "event" key (analysis scripts match
            // lines starting with {"frame")
            eprintln!(
                "{{\"frame\":{},\"bytes\":{},\"t_in\":{},\"t_sent\":{},\"rtp\":{pkts},\"pts_minus_pcr_ms\":{:.1}}}",
                self.frame,
                h.au.len(),
                h.t_in,
                unix_us(),
                (pts90 as f64 * PCR_PER_PTS as f64 - pcr as f64) / TICKS27_PER_MS
            );
        }
        self.frame += 1;
        self.last_vid = Instant::now();
        if self.pcr_gap >= RESUMED_AFTER_PCR_ONLY {
            wfd_util::event!("video_resumed", "pcr_only" => self.pcr_gap);
        }
        self.pcr_gap = 0;
    }

    /// A PCR-only packet is due: video has started and none was sent for PCR_EVERY.
    pub fn pcr_due(&self) -> bool {
        self.frame > 0 && self.last_vid.elapsed() >= PCR_EVERY
    }

    /// No video (encoder restart): keep the PCR going so the sink's clock does not stall.
    pub fn send_pcr_only(&mut self, s: &mut Stream, clk: &Clock) {
        self.out.clear();
        let pcr = clk.now27();
        s.mux.write_pcr_only(pcr / PCR_PER_PTS, (pcr % PCR_PER_PTS) as u16, &mut self.out);
        s.pad.add(self.out.len());
        s.send(&self.out, pcr);
        self.last_vid = Instant::now();
        self.pcr_gap += 1;
    }
}

/// Keeps a sent or dropped AU's buffer for the next one.
fn recycle(spare: &mut Vec<Vec<u8>>, au: Vec<u8>) {
    if spare.len() < SPARE_MAX {
        spare.push(au);
    }
}
