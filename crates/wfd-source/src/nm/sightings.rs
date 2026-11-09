//! Last P2P-DEVICE-FOUND per MAC from wpa_supplicant's journal.
//!
//! NM's LastSeen is not updated when an already known peer is found again (measured 2026-11-08), so it
//! cannot tell a stale cache entry from a TV that answers right now. wpa_supplicant reports every peer
//! once per p2p_find, so a sighting after our StartFind means "answered now".
//! Also the session availability from the same event's `wfd_dev_info`: NM keeps a peer's WfdIEs from its
//! first sighting, so after the TV's consent prompt (0x0101) it reported "not available" forever while
//! the TV advertised 0x0111 again (2026-11-09, sinks/ Q4).

use std::collections::HashMap;
use std::io::BufRead;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use wfd_util::Backoff;

/// A sighting this recent means "the TV answered our find now".
pub const FRESH: Duration = Duration::from_secs(10);
/// Every device around is reported; only recent sightings matter (FRESH), older ones are dropped.
pub const RETENTION: Duration = Duration::from_secs(60);
/// journalctl that ran shorter than this "ended at once" (no journal access): back off.
const QUICK_EXIT: Duration = Duration::from_secs(10);
/// Respawn delays after quick exits: 2 s doubling to 5 min (no jitter, one local process).
const RESPAWN_MIN: Duration = Duration::from_secs(2);
const RESPAWN_MAX: Duration = Duration::from_secs(300);
/// Respawn delay after a journalctl that ran for a while.
const RESPAWN_AFTER_RUN: Duration = Duration::from_secs(1);
/// journalctl could not be started at all: try again after this long.
const SPAWN_RETRY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sighting {
    pub at: Instant,
    /// "WFD session available" from the event's `wfd_dev_info` (None without one).
    pub available: Option<bool>,
}

/// Sightings by MAC (lowercase), shared between the watcher thread and lookups.
#[derive(Debug, Clone, Default)]
pub struct Sightings(Arc<Mutex<HashMap<String, Sighting>>>);

impl Sightings {
    fn map(&self) -> MutexGuard<'_, HashMap<String, Sighting>> {
        // a poisoned lock must not kill the watcher (the TV would never look "fresh" again)
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn record(&self, mac: &str, at: Instant, available: Option<bool>) {
        let mut m = self.map();
        m.retain(|_, s| at.saturating_duration_since(s.at) < RETENTION);
        m.insert(mac.to_ascii_lowercase(), Sighting { at, available });
    }

    /// Record a wpa_supplicant log line if it is a P2P-DEVICE-FOUND event.
    pub fn record_line(&self, line: &str, at: Instant) {
        if let Some(rest) = line.split("P2P-DEVICE-FOUND ").nth(1) {
            if let Some(mac) = rest.split_whitespace().next() {
                self.record(mac, at, journal_availability(rest));
            }
        }
    }

    pub fn get(&self, mac: &str) -> Option<Sighting> {
        self.map().get(&mac.to_ascii_lowercase()).copied()
    }

    /// Seen within FRESH of `now`, and not before `min_seen` (stale advertisement after a TEARDOWN).
    pub fn fresh(&self, mac: &str, now: Instant, min_seen: Option<Instant>) -> bool {
        self.get(mac)
            .is_some_and(|s| now.saturating_duration_since(s.at) <= FRESH && min_seen.is_none_or(|m| s.at >= m))
    }
}

/// Session availability from a P2P-DEVICE-FOUND line's `wfd_dev_info=0x<device info><port><throughput>`.
fn journal_availability(event: &str) -> Option<bool> {
    let hex = event.split("wfd_dev_info=0x").nth(1)?.get(..4)?;
    let bm = u16::from_str_radix(hex, 16).ok()?;
    Some(wfd_proto::device_info::session_available(bm))
}

/// Follows wpa_supplicant's P2P-DEVICE-FOUND events (journald; needs journal read access) into
/// `Sightings`. Dropping it kills the journalctl child and ends the thread.
pub struct Watcher {
    stop: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Watcher {
    pub fn spawn(seen: Sightings) -> Watcher {
        let w = Watcher { stop: Arc::default(), child: Arc::default() };
        let (stop, slot) = (w.stop.clone(), w.child.clone());
        std::thread::spawn(move || watch(&seen, &stop, &slot));
        w
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // the child is still in the slot only while it is not reaped, so the pid is still ours
        if let Some(c) = self.child.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            let _ = c.kill();
        }
    }
}

fn watch(seen: &Sightings, stop: &AtomicBool, slot: &Mutex<Option<Child>>) {
    let mut backoff = Backoff::new(RESPAWN_MIN, RESPAWN_MAX);
    while !stop.load(Ordering::Relaxed) {
        let started = Instant::now();
        let child = Command::new("journalctl")
            .args(["-f", "-n0", "-o", "cat", "-t", "wpa_supplicant"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else {
            wfd_util::event!("journal_unavailable", "why" => "journalctl did not start");
            std::thread::sleep(SPAWN_RETRY);
            continue;
        };
        let out = child.stdout.take().expect("piped");
        *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
        if stop.load(Ordering::Relaxed) {
            // dropped between the spawn and the slot: Drop did not see this child
            if let Some(c) = slot.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                let _ = c.kill();
            }
        }
        for line in std::io::BufReader::new(out).lines().map_while(|l| l.ok()) {
            seen.record_line(&line, Instant::now());
        }
        if let Some(mut c) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = c.wait();
        }
        // journalctl that ends at once (no journal access) must not be respawned every second forever
        let delay = if started.elapsed() < QUICK_EXIT {
            if backoff.current() == RESPAWN_MIN {
                // once per streak: without wpa_supplicant's sightings the TV is never seen as "fresh"
                wfd_util::event!("journal_unavailable", "why" => "journalctl ended at once (journal access?)");
            }
            backoff.next_with(0.0)
        } else {
            backoff.reset();
            RESPAWN_AFTER_RUN
        };
        std::thread::sleep(delay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = "P2P-DEVICE-FOUND aa:bb p2p_dev_addr=aa:bb pri_dev_type=7-0050F204-1 name='TV' wfd_dev_info=0x01111c440036 vendor_elems=1";

    #[test]
    fn availability_from_the_wpa_supplicant_event() {
        assert_eq!(journal_availability(LINE), Some(true));
        assert_eq!(journal_availability(&LINE.replace("0x0111", "0x0101")), Some(false));
        assert_eq!(journal_availability("P2P-DEVICE-FOUND aa:bb name='phone'"), None);
    }

    #[test]
    fn records_found_lines_case_insensitively() {
        let s = Sightings::default();
        let t0 = Instant::now();
        s.record_line(&LINE.replace("aa:bb p2p", "AA:BB p2p"), t0);
        s.record_line("P2P-GROUP-STARTED p2p-wlp1s0-0 GO", t0);
        assert_eq!(s.get("Aa:Bb"), Some(Sighting { at: t0, available: Some(true) }));
        assert_eq!(s.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn fresh_for_ten_seconds_and_not_before_min_seen() {
        let s = Sightings::default();
        let t0 = Instant::now();
        s.record("aa:bb", t0, None);
        assert!(s.fresh("aa:bb", t0 + FRESH, None));
        assert!(!s.fresh("aa:bb", t0 + FRESH + Duration::from_millis(1), None));
        assert!(!s.fresh("cc:dd", t0, None));
        // after a TEARDOWN: the cached sighting does not count, a later one does
        let min = t0 + Duration::from_secs(5);
        assert!(!s.fresh("aa:bb", t0 + Duration::from_secs(1), Some(min)));
        s.record("aa:bb", min, Some(true));
        assert!(s.fresh("aa:bb", min, Some(min)));
    }

    #[test]
    fn old_sightings_are_dropped_after_a_minute() {
        let s = Sightings::default();
        let t0 = Instant::now();
        s.record("aa:bb", t0, None);
        s.record("cc:dd", t0 + RETENTION - Duration::from_millis(1), None);
        assert!(s.get("aa:bb").is_some());
        s.record("ee:ff", t0 + RETENTION, None);
        assert!(s.get("aa:bb").is_none());
        assert!(s.get("cc:dd").is_some());
    }
}
