//! RTP out: TS packets, 7 per datagram (PT 33, RFC 2250), one UDP socket, counters for the RTCP
//! Sender Report.

use std::io::Read as _;
use std::net::{SocketAddr, UdpSocket};
use std::os::fd::AsRawFd;

use crate::ts::TS;

/// MP2T payload type (RFC 3551).
const PT_MP2T: u8 = 33;
const RTP_HEADER: usize = 12;
/// TS packets per RTP datagram: 7 * 188 + 12 fits a 1500-byte MTU.
const TS_PER_RTP: usize = 7;
/// DSCP AF41 (multimedia conferencing), as the IP TOS byte's upper 6 bits.
const DSCP: u8 = 34;

pub struct Rtp {
    sock: UdpSocket,
    dest: SocketAddr,
    seq: u16,
    pub ssrc: u32,
    /// sent so far, for the RTCP Sender Report (wrap like the RFC counters)
    pub packets: u32,
    pub octets: u32,
    /// one datagram, reused
    pkt: [u8; RTP_HEADER + TS_PER_RTP * TS],
    /// send_error rate limit: failures since the last line, Unix second of the last line
    fails: u64,
    fail_s: u64,
}

impl Rtp {
    /// An unbound-port UDP socket towards `dest`, marked DSCP AF41. Exits on failure (start-up).
    pub fn open(dest: SocketAddr) -> Rtp {
        let sock = UdpSocket::bind("0.0.0.0:0").unwrap_or_else(|e| wfd_util::die("rtp_bind_failed", &e.to_string()));
        let tos: libc::c_int = (DSCP as libc::c_int) << 2;
        // SAFETY: the socket fd is open; `tos` is a valid c_int and its size is passed.
        unsafe {
            libc::setsockopt(
                sock.as_raw_fd(),
                libc::IPPROTO_IP,
                libc::IP_TOS,
                &tos as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );
        }
        let r: [u8; 6] = random_bytes();
        Rtp {
            sock,
            dest,
            seq: u16::from_be_bytes([r[0], r[1]]),
            ssrc: u32::from_be_bytes([r[2], r[3], r[4], r[5]]),
            packets: 0,
            octets: 0,
            pkt: [0; RTP_HEADER + TS_PER_RTP * TS],
            fails: 0,
            fail_s: 0,
        }
    }

    /// TS bytes as RTP with timestamp `ts90`; returns the number of RTP packets sent (0 on an error,
    /// which is logged rate-limited).
    pub fn send(&mut self, ts_bytes: &[u8], ts90: u32) -> usize {
        match self.send_ts(ts_bytes, ts90) {
            Ok(n) => n,
            Err(e) => {
                self.log_send_error(&e);
                0
            }
        }
    }

    /// Stops at the first failing datagram (the rest of this AU is not sent; seq is not advanced for
    /// it, so the sink sees no gap in the RTP sequence, only missing TS).
    fn send_ts(&mut self, ts_bytes: &[u8], ts90: u32) -> std::io::Result<usize> {
        let mut n = 0;
        for chunk in ts_bytes.chunks(TS_PER_RTP * TS) {
            let len = rtp_packet(&mut self.pkt, self.seq, ts90, self.ssrc, chunk);
            self.sock.send_to(&self.pkt[..len], self.dest)?;
            self.seq = self.seq.wrapping_add(1);
            self.packets = self.packets.wrapping_add(1);
            self.octets = self.octets.wrapping_add(chunk.len() as u32);
            n += 1;
        }
        Ok(n)
    }

    /// At most one `send_error` line per second (a dead link fails every packet, 60+ times a second),
    /// with the number of failures since the last line.
    fn log_send_error(&mut self, e: &std::io::Error) {
        self.fails += 1;
        let now_s = wfd_util::unix_us() / 1_000_000;
        if self.fail_s != now_s {
            self.fail_s = now_s;
            let n = self.fails;
            self.fails = 0;
            wfd_util::event!("send_error", "error" => e.to_string().as_str(), "count" => n);
        }
    }
}

/// Writes one RTP datagram (version 2, PT 33, `payload` after the header) into `pkt`; returns its length.
fn rtp_packet(pkt: &mut [u8], seq: u16, ts90: u32, ssrc: u32, payload: &[u8]) -> usize {
    pkt[..2].copy_from_slice(&[0x80, PT_MP2T]);
    pkt[2..4].copy_from_slice(&seq.to_be_bytes());
    pkt[4..8].copy_from_slice(&ts90.to_be_bytes());
    pkt[8..12].copy_from_slice(&ssrc.to_be_bytes());
    pkt[RTP_HEADER..RTP_HEADER + payload.len()].copy_from_slice(payload);
    RTP_HEADER + payload.len()
}

/// Random bytes from /dev/urandom (SSRC and initial sequence number, RFC 3550 §5.1); if that fails,
/// mixed from the clocks (still differs between sessions).
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_ok() {
        return b;
    }
    let mut x = wfd_util::unix_us() ^ (wfd_util::mono_us() as u64).rotate_left(32) ^ 0x5753_4d43; // "WSMC"
    for v in b.iter_mut() {
        // splitmix64 step
        x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        *v = (z ^ (z >> 31)) as u8;
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtp_header_layout() {
        let mut pkt = [0u8; RTP_HEADER + TS_PER_RTP * TS];
        let n = rtp_packet(&mut pkt, 0x1234, 0xdead_beef, 0x0102_0304, &[0x47; TS]);
        assert_eq!(n, RTP_HEADER + TS);
        assert_eq!(&pkt[..12], &[0x80, 33, 0x12, 0x34, 0xde, 0xad, 0xbe, 0xef, 1, 2, 3, 4]);
        assert_eq!(pkt[12], 0x47);
    }

    #[test]
    fn random_bytes_differ() {
        // 8 random bytes equal twice in a row: 2^-64
        assert_ne!(random_bytes::<8>(), random_bytes::<8>());
    }
}
