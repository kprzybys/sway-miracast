//! Command line (the --help text is the `//!` header of main.rs).

#[derive(Debug, Clone, PartialEq)]
pub struct Opts {
    pub output: Option<String>,
    pub size: Option<(u32, u32)>,
    pub fps: u32,
    pub kbps: u32,
    pub gop: u32,
    pub cursor: bool,
    pub quiet: bool,
    pub dmabuf: bool,
    pub lp: bool,
    pub framed: bool,
    /// None: autodetect (renderD128 if it opens, else the first /dev/dri/renderD* that does)
    pub render_node: Option<String>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            output: None,
            size: None,
            fps: 30,
            kbps: 8000,
            gop: 30,
            cursor: true,
            quiet: false,
            dmabuf: false,
            lp: false,
            framed: false,
            render_node: None,
        }
    }
}

pub fn parse_opts() -> Result<Opts, String> {
    parse_args(std::env::args().skip(1))
}

/// Parse the arguments (without the program name). Err = message (or the help text) for stderr, exit 2.
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Opts, String> {
    let mut o = Opts::default();
    let mut a = args.into_iter();
    while let Some(k) = a.next() {
        let mut v = || a.next().ok_or(format!("{k} needs a value"));
        let num = |s: String| s.parse::<u32>().map_err(|e| format!("{k}: {e}"));
        match k.as_str() {
            "--output" => o.output = Some(v()?),
            "--size" => {
                let s = v()?;
                let bad = || format!("--size WxH (even, non-zero): {s}");
                let (w, h) = s.split_once('x').ok_or_else(bad)?;
                let (w, h): (u32, u32) = (w.parse().map_err(|_| bad())?, h.parse().map_err(|_| bad())?);
                if w == 0 || h == 0 || w % 2 != 0 || h % 2 != 0 {
                    return Err(bad());
                }
                o.size = Some((w, h));
            }
            "--fps" => o.fps = num(v()?)?,
            "--kbps" => o.kbps = num(v()?)?,
            "--gop" => o.gop = num(v()?)?,
            "--framed" => o.framed = true,
            "--no-cursor" => o.cursor = false,
            "--quiet" => o.quiet = true,
            "--dmabuf" => o.dmabuf = true,
            "--lp" => o.lp = true,
            "--render-node" => o.render_node = Some(v()?),
            "-h" | "--help" => return Err(wfd_util::doc_help(include_str!("main.rs"))),
            _ => return Err(format!("unknown option {k}")),
        }
    }
    if o.fps < 1 {
        return Err("--fps must be >= 1".into());
    }
    if o.kbps == 0 {
        return Err("--kbps must be > 0".into());
    }
    if o.gop < 1 {
        return Err("--gop must be >= 1".into());
    }
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Opts, String> {
        parse_args(s.split_whitespace().map(String::from))
    }

    #[test]
    fn wfd_source_command_line_parses() {
        let o = p("--output HEADLESS-1 --size 1920x1080 --fps 60 --kbps 8000 --gop 120 --framed --quiet --dmabuf --lp --no-cursor")
            .unwrap();
        let want = Opts {
            output: Some("HEADLESS-1".into()),
            size: Some((1920, 1080)),
            fps: 60,
            gop: 120,
            framed: true,
            quiet: true,
            dmabuf: true,
            lp: true,
            cursor: false,
            ..Opts::default()
        };
        assert_eq!(o, want);
        assert_eq!(p("").unwrap(), Opts::default());
        assert_eq!(p("--render-node /dev/dri/renderD129").unwrap().render_node.as_deref(), Some("/dev/dri/renderD129"));
    }

    #[test]
    fn rejects_bad_values_and_removed_options() {
        for bad in [
            "--fps 0",
            "--kbps 0",
            "--gop 0",
            "--size 1921x1080",
            "--size 1920x1081",
            "--size 0x0",
            "--size 1920",
            "--fps -1",
            "--fps",
            "--slices 2",
            "--repeat-ms 100",
            "--no-grid",
            "--grid",
            "--dmabuf-fourcc XR24",
        ] {
            assert!(p(bad).is_err(), "{bad} accepted");
        }
    }

    #[test]
    fn help_is_the_main_header() {
        let h = p("--help").unwrap_err();
        assert!(h.contains("Usage: wfd-media") && h.contains("--render-node"), "{h}");
    }
}
