//! `wfd_audio_codecs`: e.g. `LPCM 00000003 00, AAC 00000001 00`.

/// M4 `wfd_audio_codecs` for what we send: LPCM 48 kHz 16 bit stereo (mode bit 1), no extra latency.
pub const LPCM_48K_STEREO: &str = "LPCM 00000002 00";
/// M4 `wfd_audio_codecs` without audio: Samsung ends the session after SETUP if M4 names no codec at all.
pub const AAC_48K_STEREO: &str = "AAC 00000001 00";

#[derive(Debug, Clone, PartialEq)]
pub struct AudioCodec {
    pub name: String,
    pub modes: u32,
    pub latency: u8,
}

impl AudioCodec {
    /// Human-readable modes for the three WFD audio formats.
    pub fn describe(&self) -> Vec<&'static str> {
        let table: &[&'static str] = match self.name.as_str() {
            "LPCM" => &["44.1kHz 16b 2ch", "48kHz 16b 2ch"],
            "AAC" => &["48kHz 16b 2ch", "48kHz 16b 4ch", "48kHz 16b 6ch", "48kHz 16b 8ch"],
            "AC3" => &["48kHz 16b 2ch", "48kHz 16b 4ch", "48kHz 16b 6ch"],
            _ => &[],
        };
        table.iter().enumerate().filter(|(i, _)| self.modes & (1 << i) != 0).map(|(_, d)| *d).collect()
    }
}

pub fn parse(s: &str) -> Vec<AudioCodec> {
    let s = s.trim();
    if s == "none" {
        return vec![];
    }
    s.split(',')
        .filter_map(|c| {
            let f: Vec<&str> = c.split_whitespace().collect();
            (f.len() == 3).then(|| AudioCodec {
                name: f[0].to_string(),
                modes: u32::from_str_radix(f[1], 16).unwrap_or(0),
                latency: u8::from_str_radix(f[2], 16).unwrap_or(0),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn samsung() {
        let a = super::parse("LPCM 00000003 00, AAC 00000001 00");
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].describe(), ["44.1kHz 16b 2ch", "48kHz 16b 2ch"]);
        assert_eq!(a[1].describe(), ["48kHz 16b 2ch"]);
        assert!(super::parse("none").is_empty());
    }

    #[test]
    fn m4_codec_strings() {
        let l = super::parse(super::LPCM_48K_STEREO);
        assert_eq!((l[0].name.as_str(), l[0].describe()), ("LPCM", vec!["48kHz 16b 2ch"]));
        let a = super::parse(super::AAC_48K_STEREO);
        assert_eq!((a[0].name.as_str(), a[0].describe()), ("AAC", vec!["48kHz 16b 2ch"]));
    }
}
