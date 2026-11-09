//! RTCP (RFC 3550) for the one RTP stream: parse the sink's Receiver Reports, build our Sender Report.
//! Pure functions (no I/O, time as arguments). The Samsung sink sends RR + SDES (TOOL "GStreamer") about
//! once a second to our `server_port` + 1 (sinks/ profile, data/probe/capabilities.md §6).

/// Seconds from 1900-01-01 (NTP) to 1970-01-01 (Unix).
const NTP_UNIX_OFFSET: u64 = 2_208_988_800;
const PT_SR: u8 = 200;
const PT_RR: u8 = 201;
const PT_SDES: u8 = 202;
const SDES_CNAME: u8 = 1;
const SDES_TOOL: u8 = 6;

/// One report block of a Receiver Report: how the sink sees one of our streams.
#[derive(Debug, PartialEq)]
pub struct ReportBlock {
    pub ssrc: u32,
    /// fraction of packets lost since the previous report (/256)
    pub fraction_lost: u8,
    /// cumulative packets lost (24-bit signed; negative = duplicates)
    pub cum_lost: i32,
    /// extended highest sequence number received
    pub ext_seq: u32,
    /// interarrival jitter in RTP timestamp units (90 kHz)
    pub jitter: u32,
    /// middle 32 bits of the NTP time of our last SR (0: none received)
    pub lsr: u32,
    /// delay since that SR, 1/65536 s
    pub dlsr: u32,
}

/// What one compound RTCP packet from the sink told us.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub sender_ssrc: u32,
    pub blocks: Vec<ReportBlock>,
    pub cname: Option<String>,
    pub tool: Option<String>,
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// Parses a compound RTCP packet (RR, SR, SDES are read, other types skipped). None when it is not
/// RTCP version 2 or a length field runs past the datagram.
pub fn parse(buf: &[u8]) -> Option<Report> {
    let mut r = Report::default();
    let mut seen = false;
    let mut p = buf;
    while p.len() >= 4 {
        if p[0] >> 6 != 2 {
            return None;
        }
        let count = (p[0] & 0x1f) as usize;
        let len = (u16::from_be_bytes([p[2], p[3]]) as usize + 1) * 4;
        if len > p.len() {
            return None;
        }
        let body = &p[4..len];
        match p[1] {
            PT_RR | PT_SR if body.len() >= 4 => {
                r.sender_ssrc = be32(body);
                let first = if p[1] == PT_SR { 24 } else { 4 };
                for i in 0..count {
                    let b = body.get(first + i * 24..first + (i + 1) * 24)?;
                    let lost = u32::from_be_bytes([0, b[5], b[6], b[7]]);
                    r.blocks.push(ReportBlock {
                        ssrc: be32(b),
                        fraction_lost: b[4],
                        cum_lost: ((lost << 8) as i32) >> 8,
                        ext_seq: be32(&b[8..]),
                        jitter: be32(&b[12..]),
                        lsr: be32(&b[16..]),
                        dlsr: be32(&b[20..]),
                    });
                }
                seen = true;
            }
            PT_SDES if body.len() >= 4 => {
                // first chunk only (one source): SSRC, then items type/len/text up to a 0 type
                let mut it = &body[4..];
                while it.len() >= 2 && it[0] != 0 {
                    let (t, n) = (it[0], it[1] as usize);
                    let text = it.get(2..2 + n)?;
                    let s = Some(String::from_utf8_lossy(text).into_owned());
                    match t {
                        SDES_CNAME => r.cname = s,
                        SDES_TOOL => r.tool = s,
                        _ => {}
                    }
                    it = &it[2 + n..];
                }
                seen = true;
            }
            _ => {}
        }
        p = &p[len..];
    }
    seen.then_some(r)
}

/// 64-bit NTP timestamp (32.32 fixed point) of a Unix time in microseconds.
pub fn ntp_from_unix_us(us: u64) -> u64 {
    let secs = us / 1_000_000 + NTP_UNIX_OFFSET;
    let frac = ((us % 1_000_000) << 32) / 1_000_000;
    (secs << 32) | frac
}

/// Middle 32 bits of an NTP timestamp (the LSR format).
pub fn ntp_mid32(ntp: u64) -> u32 {
    (ntp >> 16) as u32
}

/// Round trip from a report block (RFC 3550 §6.4.1): now - LSR - DLSR, in ms. None before the sink
/// has seen one of our SRs, and when DLSR is longer than the time since that SR (sink clock or a
/// bogus block: the difference would wrap to ~65000 s).
pub fn rtt_ms(now_ntp: u64, lsr: u32, dlsr: u32) -> Option<f64> {
    if lsr == 0 {
        return None;
    }
    let since_sr = ntp_mid32(now_ntp).wrapping_sub(lsr);
    let a = since_sr.checked_sub(dlsr)?;
    Some(a as f64 * 1000.0 / 65536.0)
}

/// Compound packet: Sender Report (no report blocks) + SDES CNAME, as RFC 3550 §6.1 asks.
/// `rtp_ts` must be the RTP timestamp of the same instant as `ntp` on the stream's clock.
pub fn sender_report(ssrc: u32, ntp: u64, rtp_ts: u32, packets: u32, octets: u32, cname: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(28 + 8 + cname.len() + 4);
    v.extend_from_slice(&[0x80, PT_SR, 0, 6]);
    v.extend_from_slice(&ssrc.to_be_bytes());
    v.extend_from_slice(&ntp.to_be_bytes());
    v.extend_from_slice(&rtp_ts.to_be_bytes());
    v.extend_from_slice(&packets.to_be_bytes());
    v.extend_from_slice(&octets.to_be_bytes());
    // SDES: one chunk, CNAME item, end item, padded to 32 bits
    let start = v.len();
    v.extend_from_slice(&[0x81, PT_SDES, 0, 0]);
    v.extend_from_slice(&ssrc.to_be_bytes());
    let name = &cname.as_bytes()[..cname.len().min(255)];
    v.extend_from_slice(&[SDES_CNAME, name.len() as u8]);
    v.extend_from_slice(name);
    v.push(0);
    while !(v.len() - start).is_multiple_of(4) {
        v.push(0);
    }
    let words = ((v.len() - start) / 4 - 1) as u16;
    v[start + 2..start + 4].copy_from_slice(&words.to_be_bytes());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RR as sent by the Samsung sink (capture 2026-11-09), followed by an SDES with CNAME and TOOL.
    fn samsung_rr() -> Vec<u8> {
        let mut v = vec![
            0x81, 0xc9, 0x00, 0x07, 0xe8, 0xc7, 0x2a, 0x13, 0x7d, 0x3c, 0xf2, 0xc9, 0x00, 0xff, 0xff, 0xff, 0x00, 0x1e,
            0x0f, 0xaa, 0x00, 0x00, 0x00, 0x2b, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let mut sdes = vec![0x81, 0xca, 0, 0, 0xe8, 0xc7, 0x2a, 0x13, 1, 6];
        sdes.extend_from_slice(b"user@x");
        sdes.extend_from_slice(&[6, 9]);
        sdes.extend_from_slice(b"GStreamer");
        sdes.push(0);
        while sdes.len() % 4 != 0 {
            sdes.push(0);
        }
        let w = (sdes.len() / 4 - 1) as u16;
        sdes[2..4].copy_from_slice(&w.to_be_bytes());
        v.extend(sdes);
        v
    }

    #[test]
    fn parses_the_samsung_receiver_report() {
        let r = parse(&samsung_rr()).expect("valid RTCP");
        assert_eq!(r.sender_ssrc, 0xe8c7_2a13);
        assert_eq!(
            r.blocks,
            vec![ReportBlock {
                ssrc: 0x7d3c_f2c9,
                fraction_lost: 0,
                cum_lost: -1,
                ext_seq: 0x001e_0faa,
                jitter: 43,
                lsr: 0,
                dlsr: 0
            }]
        );
        assert_eq!(r.tool.as_deref(), Some("GStreamer"));
        assert_eq!(r.cname.as_deref(), Some("user@x"));
    }

    #[test]
    fn rejects_garbage_and_truncated_packets() {
        assert_eq!(parse(&[]), None);
        assert_eq!(parse(&[0x47, 0x40, 0, 0x10]), None, "TS, not RTCP");
        let mut rr = samsung_rr();
        rr.truncate(20);
        assert_eq!(parse(&rr), None, "length runs past the datagram");
    }

    #[test]
    fn sender_report_round_trips_through_the_parser() {
        let ntp = ntp_from_unix_us(1_794_240_000_250_000);
        let sr = sender_report(0x1234_5678, ntp, 90_000, 10, 1316, "wfd-tx@sway-miracast");
        assert_eq!(sr.len() % 4, 0);
        assert_eq!(&sr[..4], &[0x80, 200, 0, 6]);
        assert_eq!(u64::from_be_bytes(sr[8..16].try_into().unwrap()), ntp);
        let r = parse(&sr).expect("own SR parses");
        assert_eq!(r.sender_ssrc, 0x1234_5678);
        assert!(r.blocks.is_empty());
        assert_eq!(r.cname.as_deref(), Some("wfd-tx@sway-miracast"));
    }

    #[test]
    fn ntp_and_round_trip() {
        let ntp = ntp_from_unix_us(1_000_000_500_000);
        assert_eq!(ntp >> 32, 1_000_000 + NTP_UNIX_OFFSET);
        assert_eq!(ntp & 0xffff_ffff, 1 << 31, "half a second");
        // SR sent at t, sink held it 10 ms, reply arrives at t + 30 ms: RTT 20 ms
        let t = ntp_from_unix_us(1_794_240_000_000_000);
        let dlsr = 655; // 10 ms in 1/65536 s
        let now = ntp_from_unix_us(1_794_240_000_030_000);
        let rtt = rtt_ms(now, ntp_mid32(t), dlsr).unwrap();
        assert!((rtt - 20.0).abs() < 0.1, "{rtt}");
        assert_eq!(rtt_ms(now, 0, 0), None);
    }

    #[test]
    fn rtt_is_none_when_dlsr_exceeds_the_elapsed_time() {
        // 30 ms since our SR, but the sink claims it held it 40 ms: no negative (wrapped) RTT
        let t = ntp_from_unix_us(1_794_240_000_000_000);
        let now = ntp_from_unix_us(1_794_240_000_030_000);
        assert_eq!(rtt_ms(now, ntp_mid32(t), 2621), None);
        // exactly the elapsed time: 0 ms
        let since = ntp_mid32(now).wrapping_sub(ntp_mid32(t));
        assert_eq!(rtt_ms(now, ntp_mid32(t), since), Some(0.0));
    }
}
