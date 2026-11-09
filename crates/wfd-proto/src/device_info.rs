//! WFD Device Information subelement (WFD spec §5.1.2, subelement ID 0): the 6-byte body a device puts
//! in its P2P probe/beacon WFD IE: device-info bitmap, RTSP control port, maximum throughput (Mb/s).

/// The RTSP control port every WFD sink connects to (spec default).
pub const RTSP_PORT: u16 = 7236;
/// Subelement ID of the device information.
pub const SUBELEMENT_DEVICE_INFO: u8 = 0;
const DEVICE_INFO_LEN: u16 = 6;

/// Bits 1:0 of the device-info bitmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Source = 0,
    PrimarySink = 1,
    SecondarySink = 2,
    DualRole = 3,
}

/// Bits 5:4 of the device-info bitmap: 01 = "available for WFD session".
const SESSION_SHIFT: u16 = 4;
const SESSION_AVAILABLE: u16 = 1;

/// "Available for WFD session" from a device-info bitmap (bits 5:4 == 01).
pub fn session_available(bm: u16) -> bool {
    (bm >> SESSION_SHIFT) & 3 == SESSION_AVAILABLE
}

/// The device-info subelement from named fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceInfo {
    pub device_type: DeviceType,
    pub session_available: bool,
    pub rtsp_port: u16,
    pub max_throughput_mbps: u16,
}

impl DeviceInfo {
    pub fn bitmap(&self) -> u16 {
        let avail = if self.session_available { SESSION_AVAILABLE << SESSION_SHIFT } else { 0 };
        self.device_type as u16 | avail
    }

    /// The whole subelement: ID, length (6), bitmap, port, throughput (all big-endian).
    pub fn subelement(&self) -> [u8; 9] {
        let [l0, l1] = DEVICE_INFO_LEN.to_be_bytes();
        let [b0, b1] = self.bitmap().to_be_bytes();
        let [p0, p1] = self.rtsp_port.to_be_bytes();
        let [t0, t1] = self.max_throughput_mbps.to_be_bytes();
        [SUBELEMENT_DEVICE_INFO, l0, l1, b0, b1, p0, p1, t0, t1]
    }
}

/// "Session available" from raw WFD IE subelements (None if there is no device-info subelement).
pub fn session_available_in_ies(ies: &[u8]) -> Option<bool> {
    let mut i = 0;
    while i + 3 <= ies.len() {
        let (id, len) = (ies[i], u16::from_be_bytes([ies[i + 1], ies[i + 2]]) as usize);
        let body = ies.get(i + 3..i + 3 + len)?;
        if id == SUBELEMENT_DEVICE_INFO && len >= 2 {
            return Some(session_available(u16::from_be_bytes([body[0], body[1]])));
        }
        i += 3 + len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_source_subelement_is_byte_exact() {
        let ie = DeviceInfo {
            device_type: DeviceType::Source,
            session_available: true,
            rtsp_port: RTSP_PORT,
            max_throughput_mbps: 50,
        }
        .subelement();
        assert_eq!(ie, [0x00, 0x00, 0x06, 0x00, 0x10, 0x1c, 0x44, 0x00, 0x32]);
    }

    #[test]
    fn session_available_bits() {
        assert!(session_available(0x0111));
        assert!(!session_available(0x0101)); // Samsung after its consent prompt
        assert!(!session_available(0x0131)); // 11 = reserved
        assert!(session_available(0x0010));
    }

    #[test]
    fn session_available_from_ies() {
        assert_eq!(session_available_in_ies(&[0, 0, 6, 0x01, 0x11, 0x1c, 0x44, 0, 0x36]), Some(true));
        assert_eq!(session_available_in_ies(&[0, 0, 6, 0x01, 0x01, 0x1c, 0x44, 0, 0x36]), Some(false));
        assert_eq!(session_available_in_ies(&[]), None);
        // another subelement first, then device info
        assert_eq!(session_available_in_ies(&[7, 0, 1, 0xff, 0, 0, 2, 0x00, 0x10]), Some(true));
        // truncated body
        assert_eq!(session_available_in_ies(&[0, 0, 6, 0x00]), None);
    }
}
