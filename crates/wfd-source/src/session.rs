//! One WFD RTSP session (source side): M1-M7, then keepalive (M16) and sink requests until it ends.
//! Every wait has a deadline; any protocol violation ends the session (the supervisor retries).

use crate::media::{Exit, Media, MediaCfg, RTCP_PORT, RTP_PORT};
use crate::stopping;
use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};
use wfd_proto::audio::{AAC_48K_STEREO as AUDIO_OFF, LPCM_48K_STEREO as AUDIO_ON};
use wfd_proto::rtsp::StartLine;
use wfd_proto::video::{Mode, Selection, VideoFormats};
use wfd_proto::{Decoder, Message, Params};
use wfd_util::{event, J};

/// Default RTSP port; WFD_RTSP_PORT overrides it (tests).
pub const RTSP_PORT: u16 = wfd_proto::device_info::RTSP_PORT;
const URI: &str = "rtsp://localhost/wfd1.0";
const TIMEOUT: u32 = 30; // Session timeout we announce; keepalive at well under half of it
const RESPONSE_DEADLINE: Duration = Duration::from_secs(5);
/// M3: the sink capabilities we ask for.
const M3_QUERY: [&str; 4] = ["wfd_video_formats", "wfd_audio_codecs", "wfd_client_rtp_ports", "wfd_content_protection"];
/// M2 reply: the methods we support.
const PUBLIC: &str = "org.wfa.wfd1.0, GET_PARAMETER, SET_PARAMETER, SETUP, PLAY, PAUSE, TEARDOWN";
/// Audio capture restarts are cheap and harmless: retry forever, but not more often than this.
const AUDIO_RESTART_GAP: Duration = Duration::from_secs(5);
/// More encoder exits than this within ENCODER_RESTART_WINDOW = a real failure: end the session.
const ENCODER_RESTARTS: usize = 3;
const ENCODER_RESTART_WINDOW: Duration = Duration::from_secs(60);
/// Samsung reports Multi View after PLAY; ask for full screen at most this many extra times.
const FULL_VIEW_ASKS: u32 = 2;
const WATCHDOG_EVERY: Duration = Duration::from_secs(5);

/// Session timing (tests shorten it).
#[derive(Debug, Clone, Copy)]
struct Timing {
    keepalive: Duration,
    /// After M5 the sink sends SETUP (M6) and then PLAY (M7) right away; waiting longer means it is stuck.
    setup_play: Duration,
    /// Samsung `wfd_sec_low_latency_mode: on` goes out this long after PLAY (to a sink that has sent a
    /// `wfd_sec_*` parameter, see `ask_low_latency`), like the live switch that was
    /// verified on the TV (2026-11-09: audio and video in sync).
    low_latency_after: Duration,
    /// The serve loop polls the RTSP socket this long, so it turns at least twice a second.
    serve_tick: Duration,
}

const TIMING: Timing = Timing {
    keepalive: Duration::from_secs(10),
    setup_play: Duration::from_secs(10),
    low_latency_after: Duration::from_secs(3),
    serve_tick: Duration::from_millis(500),
};

pub struct SessionCfg {
    pub modes: Vec<(u16, u16, u8)>,
    pub media: MediaCfg,
    /// `wfd_sec_low_latency_mode` to send after PLAY; None: not sent (see `low_latency_mode`).
    pub low_latency: Option<String>,
}

/// The low-latency value from env SEC_LOW_LATENCY_MODE: "on" when unset, None (skip) for "off".
pub fn low_latency_mode(env: Option<String>) -> Option<String> {
    let mode = env.unwrap_or_else(|| "on".into());
    let mode = mode.trim();
    (mode != "off").then(|| mode.to_string())
}

/// What the session needs from the data plane (`Media`; a fake in the tests).
pub trait DataPlane {
    fn output(&self) -> String;
    fn idr(&self);
    fn exited(&mut self) -> Option<Exit>;
    fn restart_encoder(&mut self) -> std::io::Result<()>;
    fn restart_audio(&mut self) -> std::io::Result<()>;
}

impl DataPlane for Media {
    fn output(&self) -> String {
        Media::output(self)
    }
    fn idr(&self) {
        Media::idr(self)
    }
    fn exited(&mut self) -> Option<Exit> {
        Media::exited(self)
    }
    fn restart_encoder(&mut self) -> std::io::Result<()> {
        Media::restart_encoder(self)
    }
    fn restart_audio(&mut self) -> std::io::Result<()> {
        Media::restart_audio(self)
    }
}

/// Starts the data plane for a mode, to the sink's IP and RTP port.
type StartMedia<'a> = &'a dyn Fn(&MediaCfg, &Mode, &str, u16) -> std::io::Result<Box<dyn DataPlane>>;

fn start_media(cfg: &MediaCfg, m: &Mode, sink_ip: &str, port: u16) -> std::io::Result<Box<dyn DataPlane>> {
    Ok(Box::new(Media::start(cfg, m.width, m.height, m.fps, sink_ip, port)?))
}

/// Our RTP/RTCP server ports for the SETUP reply.
fn server_ports() -> String {
    format!("{RTP_PORT}-{RTCP_PORT}")
}

/// A session id in 1000000..=9999999 from the pid and the clock.
fn session_id(pid: u32, now_us: u64) -> String {
    format!("{}", 1_000_000 + (pid as u64).wrapping_mul(7919).wrapping_add(now_us) % 9_000_000)
}

#[derive(Debug)]
pub enum End {
    /// Sink sent TEARDOWN (user stopped mirroring on the TV).
    SinkTeardown,
    /// We were asked to stop.
    Stopped,
    /// Anything else: transient, retry.
    Failed(String),
}

/// What the RTSP connection needs from its transport: bytes and a read timeout. TcpStream in the
/// service, a scripted sink in the tests.
trait Transport: Read + Write {
    fn set_timeout(&self, t: Option<Duration>) -> std::io::Result<()>;
}

impl Transport for TcpStream {
    fn set_timeout(&self, t: Option<Duration>) -> std::io::Result<()> {
        self.set_read_timeout(t)
    }
}

struct Rtsp<S: Transport> {
    s: S,
    dec: Decoder,
    cseq: u32,
    pending: VecDeque<Message>,
}

impl<S: Transport> Rtsp<S> {
    fn send(&mut self, m: &Message) -> Result<(), String> {
        self.s.write_all(&m.to_bytes()).map_err(|e| format!("send: {e}"))
    }

    /// Next message from the sink within `timeout` (None on timeout).
    fn read(&mut self, timeout: Duration) -> Result<Option<Message>, String> {
        let deadline = Instant::now() + timeout;
        let mut buf = [0u8; 16384];
        loop {
            if let Some(m) = self.dec.next_message().map_err(|e| format!("rtsp parse: {e}"))? {
                return Ok(Some(m));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            self.s.set_timeout(Some(left.max(Duration::from_millis(1)))).ok();
            match self.s.read(&mut buf) {
                Ok(0) => return Err("sink closed TCP".into()),
                Ok(n) => self.dec.feed(&buf[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => return Ok(None),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(format!("recv: {e}")),
            }
        }
    }

    /// Send a request and wait for its response; sink requests arriving meanwhile are queued.
    fn request(&mut self, mut m: Message) -> Result<Message, String> {
        self.cseq += 1;
        let cseq = self.cseq;
        m.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("CSeq"));
        m.headers.insert(0, ("CSeq".into(), cseq.to_string()));
        self.send(&m)?;
        let deadline = Instant::now() + RESPONSE_DEADLINE;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(r) = self.read(left)? else {
                return Err(format!("no response to {} (CSeq {cseq})", m.method().unwrap_or("?")));
            };
            match r.start {
                StartLine::Response { .. } if r.cseq() == Some(cseq) => return Ok(r),
                StartLine::Response { .. } => {} // stale response: ignore
                StartLine::Request { .. } => self.pending.push_back(r),
            }
        }
    }

    fn next_request(&mut self, timeout: Duration) -> Result<Option<Message>, String> {
        if let Some(m) = self.pending.pop_front() {
            return Ok(Some(m));
        }
        match self.read(timeout)? {
            Some(m) if matches!(m.start, StartLine::Request { .. }) => Ok(Some(m)),
            _ => Ok(None),
        }
    }

    fn reply(&mut self, req: &Message, headers: &[(&str, String)], body: Option<String>) -> Result<(), String> {
        let mut r = Message::response(200, "OK", req.cseq().unwrap_or(0));
        for (k, v) in headers {
            r = r.header(k, v.clone());
        }
        if let Some(b) = body {
            r = r.with_body(b);
        }
        self.send(&r)
    }
}

fn set_parameter(body: String) -> Message {
    Message::request("SET_PARAMETER", URI, 0).with_body(body)
}

fn in_session(m: Message, session: &str) -> Message {
    m.header("Session", session)
}

/// SET_PARAMETER in the session; the reply's status (or the error, under `error_key`) goes to the log as
/// event `name` after `kv`. The session goes on either way.
fn set_param_logged<S: Transport>(
    r: &mut Rtsp<S>,
    session: &str,
    body: String,
    name: &str,
    kv: &[(&str, J)],
    error_key: &'static str,
) {
    let res = r.request(in_session(set_parameter(body), session));
    let mut all = kv.to_vec();
    match &res {
        Ok(m) => all.push(("status", J::from(m.status().unwrap_or(0)))),
        Err(e) => all.push((error_key, J::from(e))),
    }
    wfd_util::event(name, &all);
}

/// Live RTSP parameters for the sink, like a Samsung phone switching `wfd_sec_low_latency_mode` during
/// a call: write "name: value" (or name=value) lines to `$XDG_RUNTIME_DIR/sway-miracast/rtsp-set`; the
/// running session sends them as one SET_PARAMETER and deletes the file.
const RTSP_SET_FILE: &str = "rtsp-set";

/// SET_PARAMETER body from the control file text; None when there is nothing to send.
fn rtsp_set_body(text: &str) -> Option<String> {
    let mut body = String::new();
    for l in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let (k, v) = l.split_once(':').or_else(|| l.split_once('='))?;
        let (k, v) = (k.trim(), v.trim());
        if k.is_empty() || k.contains(char::is_whitespace) || v.contains(['\r', '\n']) {
            return None;
        }
        body.push_str(&format!("{k}: {v}\r\n"));
    }
    (!body.is_empty()).then_some(body)
}

/// Sends the control file's parameters once (see RTSP_SET_FILE); the reply status goes to the log.
fn send_rtsp_set<S: Transport>(r: &mut Rtsp<S>, session: &str, dir: &std::path::Path) {
    let f = dir.join(RTSP_SET_FILE);
    let Ok(text) = std::fs::read_to_string(&f) else { return };
    let _ = std::fs::remove_file(&f);
    let Some(body) = rtsp_set_body(&text) else {
        event!("rtsp_set_invalid", "text" => &text);
        return;
    };
    event!("rtsp_set", "body" => &body.trim_end().replace("\r\n", "; "));
    set_param_logged(r, session, body, "rtsp_set_reply", &[], "status");
}

fn ok(r: &Message, what: &str) -> Result<(), String> {
    match r.status() {
        Some(200) => Ok(()),
        s => Err(format!("{what}: status {s:?}")),
    }
}

/// Wait for the sink's TCP connection (it connects ~10-15 s after the P2P group is up).
pub fn accept(listener: &TcpListener, timeout: Duration) -> Result<(TcpStream, String), String> {
    listener.set_nonblocking(true).ok();
    let t0 = Instant::now();
    while t0.elapsed() < timeout {
        if stopping() {
            return Err("stopping".into());
        }
        match listener.accept() {
            Ok((s, addr)) => {
                s.set_nonblocking(false).ok();
                s.set_nodelay(true).ok();
                return Ok((s, addr.ip().to_string()));
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                crate::notify("WATCHDOG=1");
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(e) => return Err(format!("accept: {e}")),
        }
    }
    Err("sink did not open RTSP".into())
}

/// Restarts allowed within a sliding window (encoder: 3 a minute).
struct RestartBudget {
    times: Vec<Instant>,
    max: usize,
    window: Duration,
}

impl RestartBudget {
    fn new(max: usize, window: Duration) -> Self {
        RestartBudget { times: Vec::new(), max, window }
    }

    /// Records a restart at `now` and says whether it is still within the budget.
    fn allow(&mut self, now: Instant) -> bool {
        self.times.retain(|t| now.saturating_duration_since(*t) < self.window);
        if self.times.len() >= self.max {
            return false;
        }
        self.times.push(now);
        true
    }
}

pub fn run(s: TcpStream, sink_ip: &str, cfg: &SessionCfg, link_ok: &dyn Fn() -> bool) -> End {
    // our address on the P2P group (NM "shared" range), for the presentation URL
    let local_ip = s.local_addr().map(|a| a.ip().to_string()).unwrap_or_else(|_| "10.42.0.1".into());
    run_on(s, &local_ip, sink_ip, cfg, link_ok, &start_media, &TIMING)
}

fn run_on<S: Transport>(
    s: S,
    local_ip: &str,
    sink_ip: &str,
    cfg: &SessionCfg,
    link_ok: &dyn Fn() -> bool,
    start: StartMedia,
    t: &Timing,
) -> End {
    let mut r = Rtsp { s, dec: Decoder::new(), cseq: 0, pending: VecDeque::new() };
    let result = negotiate(&mut r, cfg, local_ip).and_then(|neg| serve(&mut r, &neg, sink_ip, cfg, link_ok, start, t));
    result.unwrap_or_else(End::Failed)
}

/// Outcome of M1-M5: what we will send and where.
#[derive(Debug)]
struct Negotiated {
    sel: Selection,
    rtp_port: u16,
    session: String,
}

/// M1-M5: capabilities, our choice, trigger SETUP.
fn negotiate<S: Transport>(r: &mut Rtsp<S>, cfg: &SessionCfg, local_ip: &str) -> Result<Negotiated, String> {
    // M1: our OPTIONS
    let m1 = r.request(Message::request("OPTIONS", "*", 0).header("Require", "org.wfa.wfd1.0"))?;
    ok(&m1, "M1")?;
    // M2: sink's OPTIONS
    let m2 = r.next_request(RESPONSE_DEADLINE)?.ok_or("no M2")?;
    r.reply(&m2, &[("Public", PUBLIC.into())], None)?;
    event!("M2");
    // M3: capabilities
    let m3 = r.request(Message::request("GET_PARAMETER", URI, 0).with_body(Params::names_body(M3_QUERY)))?;
    ok(&m3, "M3")?;
    let caps = Params::parse(&m3.body);
    let vf = caps.get("wfd_video_formats").ok_or("M3 without wfd_video_formats")?;
    let formats = VideoFormats::parse(vf).map_err(|e| format!("video formats: {e:?}"))?.ok_or("sink has no video")?;
    let sel = formats.select(&cfg.modes).ok_or("no common video mode")?;
    let rtp = caps.get("wfd_client_rtp_ports").ok_or("M3 without rtp ports")?.to_string();
    let rtp_port = wfd_proto::params::client_rtp_port0(&rtp).ok_or("bad rtp ports")?;
    let audio = caps.get("wfd_audio_codecs").map(wfd_proto::audio::parse).unwrap_or_default();
    let audio_desc: Vec<String> = audio.iter().map(|c| format!("{} {:?}", c.name, c.describe())).collect();
    let audio_desc = audio_desc.join("; ");
    event!("M3", "mode" => &sel.mode.to_string(), "rtp_port" => rtp_port, "audio" => &audio_desc);
    // we only send LPCM 48 kHz 16 bit stereo (mode bit 1)
    if cfg.media.audio && !audio.iter().any(|c| c.name == "LPCM" && c.modes & 0b10 != 0) {
        event!("sink_lacks_lpcm_48k", "audio" => &audio_desc);
    }
    // M4: our choice. Samsung needs content protection and an audio codec here (else TEARDOWN after SETUP).
    let m4 = Params::default()
        .set("wfd_content_protection", "none")
        .set("wfd_video_formats", sel.to_string())
        .set("wfd_audio_codecs", if cfg.media.audio { AUDIO_ON } else { AUDIO_OFF })
        .set("wfd_presentation_URL", format!("rtsp://{local_ip}/wfd1.0/streamid=0 none"))
        .set("wfd_client_rtp_ports", rtp);
    let m4r = r.request(set_parameter(m4.to_body()))?;
    ok(&m4r, "M4")?;
    // M5: trigger SETUP
    let m5 = r.request(set_parameter("wfd_trigger_method: SETUP\r\n".into()))?;
    ok(&m5, "M5")?;
    let session = session_id(std::process::id(), wfd_util::unix_us());
    Ok(Negotiated { sel, rtp_port, session })
}

/// Where a session is after M5.
enum Phase {
    /// M5 sent: the sink must send SETUP (M6) within `Timing::setup_play` from `since`.
    AwaitSetup { since: Instant },
    /// SETUP answered: the sink must send PLAY (M7) within `Timing::setup_play` from `since`.
    AwaitPlay { since: Instant },
    /// PLAY answered, media running.
    Playing {
        media: Box<dyn DataPlane>,
        /// when PLAY was answered; None after the low-latency switch went out
        low_latency_due: Option<Instant>,
        /// extra "full view" asks after Multi View reports
        full_asks: u32,
    },
}

/// State of a session after M5.
struct Serving {
    phase: Phase,
    /// last keepalive (M16); counted from M5, keepalives go out only while Playing
    last_ka: Instant,
    enc_restarts: RestartBudget,
    last_audio_restart: Option<Instant>,
    last_wd: Instant,
    /// the sink has sent a `wfd_sec_*` parameter itself: it speaks Samsung's extensions (`ask_low_latency`)
    sec_ext: bool,
    /// `low_latency_not_sent` was logged (once per session)
    ll_skip_logged: bool,
}

/// M6/M7, then keepalive, sink requests and media supervision until the session ends.
fn serve<S: Transport>(
    r: &mut Rtsp<S>,
    neg: &Negotiated,
    sink_ip: &str,
    cfg: &SessionCfg,
    link_ok: &dyn Fn() -> bool,
    start: StartMedia,
    t: &Timing,
) -> Result<End, String> {
    let now = Instant::now();
    let mut st = Serving {
        phase: Phase::AwaitSetup { since: now },
        last_ka: now,
        enc_restarts: RestartBudget::new(ENCODER_RESTARTS, ENCODER_RESTART_WINDOW),
        last_audio_restart: None,
        last_wd: now,
        sec_ext: false,
        ll_skip_logged: false,
    };
    loop {
        if stopping() {
            let _ = r.request(set_parameter("wfd_trigger_method: TEARDOWN\r\n".into()));
            return Ok(End::Stopped);
        }
        // the loop turns every <= serve_tick while the session is healthy, also before PLAY
        if st.last_wd.elapsed() >= WATCHDOG_EVERY {
            crate::notify("WATCHDOG=1");
            st.last_wd = Instant::now();
        }
        let overdue = match st.phase {
            Phase::AwaitSetup { since } if since.elapsed() >= t.setup_play => Some("SETUP (M6)"),
            Phase::AwaitPlay { since } if since.elapsed() >= t.setup_play => Some("PLAY (M7)"),
            _ => None,
        };
        if let Some(what) = overdue {
            return Err(format!("sink sent no {what} within {} s", t.setup_play.as_secs()));
        }
        if let Some(req) = r.next_request(t.serve_tick)? {
            if let Some(end) = handle_request(r, &req, &mut st, neg, sink_ip, cfg, start)? {
                return Ok(end);
            }
        }
        let playing = matches!(st.phase, Phase::Playing { .. });
        if playing && st.last_ka.elapsed() >= t.keepalive {
            let ka = r.request(in_session(Message::request("GET_PARAMETER", URI, 0), &neg.session))?;
            ok(&ka, "M16")?;
            st.last_ka = Instant::now();
        }
        supervise_media(&mut st)?;
        if let Phase::Playing { low_latency_due, .. } = &mut st.phase {
            if low_latency_due.is_some_and(|d| d.elapsed() >= t.low_latency_after) && cfg.low_latency.is_some() {
                if st.sec_ext {
                    *low_latency_due = None;
                    ask_low_latency(r, &neg.session, cfg);
                } else if !st.ll_skip_logged {
                    // still due: goes out as soon as the sink shows a wfd_sec_* parameter
                    st.ll_skip_logged = true;
                    event!("low_latency_not_sent", "why" => "sink sent no wfd_sec_* parameter (not a Samsung)");
                }
            }
            send_rtsp_set(r, &neg.session, &cfg.media.dir);
        }
        if !link_ok() {
            return Err("P2P link down".into());
        }
    }
}

/// One request from the sink. Some(end) when it ends the session.
fn handle_request<S: Transport>(
    r: &mut Rtsp<S>,
    req: &Message,
    st: &mut Serving,
    neg: &Negotiated,
    sink_ip: &str,
    cfg: &SessionCfg,
    start: StartMedia,
) -> Result<Option<End>, String> {
    let session = &neg.session;
    let method = req.method().unwrap_or("").to_string();
    match method.as_str() {
        "SETUP" => {
            let tr = req.get("Transport").ok_or("SETUP without Transport")?.trim().to_string();
            r.reply(
                req,
                &[
                    ("Session", format!("{session};timeout={TIMEOUT}")),
                    ("Transport", format!("{tr};server_port={}", server_ports())),
                ],
                None,
            )?;
            event!("M6");
            // a SETUP while playing keeps the media running (as before: only the deadline restarts)
            if !matches!(st.phase, Phase::Playing { .. }) {
                st.phase = Phase::AwaitPlay { since: Instant::now() };
            }
        }
        "PLAY" => {
            r.reply(req, &[("Session", session.clone()), ("Range", "npt=now-".into())], None)?;
            // a second PLAY (or PLAY after PAUSE) while media runs: nothing to start
            if matches!(st.phase, Phase::Playing { .. }) {
                return Ok(None);
            }
            let m = &neg.sel.mode;
            let media = start(&cfg.media, m, sink_ip, neg.rtp_port).map_err(|e| format!("media: {e}"))?;
            event!("M7_PLAY", "mode" => &m.to_string());
            // Samsung: ask for full-screen view instead of Multi View (TV may ignore it)
            ask_full_view(r, session);
            st.phase = Phase::Playing { media, low_latency_due: Some(Instant::now()), full_asks: 0 };
            crate::notify(&format!("STATUS=casting {m}"));
        }
        "TEARDOWN" => {
            r.reply(req, &[("Session", session.clone())], None)?;
            return Ok(Some(End::SinkTeardown));
        }
        "SET_PARAMETER" | "GET_PARAMETER" | "OPTIONS" => {
            let p = Params::parse(&req.body);
            if !st.sec_ext && p.names().any(is_sec_ext) {
                st.sec_ext = true;
                event!("sink_sec_ext");
            }
            if p.names().any(|n| n == "wfd_idr_request") {
                if let Phase::Playing { media, .. } = &st.phase {
                    media.idr();
                }
                event!("idr_request");
            } else if !req.body.is_empty() {
                event!("sink_request", "method" => &method, "body" => &req.body.trim().replace("\r\n", "; "));
            }
            r.reply(req, &[("Session", session.clone())], None)?;
            if let Phase::Playing { full_asks, .. } = &mut st.phase {
                if p.get("wfd_sec_view_mode") == Some("multi") && *full_asks < FULL_VIEW_ASKS {
                    *full_asks += 1;
                    ask_full_view(r, session);
                }
            }
        }
        _ => r.reply(req, &[], None)?,
    }
    Ok(None)
}

fn ask_full_view<S: Transport>(r: &mut Rtsp<S>, session: &str) {
    set_param_logged(r, session, "wfd_sec_view_mode: full\r\n".into(), "view_full", &[], "error");
}

/// A Samsung extension parameter (`wfd_sec_view_mode`, `wfd_sec_dmr_support`, ...).
fn is_sec_ext(name: &str) -> bool {
    name.starts_with("wfd_sec_")
}

/// Samsung extension (the TV lists it in M3 as `off`): with `on` the TV shrinks its audio buffer and
/// audio lines up with video. The reply only goes to the log. `SEC_LOW_LATENCY_MODE=off` in the env file
/// skips it (cfg.low_latency None).
///
/// Sent only after the sink itself has sent a `wfd_sec_*` parameter (the Samsung TV sends
/// `wfd_sec_view_mode` + `wfd_sec_dmr_support` within ~0.1 s of PLAY, sinks/ Q5). Not asked in M3: WFD v2.1
/// §6.2.2 lets a sink ignore unknown names in GET_PARAMETER, but §6.4.3 limits M3 to the §6.3 parameters,
/// and a strict RTSP sink may answer 451 (Parameter Not Understood, RFC 2326 §11.3.2, WFD Table 96)
/// to the whole M3, which would end every session. For SET_PARAMETER §6.4.4 only *recommends* ignoring
/// unknown parameters ("highly recommended"), and §6.2.3 allows a 303 for them, so an unknown parameter is
/// never sent unprompted: a sink that sent no `wfd_sec_*` never sees one from us.
fn ask_low_latency<S: Transport>(r: &mut Rtsp<S>, session: &str, cfg: &SessionCfg) {
    let Some(mode) = &cfg.low_latency else { return };
    let body = format!("wfd_sec_low_latency_mode: {mode}\r\n");
    set_param_logged(r, session, body, "low_latency", &[("mode", J::from(mode))], "error");
}

/// Restart what exited: audio freely (paced), the encoder within its budget; wfd-tx ends the session.
fn supervise_media(st: &mut Serving) -> Result<(), String> {
    let Phase::Playing { media: m, .. } = &mut st.phase else { return Ok(()) };
    if crate::media::RECAPTURE.swap(false, std::sync::atomic::Ordering::Relaxed) {
        // asked for (tools/miracast-screen.sh), not a crash: outside the restart budget
        event!("capture_switch", "output" => &m.output());
        // a fresh encoder starts with an IDR (no SIGUSR1: right after spawn it would end wfd-media)
        m.restart_encoder().map_err(|e| format!("encoder restart: {e}"))?;
        return Ok(());
    }
    match m.exited() {
        Some(Exit::Sender(why)) => return Err(format!("media: {why}")),
        Some(Exit::Audio(why)) => {
            // own pacing, separate from the encoder: a flapping parec must not use up the
            // encoder's restart budget (and the other way round)
            if st.last_audio_restart.is_none_or(|t| t.elapsed() >= AUDIO_RESTART_GAP) {
                st.last_audio_restart = Some(Instant::now());
                event!("audio_restart", "why" => &why);
                // audio is not worth the session: a failed restart is retried after the gap
                if let Err(e) = m.restart_audio() {
                    event!("audio_restart_failed", "error" => &e.to_string());
                }
            }
        }
        Some(Exit::Encoder(why)) => {
            // restart the encoder only (upgrade, output change, crash, stall); wfd-tx keeps the stream
            if !st.enc_restarts.allow(Instant::now()) {
                return Err(format!("encoder keeps exiting: {why}"));
            }
            event!("encoder_restart", "why" => &why);
            m.restart_encoder().map_err(|e| format!("encoder restart: {e}"))?;
        }
        None => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::rc::Rc;

    #[test]
    fn rtsp_set_body_from_control_file() {
        assert_eq!(
            rtsp_set_body("wfd_sec_low_latency_mode: on\n# c\n\nmicrosoft_latency_management_capability=low\n")
                .as_deref(),
            Some("wfd_sec_low_latency_mode: on\r\nmicrosoft_latency_management_capability: low\r\n")
        );
        assert_eq!(rtsp_set_body(""), None);
        assert_eq!(rtsp_set_body("no separator"), None);
        assert_eq!(rtsp_set_body("bad name: x"), None);
    }

    #[test]
    fn restart_budget_three_a_minute() {
        let t0 = Instant::now();
        let mut b = RestartBudget::new(3, Duration::from_secs(60));
        assert!(b.allow(t0));
        assert!(b.allow(t0 + Duration::from_secs(1)));
        assert!(b.allow(t0 + Duration::from_secs(2)));
        assert!(!b.allow(t0 + Duration::from_secs(3)), "4th within a minute");
        // the first one leaves the window after 60 s
        assert!(b.allow(t0 + Duration::from_secs(61)));
    }

    #[test]
    fn session_id_has_seven_digits() {
        for (pid, us) in [(1, 0), (4_194_304, 1_762_700_000_000_000), (u32::MAX, u64::MAX), (0, 8_999_999)] {
            let id: u64 = session_id(pid, us).parse().unwrap();
            assert!((1_000_000..=9_999_999).contains(&id), "{id}");
        }
    }

    #[test]
    fn low_latency_mode_from_env() {
        assert_eq!(low_latency_mode(None).as_deref(), Some("on"));
        assert_eq!(low_latency_mode(Some(" off\n".into())), None);
        assert_eq!(low_latency_mode(Some("on ".into())).as_deref(), Some("on"));
    }

    /// A scripted Samsung-like sink: answers our requests (M3 with its capabilities) and sends its own
    /// requests when we send something matching a script entry (see `describe`); each entry fires once.
    struct FakeSink {
        to_us: Vec<u8>,
        from_us: Decoder,
        /// what we sent, as `describe` puts it
        seen: Vec<String>,
        msgs: Vec<Message>,
        script: Vec<Option<(&'static str, Message)>>,
        cseq: u32,
        sent_methods: HashMap<u32, String>,
    }

    impl FakeSink {
        fn new(script: Vec<(&'static str, Message)>) -> Self {
            FakeSink {
                to_us: Vec::new(),
                from_us: Decoder::new(),
                seen: Vec::new(),
                msgs: Vec::new(),
                script: script.into_iter().map(Some).collect(),
                cseq: 0,
                sent_methods: HashMap::new(),
            }
        }

        /// "METHOD" or "METHOD <first body line>" for our requests, "reply METHOD" for our replies.
        fn describe(&self, m: &Message) -> String {
            match m.method() {
                Some(method) => match m.body.lines().next() {
                    Some(l) => format!("{method} {}", l.trim_end()),
                    None => method.to_string(),
                },
                None => format!("reply {}", self.sent_methods.get(&m.cseq().unwrap_or(0)).cloned().unwrap_or_default()),
            }
        }

        fn fire(&mut self, what: &str) {
            let hit = self.script.iter_mut().find(|e| e.as_ref().is_some_and(|(t, _)| *t == what));
            if let Some((_, mut m)) = hit.and_then(Option::take) {
                self.cseq += 1;
                m.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("CSeq"));
                m.headers.insert(0, ("CSeq".into(), self.cseq.to_string()));
                self.sent_methods.insert(self.cseq, m.method().unwrap().to_string());
                self.to_us.extend(m.to_bytes());
            }
        }
    }

    impl Transport for FakeSink {
        fn set_timeout(&self, _: Option<Duration>) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Read for FakeSink {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.to_us.is_empty() {
                return Err(std::io::Error::new(ErrorKind::WouldBlock, "nothing scripted"));
            }
            let n = buf.len().min(self.to_us.len());
            buf[..n].copy_from_slice(&self.to_us[..n]);
            self.to_us.drain(..n);
            Ok(n)
        }
    }

    impl Write for FakeSink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.from_us.feed(buf);
            while let Some(m) = self.from_us.next_message().unwrap() {
                let what = self.describe(&m);
                if let Some(method) = m.method() {
                    let mut resp = Message::response(200, "OK", m.cseq().unwrap());
                    if method == "GET_PARAMETER" {
                        resp = resp.with_body(
                            "wfd_video_formats: 40 00 01 10 000001e3 0f3fffff 00000fff 00 0000 00c8 01 none none\r\n\
                             wfd_audio_codecs: LPCM 00000002 00\r\n\
                             wfd_client_rtp_ports: RTP/AVP/UDP;unicast 19002 0 mode=play\r\n\
                             wfd_content_protection: none\r\n",
                        );
                    }
                    self.to_us.extend(resp.to_bytes());
                }
                self.fire(&what);
                self.seen.push(what);
                self.msgs.push(m);
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn cfg(dir: &str) -> SessionCfg {
        SessionCfg {
            modes: vec![(1920, 1080, 60)],
            media: MediaCfg {
                output: "eDP-1".into(),
                kbps: 8000,
                extra_media: vec![],
                audio: true,
                mute_local: false,
                video_hold_ms: 0.0,
                audio_delay_ms: 0.0,
                dir: std::env::temp_dir().join(format!("wfd-session-test-{dir}-{}", std::process::id())),
            },
            low_latency: Some("on".into()),
        }
    }

    fn sink_req(method: &str, body: &str) -> Message {
        let m = Message::request(method, URI, 0);
        if body.is_empty() {
            m
        } else {
            m.with_body(body)
        }
    }

    /// M2 after our M1, SETUP after M5, PLAY after the SETUP reply.
    fn up_to_play() -> Vec<(&'static str, Message)> {
        vec![
            ("OPTIONS", Message::request("OPTIONS", "*", 0).header("Require", "org.wfa.wfd1.0")),
            (
                "SET_PARAMETER wfd_trigger_method: SETUP",
                sink_req("SETUP", "").header("Transport", "RTP/AVP/UDP;unicast;client_port=19002"),
            ),
            ("reply SETUP", sink_req("PLAY", "")),
        ]
    }

    #[derive(Default)]
    struct Counts {
        starts: Cell<u32>,
        idrs: Cell<u32>,
    }

    struct FakeMedia(Rc<Counts>);

    impl DataPlane for FakeMedia {
        fn output(&self) -> String {
            "eDP-1".into()
        }
        fn idr(&self) {
            self.0.idrs.set(self.0.idrs.get() + 1);
        }
        fn exited(&mut self) -> Option<Exit> {
            None
        }
        fn restart_encoder(&mut self) -> std::io::Result<()> {
            Ok(())
        }
        fn restart_audio(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    const FAST: Timing = Timing {
        keepalive: Duration::from_millis(100),
        setup_play: Duration::from_millis(300),
        low_latency_after: Duration::ZERO,
        serve_tick: Duration::from_millis(5),
    };

    /// Runs a whole session against the scripted sink; the sink and how often media was started.
    fn session(script: Vec<(&'static str, Message)>, cfg: &SessionCfg) -> (End, FakeSink, Rc<Counts>) {
        let counts = Rc::new(Counts::default());
        let c = counts.clone();
        let start = move |_: &MediaCfg, _: &Mode, ip: &str, port: u16| -> std::io::Result<Box<dyn DataPlane>> {
            assert_eq!((ip, port), ("10.42.0.103", 19002));
            c.starts.set(c.starts.get() + 1);
            Ok(Box::new(FakeMedia(c.clone())))
        };
        let mut r = Rtsp { s: FakeSink::new(script), dec: Decoder::new(), cseq: 0, pending: VecDeque::new() };
        let end = negotiate(&mut r, cfg, "10.42.0.1")
            .and_then(|neg| serve(&mut r, &neg, "10.42.0.103", cfg, &|| true, &start, &FAST))
            .unwrap_or_else(End::Failed);
        (end, r.s, counts)
    }

    #[test]
    fn negotiates_m1_to_m5_with_a_samsung_like_sink() {
        let mut r = Rtsp { s: FakeSink::new(up_to_play()), dec: Decoder::new(), cseq: 0, pending: VecDeque::new() };
        let neg = negotiate(&mut r, &cfg("m1"), "10.42.0.1").expect("negotiation");
        assert_eq!(neg.sel.mode.to_string(), "1920x1080p60");
        assert_eq!(neg.rtp_port, 19002);
        assert_eq!(
            r.s.seen,
            [
                "OPTIONS",
                "reply OPTIONS",
                "GET_PARAMETER wfd_video_formats",
                "SET_PARAMETER wfd_content_protection: none",
                "SET_PARAMETER wfd_trigger_method: SETUP"
            ]
        );
        let m4 = Params::parse(&r.s.msgs[3].body);
        assert_eq!(m4.get("wfd_audio_codecs"), Some("LPCM 00000002 00"));
        assert_eq!(m4.get("wfd_presentation_URL"), Some("rtsp://10.42.0.1/wfd1.0/streamid=0 none"));
        assert_eq!(m4.get("wfd_client_rtp_ports"), Some("RTP/AVP/UDP;unicast 19002 0 mode=play"));
        assert_eq!(r.s.msgs[4].body, "wfd_trigger_method: SETUP\r\n");
    }

    #[test]
    fn setup_play_keepalive_teardown() {
        let mut script = up_to_play();
        script.push((
            "reply PLAY",
            sink_req("SET_PARAMETER", "wfd_sec_view_mode: multi\r\nwfd_sec_dmr_support: disable\r\n"),
        ));
        script.push(("GET_PARAMETER", sink_req("TEARDOWN", "")));
        let (end, sink, counts) = session(script, &cfg("full"));
        assert!(matches!(end, End::SinkTeardown), "{end:?}");
        assert_eq!(counts.starts.get(), 1);
        assert_eq!(
            sink.seen[5..],
            [
                "reply SETUP",
                "reply PLAY",
                "SET_PARAMETER wfd_sec_view_mode: full",
                "reply SET_PARAMETER",
                "SET_PARAMETER wfd_sec_view_mode: full",
                "SET_PARAMETER wfd_sec_low_latency_mode: on",
                "GET_PARAMETER",
                "reply TEARDOWN"
            ]
        );
        let setup = &sink.msgs[5];
        assert_eq!(setup.get("Transport"), Some("RTP/AVP/UDP;unicast;client_port=19002;server_port=19100-19101"));
        let session = setup.get("Session").unwrap().strip_suffix(";timeout=30").expect("timeout announced");
        for m in &sink.msgs[6..] {
            assert_eq!(m.get("Session"), Some(session), "{m:?}");
        }
        assert_eq!(sink.msgs[6].get("Range"), Some("npt=now-"));
    }

    #[test]
    fn second_play_starts_media_only_once() {
        let mut script = up_to_play();
        script.push(("reply PLAY", sink_req("PLAY", "")));
        script.push(("GET_PARAMETER", sink_req("TEARDOWN", "")));
        let (end, sink, counts) = session(script, &cfg("play2"));
        assert!(matches!(end, End::SinkTeardown), "{end:?}");
        assert_eq!(counts.starts.get(), 1);
        assert_eq!(sink.seen.iter().filter(|s| *s == "reply PLAY").count(), 2);
        assert_eq!(sink.seen.iter().filter(|s| s.starts_with("SET_PARAMETER wfd_sec_view_mode")).count(), 1);
    }

    #[test]
    fn idr_and_multi_view_while_playing() {
        let mut script = up_to_play();
        script.push(("reply PLAY", sink_req("SET_PARAMETER", "wfd_idr_request\r\n")));
        for _ in 0..3 {
            script.push(("reply SET_PARAMETER", sink_req("SET_PARAMETER", "wfd_sec_view_mode: multi\r\n")));
        }
        script.push(("GET_PARAMETER", sink_req("TEARDOWN", "")));
        let (end, sink, counts) = session(script, &cfg("idr"));
        assert!(matches!(end, End::SinkTeardown), "{end:?}");
        assert_eq!(counts.idrs.get(), 1);
        // one after PLAY, then at most FULL_VIEW_ASKS more for three "multi" reports
        let asks = sink.seen.iter().filter(|s| *s == "SET_PARAMETER wfd_sec_view_mode: full").count();
        assert_eq!(asks, 1 + FULL_VIEW_ASKS as usize);
    }

    #[test]
    fn low_latency_only_to_a_sink_with_samsung_extensions() {
        let ll = |sink: &FakeSink| sink.seen.iter().filter(|s| s.contains("wfd_sec_low_latency_mode")).count();
        // a plain sink (no wfd_sec_* from it): never, though the session runs past several keepalives
        let mut script = up_to_play();
        script.push(("GET_PARAMETER", sink_req("GET_PARAMETER", "")));
        script.push(("reply GET_PARAMETER", sink_req("SET_PARAMETER", "wfd_idr_request\r\n")));
        script.push(("reply SET_PARAMETER", sink_req("TEARDOWN", "")));
        let (end, sink, _) = session(script, &cfg("llplain"));
        assert!(matches!(end, End::SinkTeardown), "{end:?}");
        assert!(sink.seen.iter().filter(|s| *s == "GET_PARAMETER").count() >= 1);
        assert_eq!(ll(&sink), 0, "{:?}", sink.seen);
        // the extension shows up late (after the first keepalive): low latency follows it, once
        let mut script = up_to_play();
        script.push(("GET_PARAMETER", sink_req("SET_PARAMETER", "wfd_sec_dmr_support: disable\r\n")));
        script.push((
            "SET_PARAMETER wfd_sec_low_latency_mode: on",
            sink_req("SET_PARAMETER", "wfd_sec_view_mode: full\r\n"),
        ));
        script.push(("reply SET_PARAMETER", sink_req("TEARDOWN", "")));
        let (end, sink, _) = session(script, &cfg("lllate"));
        assert!(matches!(end, End::SinkTeardown), "{end:?}");
        assert_eq!(ll(&sink), 1, "{:?}", sink.seen);
        let first_sec = sink.seen.iter().position(|s| s == "reply SET_PARAMETER").unwrap();
        let ll_at = sink.seen.iter().position(|s| s.contains("wfd_sec_low_latency_mode")).unwrap();
        assert!(ll_at > first_sec, "{:?}", sink.seen);
    }

    #[test]
    fn low_latency_off_is_not_sent() {
        let mut script = up_to_play();
        script.push(("reply PLAY", sink_req("SET_PARAMETER", "wfd_sec_view_mode: multi\r\n")));
        script.push(("GET_PARAMETER", sink_req("TEARDOWN", "")));
        let mut c = cfg("lloff");
        c.low_latency = None;
        let (_, sink, _) = session(script, &c);
        assert!(!sink.seen.iter().any(|s| s.contains("low_latency")));
    }

    #[test]
    fn no_setup_or_no_play_ends_the_session() {
        let mut script = up_to_play();
        script.pop(); // no PLAY
        let (end, _, counts) = session(script.clone(), &cfg("noplay"));
        assert!(matches!(&end, End::Failed(e) if e.starts_with("sink sent no PLAY (M7)")), "{end:?}");
        assert_eq!(counts.starts.get(), 0);
        script.pop(); // no SETUP
        let (end, _, _) = session(script, &cfg("nosetup"));
        assert!(matches!(&end, End::Failed(e) if e.starts_with("sink sent no SETUP (M6)")), "{end:?}");
    }

    #[test]
    fn rtsp_set_file_is_sent_once_while_playing() {
        let c = cfg("rtspset");
        std::fs::create_dir_all(&c.media.dir).unwrap();
        std::fs::write(c.media.dir.join(RTSP_SET_FILE), "wfd_sec_low_latency_mode=off\n").unwrap();
        let mut script = up_to_play();
        script.push(("GET_PARAMETER", sink_req("TEARDOWN", "")));
        let (_, sink, _) = session(script, &c);
        assert_eq!(sink.seen.iter().filter(|s| *s == "SET_PARAMETER wfd_sec_low_latency_mode: off").count(), 1);
        assert!(!c.media.dir.join(RTSP_SET_FILE).exists());
        let _ = std::fs::remove_dir_all(&c.media.dir);
    }
}
