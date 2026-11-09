//! `text/parameters` bodies: `name: value` lines (values may be absent in GET_PARAMETER requests).

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Params(pub Vec<(String, String)>);

impl Params {
    pub fn parse(body: &str) -> Self {
        Params(
            body.split("\r\n")
                .flat_map(|l| l.split('\n'))
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(|l| match l.split_once(':') {
                    Some((n, v)) => (n.trim().to_string(), v.trim().to_string()),
                    None => (l.to_string(), String::new()),
                })
                .collect(),
        )
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(n, _)| n.as_str())
    }

    pub fn set(mut self, name: &str, value: impl Into<String>) -> Self {
        self.0.push((name.into(), value.into()));
        self
    }

    /// Body for SET_PARAMETER / GET_PARAMETER responses.
    pub fn to_body(&self) -> String {
        self.0.iter().map(|(n, v)| format!("{n}: {v}\r\n")).collect()
    }

    /// Body for a GET_PARAMETER request (names only).
    pub fn names_body<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
        names.into_iter().map(|n| format!("{n}\r\n")).collect()
    }
}

/// The sink's RTP port 0 from `wfd_client_rtp_ports` (`RTP/AVP/UDP;unicast 19002 0 mode=play`).
pub fn client_rtp_port0(value: &str) -> Option<u16> {
    value.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sink_reply_and_requests() {
        let p = Params::parse("wfd_sec_low_latency_mode: off\r\nwfd_sec_view_mode: multi\r\n");
        assert_eq!(p.get("wfd_sec_low_latency_mode"), Some("off"));
        let q = Params::parse("wfd_idr_request\r\n");
        assert_eq!(q.names().collect::<Vec<_>>(), ["wfd_idr_request"]);
        assert_eq!(Params::names_body(["a", "b"]), "a\r\nb\r\n");
    }

    #[test]
    fn value_with_colons_kept() {
        let p = Params::parse("wfd_presentation_URL: rtsp://10.42.0.1:7236/wfd1.0/streamid=0 none");
        assert_eq!(p.get("wfd_presentation_URL"), Some("rtsp://10.42.0.1:7236/wfd1.0/streamid=0 none"));
    }

    #[test]
    fn client_rtp_port0_from_m3() {
        assert_eq!(client_rtp_port0("RTP/AVP/UDP;unicast 19002 0 mode=play"), Some(19002));
        assert_eq!(client_rtp_port0("RTP/AVP/UDP;unicast"), None);
        assert_eq!(client_rtp_port0("RTP/AVP/UDP;unicast x 0 mode=play"), None);
        assert_eq!(client_rtp_port0("RTP/AVP/UDP;unicast 70000 0 mode=play"), None);
    }
}
