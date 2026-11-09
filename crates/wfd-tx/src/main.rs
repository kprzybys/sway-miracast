//! wfd-tx: H.264 access units (+ LPCM audio) -> MPEG-TS -> RTP (PT 33) -> UDP.
//!
//! One job: the WFD data plane after the encoder. No queues: an access unit is muxed and sent the moment
//! it is complete. Input is framed: wfd-media --framed writes "WFD1" + u32 BE length before each AU (exact
//! boundaries). Audio: 10 ms LPCM chunks from `--audio`, PTS never backwards.
//! Constant TS rate with null packets (`--mux-kbps-file`); a PCR-only packet every 40 ms while no video comes.
//! "Film" mode (`--video-hold-file` + SIGHUP): each AU is sent that long after it arrived, so picture and
//! audio leave the sink together (the sink ignores video PTS and buffers audio ~0.45 s). Shortening the
//! hold drops the late AUs and waits for an IDR (send SIGUSR1 to wfd-media).
//!
//! Timing: PCR = sender clock at send time; PTS = sender clock when the AU arrived + hold.
//!
//! Per-frame log (JSON lines, stderr, not with --quiet): frame, bytes, t_in/t_sent (Unix µs), rtp, pts_minus_pcr_ms.
//!
//! Usage: wfd-tx --dest IP:PORT [--quiet] [--in FIFO] [--video-hold-file FILE]
//!               [--audio FIFO] [--audio-delay-file FILE] [--mux-kbps-file FILE]
//!               [--rtcp-port 19101] [--rtcp-log FILE]
//! --in FIFO: read from a named pipe opened read-write and non-blocking (feeders come and go, no EOF;
//!            the RTP/TS stream and the PCR stay continuous). Without --in: stdin, the program ends with it.
//! --video-hold-file, --audio-delay-file, --mux-kbps-file: one number each (ms, ms, kb/s), read at start
//!            and again on SIGHUP. Missing: hold 0, audio delay 0, no padding.
//! --rtcp-port: our RTCP port (= server_port + 1 from SETUP); receives the sink's Receiver Reports and sends a
//!              Sender Report every second to the sink's RTP port + 1. --rtcp-log: one JSON line per report.
//! --framed: accepted and ignored (framed input is the only mode).

mod audio;
mod logic;
mod net;
mod opts;
mod rtcp;
mod rtcp_io;
mod ts;
mod video;

use std::net::{SocketAddr, ToSocketAddrs};
use std::os::fd::{AsFd, AsRawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use audio::Audio;
use logic::{PadBudget, PCR_PER_PTS};
use net::Rtp;
use opts::Opts;
use rtcp_io::Rtcp;
use video::Video;

static HUP: AtomicBool = AtomicBool::new(false);

/// The video pipe is enlarged to this, so a large IDR fits in one write of the encoder.
const PIPE_SIZE: libc::c_int = 1 << 20;
/// The 27 MHz clock starts here (1 s), so timestamps never start at 0.
const CLK27_START: u64 = 27_000_000;

/// The sender clock: 27 MHz since start (PCR, and PTS via / 300).
pub struct Clock {
    t0: Instant,
}

impl Clock {
    pub fn now27(&self) -> u64 {
        logic::ticks27(self.t0.elapsed()) + CLK27_START
    }

    /// Seconds since start (the padding budget's time).
    pub fn secs(&self) -> f64 {
        self.t0.elapsed().as_secs_f64()
    }
}

/// The one TS/RTP stream both sides write into.
pub struct Stream {
    pub mux: ts::Muxer,
    pub pad: PadBudget,
    pub rtp: Rtp,
}

impl Stream {
    /// TS bytes as RTP stamped with `pcr` (27 MHz, as 90 kHz); returns the number of RTP packets.
    pub fn send(&mut self, out: &[u8], pcr: u64) -> usize {
        self.rtp.send(out, (pcr / PCR_PER_PTS) as u32)
    }
}

/// The whole sender: one thread, one poll loop over the video and audio FIFOs and the RTCP socket.
struct Tx {
    o: Opts,
    clk: Clock,
    stream: Stream,
    video: Video,
    audio: Option<Audio>,
    rtcp: Option<Rtcp>,
}

fn main() {
    let o = match opts::parse(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    wfd_util::install_panic_hook("wfd-tx");
    wfd_util::signal_flag(libc::SIGHUP, &HUP);
    // apply the tuning files at start, as if SIGHUP
    HUP.store(
        o.audio_delay_file.is_some() || o.mux_kbps_file.is_some() || o.video_hold_file.is_some(),
        Ordering::Relaxed,
    );
    let mut tx = Tx::new(o);
    while tx.step() {}
}

/// Reads a FIFO opened read-write + non-blocking: never blocks waiting for a writer and never sees
/// EOF when the writer restarts, so audio and PCR keep flowing meanwhile. Exits on failure (start-up).
fn open_fifo(p: &str, what: &str) -> std::fs::File {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(p)
        .unwrap_or_else(|e| wfd_util::die("open_failed", &format!("{what} {p}: {e}")))
}

/// --dest resolved once (not on every send). Exits on failure (start-up).
fn resolve(dest: &str) -> SocketAddr {
    match dest.to_socket_addrs().map(|mut a| a.next()) {
        Ok(Some(a)) => a,
        Ok(None) => wfd_util::die("dest_invalid", &format!("{dest}: no address")),
        Err(e) => wfd_util::die("dest_invalid", &format!("{dest}: {e}")),
    }
}

impl Tx {
    fn new(o: Opts) -> Self {
        let input = match &o.input {
            Some(p) => open_fifo(p, "--in"),
            None => std::fs::File::from(
                std::io::stdin()
                    .as_fd()
                    .try_clone_to_owned()
                    .unwrap_or_else(|e| wfd_util::die("open_failed", &format!("stdin: {e}"))),
            ),
        };
        // SAFETY: the input fd is open; F_SETPIPE_SZ only resizes the pipe buffer (fails harmlessly on non-pipes).
        unsafe {
            libc::fcntl(input.as_raw_fd(), libc::F_SETPIPE_SZ, PIPE_SIZE);
        }
        let now = Instant::now();
        let dest = resolve(&o.dest);
        let audio = o.audio.as_ref().map(|p| Audio::new(open_fifo(p, "--audio"), now));
        let rtcp = o.rtcp_port.and_then(|port| Rtcp::open(port, dest, o.rtcp_log.as_deref()));
        let mut mux = ts::Muxer::new();
        mux.audio = audio.is_some();
        Tx {
            clk: Clock { t0: now },
            stream: Stream { mux, pad: PadBudget::new(0.0, 0.0), rtp: Rtp::open(dest) },
            video: Video::new(input, o.input.is_none(), now),
            audio,
            rtcp,
            o,
        }
    }

    /// One turn of the loop; false when stdin has ended.
    fn step(&mut self) -> bool {
        if HUP.swap(false, Ordering::Relaxed) {
            self.apply_hup();
        }
        if self.video.finished() {
            return false; // stdin ended (a FIFO opened read-write never reports EOF)
        }
        let (video_ready, audio_ready, rtcp_ready) = match self.poll() {
            Some(r) => r,
            None => return true, // EINTR (SIGHUP)
        };
        if rtcp_ready {
            if let Some(r) = self.rtcp.as_mut() {
                r.read();
            }
        }
        if let Some(r) = self.rtcp.as_mut() {
            let clk = &self.clk;
            r.send_sr_if_due(&self.stream.rtp, || (clk.now27() / PCR_PER_PTS) as u32);
        }
        if audio_ready {
            if let Some(a) = self.audio.as_mut() {
                a.read();
            }
        }
        if let Some(a) = self.audio.as_mut() {
            a.send(&mut self.stream, &self.clk, self.o.quiet);
        }
        if video_ready {
            self.video.read(&self.clk);
        }
        self.video.cut_aus(&self.clk);
        self.video.send_due(&mut self.stream, &self.clk, self.o.quiet);
        if self.video.pcr_due() {
            self.video.send_pcr_only(&mut self.stream, &self.clk);
        }
        true
    }

    /// SIGHUP: re-read the live tuning files.
    fn apply_hup(&mut self) {
        let read = |p: &Option<String>| -> Option<f64> {
            std::fs::read_to_string(p.as_ref()?).ok().and_then(|s| s.trim().parse::<f64>().ok())
        };
        if let Some(k) = read(&self.o.mux_kbps_file) {
            // only a real change restarts the budget: SIGHUP also comes for the other tuning files
            if self.stream.pad.set_kbps(k, self.clk.secs()) {
                wfd_util::event!("mux_kbps", "kbps" => k);
            }
        }
        if let Some(ms) = read(&self.o.video_hold_file) {
            self.video.set_hold(ms.clamp(0.0, logic::MAX_HOLD_MS), &self.clk);
        }
        if let (Some(ms), Some(a)) = (read(&self.o.audio_delay_file), self.audio.as_mut()) {
            a.set_delay_ms(ms);
        }
    }

    /// Waits for video, audio or RTCP input, or the next deadline. None after EINTR.
    // Note: any other poll error (EINVAL, ENOMEM) also returns None, and the loop would retry at once;
    // not seen in practice, left as it was.
    fn poll(&self) -> Option<(bool, bool, bool)> {
        let mut fds = [
            libc::pollfd { fd: self.video.poll_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd {
                fd: self.audio.as_ref().map(|a| a.fifo.as_raw_fd()).unwrap_or(-1),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: self.rtcp.as_ref().map(|r| r.sock.as_raw_fd()).unwrap_or(-1),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let now = Instant::now();
        let mut wait: Option<Duration> = None;
        let mut wake = |t: Instant| {
            let d = t.saturating_duration_since(now);
            wait = Some(wait.map_or(d, |w: Duration| w.min(d)));
        };
        if let Some(a) = &self.audio {
            wake(a.deadline(now));
        }
        if let Some(r) = &self.rtcp {
            wake(r.next_sr);
        }
        self.video.deadlines(now, &self.clk, &mut wake);
        let timeout_ms = wait.map_or(-1, |w| w.as_micros().div_ceil(1000) as i32);
        // SAFETY: `fds` is a valid array of 3 pollfds for the duration of the call.
        if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) } < 0 {
            return None;
        }
        Some((
            fds[0].revents & (libc::POLLIN | libc::POLLHUP) != 0,
            fds[1].revents & libc::POLLIN != 0,
            fds[2].revents & libc::POLLIN != 0,
        ))
    }
}
