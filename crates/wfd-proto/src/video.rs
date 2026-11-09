//! `wfd_video_formats` (WFD R1, H.264): parsing sink capabilities and building the M4 selection.

use std::fmt;

#[derive(Debug, thiserror::Error, PartialEq)]
#[error("bad wfd_video_formats: {0}")]
pub struct Error(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Table {
    Cea = 0,
    Vesa = 1,
    Hh = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    pub table: Table,
    pub bit: u8,
    pub width: u16,
    pub height: u16,
    pub fps: u8,
    pub interlaced: bool,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}{}{}", self.width, self.height, if self.interlaced { "i" } else { "p" }, self.fps)
    }
}

const fn m(table: Table, bit: u8, width: u16, height: u16, fps: u8, interlaced: bool) -> Mode {
    Mode { table, bit, width, height, fps, interlaced }
}

use Table::*;
/// Resolution/refresh tables from the WFD specification (bit index = position in the mask).
pub const MODES: &[Mode] = &[
    m(Cea, 0, 640, 480, 60, false),
    m(Cea, 1, 720, 480, 60, false),
    m(Cea, 2, 720, 480, 60, true),
    m(Cea, 3, 720, 576, 50, false),
    m(Cea, 4, 720, 576, 50, true),
    m(Cea, 5, 1280, 720, 30, false),
    m(Cea, 6, 1280, 720, 60, false),
    m(Cea, 7, 1920, 1080, 30, false),
    m(Cea, 8, 1920, 1080, 60, false),
    m(Cea, 9, 1920, 1080, 60, true),
    m(Cea, 10, 1280, 720, 25, false),
    m(Cea, 11, 1280, 720, 50, false),
    m(Cea, 12, 1920, 1080, 25, false),
    m(Cea, 13, 1920, 1080, 50, false),
    m(Cea, 14, 1920, 1080, 50, true),
    m(Cea, 15, 1280, 720, 24, false),
    m(Cea, 16, 1920, 1080, 24, false),
    m(Vesa, 0, 800, 600, 30, false),
    m(Vesa, 1, 800, 600, 60, false),
    m(Vesa, 2, 1024, 768, 30, false),
    m(Vesa, 3, 1024, 768, 60, false),
    m(Vesa, 4, 1152, 864, 30, false),
    m(Vesa, 5, 1152, 864, 60, false),
    m(Vesa, 6, 1280, 768, 30, false),
    m(Vesa, 7, 1280, 768, 60, false),
    m(Vesa, 8, 1280, 800, 30, false),
    m(Vesa, 9, 1280, 800, 60, false),
    m(Vesa, 10, 1360, 768, 30, false),
    m(Vesa, 11, 1360, 768, 60, false),
    m(Vesa, 12, 1366, 768, 30, false),
    m(Vesa, 13, 1366, 768, 60, false),
    m(Vesa, 14, 1280, 1024, 30, false),
    m(Vesa, 15, 1280, 1024, 60, false),
    m(Vesa, 16, 1400, 1050, 30, false),
    m(Vesa, 17, 1400, 1050, 60, false),
    m(Vesa, 18, 1440, 900, 30, false),
    m(Vesa, 19, 1440, 900, 60, false),
    m(Vesa, 20, 1600, 900, 30, false),
    m(Vesa, 21, 1600, 900, 60, false),
    m(Vesa, 22, 1600, 1200, 30, false),
    m(Vesa, 23, 1600, 1200, 60, false),
    m(Vesa, 24, 1680, 1024, 30, false),
    m(Vesa, 25, 1680, 1024, 60, false),
    m(Vesa, 26, 1680, 1050, 30, false),
    m(Vesa, 27, 1680, 1050, 60, false),
    m(Vesa, 28, 1920, 1200, 30, false),
    m(Hh, 0, 800, 480, 30, false),
    m(Hh, 1, 800, 480, 60, false),
    m(Hh, 2, 854, 480, 30, false),
    m(Hh, 3, 854, 480, 60, false),
    m(Hh, 4, 864, 480, 30, false),
    m(Hh, 5, 864, 480, 60, false),
    m(Hh, 6, 640, 360, 30, false),
    m(Hh, 7, 640, 360, 60, false),
    m(Hh, 8, 960, 540, 30, false),
    m(Hh, 9, 960, 540, 60, false),
    m(Hh, 10, 848, 480, 30, false),
    m(Hh, 11, 848, 480, 60, false),
];

pub const PROFILE_CBP: u8 = 0x01;
pub const PROFILE_CHP: u8 = 0x02;

/// H.264 level bitmap value -> level_idc*10 (e.g. 0x10 -> 42).
pub fn level_idc(bit: u8) -> Option<u8> {
    Some(match bit {
        0x01 => 31,
        0x02 => 32,
        0x04 => 40,
        0x08 => 41,
        0x10 => 42,
        _ => return None,
    })
}

/// Smallest H.264 level (bitmap value) that carries the mode (Table A-1 macroblock rates).
pub fn level_for(mode: &Mode) -> u8 {
    let mbs = (mode.width as u32).div_ceil(16) * (mode.height as u32).div_ceil(16);
    let rate = mbs * mode.fps as u32;
    match (mbs, rate) {
        (_, r) if r <= 108_000 && mbs <= 3_600 => 0x01, // 3.1
        (_, r) if r <= 216_000 && mbs <= 5_120 => 0x02, // 3.2
        (_, r) if r <= 245_760 && mbs <= 8_192 => 0x04, // 4.0
        _ => 0x10,                                      // 4.2 (522240 MB/s)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct H264Codec {
    pub profile: u8,
    pub level: u8,
    pub cea: u32,
    pub vesa: u32,
    pub hh: u32,
    /// Decoder latency declared by the sink, in units of 5 ms (0 = not specified).
    pub latency: u8,
    pub min_slice_size: u16,
    pub slice_enc_params: u16,
    pub frame_rate_control: u8,
    pub max_hres: Option<u16>,
    pub max_vres: Option<u16>,
}

impl H264Codec {
    pub fn mask(&self, t: Table) -> u32 {
        match t {
            Cea => self.cea,
            Vesa => self.vesa,
            Hh => self.hh,
        }
    }

    pub fn supports(&self, mode: &Mode) -> bool {
        self.mask(mode.table) & (1 << mode.bit) != 0
    }

    pub fn modes(&self) -> impl Iterator<Item = &'static Mode> + '_ {
        MODES.iter().filter(|md| self.supports(md))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoFormats {
    pub native: u8,
    pub preferred_display_mode: u8,
    pub codecs: Vec<H264Codec>,
}

impl VideoFormats {
    pub fn parse(s: &str) -> Result<Option<Self>, Error> {
        let s = s.trim();
        if s == "none" {
            return Ok(None);
        }
        let e = |what: &str| Error(format!("{what} in {s:?}"));
        let mut parts = s.splitn(3, ' ');
        let native = hex8(parts.next().ok_or_else(|| e("native"))?).ok_or_else(|| e("native"))?;
        let pref = hex8(parts.next().ok_or_else(|| e("preferred"))?).ok_or_else(|| e("preferred"))?;
        let mut codecs = Vec::new();
        for c in parts.next().ok_or_else(|| e("codec list"))?.split(',') {
            let f: Vec<&str> = c.split_whitespace().collect();
            if f.len() != 11 {
                return Err(e("codec field count"));
            }
            let res = |v: &str| {
                if v == "none" {
                    Ok(None)
                } else {
                    u16::from_str_radix(v, 16).map(Some).map_err(|_| e("max res"))
                }
            };
            codecs.push(H264Codec {
                profile: hex8(f[0]).ok_or_else(|| e("profile"))?,
                level: hex8(f[1]).ok_or_else(|| e("level"))?,
                cea: u32::from_str_radix(f[2], 16).map_err(|_| e("CEA"))?,
                vesa: u32::from_str_radix(f[3], 16).map_err(|_| e("VESA"))?,
                hh: u32::from_str_radix(f[4], 16).map_err(|_| e("HH"))?,
                latency: hex8(f[5]).ok_or_else(|| e("latency"))?,
                min_slice_size: u16::from_str_radix(f[6], 16).map_err(|_| e("min slice"))?,
                slice_enc_params: u16::from_str_radix(f[7], 16).map_err(|_| e("slice enc"))?,
                frame_rate_control: hex8(f[8]).ok_or_else(|| e("frame rate ctl"))?,
                max_hres: res(f[9])?,
                max_vres: res(f[10])?,
            });
        }
        Ok(Some(VideoFormats { native, preferred_display_mode: pref, codecs }))
    }

    pub fn native_mode(&self) -> Option<&'static Mode> {
        let table = match self.native & 0x07 {
            0 => Cea,
            1 => Vesa,
            2 => Hh,
            _ => return None,
        };
        MODES.iter().find(|m| m.table == table && m.bit == self.native >> 3)
    }

    /// Picks the first wanted mode a CBP-capable codec supports and returns the M4 selection.
    pub fn select(&self, wanted: &[(u16, u16, u8)]) -> Option<Selection> {
        let codec = self.codecs.iter().find(|c| c.profile & PROFILE_CBP != 0)?;
        wanted.iter().find_map(|&(w, h, fps)| {
            let mode = codec.modes().find(|m| m.width == w && m.height == h && m.fps == fps && !m.interlaced)?;
            let level = level_for(mode).min(highest_bit(codec.level)?);
            Some(Selection { mode: *mode, level, profile: PROFILE_CBP })
        })
    }
}

/// The single mode a source announces in M4.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Selection {
    pub mode: Mode,
    pub profile: u8,
    pub level: u8,
}

impl fmt::Display for Selection {
    /// e.g. `38 00 01 04 00000080 00000000 00000000 00 0000 0000 00 none none` (1080p30, CBP, 4.0).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mask = |t: Table| if self.mode.table == t { 1u32 << self.mode.bit } else { 0 };
        write!(
            f,
            "{:02x} 00 {:02x} {:02x} {:08x} {:08x} {:08x} 00 0000 0000 00 none none",
            (self.mode.bit << 3) | self.mode.table as u8,
            self.profile,
            self.level,
            mask(Cea),
            mask(Vesa),
            mask(Hh)
        )
    }
}

fn hex8(s: &str) -> Option<u8> {
    u8::from_str_radix(s, 16).ok()
}

fn highest_bit(v: u8) -> Option<u8> {
    (v != 0).then(|| 1 << (7 - v.leading_zeros() as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMSUNG: &str = "40 00 01 10 000001e3 0f3fffff 00000fff 00 0000 00c8 01 none none";

    #[test]
    fn level_for_picks_the_lowest_sufficient_level() {
        let m = |width, height, fps| Mode { table: Cea, bit: 0, width, height, fps, interlaced: false };
        assert_eq!(level_for(&m(1280, 720, 30)), 0x01, "3.1: 3600 MBs x 30 = 108000");
        assert_eq!(level_for(&m(1280, 720, 60)), 0x02, "3.2: 216000 MB/s");
        assert_eq!(level_for(&m(1920, 1080, 30)), 0x04, "4.0: 8160 MBs x 30 = 244800");
        assert_eq!(level_for(&m(1920, 1080, 60)), 0x10, "4.2: 489600 MB/s");
    }

    #[test]
    fn parses_samsung_sink() {
        let v = VideoFormats::parse(SAMSUNG).unwrap().unwrap();
        assert_eq!(v.native_mode().unwrap().to_string(), "1920x1080p60");
        let c = &v.codecs[0];
        assert_eq!(c.profile, PROFILE_CBP);
        assert_eq!(level_idc(c.level), Some(42));
        let cea: Vec<String> = c.modes().filter(|m| m.table == Cea).map(|m| m.to_string()).collect();
        assert_eq!(cea, ["640x480p60", "720x480p60", "1280x720p30", "1280x720p60", "1920x1080p30", "1920x1080p60"]);
        assert_eq!(c.modes().filter(|m| m.table == Hh).count(), 12);
        assert_eq!(c.slice_enc_params, 0x00c8);
        assert_eq!(c.frame_rate_control, 1);
        assert_eq!(c.max_hres, None);
    }

    #[test]
    fn selection_matches_known_good_m4() {
        let v = VideoFormats::parse(SAMSUNG).unwrap().unwrap();
        let s = v.select(&[(1920, 1080, 30)]).unwrap();
        // fluxcast sent exactly this and the TV accepted it
        assert_eq!(s.to_string(), "38 00 01 04 00000080 00000000 00000000 00 0000 0000 00 none none");
        let s60 = v.select(&[(1920, 1080, 50), (1920, 1080, 60)]).unwrap();
        assert_eq!(s60.mode.to_string(), "1920x1080p60");
        assert_eq!(s60.level, 0x10);
        assert_eq!(v.select(&[(1280, 720, 60)]).unwrap().level, 0x02);
    }

    #[test]
    fn none_and_garbage() {
        assert_eq!(VideoFormats::parse("none").unwrap(), None);
        assert!(VideoFormats::parse("40 00 01").is_err());
    }
}
