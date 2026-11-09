//! Wi-Fi Direct through NetworkManager's D-Bus API (works as a normal user via polkit; wpa_supplicant's
//! own D-Bus API is denied to users on this system).
//!
//! The station interface is never touched: NM/wpa_supplicant put the P2P group on the STA channel when
//! the radio allows only one channel (single-channel concurrency, see data/probe/capabilities.md).

mod sightings;

use sightings::{Sightings, Watcher};
use std::cell::Cell;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use wfd_proto::device_info::{DeviceInfo, DeviceType};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const IF_DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const IF_P2P: &str = "org.freedesktop.NetworkManager.Device.WifiP2P";
const IF_PEER: &str = "org.freedesktop.NetworkManager.WifiP2PPeer";
const IF_ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const DEVICE_TYPE_WIFI_P2P: u32 = 30;
const AC_STATE_ACTIVATED: u32 = 2;
const AC_STATE_DEACTIVATED: u32 = 4;
/// Our WFD IE: subelement 0 (device info): source, session available, RTSP port 7236, 50 Mb/s.
const OUR_DEVICE_INFO: DeviceInfo = DeviceInfo {
    device_type: DeviceType::Source,
    session_available: true,
    rtsp_port: wfd_proto::device_info::RTSP_PORT,
    max_throughput_mbps: 50,
};
/// Find and activation poll step (also the watchdog cadence while waiting).
const POLL_STEP: Duration = Duration::from_millis(250);
pub const CONN_ID: &str = "wfd-source";

pub type Result<T> = std::result::Result<T, String>;

fn e(ctx: &str) -> impl Fn(zbus::Error) -> String + '_ {
    move |err| format!("{ctx}: {err}")
}

pub struct Nm {
    conn: Connection,
    dev: OwnedObjectPath,
    /// Ignore sightings before this instant: after the sink ended a session its cached "available"
    /// advertisement is stale; wait until a scan sees it again (user re-entered Smart View).
    min_seen: Cell<Option<Instant>>,
    /// wpa_supplicant's P2P-DEVICE-FOUND events (sightings.rs: why not NM's LastSeen/WfdIEs).
    seen: Sightings,
    _watcher: Watcher,
}

type Settings = HashMap<&'static str, HashMap<&'static str, Value<'static>>>;

/// AddAndActivateConnection2 settings: a volatile wifi-p2p connection to `mac` with our WFD IE.
fn p2p_settings(mac: &str) -> Settings {
    let con = HashMap::from([
        ("id", Value::from(CONN_ID)),
        ("type", Value::from("wifi-p2p")),
        ("autoconnect", Value::Bool(false)),
    ]);
    let p2p = HashMap::from([
        ("peer", Value::from(mac.to_ascii_uppercase())),
        ("wfd-ies", Value::from(OUR_DEVICE_INFO.subelement().to_vec())),
    ]);
    let ip4 = HashMap::from([("method", Value::from("auto")), ("never-default", Value::Bool(true))]);
    let ip6 = HashMap::from([("method", Value::from("disabled"))]);
    HashMap::from([("connection", con), ("wifi-p2p", p2p), ("ipv4", ip4), ("ipv6", ip6)])
}

impl Nm {
    pub fn new() -> Result<Self> {
        let conn = Connection::system().map_err(e("system bus"))?;
        let nm = Proxy::new(&conn, NM, NM_PATH, NM).map_err(e("nm proxy"))?;
        let devs: Vec<OwnedObjectPath> = nm.call("GetDevices", &()).map_err(e("GetDevices"))?;
        for d in devs {
            let p = Proxy::new(&conn, NM, d.as_str(), IF_DEVICE).map_err(e("dev"))?;
            if p.get_property::<u32>("DeviceType").map_err(e("DeviceType"))? == DEVICE_TYPE_WIFI_P2P {
                let seen = Sightings::default();
                let _watcher = Watcher::spawn(seen.clone());
                return Ok(Nm { conn: conn.clone(), dev: d.clone(), min_seen: Cell::new(None), seen, _watcher });
            }
        }
        Err("no Wi-Fi P2P device in NetworkManager".into())
    }

    fn p2p(&self) -> Result<Proxy<'_>> {
        Proxy::new(&self.conn, NM, self.dev.as_str(), IF_P2P).map_err(e("p2p"))
    }

    /// Peer with this MAC that wpa_supplicant found within the last 10 s (sightings::FRESH) and that advertises
    /// "WFD session available" (WFD spec §4.5).
    /// A stale cache entry (TV left Smart View) or a busy sink must not trigger a connection attempt:
    /// each failed attempt throws the TV out of Smart View.
    pub fn lookup(&self, mac: &str) -> Result<Option<OwnedObjectPath>> {
        let peers: Vec<OwnedObjectPath> = self.p2p()?.get_property("Peers").map_err(e("Peers"))?;
        for p in peers {
            let pp = Proxy::new(&self.conn, NM, p.as_str(), IF_PEER).map_err(e("peer"))?;
            if let Ok(hw) = pp.get_property::<String>("HwAddress") {
                if hw.eq_ignore_ascii_case(mac) {
                    let fresh = self.seen.fresh(mac, Instant::now(), self.min_seen.get());
                    // the fresh sighting's own flag first; NM's cached WfdIEs only as a fallback
                    let avail = self.seen.get(mac).and_then(|s| s.available).or_else(|| {
                        let ies: Vec<u8> = pp.get_property("WfdIEs").unwrap_or_default();
                        wfd_proto::device_info::session_available_in_ies(&ies)
                    });
                    if fresh && avail != Some(false) {
                        return Ok(Some(p.clone()));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Short P2P find (it takes the radio off the STA channel, so keep it short and stop early).
    /// Ends early on SIGTERM (Ok(None)); feeds the watchdog while it polls.
    pub fn find(&self, mac: &str, max: Duration) -> Result<Option<OwnedObjectPath>> {
        if let Some(p) = self.lookup(mac)? {
            return Ok(Some(p.clone()));
        }
        let p2p = self.p2p()?;
        let mut opts: HashMap<&str, Value> = HashMap::new();
        opts.insert("timeout", Value::I32(max.as_secs().max(1) as i32));
        p2p.call_method("StartFind", &(opts,)).map_err(e("StartFind"))?;
        let t0 = Instant::now();
        let mut found = None;
        while t0.elapsed() < max && !crate::stopping() {
            std::thread::sleep(POLL_STEP);
            crate::notify("WATCHDOG=1");
            if let Some(p) = self.lookup(mac)? {
                found = Some(p);
                break;
            }
        }
        let _ = p2p.call_method("StopFind", &());
        Ok(found)
    }

    /// Deactivate our P2P connection if it is active (idempotent; also cleans up after a crash).
    pub fn deactivate_ours(&self) {
        let Ok(nm) = Proxy::new(&self.conn, NM, NM_PATH, NM) else { return };
        let Ok(acs) = nm.get_property::<Vec<OwnedObjectPath>>("ActiveConnections") else { return };
        for ac in acs {
            let Ok(p) = Proxy::new(&self.conn, NM, ac.as_str(), IF_ACTIVE) else {
                continue;
            };
            let id: String = p.get_property("Id").unwrap_or_default();
            let ty: String = p.get_property("Type").unwrap_or_default();
            if id == CONN_ID || (ty == "wifi-p2p" && id.starts_with("wfd")) {
                let _ = nm.call_method("DeactivateConnection", &(&ac,));
            }
        }
    }

    /// Connect to the peer as a WFD source; returns the active connection once ACTIVATED.
    pub fn connect(&self, mac: &str, peer: &OwnedObjectPath, timeout: Duration) -> Result<OwnedObjectPath> {
        let nm = Proxy::new(&self.conn, NM, NM_PATH, NM).map_err(e("nm"))?;
        let settings = p2p_settings(mac);
        let mut opts: HashMap<&str, Value> = HashMap::new();
        opts.insert("persist", Value::from("volatile"));
        let (_path, ac, _res): (OwnedObjectPath, OwnedObjectPath, HashMap<String, OwnedValue>) = nm
            .call("AddAndActivateConnection2", &(settings, &self.dev, peer, opts))
            .map_err(e("AddAndActivateConnection2"))?;
        let t0 = Instant::now();
        while t0.elapsed() < timeout {
            if crate::stopping() {
                self.deactivate_ours();
                return Err("stopping".into());
            }
            match self.ac_state(&ac) {
                Ok(AC_STATE_ACTIVATED) => return Ok(ac),
                Ok(AC_STATE_DEACTIVATED) | Err(_) => return Err("activation failed (group formation)".into()),
                _ => {
                    crate::notify("WATCHDOG=1");
                    std::thread::sleep(POLL_STEP)
                }
            }
        }
        self.deactivate_ours();
        Err("activation timeout".into())
    }

    /// Only accept sightings from `secs` seconds from now on.
    pub fn ignore_sightings_for(&self, secs: u64) {
        self.min_seen.set(Some(Instant::now() + Duration::from_secs(secs)));
    }

    /// Is the active connection still up?
    pub fn is_active(&self, ac: &OwnedObjectPath) -> bool {
        self.ac_state(ac).is_ok_and(|s| s == AC_STATE_ACTIVATED)
    }

    /// NMActiveConnectionState of an active connection.
    fn ac_state(&self, ac: &OwnedObjectPath) -> zbus::Result<u32> {
        Proxy::new(&self.conn, NM, ac.as_str(), IF_ACTIVE).and_then(|p| p.get_property::<u32>("State"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_wfd_ie_is_the_one_the_tv_accepts() {
        assert_eq!(OUR_DEVICE_INFO.subelement(), [0x00, 0x00, 0x06, 0x00, 0x10, 0x1c, 0x44, 0x00, 0x32]);
    }

    #[test]
    fn connection_settings() {
        let s = p2p_settings("aa:bb:cc:dd:ee:ff");
        assert_eq!(s["connection"]["id"], Value::from(CONN_ID));
        assert_eq!(s["connection"]["type"], Value::from("wifi-p2p"));
        assert_eq!(s["connection"]["autoconnect"], Value::Bool(false));
        assert_eq!(s["wifi-p2p"]["peer"], Value::from("AA:BB:CC:DD:EE:FF"));
        assert_eq!(s["wifi-p2p"]["wfd-ies"], Value::from(OUR_DEVICE_INFO.subelement().to_vec()));
        assert_eq!(s["ipv4"]["method"], Value::from("auto"));
        assert_eq!(s["ipv4"]["never-default"], Value::Bool(true));
        assert_eq!(s["ipv6"]["method"], Value::from("disabled"));
        assert_eq!(s.len(), 4);
    }
}
