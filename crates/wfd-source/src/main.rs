//! wfd-source: Miracast (Wi-Fi Display R1) source daemon — mirrors a wlroots output to one TV.
//!
//! One job: keep a WFD session with the selected sink alive. The data plane runs as child processes
//! (`wfd-media | wfd-tx`); this process does Wi-Fi Direct (NetworkManager D-Bus), RTSP and supervision.
//!
//! State machine (each state has a deadline; failures back off exponentially with jitter, 2 s .. 60 s):
//!   WAIT_SINK  (sink not discoverable: TV not in Smart View)  -> CONNECT
//!   CONNECT    (P2P group formation, <= 45 s)                  -> RTSP   | fail -> WAIT_SINK
//!   RTSP       (sink opens TCP 7236, <= 40 s; M1-M7)           -> CASTING| fail -> WAIT_SINK
//!   CASTING    (keepalive every 10 s, sink requests, media)    -> WAIT_SINK on any loss
//! A sink TEARDOWN (user stopped mirroring on the TV) is not an error: back to WAIT_SINK quietly.
//! SIGTERM/SIGINT: TEARDOWN trigger, stop media, deactivate the P2P connection, exit 0.
//! SIGUSR2: restart the encoder on the output named in `capture-output` (media.rs, tools/miracast-screen.sh).
//! systemd: Type=notify (READY=1, STATUS=..., WATCHDOG=1).
//!
//! Usage: wfd-source [--sink MAC] [--output auto|NAME] [--mode 1920x1080p60[,1920x1080p30,...]] [--kbps 12000]
//!                   [--find-secs 4] [--scan-every 4] [--no-audio] [--keep-local-audio] [--notify-cmd 'CMD'] [-- extra wfd-media args]
//! --notify-cmd: a shell command run (`sh -c`) on each change of what the user should know, with the environment
//! WFD_EVENT = waiting (TV not found: turn on Smart View) | connecting | casting | failed (could not connect) |
//! ended (the session with the TV is over), WFD_SINK = the TV's MAC, WFD_QUIET = 1 when only a status display
//! should follow (a waiting right after failed/ended: that message stays on screen), else 0. The command picks
//! the words (translations live with the tools, e.g. `miracast-status --notify-event`).
//! TV (sink) selection, first that is set: --sink MAC, env TV_MAC (non-empty), the line `TV_MAC=AA:BB:CC:DD:EE:FF`
//! in `$XDG_STATE_HOME/sway-miracast/sink` (default ~/.local/state/sway-miracast/sink, written by miracast-menu).
//! None: event `no_sink`, STATUS "no TV selected (miracast-menu)", exit 0. A malformed MAC: exit 2.
//! On-demand TVs (some wake from standby when a source connects, so connecting on its own at login would switch
//! the TV on): their MACs, one per line, in `$XDG_STATE_HOME/sway-miracast/on-demand` (miracast-menu, "TV
//! settings"). Such a TV is connected only when `$XDG_RUNTIME_DIR/sway-miracast/connect-now` holds its MAC
//! (miracast-menu writes it on a pick); otherwise event `on_demand_skip`, exit 0. After a session that reached
//! PLAY ends, the marker is removed and wfd-source exits 0 instead of waiting for the TV again.
//! --output: the screen to mirror, default `auto` (env MONITOR when non-empty; --output wins). Passed to
//! wfd-media unchanged (`auto` = the built-in panel there: first eDP*/LVDS*/DSI* output, else the first one) and
//! written per session to `$XDG_RUNTIME_DIR/sway-miracast/capture-output`.
//! Session files in `$XDG_RUNTIME_DIR/sway-miracast/` (removed when the session ends): `capture-output`, and
//! `mode` = the negotiated mode as `WIDTHxHEIGHT@FPS` (e.g. 1920x1080@60), written atomically after PLAY.
//! Sink extensions: `wfd_sec_view_mode: full` (after PLAY and when the sink reports `multi`) and
//! `wfd_sec_low_latency_mode` (3 s after PLAY, only to a sink that has sent a `wfd_sec_*` parameter itself).
//! Events: one JSON line per event on stderr (journald), `{"event":...,"t":UNIX_US}` (wfd_util::event).
//! Environment (read once at start, unit EnvironmentFile): TV_MAC, MONITOR (above), WFD_RTSP_PORT (tests),
//! SEC_LOW_LATENCY_MODE (on|off, default on), VIDEO_HOLD_MS (default 0), AUDIO_DELAY_MS (default -1000).

mod media;
mod nm;
mod session;

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use wfd_util::{event, Backoff};
use zbus::zvariant::OwnedObjectPath;

static STOP: AtomicBool = AtomicBool::new(false);
pub fn stopping() -> bool {
    STOP.load(Ordering::Relaxed)
}

/// sd_notify without libsystemd.
pub fn notify(msg: &str) {
    let Ok(path) = std::env::var("NOTIFY_SOCKET") else { return };
    let Ok(sock) = std::os::unix::net::UnixDatagram::unbound() else { return };
    let _ = if let Some(name) = path.strip_prefix('@') {
        use std::os::linux::net::SocketAddrExt;
        std::os::unix::net::SocketAddr::from_abstract_name(name).and_then(|a| sock.send_to_addr(msg.as_bytes(), &a))
    } else {
        sock.send_to(msg.as_bytes(), &path)
    };
}

/// What the user is told about through --notify-cmd (WFD_EVENT).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UserEvent {
    /// the TV is not discoverable (not in Smart View)
    Waiting,
    /// the TV was found, the P2P group and RTSP are being set up
    Connecting,
    /// PLAY: the picture goes to the TV
    Casting,
    /// a connection attempt failed before the picture reached the TV
    Failed,
    /// a session that was casting is over (TEARDOWN from the TV or a lost link)
    Ended,
}

impl UserEvent {
    fn key(self) -> &'static str {
        match self {
            UserEvent::Waiting => "waiting",
            UserEvent::Connecting => "connecting",
            UserEvent::Casting => "casting",
            UserEvent::Failed => "failed",
            UserEvent::Ended => "ended",
        }
    }
}

/// Whether `ev` after `prev` is announced: None = no change (the same state again, e.g. each WAIT_SINK
/// cycle), Some(quiet). A waiting right after failed/ended is quiet: that message tells the user more.
fn announce_plan(prev: Option<UserEvent>, ev: UserEvent) -> Option<bool> {
    if prev == Some(ev) {
        return None;
    }
    Some(ev == UserEvent::Waiting && matches!(prev, Some(UserEvent::Failed | UserEvent::Ended)))
}

/// (--notify-cmd, sink MAC); set once in main, absent in the tests.
static ANNOUNCE_TO: OnceLock<(String, String)> = OnceLock::new();
static LAST_EVENT: Mutex<Option<UserEvent>> = Mutex::new(None);

fn last_event() -> Option<UserEvent> {
    LAST_EVENT.lock().map(|l| *l).unwrap_or(None)
}

/// Record a state change and run --notify-cmd for it (see the module docs). Nothing while stopping: the
/// one who stopped the service says so. Returns true when the state changed.
pub fn user_event(ev: UserEvent) -> bool {
    if stopping() {
        return false;
    }
    let quiet = {
        let Ok(mut last) = LAST_EVENT.lock() else { return false };
        let Some(quiet) = announce_plan(*last, ev) else { return false };
        *last = Some(ev);
        quiet
    };
    event!("user_event", "state" => ev.key(), "quiet" => quiet);
    if let Some((cmd, sink)) = ANNOUNCE_TO.get() {
        let child = std::process::Command::new("sh")
            .args(["-c", cmd])
            .env("WFD_EVENT", ev.key())
            .env("WFD_SINK", sink)
            .env("WFD_QUIET", if quiet { "1" } else { "0" })
            .spawn();
        // reap it in a thread: no zombie per event, and a notify-send that hangs (no notification
        // daemon) does not block the service
        if let Ok(mut child) = child {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
    true
}

#[derive(Debug)]
struct Opts {
    /// from --sink only; the effective sink comes from `resolve_sink`
    sink: Option<String>,
    output: String,
    modes: Vec<(u16, u16, u8)>,
    kbps: u32,
    find_secs: u64,
    scan_every: u64,
    notify_cmd: Option<String>,
    no_p2p: bool,
    audio: bool,
    keep_local_audio: bool,
    extra_media: Vec<String>,
}

fn parse_mode(s: &str) -> Option<(u16, u16, u8)> {
    let (wh, fps) = s.split_once('p')?;
    let (w, h) = wh.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?, fps.parse().ok()?))
}

/// Command line (without the program name); `env` supplies MONITOR. Err = message or help text, exit 2.
fn parse_args(args: impl IntoIterator<Item = String>, env: impl Fn(&str) -> Option<String>) -> Result<Opts, String> {
    let mut o = Opts {
        sink: None,
        output: env("MONITOR").map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or("auto".into()),
        modes: vec![(1920, 1080, 60), (1920, 1080, 30), (1280, 720, 60)],
        kbps: 12000,
        find_secs: 4,
        scan_every: 4,
        notify_cmd: None,
        no_p2p: false,
        audio: true,
        keep_local_audio: false,
        extra_media: vec![],
    };
    let mut a = args.into_iter();
    while let Some(k) = a.next() {
        let mut v = || a.next().ok_or(format!("{k} needs a value"));
        match k.as_str() {
            "--sink" => o.sink = Some(v()?),
            "--output" => o.output = v()?,
            "--mode" => {
                o.modes =
                    v()?.split(',').map(|m| parse_mode(m).ok_or(format!("bad mode {m}"))).collect::<Result<_, _>>()?
            }
            "--kbps" => o.kbps = v()?.parse().map_err(|_| "--kbps")?,
            "--find-secs" => o.find_secs = v()?.parse().map_err(|_| "--find-secs")?,
            "--scan-every" => o.scan_every = v()?.parse().map_err(|_| "--scan-every")?,
            "--notify-cmd" => o.notify_cmd = Some(v()?),
            "--no-audio" => o.audio = false,
            "--keep-local-audio" => o.keep_local_audio = true,
            "--no-p2p" => o.no_p2p = true, // testing: sink connects over an existing network
            "--" => o.extra_media = a.by_ref().collect(),
            "-h" | "--help" => return Err(wfd_util::doc_help(include_str!("main.rs"))),
            _ => return Err(format!("unknown option {k}")),
        }
    }
    if o.output.is_empty() {
        return Err("--output needs a name (or auto)".into());
    }
    Ok(o)
}

/// `$XDG_STATE_HOME/sway-miracast/sink` (XDG: a relative or empty XDG_STATE_HOME is ignored), else
/// `$HOME/.local/state/sway-miracast/sink`. None without either variable.
fn sink_file(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    Some(state_dir(env)?.join("sink"))
}

/// `$XDG_STATE_HOME/sway-miracast` (see `sink_file`).
fn state_dir(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let base = match env("XDG_STATE_HOME").filter(|d| d.starts_with('/')) {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(env("HOME").filter(|h| !h.is_empty())?).join(".local/state"),
    };
    Some(base.join("sway-miracast"))
}

/// Whether `mac` is listed (one MAC per line, any case, `#` comments) in the on-demand file's text.
fn listed(text: &str, mac: &str) -> bool {
    text.lines().map(|l| l.split('#').next().unwrap_or("").trim()).any(|l| l.eq_ignore_ascii_case(mac))
}

/// The runtime marker `connect-now`: miracast-menu writes the picked TV's MAC there.
fn connect_now_file() -> PathBuf {
    media::runtime_dir().join("connect-now")
}

/// The `TV_MAC=` value in the sink file (shell syntax as written by miracast-menu: optional `export`,
/// quotes, comments); None when the file has no non-empty TV_MAC.
fn sink_from_file(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("export ").unwrap_or(l).trim_start().strip_prefix("TV_MAC="))
        .map(|v| v.trim().trim_matches(['"', '\'']).trim().to_string())
        .rfind(|v| !v.is_empty())
}

/// A MAC address as six hex pairs separated by `:` (NetworkManager's peer HwAddress format).
fn valid_mac(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    parts.len() == 6 && parts.iter().all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The sink to cast to, first that is set: --sink, env TV_MAC (non-empty), the sink file. Ok(None): no TV
/// selected; Err: the chosen value is not a MAC (with where it came from).
fn resolve_sink(
    cli: Option<&str>,
    env: impl Fn(&str) -> Option<String>,
    read: impl Fn(&std::path::Path) -> Option<String>,
) -> Result<Option<String>, String> {
    let file = || {
        let f = sink_file(&env)?;
        sink_from_file(&read(&f)?).map(|m| (m, f.display().to_string()))
    };
    let chosen = cli
        .map(|m| (m.trim().to_string(), "--sink".to_string()))
        .or_else(|| env("TV_MAC").map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).map(|m| (m, "TV_MAC".into())))
        .or_else(file);
    match chosen {
        None => Ok(None),
        Some((m, _)) if valid_mac(&m) => Ok(Some(m)),
        Some((m, from)) => Err(format!("{from}: not a MAC address: {m:?}")),
    }
}

/// P2P group formation (NM activation) may take this long.
const CONNECT_DEADLINE: Duration = Duration::from_secs(45);
/// After the group is up, the TV opens RTSP within ~10-15 s; give it this long.
const RTSP_DEADLINE: Duration = Duration::from_secs(40);
/// --no-p2p (tests): wait this long for a sink on the existing network.
const NO_P2P_ACCEPT: Duration = Duration::from_secs(3600);
/// A session that lasted this long resets the backoff.
const SESSION_RESETS_BACKOFF: Duration = Duration::from_secs(60);
/// After a TEARDOWN from the TV its cached "available" advertisement is stale for this long (s).
const TEARDOWN_IGNORE_S: u64 = 5;
const BACKOFF_MIN: Duration = Duration::from_secs(2);
const BACKOFF_MAX: Duration = Duration::from_secs(60);

/// Settings from the environment (unit EnvironmentFile ~/.config/sway-miracast/env), read once at start,
/// so recalibrating A/V sync is an edit there, not a rebuild.
#[derive(Debug, Clone, PartialEq)]
struct EnvCfg {
    rtsp_port: u16,
    low_latency: Option<String>,
    video_hold_ms: f64,
    audio_delay_ms: f64,
}

impl EnvCfg {
    fn from(get: impl Fn(&str) -> Option<String>) -> Self {
        let f64_or = |k: &str, d: f64| get(k).and_then(|s| s.trim().parse().ok()).unwrap_or(d);
        EnvCfg {
            rtsp_port: get("WFD_RTSP_PORT").and_then(|p| p.parse().ok()).unwrap_or(session::RTSP_PORT),
            low_latency: session::low_latency_mode(get("SEC_LOW_LATENCY_MODE")),
            video_hold_ms: f64_or("VIDEO_HOLD_MS", media::DEFAULT_VIDEO_HOLD_MS),
            audio_delay_ms: f64_or("AUDIO_DELAY_MS", media::DEFAULT_AUDIO_DELAY_MS),
        }
    }
}

fn sleep_interruptible(d: Duration) {
    let t0 = Instant::now();
    while t0.elapsed() < d && !stopping() {
        std::thread::sleep(Duration::from_millis(200).min(d));
        notify("WATCHDOG=1");
    }
}

/// Everything a cast attempt needs.
struct Ctx {
    o: Opts,
    /// the sink's MAC (`resolve_sink`)
    sink: String,
    nm: nm::Nm,
    listener: TcpListener,
    cfg: session::SessionCfg,
    /// between failed attempts: 2 s doubling to 60 s, ±25 % jitter
    backoff: Backoff,
    /// on-demand TV: exit after the first session that reached PLAY (do not wake the TV again)
    once: bool,
}

fn main() {
    let env = |k: &str| std::env::var(k).ok();
    let o = parse_args(std::env::args().skip(1), env).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2)
    });
    let sink = match resolve_sink(o.sink.as_deref(), env, |f| std::fs::read_to_string(f).ok()) {
        Ok(Some(m)) => m,
        Ok(None) => {
            // not a failure: the unit (Restart=on-failure) stays stopped until miracast-menu picks a TV.
            // READY first, so a Type=notify unit ends as a clean stop, not a start-up protocol error.
            event!("no_sink", "file" => &sink_file(env).map(|f| f.display().to_string()).unwrap_or_default());
            notify("READY=1\nSTATUS=no TV selected (miracast-menu)");
            std::process::exit(0)
        }
        Err(e) => {
            event!("bad_sink", "error" => &e);
            std::process::exit(2)
        }
    };
    wfd_util::install_panic_hook("wfd-source");
    for s in [libc::SIGTERM, libc::SIGINT] {
        wfd_util::signal_flag(s, &STOP);
    }
    wfd_util::signal_flag(libc::SIGUSR2, &media::RECAPTURE);
    // SAFETY: setting SIGPIPE to SIG_IGN has no preconditions (broken pipes then return EPIPE).
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) };
    if o.audio {
        // also before an on-demand skip: a crashed session must not leave the laptop without sound
        media::restore_leftover_audio(&media::runtime_dir());
    }
    let read = |f: Option<PathBuf>| f.and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
    let on_demand = o.sink.is_none() && listed(&read(state_dir(env).map(|d| d.join("on-demand"))), &sink);
    if on_demand && !listed(&read(Some(connect_now_file())), &sink) {
        // not a failure either: an on-demand TV waits for a pick in miracast-menu (it may wake from standby)
        event!("on_demand_skip", "sink" => &sink);
        notify("READY=1\nSTATUS=on-demand TV: pick it in miracast-menu");
        std::process::exit(0)
    }
    if let Some(cmd) = &o.notify_cmd {
        let _ = ANNOUNCE_TO.set((cmd.clone(), sink.clone()));
    }
    let mut c = setup(o, sink, EnvCfg::from(env));
    c.once = on_demand;
    notify("READY=1");
    event!("start", "sink" => &c.sink, "output" => &c.o.output);
    while !stopping() {
        let step = if c.o.no_p2p { serve_no_p2p(&mut c) } else { cast_once(&mut c) };
        if let Step::Stop = step {
            break;
        }
    }
    if !c.o.no_p2p {
        c.nm.deactivate_ours();
    }
    notify("STOPPING=1");
    event!("stop");
    // dropping `c` ends the journal watcher (nm)
}

/// What the main loop does after one attempt.
enum Step {
    Retry,
    Stop,
}

fn setup(o: Opts, sink: String, env: EnvCfg) -> Ctx {
    let nm = nm::Nm::new().unwrap_or_else(|e| wfd_util::die("fatal", &e));
    if !o.no_p2p {
        nm.deactivate_ours(); // leftovers from a crash (never in test mode: it would kill a live session)
    }
    let port = env.rtsp_port;
    let listener =
        TcpListener::bind(("0.0.0.0", port)).unwrap_or_else(|e| wfd_util::die("fatal", &format!("bind {port}: {e}")));
    let cfg = session::SessionCfg {
        modes: o.modes.clone(),
        media: media::MediaCfg {
            output: o.output.clone(),
            kbps: o.kbps,
            extra_media: o.extra_media.clone(),
            audio: o.audio,
            mute_local: !o.keep_local_audio,
            video_hold_ms: env.video_hold_ms,
            audio_delay_ms: env.audio_delay_ms,
            dir: media::runtime_dir(),
        },
        low_latency: env.low_latency,
    };
    Ctx { o, sink, nm, listener, cfg, backoff: Backoff::new(BACKOFF_MIN, BACKOFF_MAX), once: false }
}

fn backoff_wait(c: &mut Ctx) {
    sleep_interruptible(c.backoff.next_jittered());
}

/// --no-p2p (tests): one session with a sink on the existing network.
fn serve_no_p2p(c: &mut Ctx) -> Step {
    let end = match session::accept(&c.listener, NO_P2P_ACCEPT) {
        Ok((s, ip)) => {
            event!("sink_connected", "ip" => &ip);
            session::run(s, &ip, &c.cfg, &|| true)
        }
        Err(e) => session::End::Failed(e),
    };
    event!("session_end", "end" => &format!("{end:?}"));
    if matches!(end, session::End::Stopped) {
        return Step::Stop;
    }
    backoff_wait(c);
    Step::Retry
}

/// One pass of WAIT_SINK -> CONNECT -> RTSP -> CASTING (see the state machine above).
fn cast_once(c: &mut Ctx) -> Step {
    let peer = match wait_sink(c) {
        Ok(p) => p,
        Err(step) => return step,
    };
    let ac = match connect(c, &peer) {
        Ok(ac) => ac,
        Err(step) => return step,
    };
    run_session(c, &ac)
}

/// WAIT_SINK: a short find for the TV; Err(step) when there is nothing to connect to now.
fn wait_sink(c: &mut Ctx) -> Result<OwnedObjectPath, Step> {
    notify("STATUS=waiting for the TV (Smart View)");
    match c.nm.find(&c.sink, Duration::from_secs(c.o.find_secs)) {
        Ok(Some(p)) => Ok(p),
        Ok(None) if stopping() => Err(Step::Stop),
        Ok(None) => {
            if user_event(UserEvent::Waiting) {
                event!("wait_sink");
            }
            // The TV is discoverable only for a short window after entering Smart View (its screen
            // keeps showing "waiting" after that). Scan at a steady cadence, no backoff here.
            sleep_interruptible(Duration::from_secs(c.o.scan_every));
            Err(Step::Retry)
        }
        Err(e) => {
            event!("nm_error", "error" => &e);
            backoff_wait(c);
            Err(Step::Retry)
        }
    }
}

/// CONNECT: P2P group formation with the peer; Err(step) after a failure (backed off).
fn connect(c: &mut Ctx, peer: &OwnedObjectPath) -> Result<OwnedObjectPath, Step> {
    notify("STATUS=connecting");
    event!("connect");
    user_event(UserEvent::Connecting);
    c.nm.connect(&c.sink, peer, CONNECT_DEADLINE).map_err(|e| {
        event!("connect_failed", "error" => &e);
        user_event(UserEvent::Failed);
        c.nm.deactivate_ours();
        backoff_wait(c);
        Step::Retry
    })
}

/// RTSP, then CASTING inside session::run; the P2P connection is taken down afterwards.
fn run_session(c: &mut Ctx, ac: &OwnedObjectPath) -> Step {
    notify("STATUS=waiting for RTSP from the TV");
    let end = match session::accept(&c.listener, RTSP_DEADLINE) {
        Ok((s, ip)) => {
            event!("sink_connected", "ip" => &ip);
            let t0 = Instant::now();
            let end = session::run(s, &ip, &c.cfg, &|| c.nm.is_active(ac));
            if t0.elapsed() > SESSION_RESETS_BACKOFF {
                c.backoff.reset();
            }
            end
        }
        Err(e) => session::End::Failed(e),
    };
    c.nm.deactivate_ours();
    // the session reached PLAY (session.rs announced casting): its end is "disconnected", not a failed attempt
    let was_casting = last_event() == Some(UserEvent::Casting);
    match &end {
        session::End::Stopped => {}
        session::End::SinkTeardown => {
            user_event(UserEvent::Ended);
        }
        session::End::Failed(_) => {
            user_event(if was_casting { UserEvent::Ended } else { UserEvent::Failed });
        }
    }
    if c.once && (was_casting || matches!(end, session::End::SinkTeardown)) {
        // on-demand TV: the user picks it again in the menu; waiting here would reconnect (and wake) it
        event!("on_demand_done", "end" => &format!("{end:?}"));
        let _ = std::fs::remove_file(connect_now_file());
        return Step::Stop;
    }
    match end {
        session::End::Stopped => return Step::Stop,
        session::End::SinkTeardown => {
            event!("sink_teardown");
            c.nm.ignore_sightings_for(TEARDOWN_IGNORE_S);
            c.backoff.reset();
            backoff_wait(c);
        }
        session::End::Failed(e) => {
            event!("session_failed", "error" => &e);
            backoff_wait(c);
        }
    }
    Step::Retry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_events_change_state_once() {
        use UserEvent::*;
        assert_eq!(announce_plan(None, Waiting), Some(false));
        assert_eq!(announce_plan(Some(Waiting), Waiting), None, "every WAIT_SINK cycle would notify");
        assert_eq!(announce_plan(Some(Waiting), Connecting), Some(false));
        assert_eq!(announce_plan(Some(Connecting), Casting), Some(false));
        assert_eq!(announce_plan(Some(Casting), Ended), Some(false));
        // after a failure or the end of a session that message stays; waiting only refreshes the bar
        assert_eq!(announce_plan(Some(Ended), Waiting), Some(true));
        assert_eq!(announce_plan(Some(Failed), Waiting), Some(true));
        assert_eq!(announce_plan(Some(Failed), Connecting), Some(false));
        assert_eq!(announce_plan(Some(Connecting), Failed), Some(false));
        let keys: Vec<_> = [Waiting, Connecting, Casting, Failed, Ended].iter().map(|e| e.key()).collect();
        assert_eq!(keys, ["waiting", "connecting", "casting", "failed", "ended"]);
    }

    #[test]
    fn parses_modes() {
        assert_eq!(parse_mode("1920x1080p60"), Some((1920, 1080, 60)));
        assert_eq!(parse_mode("1280x720p30"), Some((1280, 720, 30)));
        for bad in ["1920x1080", "1920p60", "x1080p60", "1920x1080p", "1920x1080p999"] {
            assert_eq!(parse_mode(bad), None, "{bad}");
        }
    }

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn output_is_auto_unless_monitor_or_option() {
        let no_env = |_: &str| None;
        assert_eq!(parse_args(args(""), no_env).unwrap().output, "auto");
        let monitor = |k: &str| (k == "MONITOR").then(|| "HDMI-A-1".to_string());
        assert_eq!(parse_args(args(""), monitor).unwrap().output, "HDMI-A-1");
        assert_eq!(parse_args(args("--output eDP-2"), monitor).unwrap().output, "eDP-2");
        let empty = |k: &str| (k == "MONITOR").then(|| " ".to_string());
        assert_eq!(parse_args(args(""), empty).unwrap().output, "auto");
        let o = parse_args(args("--sink 00:11:22:33:44:55 --no-p2p"), no_env).unwrap();
        assert_eq!((o.sink.as_deref(), o.no_p2p), (Some("00:11:22:33:44:55"), true));
        assert!(parse_args(args("--help"), no_env).unwrap_err().contains("TV_MAC"));
    }

    #[test]
    fn sink_file_follows_xdg_state_home() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        assert_eq!(sink_file(env(&[("XDG_STATE_HOME", "/s"), ("HOME", "/h")])), Some("/s/sway-miracast/sink".into()));
        let home = Some(PathBuf::from("/h/.local/state/sway-miracast/sink"));
        assert_eq!(sink_file(env(&[("HOME", "/h")])), home);
        assert_eq!(sink_file(env(&[("XDG_STATE_HOME", ""), ("HOME", "/h")])), home);
        assert_eq!(sink_file(env(&[("XDG_STATE_HOME", "rel"), ("HOME", "/h")])), home);
        assert_eq!(sink_file(env(&[])), None);
    }

    #[test]
    fn sink_line_in_the_state_file() {
        assert_eq!(sink_from_file("TV_MAC=AA:BB:CC:DD:EE:FF\n").as_deref(), Some("AA:BB:CC:DD:EE:FF"));
        assert_eq!(
            sink_from_file("# chosen\nexport TV_MAC='aa:bb:cc:dd:ee:ff'\n").as_deref(),
            Some("aa:bb:cc:dd:ee:ff")
        );
        assert_eq!(sink_from_file(" TV_MAC=\"AA:BB:CC:DD:EE:01\" \nOTHER=1").as_deref(), Some("AA:BB:CC:DD:EE:01"));
        assert_eq!(sink_from_file("TV_MAC=\n"), None);
        assert_eq!(sink_from_file("MY_TV_MAC=AA:BB:CC:DD:EE:FF\n"), None);
        assert_eq!(sink_from_file(""), None);
    }

    #[test]
    fn on_demand_list() {
        let text = "AA:BB:CC:DD:EE:01\n  aa:bb:cc:dd:ee:02  # living room\n# AA:BB:CC:DD:EE:03\n";
        assert!(listed(text, "AA:BB:CC:DD:EE:01"));
        assert!(listed(text, "AA:BB:CC:DD:EE:02"));
        assert!(!listed(text, "AA:BB:CC:DD:EE:03"), "commented out");
        assert!(!listed(text, "AA:BB:CC:DD:EE:0"));
        assert!(!listed("", "AA:BB:CC:DD:EE:01"));
        assert_eq!(state_dir(|k| (k == "HOME").then(|| "/h".into())), Some("/h/.local/state/sway-miracast".into()));
    }

    #[test]
    fn mac_format() {
        assert!(valid_mac("00:00:00:00:00:00"));
        assert!(valid_mac("aA:bB:cC:dD:eE:fF"));
        for bad in
            ["", "00:00:00:00:00", "00:00:00:00:00:00:00", "0:00:00:00:00:00", "00-00-00-00-00-00", "gg:00:00:00:00:00"]
        {
            assert!(!valid_mac(bad), "{bad}");
        }
    }

    #[test]
    fn sink_order_option_env_file() {
        let file = |_: &std::path::Path| Some("TV_MAC=00:00:00:00:00:03\n".to_string());
        let no_file = |_: &std::path::Path| None;
        let env = |tv: &'static str| {
            move |k: &str| match k {
                "TV_MAC" => Some(tv.to_string()),
                "XDG_STATE_HOME" => Some("/state".to_string()),
                _ => None,
            }
        };
        assert_eq!(
            resolve_sink(Some("00:00:00:00:00:01"), env("00:00:00:00:00:02"), file),
            Ok(Some("00:00:00:00:00:01".into()))
        );
        assert_eq!(resolve_sink(None, env("00:00:00:00:00:02"), file), Ok(Some("00:00:00:00:00:02".into())));
        assert_eq!(resolve_sink(None, env(""), file), Ok(Some("00:00:00:00:00:03".into())));
        assert_eq!(resolve_sink(None, env(""), no_file), Ok(None));
        // the file is looked up where XDG_STATE_HOME says
        let seen = std::cell::RefCell::new(None);
        let spy = |f: &std::path::Path| {
            *seen.borrow_mut() = Some(f.to_path_buf());
            None
        };
        assert_eq!(resolve_sink(None, env(""), spy), Ok(None));
        assert_eq!(seen.into_inner(), Some(PathBuf::from("/state/sway-miracast/sink")));
        // garbage is an error from whichever source won, not a fallthrough to the next one
        assert!(resolve_sink(Some("tv"), env("00:00:00:00:00:02"), file).unwrap_err().starts_with("--sink"));
        assert!(resolve_sink(None, env("x"), file).unwrap_err().starts_with("TV_MAC"));
        let bad_file = |_: &std::path::Path| Some("TV_MAC=00:00\n".to_string());
        assert!(resolve_sink(None, env(""), bad_file).unwrap_err().contains("/state/sway-miracast/sink"));
    }

    #[test]
    fn env_defaults_and_overrides() {
        let none = EnvCfg::from(|_| None);
        assert_eq!(
            none,
            EnvCfg { rtsp_port: 7236, low_latency: Some("on".into()), video_hold_ms: 0.0, audio_delay_ms: -1000.0 }
        );
        let set = |k: &str| {
            Some(
                match k {
                    "WFD_RTSP_PORT" => "17236",
                    "SEC_LOW_LATENCY_MODE" => "off",
                    "VIDEO_HOLD_MS" => " 120 ",
                    "AUDIO_DELAY_MS" => "x",
                    _ => return None,
                }
                .to_string(),
            )
        };
        let got = EnvCfg::from(set);
        assert_eq!(got, EnvCfg { rtsp_port: 17236, low_latency: None, video_hold_ms: 120.0, audio_delay_ms: -1000.0 });
    }
}
