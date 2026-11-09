//! RTCP I/O: the sink's Receiver Reports in, our Sender Report out every second (rtcp.rs is the codec).

use std::io::Write as _;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use wfd_util::unix_us;

use crate::net::Rtp;
use crate::rtcp;

const SR_EVERY: Duration = Duration::from_secs(1);
const RTCP_SUMMARY_EVERY: Duration = Duration::from_secs(60);
/// SDES CNAME of our Sender Reports.
const SR_CNAME: &str = "wfd-tx@sway-miracast";
/// Largest RTCP datagram we read (one MTU).
const RTCP_MAX: usize = 1500;

pub struct Rtcp {
    pub sock: UdpSocket,
    dest: SocketAddr,
    pub next_sr: Instant,
    log: Option<std::fs::File>,
    reports: u64,
    /// worst jitter (90 kHz units) and any loss since the last summary line
    max_jitter: u32,
    lost_seen: bool,
    last_summary: Instant,
}

impl Rtcp {
    /// Our RTCP socket on `port`; the sink's RTCP port is its RTP port + 1 (RFC 3550 §11). None (logged)
    /// when the port cannot be bound; RTCP is optional.
    pub fn open(port: u16, rtp_dest: SocketAddr, log: Option<&str>) -> Option<Rtcp> {
        let Some(peer_port) = rtp_dest.port().checked_add(1) else {
            wfd_util::event!("rtcp_bad_peer_port", "port" => rtp_dest.port());
            return None;
        };
        let sock = match UdpSocket::bind(("0.0.0.0", port)) {
            Ok(s) => s,
            Err(e) => {
                wfd_util::event!("rtcp_bind_failed", "port" => port, "error" => e.to_string().as_str());
                return None;
            }
        };
        sock.set_nonblocking(true).ok()?;
        let mut dest = rtp_dest;
        dest.set_port(peer_port);
        let now = Instant::now();
        Some(Rtcp {
            sock,
            dest,
            next_sr: now,
            log: log.and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok()),
            reports: 0,
            max_jitter: 0,
            lost_seen: false,
            last_summary: now,
        })
    }

    /// Sender Report: maps our RTP clock (`rtp_ts`, PCR / 300 now) to wall-clock time, so the sink's
    /// reports carry LSR/DLSR and give us the round trip.
    pub fn send_sr_if_due(&mut self, rtp: &Rtp, rtp_ts: impl FnOnce() -> u32) {
        let now = Instant::now();
        if now < self.next_sr {
            return;
        }
        self.next_sr = now + SR_EVERY;
        let ntp = rtcp::ntp_from_unix_us(unix_us());
        let sr = rtcp::sender_report(rtp.ssrc, ntp, rtp_ts(), rtp.packets, rtp.octets, SR_CNAME);
        let _ = self.sock.send_to(&sr, self.dest);
    }

    /// The sink's Receiver Reports: every one to --rtcp-log, the first and a summary each minute (or on
    /// loss) to stderr.
    pub fn read(&mut self) {
        let mut buf = [0u8; RTCP_MAX];
        while let Ok((n, _)) = self.sock.recv_from(&mut buf) {
            let Some(rep) = rtcp::parse(&buf[..n]) else { continue };
            let now_us = unix_us();
            let ntp = rtcp::ntp_from_unix_us(now_us);
            for b in &rep.blocks {
                let rtt = rtcp::rtt_ms(ntp, b.lsr, b.dlsr);
                let rtt_s = rtt.map_or("null".into(), |x| format!("{x:.1}"));
                if let Some(f) = self.log.as_mut() {
                    // the log file's own line format (no "event"; tools/latency_track.py reads it)
                    let _ = writeln!(
                        f,
                        "{{\"t\":{now_us},\"lost_frac\":{},\"lost\":{},\"seq\":{},\"jitter_ms\":{:.2},\"rtt_ms\":{rtt_s}}}",
                        b.fraction_lost,
                        b.cum_lost,
                        b.ext_seq,
                        b.jitter as f64 / 90.0
                    );
                }
                self.max_jitter = self.max_jitter.max(b.jitter);
                self.lost_seen |= b.fraction_lost > 0;
            }
            self.reports += 1;
            if self.reports == 1 {
                wfd_util::event!("rtcp_first", "tool" => rep.tool.as_deref().unwrap_or(""), "blocks" => rep.blocks.len());
            }
            if self.lost_seen || self.last_summary.elapsed() >= RTCP_SUMMARY_EVERY {
                wfd_util::event!(
                    "rtcp",
                    "t" => now_us,
                    "reports" => self.reports,
                    "max_jitter_ms" => crate::logic::round2(self.max_jitter as f64 / 90.0),
                    "loss" => self.lost_seen,
                );
                (self.max_jitter, self.lost_seen, self.last_summary) = (0, false, Instant::now());
            }
        }
    }
}
