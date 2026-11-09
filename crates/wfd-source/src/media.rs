//! The data plane as child processes: `wfd-media > FIFO`, `wfd-tx --in FIFO`.
//!
//! wfd-tx lives for the whole session: the TV does not accept a restarted RTP/TS stream (new SSRC,
//! sequence, continuity counters, PCR jump) and ends the session ~25 s later. The encoder can be
//! restarted (upgrade, output change, crash); wfd-tx reopens the FIFO and the stream stays continuous.
//! Each child has its own process group; dropping `Media` kills both.
//!
//! Captured output: `$XDG_RUNTIME_DIR/sway-miracast/capture-output` (reset to `--output` per session) is read
//! at every encoder restart; SIGUSR2 to wfd-source restarts the encoder on purpose (tools/miracast-screen.sh:
//! mirror the laptop or a separate virtual Sway output). A virtual output (HEADLESS-*) still captured when
//! the session ends is unplugged, so no invisible screen is left behind.
//!
//! Negotiated mode: `$XDG_RUNTIME_DIR/sway-miracast/mode` (`WIDTHxHEIGHT@FPS`), written atomically when the
//! media starts (after PLAY) and removed with `capture-output` when the session ends (scripts size a virtual
//! output to it).
//!
//! Audio clock: captured from a PipeWire null sink (NullSink) that becomes the graph's clock driver
//! (CLOCK_MONOTONIC, the PCR base), not the sound card's monitor (~250-300 ppm slow), which made the TV
//! audio buffer ratchet up over a session (sinks/ Q11).

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

/// Our RTP port: the first `server_port` given in SETUP (session.rs). We only send, the sink never uses it.
pub const RTP_PORT: u16 = 19100;
/// Our RTCP port: the second `server_port` given in SETUP. The sink sends its Receiver Reports there;
/// wfd-tx answers with Sender Reports.
pub const RTCP_PORT: u16 = RTP_PORT + 1;
/// AUDIO_DELAY_MS default: pins audio to the TV buffer floor (min, stable latency).
pub const DEFAULT_AUDIO_DELAY_MS: f64 = -1000.0;
/// VIDEO_HOLD_MS default: no A/V correction (desk mode).
pub const DEFAULT_VIDEO_HOLD_MS: f64 = 0.0;
/// `mux-kbps` tuning file default. Samsung HG50BU800: sinks/ Q10 (thin TS).
pub const DEFAULT_MUX_KBPS: &str = "8000";
/// A child that ignores SIGTERM this long gets SIGKILL.
const STOP_GRACE: Duration = Duration::from_secs(3);

/// SIGUSR2: restart the encoder to pick up a new `capture-output` (not a crash: no restart budget).
pub static RECAPTURE: AtomicBool = AtomicBool::new(false);

pub struct MediaCfg {
    pub output: String,
    pub kbps: u32,
    pub extra_media: Vec<String>,
    /// Capture system audio (PipeWire/Pulse monitor of the default sink) and send it as LPCM.
    pub audio: bool,
    /// Mute the laptop speakers while casting audio (the monitor is taken before mute); restored after.
    pub mute_local: bool,
    /// Per-TV A/V sync correction (env VIDEO_HOLD_MS, camera-calibrated), written to `video-hold-ms` per session.
    pub video_hold_ms: f64,
    /// Env AUDIO_DELAY_MS, written to `audio-delay-ms` per session.
    pub audio_delay_ms: f64,
    /// $XDG_RUNTIME_DIR/sway-miracast: FIFOs and the live tuning files (see `runtime_dir`).
    pub dir: PathBuf,
}

/// One `pactl` call (C locale: output is parsed); Some(trimmed stdout) on success.
fn pactl(args: &[&str]) -> Option<String> {
    let out = Command::new("pactl").args(args).env("LC_ALL", "C").output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

const NULL_SINK: &str = "sway_miracast";

/// Indices of the current playback streams (to move them onto the null sink). parec records from a
/// source, so it is not among them.
fn sink_input_ids() -> Vec<String> {
    pactl(&["list", "short", "sink-inputs"])
        .into_iter()
        .flat_map(|s| s.lines().filter_map(|l| l.split('\t').next().map(str::to_string)).collect::<Vec<_>>())
        .filter(|s| !s.is_empty())
        .collect()
}

/// A PipeWire null sink captured for audio, made the graph's clock driver so the samples are paced by
/// CLOCK_MONOTONIC (our PCR base). App audio is routed here for the session, so it reaches the TV and not
/// the laptop speakers; the sound card is suspended so it stops driving the graph. On drop the previous
/// default sink is unsuspended and restored and the module unloaded (streams reroute back).
struct NullSink {
    module: String,
    prev_default: Option<String>,
}

impl NullSink {
    fn create() -> Option<NullSink> {
        let prev_default = pactl(&["get-default-sink"]).filter(|s| !s.is_empty() && s != NULL_SINK);
        let module = pactl(&[
            "load-module",
            "module-null-sink",
            &format!("sink_name={NULL_SINK}"),
            "rate=48000",
            "channels=2",
            "channel_map=front-left,front-right",
            "sink_properties=device.description=sway-miracast",
        ])?;
        if module.is_empty() || !module.bytes().all(|b| b.is_ascii_digit()) {
            return None; // could not read the module id: do not leave one loaded we cannot unload
        }
        let ns = NullSink { module, prev_default: prev_default.clone() };
        let _ = pactl(&["set-default-sink", NULL_SINK]);
        for id in sink_input_ids() {
            let _ = pactl(&["move-sink-input", &id, NULL_SINK]);
        }
        // the sound card must stop driving the graph, or it would clock the null sink's monitor too
        if let Some(prev) = &prev_default {
            let _ = pactl(&["suspend-sink", prev, "1"]);
        }
        wfd_util::event!("null_sink", "module" => &ns.module);
        Some(ns)
    }
}

impl Drop for NullSink {
    fn drop(&mut self) {
        if let Some(prev) = &self.prev_default {
            let _ = pactl(&["suspend-sink", prev, "0"]);
            let _ = pactl(&["set-default-sink", prev]);
        }
        let _ = pactl(&["unload-module", &self.module]);
    }
}

/// Mutes the default sink for the lifetime of the value and restores the previous state on drop.
struct LocalMute {
    was_muted: bool,
}

impl LocalMute {
    fn new() -> Option<Self> {
        let was_muted = pactl(&["get-sink-mute", "@DEFAULT_SINK@"]).is_some_and(|s| s.contains("yes"));
        pactl(&["set-sink-mute", "@DEFAULT_SINK@", "1"])?;
        Some(LocalMute { was_muted })
    }
}

impl Drop for LocalMute {
    fn drop(&mut self) {
        if !self.was_muted {
            let _ = pactl(&["set-sink-mute", "@DEFAULT_SINK@", "0"]);
        }
    }
}

pub enum Exit {
    Encoder(String),
    Audio(String),
    Sender(String),
}

/// SIGTERM to each running child's process group (all first, so they end in parallel), then wait for all;
/// a child still running after STOP_GRACE gets SIGKILL. Every child must be spawned with `process_group(0)`.
fn stop_groups(children: &mut [&mut Child]) {
    for c in children.iter_mut() {
        // an exited child (maybe reaped by `exited`) is not signalled: its pid may belong to someone else
        if matches!(c.try_wait(), Ok(None)) {
            signal_group(c, libc::SIGTERM);
        }
    }
    let deadline = Instant::now() + STOP_GRACE;
    for c in children.iter_mut() {
        while matches!(c.try_wait(), Ok(None)) {
            if Instant::now() >= deadline {
                signal_group(c, libc::SIGKILL);
                let _ = c.wait();
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn signal_group(c: &Child, sig: libc::c_int) {
    // SAFETY: plain syscall. `c` is our child and try_wait just said it is not reaped (a zombie still holds
    // its pid), spawned with process_group(0), so -pid is its own group.
    unsafe { libc::kill(-(c.id() as i32), sig) };
}

/// The children and files of one session's data plane. Dropping it stops the children (encoder, sender,
/// audio) and removes the files, also when `Media::start` fails half way.
#[derive(Default)]
struct Plane {
    enc: Option<Child>,
    tx: Option<Child>,
    aud: Option<Child>,
    files: Vec<PathBuf>,
}

impl Plane {
    fn stop_children(&mut self) {
        let mut all: Vec<&mut Child> = [&mut self.enc, &mut self.tx, &mut self.aud].into_iter().flatten().collect();
        stop_groups(&mut all);
        (self.enc, self.tx, self.aud) = (None, None, None);
    }
}

impl Drop for Plane {
    fn drop(&mut self) {
        self.stop_children();
        for f in &self.files {
            let _ = std::fs::remove_file(f);
        }
    }
}

pub struct Media {
    plane: Plane,
    enc_spec: Spec,
    aud_spec: Option<Spec>,
    /// `capture-output` file and the output given on the command line
    out_file: PathBuf,
    default_output: String,
    _null: Option<NullSink>,
    _mute: Option<LocalMute>,
}

/// The output named in `capture-output`, or `default` when the file is missing or empty.
fn read_output(f: &Path, default: &str) -> String {
    std::fs::read_to_string(f).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or(default.into())
}

/// Encoder arguments with the `--output` value replaced.
fn with_output(args: &[String], output: &str) -> Vec<String> {
    let mut a = args.to_vec();
    if let Some(i) = a.iter().position(|x| x == "--output") {
        if let Some(v) = a.get_mut(i + 1) {
            *v = output.to_string();
        }
    }
    a
}

fn sibling(name: &str) -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(name))).unwrap_or_else(|| name.into())
}

/// A child that writes to a FIFO: program, arguments, FIFO path. Spawned directly (no `sh -c "exec … > fifo"`),
/// so the pid is the program from the first instant (SIGUSR1 for an IDR cannot hit a shell before its exec).
struct Spec {
    prog: PathBuf,
    args: Vec<String>,
    out: PathBuf,
}

impl Spec {
    fn spawn(&self) -> std::io::Result<Child> {
        use std::os::unix::fs::OpenOptionsExt;
        // read-write: opening a FIFO this way never blocks, even before wfd-tx has opened its end
        let out = std::fs::OpenOptions::new().read(true).write(true).custom_flags(libc::O_CLOEXEC).open(&self.out)?;
        Command::new(&self.prog).args(&self.args).stdout(out).process_group(0).spawn()
    }
}

/// `mode` file text: the negotiated mode for scripts, e.g. "1920x1080@60".
fn mode_text(w: u16, h: u16, fps: u8) -> String {
    format!("{w}x{h}@{fps}\n")
}

/// Write `text` to `f` atomically (a temporary file in the same directory, then rename), so a reader never
/// sees a half-written file.
fn write_atomic(f: &Path, text: &str) -> std::io::Result<()> {
    let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = f.with_file_name(format!(".{name}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, f)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// $XDG_RUNTIME_DIR/sway-miracast: FIFOs and the live tuning files.
pub fn runtime_dir() -> PathBuf {
    PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())).join("sway-miracast")
}

/// A fresh FIFO `<name>-<pid>.fifo` in `dir`.
fn make_fifo(dir: &Path, name: &str) -> std::io::Result<PathBuf> {
    let path = dir.join(format!("{name}-{}.fifo", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    // SAFETY: `c` is a valid NUL-terminated path (CString).
    if unsafe { libc::mkfifo(c.as_ptr(), 0o600) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(path)
}

/// A live tuning file (re-read by wfd-tx on SIGHUP), created with `default` if missing.
fn tuning_file(dir: &Path, name: &str, default: &str) -> PathBuf {
    let f = dir.join(name);
    if !f.exists() {
        let _ = std::fs::write(&f, format!("{default}\n"));
    }
    f
}

/// wfd-tx arguments. --framed: exact AU boundaries (wfd-media writes the same framing).
/// `hold`: the "film" mode file (video hold in ms, 0 = desk mode; tools/miracast-film.sh).
fn tx_args(dest: &str, video: &Path, hold: &Path, audio: Option<(&Path, &Path, &Path)>) -> Vec<std::ffi::OsString> {
    let mut a: Vec<std::ffi::OsString> =
        ["--dest", dest, "--quiet", "--framed", "--in"].iter().map(Into::into).collect();
    a.push(video.into());
    a.push("--video-hold-file".into());
    a.push(hold.into());
    a.push("--rtcp-port".into());
    a.push(RTCP_PORT.to_string().into());
    if let Some(dir) = hold.parent().filter(|d| !d.as_os_str().is_empty()) {
        // one line per sink report, for correlating audio latency jumps with the radio
        a.push("--rtcp-log".into());
        a.push(dir.join("rtcp.jsonl").into());
    }
    if let Some((fifo, delay, kbps)) = audio {
        for (k, v) in [("--audio", fifo), ("--audio-delay-file", delay), ("--mux-kbps-file", kbps)] {
            a.push(k.into());
            a.push(v.into());
        }
    }
    a
}

/// One tool, one job: parec captures `source`, wfd-tx packs (WFD LPCM: s16be, 48 kHz, stereo).
fn parec_spec(fifo: &Path, source: &str) -> Spec {
    let args = [
        "-d",
        source,
        "--format=s16be",
        "--rate=48000",
        "--channels=2",
        "--latency-msec=10",
        "--client-name=wfd-source",
    ];
    Spec { prog: "parec".into(), args: args.map(ToString::to_string).to_vec(), out: fifo.to_path_buf() }
}

fn encoder_spec(cfg: &MediaCfg, w: u16, h: u16, fps: u8, fifo: &Path) -> Spec {
    let mut args: Vec<String> = vec![
        "--output".into(),
        cfg.output.clone(),
        "--size".into(),
        format!("{w}x{h}"),
        "--fps".into(),
        fps.to_string(),
        "--kbps".into(),
        cfg.kbps.to_string(),
        // IDR every 2 s: with CBR and a ~2-frame CPB, an IDR every 0.5 s (default 30 at 60 fps)
        // pulses quality and AU size; the sink asks for one on errors (wfd_idr_request)
        "--gop".into(),
        (fps as u32 * 2).to_string(),
        "--framed".into(),
        "--quiet".into(),
        // zero-copy capture (linear dmabuf: less CPU, frames identical to shm) and the low-power encoder;
        // both fall back on their own (shm / vah264enc)
        "--dmabuf".into(),
        "--lp".into(),
    ];
    args.extend(cfg.extra_media.iter().cloned());
    Spec { prog: sibling("wfd-media"), args, out: fifo.to_path_buf() }
}

impl Media {
    /// Spawn order matters: wfd-tx before the encoder (it opens the FIFO), the null sink before parec.
    pub fn start(cfg: &MediaCfg, w: u16, h: u16, fps: u8, sink_ip: &str, sink_port: u16) -> std::io::Result<Self> {
        let dir = &cfg.dir;
        std::fs::create_dir_all(dir)?;
        let mut plane = Plane::default();
        let fifo = make_fifo(dir, "video")?;
        plane.files.push(fifo.clone());
        let afifo = if cfg.audio { Some(make_fifo(dir, "audio")?) } else { None };
        plane.files.extend(afifo.clone());
        let tuning = afifo.as_ref().map(|a| {
            // live: echo VALUE > $XDG_RUNTIME_DIR/sway-miracast/<file> && pkill -HUP -x wfd-tx
            // AUDIO_DELAY_MS pins audio to the TV buffer floor (min, stable latency); force per session
            let delay = dir.join("audio-delay-ms");
            let _ = std::fs::write(&delay, format!("{}\n", cfg.audio_delay_ms));
            let kbps = tuning_file(dir, "mux-kbps", DEFAULT_MUX_KBPS);
            (a.clone(), delay, kbps)
        });
        // VIDEO_HOLD_MS is the per-TV A/V sync correction (camera-calibrated), applied every session so
        // lip-sync is right automatically; the miracast-film binding (contrib/sway.conf) can still toggle to 0 for a responsive desktop
        let hold = dir.join("video-hold-ms");
        std::fs::write(&hold, format!("{}\n", cfg.video_hold_ms))?;
        let args = tx_args(
            &format!("{sink_ip}:{sink_port}"),
            &fifo,
            &hold,
            tuning.as_ref().map(|(a, d, k)| (a.as_path(), d.as_path(), k.as_path())),
        );
        plane.tx = Some(Command::new(sibling("wfd-tx")).args(args).process_group(0).spawn()?);
        // capture from a null sink clocked by the system clock; fall back to the sound-card monitor
        let null = if cfg.audio { NullSink::create() } else { None };
        let source = if null.is_some() { format!("{NULL_SINK}.monitor") } else { "@DEFAULT_MONITOR@".into() };
        let aud_spec = afifo.as_deref().map(|a| parec_spec(a, &source));
        // every session starts mirroring the --output screen
        let out_file = dir.join("capture-output");
        plane.files.push(out_file.clone());
        std::fs::write(&out_file, format!("{}\n", cfg.output))?;
        let mode_file = dir.join("mode");
        plane.files.push(mode_file.clone());
        write_atomic(&mode_file, &mode_text(w, h, fps))?;
        RECAPTURE.store(false, std::sync::atomic::Ordering::Relaxed);
        let enc_spec = encoder_spec(cfg, w, h, fps, &fifo);
        plane.enc = Some(enc_spec.spawn()?);
        // a failed parec start is reported by exited() as Exit::Audio and retried like a crash
        plane.aud = aud_spec.as_ref().and_then(|a| a.spawn().ok());
        // the null sink already routes audio off the laptop speakers; muting is only for the fallback
        let _mute = if cfg.audio && cfg.mute_local && null.is_none() { LocalMute::new() } else { None };
        Ok(Media { plane, enc_spec, aud_spec, out_file, default_output: cfg.output.clone(), _null: null, _mute })
    }

    pub fn exited(&mut self) -> Option<Exit> {
        match self.plane.tx.as_mut().map(Child::try_wait) {
            Some(Ok(Some(s))) => return Some(Exit::Sender(format!("wfd-tx {s}"))),
            None => return Some(Exit::Sender("wfd-tx not running".into())),
            _ => {}
        }
        match self.plane.enc.as_mut().map(Child::try_wait) {
            Some(Ok(Some(s))) => return Some(Exit::Encoder(format!("wfd-media {s}"))),
            None => return Some(Exit::Encoder("wfd-media did not start".into())),
            _ => {}
        }
        match self.plane.aud.as_mut() {
            Some(a) => {
                if let Ok(Some(s)) = a.try_wait() {
                    return Some(Exit::Audio(format!("parec {s}")));
                }
            }
            None if self.aud_spec.is_some() => return Some(Exit::Audio("parec did not start".into())),
            None => {}
        }
        None
    }

    /// Restart only the encoder; wfd-tx keeps the RTP/TS stream continuous.
    pub fn restart_encoder(&mut self) -> std::io::Result<()> {
        if let Some(mut enc) = self.plane.enc.take() {
            stop_groups(&mut [&mut enc]);
        }
        self.enc_spec.args = with_output(&self.enc_spec.args, &self.output());
        self.plane.enc = Some(self.enc_spec.spawn()?);
        Ok(())
    }

    /// The output the encoder captures from its next start on.
    pub fn output(&self) -> String {
        read_output(&self.out_file, &self.default_output)
    }

    /// Restart the audio capture (wfd-tx sends silence meanwhile).
    pub fn restart_audio(&mut self) -> std::io::Result<()> {
        let Some(spec) = &self.aud_spec else { return Ok(()) };
        if let Some(mut a) = self.plane.aud.take() {
            stop_groups(&mut [&mut a]);
        }
        self.plane.aud = Some(spec.spawn()?);
        Ok(())
    }

    pub fn idr(&self) {
        let Some(enc) = &self.plane.enc else { return };
        // SAFETY: `enc` is our not yet reaped child, spawned directly (no shell), so the pid is wfd-media's.
        // Right after spawn, before wfd-media installs its handler, SIGUSR1 would still end it (= one encoder
        // restart); the sink asks for an IDR only after it has video, i.e. well after the handler is in place.
        unsafe { libc::kill(enc.id() as i32, libc::SIGUSR1) };
    }
}

impl Drop for Media {
    /// Children first, then the virtual output; `plane` then removes the files (capture-output last read
    /// here) and `_null`/`_mute` restore the audio routing.
    fn drop(&mut self) {
        self.plane.stop_children();
        let out = self.output();
        if out.starts_with("HEADLESS-") {
            // the virtual TV screen is useless without the TV: its windows go back to the laptop
            let _ = Command::new("swaymsg").args(["output", &out, "unplug"]).output();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_and_sender_agree_on_framing_and_gop_is_two_seconds() {
        let cfg = MediaCfg {
            output: "eDP-1".into(),
            kbps: 8000,
            extra_media: vec!["--no-cursor".into()],
            audio: true,
            mute_local: false,
            video_hold_ms: DEFAULT_VIDEO_HOLD_MS,
            audio_delay_ms: DEFAULT_AUDIO_DELAY_MS,
            dir: PathBuf::from("/nonexistent"),
        };
        let enc = encoder_spec(&cfg, 1920, 1080, 60, Path::new("/run/v.fifo"));
        let a = enc.args.join(" ");
        assert!(a.contains("--size 1920x1080 --fps 60 --kbps 8000 --gop 120 --framed"), "{a}");
        assert!(a.ends_with("--no-cursor"), "extra args last: {a}");
        let tx = tx_args("10.42.0.103:19000", Path::new("/run/v.fifo"), Path::new("vh"), None);
        let t: Vec<String> = tx.iter().map(|s| s.to_string_lossy().into_owned()).collect();
        assert_eq!(
            t,
            [
                "--dest",
                "10.42.0.103:19000",
                "--quiet",
                "--framed",
                "--in",
                "/run/v.fifo",
                "--video-hold-file",
                "vh",
                "--rtcp-port",
                "19101"
            ]
        );
        let tx =
            tx_args("d", Path::new("v"), Path::new("vh"), Some((Path::new("a"), Path::new("ad"), Path::new("mk"))));
        let t: Vec<String> = tx.iter().map(|s| s.to_string_lossy().into_owned()).collect();
        assert_eq!(&t[10..], ["--audio", "a", "--audio-delay-file", "ad", "--mux-kbps-file", "mk"]);
    }

    #[test]
    fn capture_output_from_file_or_default() {
        let args: Vec<String> = ["--output", "eDP-1", "--fps", "60"].map(String::from).to_vec();
        assert_eq!(with_output(&args, "HEADLESS-1"), ["--output", "HEADLESS-1", "--fps", "60"]);
        let d = std::env::temp_dir().join(format!("wfd-out-{}", std::process::id()));
        assert_eq!(read_output(&d, "eDP-1"), "eDP-1");
        std::fs::write(&d, "HEADLESS-2\n").unwrap();
        assert_eq!(read_output(&d, "eDP-1"), "HEADLESS-2");
        std::fs::write(&d, "  \n").unwrap();
        assert_eq!(read_output(&d, "eDP-1"), "eDP-1");
        let _ = std::fs::remove_file(&d);
    }

    #[test]
    fn mode_file_is_written_whole() {
        assert_eq!(mode_text(1920, 1080, 60), "1920x1080@60\n");
        let d = std::env::temp_dir().join(format!("wfd-mode-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("mode");
        write_atomic(&f, "1280x720@30\n").unwrap();
        write_atomic(&f, &mode_text(1920, 1080, 60)).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "1920x1080@60\n");
        let names: Vec<_> = std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).collect();
        assert_eq!(names, [std::ffi::OsString::from("mode")], "no temporary file left");
        assert!(write_atomic(&d.join("missing/mode"), "x").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn parec_matches_wfd_lpcm() {
        let p = parec_spec(Path::new("/run/a.fifo"), "@DEFAULT_MONITOR@");
        assert!(p.args.iter().any(|a| a == "--format=s16be"));
        assert!(p.args.iter().any(|a| a == "--rate=48000"));
        assert!(p.args.iter().any(|a| a == "--channels=2"));
    }

    #[test]
    fn stop_groups_ends_children_and_kills_the_stubborn_one() {
        let spawn = |script: &str| Command::new("sh").args(["-c", script]).process_group(0).spawn().unwrap();
        let mut polite = spawn("exec sleep 30");
        let mut stubborn = spawn("trap '' TERM; while :; do sleep 0.05; done");
        std::thread::sleep(Duration::from_millis(100)); // let the trap be installed
        let t0 = Instant::now();
        stop_groups(&mut [&mut polite, &mut stubborn]);
        assert!(t0.elapsed() >= STOP_GRACE && t0.elapsed() < STOP_GRACE + Duration::from_secs(2));
        assert!(polite.try_wait().unwrap().is_some());
        assert!(stubborn.try_wait().unwrap().is_some());
    }

    #[test]
    fn failed_start_leaves_no_fifos_behind() {
        let dir = std::env::temp_dir().join(format!("wfd-media-start-{}", std::process::id()));
        let cfg = MediaCfg {
            output: "eDP-1".into(),
            kbps: 8000,
            extra_media: vec![],
            audio: false,
            mute_local: false,
            video_hold_ms: 0.0,
            audio_delay_ms: 0.0,
            dir: dir.clone(),
        };
        // the hold file cannot be written where a directory is in the way: start fails after the FIFO
        std::fs::create_dir_all(dir.join("video-hold-ms")).unwrap();
        assert!(Media::start(&cfg, 1280, 720, 30, "127.0.0.1", 9).is_err());
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).collect();
        assert_eq!(left, [std::ffi::OsString::from("video-hold-ms")]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
