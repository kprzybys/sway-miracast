//! Minimal MPEG-TS muxer for WFD: H.264 video (PID 0x1011, carries the PCR) and optional LPCM audio
//! (PID 0x1100, stream type 0x83, WFD spec v2.1 Appendix D / Table 106). PMT 0x0100.
//! No allocation: packets are written straight into the caller's buffer.

use crate::logic::LPCM_CHUNK;

/// One TS packet (the only packet size in the crate).
pub const TS: usize = 188;
const SYNC: u8 = 0x47;
const PAT_PID: u16 = 0x0000;
pub const PMT_PID: u16 = 0x0100;
pub const VIDEO_PID: u16 = 0x1011;
pub const AUDIO_PID: u16 = 0x1100;
pub const NULL_PID: u16 = 0x1fff;
/// Continuity counter slots.
const CC_PAT: usize = 0;
const CC_PMT: usize = 1;
const CC_VIDEO: usize = 2;
const CC_AUDIO: usize = 3;
/// Adaptation field with only the PCR: flags byte + 6 bytes PCR.
const PCR_FIELD: usize = 7;
/// adaptation_field_control: payload only / adaptation only / both (with the CC in the low nibble).
const AFC_PAYLOAD: u8 = 0x10;
const AFC_ADAPT: u8 = 0x20;
const AFC_BOTH: u8 = 0x30;
const PUSI: u8 = 0x40;
const STUFFED: [u8; TS] = [0xff; TS];
/// Null packet (PID 0x1fff, payload only, all stuffing) for the constant mux rate.
const NULL_PACKET: [u8; TS] = {
    let mut p = STUFFED;
    p[0] = SYNC;
    p[1] = (NULL_PID >> 8) as u8;
    p[2] = NULL_PID as u8;
    p[3] = AFC_PAYLOAD;
    p
};
/// PES header of a video AU: start code, stream id 0xe0, length 0 (unbounded), data_alignment,
/// PTS only, header length 5, then the PTS.
const VIDEO_PES_HEAD: usize = 14;
/// PES header of one LPCM chunk: private_stream_1, flags, PTS, 2 stuffing bytes, LPCM header.
const LPCM_PES_HEAD: usize = 20;
/// PES_packet_length: everything after the length field (1934).
const LPCM_PES_LEN: u16 = (LPCM_PES_HEAD - 6 + LPCM_CHUNK) as u16;
/// Longest PSI section here (PMT with audio: 26 bytes).
const SECTION_MAX: usize = 32;

/// PCR adaptation field body: PCR_flag, then 33-bit base, 6 reserved bits and 9-bit extension.
pub fn pcr_field(base: u64, ext: u16) -> [u8; PCR_FIELD] {
    let b = base & ((1 << 33) - 1);
    [
        0x10, // PCR_flag
        (b >> 25) as u8,
        (b >> 17) as u8,
        (b >> 9) as u8,
        (b >> 1) as u8,
        (((b & 1) << 7) as u8) | 0x7e | ((ext >> 8) as u8 & 1),
        ext as u8,
    ]
}

/// Appends `n` null packets.
pub fn write_null(n: usize, out: &mut Vec<u8>) {
    for _ in 0..n {
        out.extend_from_slice(&NULL_PACKET);
    }
}

/// A PSI section built on the stack; `finish` fills section_length and the CRC.
struct Section {
    b: [u8; SECTION_MAX],
    n: usize,
}

impl Section {
    fn new(table_id: u8) -> Self {
        // section_syntax_indicator + reserved; section_length set in finish()
        let mut s = Section { b: [0; SECTION_MAX], n: 0 };
        s.put(&[table_id, 0xb0, 0]);
        s
    }

    fn put(&mut self, x: &[u8]) {
        self.b[self.n..self.n + x.len()].copy_from_slice(x);
        self.n += x.len();
    }

    fn finish(&mut self) -> &[u8] {
        // section_length: the bytes after the length field, CRC included
        self.b[2] = (self.n - 3 + 4) as u8;
        let crc = crc32(&self.b[..self.n]);
        self.put(&crc.to_be_bytes());
        &self.b[..self.n]
    }
}

pub struct Muxer {
    cc: [u8; 4], // PAT, PMT, video, audio continuity counters
    pub audio: bool,
}

impl Default for Muxer {
    fn default() -> Self {
        Self::new()
    }
}

impl Muxer {
    pub fn new() -> Self {
        Muxer { cc: [0; 4], audio: false }
    }

    fn next_cc(&mut self, i: usize) -> u8 {
        let c = self.cc[i];
        self.cc[i] = (c + 1) & 0x0f;
        c
    }

    fn psi(&mut self, pid: u16, idx: usize, section: &[u8], out: &mut Vec<u8>) {
        let p = new_packet(out);
        p[0] = SYNC;
        p[1] = PUSI | (pid >> 8) as u8;
        p[2] = pid as u8;
        p[4] = 0; // pointer_field
        p[5..5 + section.len()].copy_from_slice(section);
        p[3] = AFC_PAYLOAD | self.next_cc(idx);
    }

    pub fn write_tables(&mut self, out: &mut Vec<u8>) {
        // PAT: program 1 -> PMT_PID
        let mut pat = Section::new(0x00);
        pat.put(&[0x00, 0x01, 0xc1, 0x00, 0x00]); // transport_stream_id 1, version 0, section 0/0
        pat.put(&[0x00, 0x01, 0xe0 | (PMT_PID >> 8) as u8, PMT_PID as u8]);
        self.psi(PAT_PID, CC_PAT, pat.finish(), out);
        // PMT: PCR_PID = video; H.264 (0x1b) and optionally LPCM (0x83)
        let mut pmt = Section::new(0x02);
        pmt.put(&[0x00, 0x01, 0xc1, 0x00, 0x00]); // program 1, version 0, section 0/0
        pmt.put(&[0xe0 | (VIDEO_PID >> 8) as u8, VIDEO_PID as u8, 0xf0, 0x00]); // PCR PID, no descriptors
        pmt.put(&[0x1b, 0xe0 | (VIDEO_PID >> 8) as u8, VIDEO_PID as u8, 0xf0, 0x00]);
        if self.audio {
            pmt.put(&[0x83, 0xe0 | (AUDIO_PID >> 8) as u8, AUDIO_PID as u8, 0xf0, 0x00]);
        }
        self.psi(PMT_PID, CC_PMT, pmt.finish(), out);
    }

    /// Packs one access unit as a PES (PTS only) into TS packets; PCR in the first packet.
    /// `pts` and `pcr_base` are 90 kHz ticks; `pcr_ext` 0..299 (27 MHz remainder).
    pub fn write_au(&mut self, au: &[u8], pts: u64, pcr_base: u64, pcr_ext: u16, out: &mut Vec<u8>) {
        let mut head = [0u8; VIDEO_PES_HEAD];
        head[..9].copy_from_slice(&[0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x84, 0x80, 0x05]);
        head[9..].copy_from_slice(&encode_pts(0x2, pts));
        self.packetize(VIDEO_PID, CC_VIDEO, &head, au, Some((pcr_base, pcr_ext)), out);
    }

    /// One LPCM PES: 480 frames (10 ms) of 48 kHz 16-bit stereo, big-endian = 1920 bytes (WFD Table 106).
    pub fn write_lpcm(&mut self, pcm: &[u8; LPCM_CHUNK], pts: u64, out: &mut Vec<u8>) {
        let [len_hi, len_lo] = LPCM_PES_LEN.to_be_bytes();
        let mut head = [0u8; LPCM_PES_HEAD];
        head[..9].copy_from_slice(&[0x00, 0x00, 0x01, 0xbd, len_hi, len_lo, 0x81, 0x80, 0x07]); // original, PTS only
        head[9..14].copy_from_slice(&encode_pts(0x2, pts));
        head[14..16].copy_from_slice(&[0xff, 0xff]); // stuffing
        head[16..].copy_from_slice(&[0xa0, 0x06, 0x00, 0x11]); // sub_stream 0, 6 AUs, 16 bit / 48 kHz / 2 ch
        self.packetize(AUDIO_PID, CC_AUDIO, &head, pcm, None, out);
    }

    /// PCR without payload on the PCR PID (adaptation_field_control = 2, continuity counter not
    /// incremented, H.222.0 §2.4.3.3). Keeps the sink's clock fed while no video AU comes (encoder
    /// restart); the spec wants a PCR at least every 100 ms.
    pub fn write_pcr_only(&self, pcr_base: u64, pcr_ext: u16, out: &mut Vec<u8>) {
        let p = new_packet(out);
        p[0] = SYNC;
        p[1] = (VIDEO_PID >> 8) as u8;
        p[2] = VIDEO_PID as u8;
        p[3] = AFC_ADAPT | self.cc[CC_VIDEO].wrapping_sub(1) & 0x0f; // CC of the last video packet
        p[4] = (TS - 5) as u8; // adaptation field fills the packet
        p[5..5 + PCR_FIELD].copy_from_slice(&pcr_field(pcr_base, pcr_ext));
    }

    /// The PES `head` + `body` (one payload, two slices: no copy into a PES buffer first) as TS packets.
    fn packetize(
        &mut self,
        pid: u16,
        cc_idx: usize,
        head: &[u8],
        body: &[u8],
        pcr: Option<(u64, u16)>,
        out: &mut Vec<u8>,
    ) {
        let total = head.len() + body.len();
        let mut off = 0;
        let mut first = true;
        while off < total {
            let cc = self.next_cc(cc_idx);
            let p = new_packet(out);
            p[0] = SYNC;
            p[1] = if first { PUSI } else { 0x00 } | (pid >> 8) as u8;
            p[2] = pid as u8;
            // adaptation field body (without its length byte) goes to p[5..5 + af_len]; the packet starts
            // all 0xff, so stuffing needs no writes
            let mut af_len = 0;
            if let (true, Some((base, ext))) = (first, pcr) {
                p[5..5 + PCR_FIELD].copy_from_slice(&pcr_field(base, ext));
                af_len = PCR_FIELD;
            }
            let remaining = total - off;
            let has_af = af_len > 0;
            let mut avail = TS - 4 - if has_af { 1 + af_len } else { 0 };
            let mut af_len_only = false; // adaptation_field_length = 0 (1 stuffing byte)
            if remaining < avail {
                let stuff = avail - remaining;
                if has_af {
                    af_len += stuff;
                } else if stuff == 1 {
                    af_len_only = true;
                } else {
                    p[5] = 0x00; // flags, then stuff - 2 stuffing bytes
                    af_len = stuff - 1;
                }
                avail = remaining;
            }
            let mut i = 4;
            if af_len > 0 || af_len_only {
                p[3] = AFC_BOTH | cc;
                p[4] = af_len as u8;
                i = 5 + af_len;
            } else {
                p[3] = AFC_PAYLOAD | cc;
            }
            debug_assert_eq!(i + avail, TS);
            copy_payload(&mut p[i..], head, body, off);
            off += avail;
            first = false;
        }
    }
}

/// Appends one all-stuffing packet and returns it for filling in.
fn new_packet(out: &mut Vec<u8>) -> &mut [u8] {
    let start = out.len();
    out.extend_from_slice(&STUFFED);
    &mut out[start..]
}

/// Fills `dst` with the payload bytes at `off` of `head` followed by `body`.
fn copy_payload(dst: &mut [u8], head: &[u8], body: &[u8], off: usize) {
    let mut d = 0;
    if off < head.len() {
        d = (head.len() - off).min(dst.len());
        dst[..d].copy_from_slice(&head[off..off + d]);
    }
    if d < dst.len() {
        let b = off + d - head.len();
        let n = dst.len() - d;
        dst[d..].copy_from_slice(&body[b..b + n]);
    }
}

fn encode_pts(prefix: u8, pts: u64) -> [u8; 5] {
    let p = pts & ((1 << 33) - 1);
    [
        (prefix << 4) | (((p >> 30) as u8 & 0x07) << 1) | 1,
        (p >> 22) as u8,
        (((p >> 15) as u8 & 0x7f) << 1) | 1,
        (p >> 7) as u8,
        ((p as u8 & 0x7f) << 1) | 1,
    ]
}

/// MPEG-2 CRC-32 (poly 0x04c11db7, no reflection) of a PSI section.
fn crc32(v: &[u8]) -> u32 {
    let mut crc: u32 = 0xffff_ffff;
    for &b in v {
        crc ^= (b as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 { (crc << 1) ^ 0x04c1_1db7 } else { crc << 1 };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packets_are_aligned_and_counted() {
        let mut m = Muxer::new();
        let mut out = Vec::new();
        m.write_tables(&mut out);
        for len in [1usize, 2, 170, 171, 172, 173, 183, 184, 500, 10_000] {
            let au = vec![0xabu8; len];
            let before = out.len();
            m.write_au(&au, 90_000, 89_000, 0, &mut out);
            assert_eq!((out.len() - before) % TS, 0, "len {len}");
            // payload bytes recovered == PES length
            let mut payload = 0;
            for p in out[before..].chunks(TS) {
                assert_eq!(p[0], 0x47);
                let afc = (p[3] >> 4) & 3;
                let start = if afc == 3 { 5 + p[4] as usize } else { 4 };
                payload += TS - start;
            }
            assert_eq!(payload, len + 14, "len {len}");
        }
    }

    #[test]
    fn pcr_only_packet_keeps_continuity() {
        let mut m = Muxer::new();
        let mut out = Vec::new();
        m.write_au(&[0u8; 10], 90_000, 89_000, 0, &mut out);
        let last_cc = out[out.len() - TS + 3] & 0x0f;
        let mut p = Vec::new();
        m.write_pcr_only(123_456_789, 299, &mut p);
        assert_eq!(p.len(), TS);
        assert_eq!(((p[1] as u16 & 0x1f) << 8) | p[2] as u16, VIDEO_PID);
        assert_eq!(p[3] >> 4, 0x2, "adaptation only");
        assert_eq!(p[3] & 0x0f, last_cc, "CC not incremented");
        assert_eq!((p[4], p[5]), (183, 0x10));
        let base = ((p[6] as u64) << 25)
            | ((p[7] as u64) << 17)
            | ((p[8] as u64) << 9)
            | ((p[9] as u64) << 1)
            | (p[10] as u64 >> 7);
        let ext = (((p[10] & 1) as u16) << 8) | p[11] as u16;
        assert_eq!((base, ext), (123_456_789, 299));
        // the next AU continues the counter
        let mut n = Vec::new();
        m.write_au(&[0u8; 10], 90_000, 89_000, 0, &mut n);
        assert_eq!(n[3] & 0x0f, (last_cc + 1) & 0x0f);
    }

    #[test]
    fn pts_encoding_roundtrips_33_bits() {
        for pts in [0u64, 1, 90_000, (1 << 32) + 12345, (1 << 33) - 1, 1 << 33] {
            let b = encode_pts(0x2, pts);
            assert_eq!(b[0] & 1, 1);
            assert_eq!(b[2] & 1, 1);
            assert_eq!(b[4] & 1, 1, "marker bits");
            let back = (((b[0] >> 1) & 7) as u64) << 30
                | (b[1] as u64) << 22
                | ((b[2] >> 1) as u64) << 15
                | (b[3] as u64) << 7
                | (b[4] >> 1) as u64;
            assert_eq!(back, pts & ((1 << 33) - 1), "pts {pts}");
        }
    }

    #[test]
    fn lpcm_chunk_is_one_pes_of_fixed_size() {
        let mut m = Muxer::new();
        m.audio = true;
        let mut out = Vec::new();
        m.write_lpcm(&[0u8; 1920], 90_000, &mut out);
        assert_eq!(out.len() % TS, 0);
        let mut payload = 0;
        for p in out.chunks(TS) {
            assert_eq!(((p[1] as u16 & 0x1f) << 8) | p[2] as u16, AUDIO_PID);
            let afc = (p[3] >> 4) & 3;
            payload += TS - if afc == 3 { 5 + p[4] as usize } else { 4 };
        }
        // PES: start code + id + length (6), flags + header length (3), PTS (5), stuffing (2),
        // LPCM header (4), samples (1920)
        assert_eq!(payload, 1940);
        let pes_len = u16::from_be_bytes([out[4 + 4], out[4 + 5]]); // first packet: no adaptation field
        assert_eq!(pes_len, 1940 - 6, "PES_packet_length");
    }

    #[test]
    fn pat_crc_matches_known_value() {
        // PAT for program 1 / PMT 0x100 is a well-known section; CRC must be self-consistent:
        let mut pat = vec![0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00];
        pat.extend_from_slice(&crc32(&pat).to_be_bytes());
        // CRC over section incl. CRC must be 0 for MPEG CRC32
        assert_eq!(crc32(&pat), 0);
    }

    /// FNV-1a 64 over the produced bytes (golden value written with the pre-refactor muxer).
    fn fnv1a(b: &[u8]) -> u64 {
        b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x0100_0000_01b3))
    }

    /// A sample session: tables, AUs of edge sizes, LPCM chunks, PCR-only packets and null packets,
    /// with and without audio. The muxer must produce exactly the same bytes as before the refactor.
    fn golden_stream(audio: bool) -> Vec<u8> {
        let mut m = Muxer::new();
        m.audio = audio;
        let mut out = Vec::new();
        let mut pcm = [0u8; LPCM_CHUNK];
        for (i, b) in pcm.iter_mut().enumerate() {
            *b = (i * 7 + 3) as u8;
        }
        let sizes = [1usize, 2, 169, 170, 171, 172, 173, 183, 184, 185, 500, 4000, 10_000, 0];
        let mut pcr: u64 = 27_000_000;
        for (n, &len) in sizes.iter().enumerate() {
            if n % 3 == 0 {
                m.write_tables(&mut out);
            }
            let au: Vec<u8> = (0..len).map(|i| (i as u8) ^ (n as u8)).collect();
            let pts = (pcr + 27_000 * n as u64) / 300 + if n == 5 { (1 << 33) - 10 } else { 0 };
            m.write_au(&au, pts, pcr / 300, (pcr % 300) as u16, &mut out);
            if audio {
                m.write_lpcm(&pcm, pcr / 300 + 900 * n as u64, &mut out);
            }
            if n % 4 == 1 {
                m.write_pcr_only((pcr + 1_080_000) / 300, ((pcr + 1_080_000) % 300) as u16, &mut out);
            }
            write_null(n % 3, &mut out);
            pcr += 450_000 + 299 * n as u64;
        }
        out
    }

    #[test]
    fn golden_bytes_unchanged() {
        let (a, v) = (golden_stream(true), golden_stream(false));
        assert_eq!((a.len(), v.len()), (52640, 23688), "sizes");
        assert_eq!((fnv1a(&a), fnv1a(&v)), (7_266_732_666_113_678_475, 8_362_313_369_797_123_339));
    }

    #[test]
    fn section_lengths_are_computed() {
        let mut m = Muxer::new();
        let mut out = Vec::new();
        m.write_tables(&mut out);
        assert_eq!((out[5 + 2], out[TS + 5 + 2]), (0x0d, 0x12), "PAT, PMT without audio");
        m.audio = true;
        out.clear();
        m.write_tables(&mut out);
        assert_eq!(out[TS + 5 + 2], 0x17, "PMT with audio");
    }

    #[test]
    fn null_packets() {
        let mut out = Vec::new();
        write_null(2, &mut out);
        assert_eq!(out.len(), 2 * TS);
        assert_eq!(&out[TS..TS + 4], &[0x47, 0x1f, 0xff, 0x10]);
        assert!(out[TS + 4..].iter().all(|&b| b == 0xff));
    }
}
