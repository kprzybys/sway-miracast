//! Command line (see the `//!` header of main.rs, which is also the --help text).

pub struct Opts {
    pub dest: String,
    pub quiet: bool,
    /// video FIFO (None: stdin)
    pub input: Option<String>,
    pub audio: Option<String>,
    pub audio_delay_file: Option<String>,
    pub mux_kbps_file: Option<String>,
    pub video_hold_file: Option<String>,
    pub rtcp_port: Option<u16>,
    pub rtcp_log: Option<String>,
}

/// Parses the arguments after the program name. Err: the message for stderr (or the --help text).
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Opts, String> {
    let mut dest = None;
    let mut o = Opts {
        dest: String::new(),
        quiet: false,
        input: None,
        audio: None,
        audio_delay_file: None,
        mux_kbps_file: None,
        video_hold_file: None,
        rtcp_port: None,
        rtcp_log: None,
    };
    let mut a = args.into_iter();
    while let Some(k) = a.next() {
        let mut v = || a.next().ok_or(format!("{k} needs a value"));
        match k.as_str() {
            "--dest" => dest = Some(v()?),
            "--quiet" => o.quiet = true,
            // framed input is the only mode now; the flag stays accepted (wfd-source passes it)
            "--framed" => {}
            "--in" => o.input = Some(v()?),
            "--audio" => o.audio = Some(v()?),
            "--audio-delay-file" => o.audio_delay_file = Some(v()?),
            "--mux-kbps-file" => o.mux_kbps_file = Some(v()?),
            "--video-hold-file" => o.video_hold_file = Some(v()?),
            "--rtcp-port" => o.rtcp_port = Some(v()?.parse().map_err(|e| format!("{k}: {e}"))?),
            "--rtcp-log" => o.rtcp_log = Some(v()?),
            "-h" | "--help" => return Err(wfd_util::doc_help(include_str!("main.rs"))),
            _ => return Err(format!("unknown option {k} (see --help)")),
        }
    }
    o.dest = dest.ok_or("need --dest HOST:PORT")?;
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn the_wfd_source_command_line_parses() {
        let o = parse(args(
            "--dest 10.0.0.2:19000 --quiet --framed --in v.fifo --video-hold-file h --rtcp-port 19001 \
             --rtcp-log r.jsonl --audio a.fifo --audio-delay-file d --mux-kbps-file k",
        ))
        .expect("parses");
        assert_eq!(o.dest, "10.0.0.2:19000");
        assert!(o.quiet);
        assert_eq!(o.input.as_deref(), Some("v.fifo"));
        assert_eq!(o.rtcp_port, Some(19001));
        assert_eq!(o.mux_kbps_file.as_deref(), Some("k"));
    }

    #[test]
    fn removed_and_missing_options_fail() {
        assert!(parse(args("--in x")).is_err(), "--dest is required");
        for gone in ["--out f", "--dscp 34", "--tables-every 3", "--idle-us 2000", "--pts-delay-ms 0", "--mux-kbps 1"] {
            let e = parse(args(&format!("--dest h:1 {gone}"))).err().expect(gone);
            assert!(e.starts_with("unknown option"), "{e}");
        }
    }
}
