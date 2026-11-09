//! Helpers shared by wfd-source, wfd-media and wfd-tx. No dependencies beyond libc; nothing here is on a
//! per-frame hot path except `unix_us`/`mono_us`.
//!
//! Log format (all three programs): one JSON object per line on stderr, `{"event":NAME,...,"t":UNIX_US}`
//! (`event`, `emit`). Values are typed (`J`): numbers stay numbers, strings are escaped with `json_str`.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Wall clock in Unix microseconds (log timestamps; never for intervals — use `Instant`/`mono_us`).
pub fn unix_us() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_micros() as u64).unwrap_or(0)
}

/// CLOCK_MONOTONIC in microseconds (the PCR base of wfd-tx, the frame clock of wfd-media).
pub fn mono_us() -> i64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `ts` is a valid, writable timespec; CLOCK_MONOTONIC is always available on Linux.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec * 1_000_000 + ts.tv_nsec / 1000
}

/// A typed JSON value for `event`.
#[derive(Debug, Clone, Copy)]
pub enum J<'a> {
    I(i64),
    U(u64),
    F(f64),
    B(bool),
    S(&'a str),
}

macro_rules! j_from {
    ($($t:ty => $v:ident as $c:ty),*) => {$(
        impl From<$t> for J<'_> {
            fn from(x: $t) -> Self {
                J::$v(x as $c)
            }
        }
    )*};
}
j_from!(i64 => I as i64, i32 => I as i64, u64 => U as u64, u32 => U as u64, u16 => U as u64,
        u8 => U as u64, usize => U as u64, f64 => F as f64, f32 => F as f64);
impl From<bool> for J<'_> {
    fn from(x: bool) -> Self {
        J::B(x)
    }
}
impl<'a> From<&'a str> for J<'a> {
    fn from(x: &'a str) -> Self {
        J::S(x)
    }
}
impl<'a> From<&'a String> for J<'a> {
    fn from(x: &'a String) -> Self {
        J::S(x)
    }
}

impl J<'_> {
    fn write(&self, o: &mut String) {
        let _ = match *self {
            J::I(v) => write!(o, "{v}"),
            J::U(v) => write!(o, "{v}"),
            // JSON has no NaN/inf
            J::F(v) if v.is_finite() => write!(o, "{v}"),
            J::F(_) => write!(o, "null"),
            J::B(v) => write!(o, "{v}"),
            J::S(s) => {
                o.push_str(&json_str(s));
                Ok(())
            }
        };
    }
}

/// One log line: `{"event":name,k:v,...,"t":unix_us}`. A key "t" in `kv` replaces the wall clock.
pub fn event_line(name: &str, kv: &[(&str, J)]) -> String {
    let mut o = String::with_capacity(48 + 24 * kv.len());
    o.push_str("{\"event\":");
    o.push_str(&json_str(name));
    for (k, v) in kv {
        o.push(',');
        o.push_str(&json_str(k));
        o.push(':');
        v.write(&mut o);
    }
    if !kv.iter().any(|(k, _)| *k == "t") {
        let _ = write!(o, ",\"t\":{}", unix_us());
    }
    o.push('}');
    o
}

/// Log one event to stderr (see `event_line`). Not for per-packet paths (it allocates).
pub fn event(name: &str, kv: &[(&str, J)]) {
    eprintln!("{}", event_line(name, kv));
}

/// `event!("name", "k" => v, ...)`: `event` with `Into<J>` values.
#[macro_export]
macro_rules! event {
    ($name:expr $(, $k:literal => $v:expr)* $(,)?) => {
        $crate::event($name, &[$(($k, $crate::J::from($v))),*])
    };
}

/// Log `{"event":name,"error":err}` and exit 1 (a supervised restart, not a panic).
pub fn die(name: &str, err: &str) -> ! {
    event(name, &[("error", J::S(err))]);
    std::process::exit(1)
}

/// Exponential backoff: `min` doubling to `max`, with ±25 % jitter from the clock.
#[derive(Debug, Clone)]
pub struct Backoff {
    cur: Duration,
    min: Duration,
    max: Duration,
}

impl Backoff {
    pub fn new(min: Duration, max: Duration) -> Self {
        Backoff { cur: min, min, max }
    }
    pub fn reset(&mut self) {
        self.cur = self.min;
    }
    /// The delay to wait now (`jitter` in -0.25..0.25), and double the next one.
    pub fn next_with(&mut self, jitter: f64) -> Duration {
        let d = self.cur.mul_f64(1.0 + jitter);
        self.cur = (self.cur * 2).min(self.max);
        d
    }
    /// `next_with` with a jitter from the wall clock's microseconds.
    pub fn next_jittered(&mut self) -> Duration {
        self.next_with((unix_us() % 500) as f64 / 1000.0 - 0.25)
    }
    /// The current (not yet doubled) delay.
    pub fn current(&self) -> Duration {
        self.cur
    }
}

/// `s` as a JSON string literal, with quotes.
pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

const NSIG: usize = 65;
static FLAGS: [AtomicPtr<AtomicBool>; NSIG] = [const { AtomicPtr::new(std::ptr::null_mut()) }; NSIG];

extern "C" fn set_flag(sig: libc::c_int) {
    // only atomic loads and stores: async-signal-safe
    if let Some(slot) = FLAGS.get(sig as usize) {
        let p = slot.load(Ordering::Acquire);
        if !p.is_null() {
            // SAFETY: only `&'static AtomicBool`s are stored in FLAGS (signal_flag below).
            unsafe { (*p).store(true, Ordering::Relaxed) };
        }
    }
}

/// On signal `sig`, set `flag` (and do nothing else). The program polls the flag.
pub fn signal_flag(sig: libc::c_int, flag: &'static AtomicBool) {
    assert!((sig as usize) < NSIG, "signal number");
    FLAGS[sig as usize].store(flag as *const AtomicBool as *mut AtomicBool, Ordering::Release);
    // SAFETY: the handler only touches atomics (async-signal-safe).
    unsafe { libc::signal(sig, set_flag as extern "C" fn(libc::c_int) as libc::sighandler_t) };
}

/// The leading `//!` block of a source file (pass `include_str!("main.rs")`) as --help text.
pub fn doc_help(src: &str) -> String {
    src.lines().take_while(|l| l.starts_with("//!")).map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n")
}

/// Panics go to stderr as one JSON line ({"event":"panic",...}) like every other event, so journald
/// filters and the supervisor see them; the process still exits (101) after the default unwinding.
pub fn install_panic_hook(program: &'static str) {
    std::panic::set_hook(Box::new(move |info| {
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        event("panic", &[("program", J::S(program)), ("msg", J::S(&msg)), ("at", J::S(&at))]);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_str_escapes() {
        assert_eq!(json_str("a\"b\\c\nd\u{1}"), "\"a\\\"b\\\\c\\nd\\u0001\"");
    }

    #[test]
    fn event_line_types_and_escapes() {
        let l = event_line("x", &[("n", 3u32.into()), ("f", 1.5.into()), ("s", "a\"b".into()), ("t", 7u64.into())]);
        assert_eq!(l, r#"{"event":"x","n":3,"f":1.5,"s":"a\"b","t":7}"#);
        assert!(event_line("y", &[]).starts_with(r#"{"event":"y","t":"#));
        assert!(event_line("z", &[("f", f64::NAN.into())]).contains(r#""f":null"#));
    }

    #[test]
    fn backoff_doubles_to_max_and_resets() {
        let mut b = Backoff::new(Duration::from_secs(2), Duration::from_secs(60));
        let d: Vec<u64> = (0..7).map(|_| b.next_with(0.0).as_secs()).collect();
        assert_eq!(d, [2, 4, 8, 16, 32, 60, 60]);
        b.reset();
        assert_eq!(b.next_with(0.25), Duration::from_millis(2500));
    }

    #[test]
    fn doc_help_takes_the_whole_header() {
        let src = "//! one\n//! two\n\nuse x;\n//! not this";
        assert_eq!(doc_help(src), " one\n two");
    }

    #[test]
    fn signal_flag_sets_the_flag() {
        static F: AtomicBool = AtomicBool::new(false);
        signal_flag(libc::SIGUSR2, &F);
        // SAFETY: raising a signal whose handler only stores to an atomic.
        unsafe { libc::raise(libc::SIGUSR2) };
        assert!(F.load(Ordering::Relaxed));
    }
}
