//! Minimal RTSP/1.0 message codec as used by WFD (requests flow in both directions).

use std::fmt;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Error {
    #[error("malformed start line: {0:?}")]
    StartLine(String),
    #[error("malformed header line: {0:?}")]
    Header(String),
    #[error("bad Content-Length: {0:?}")]
    ContentLength(String),
    #[error("message head exceeds {0} bytes")]
    TooLarge(usize),
    #[error("message body of {0} bytes exceeds the limit")]
    BodyTooLarge(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum StartLine {
    Request { method: String, uri: String },
    Response { status: u16, reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub start: StartLine,
    /// Headers in wire order; names compared case-insensitively.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Message {
    pub fn request(method: &str, uri: &str, cseq: u32) -> Self {
        Message {
            start: StartLine::Request { method: method.into(), uri: uri.into() },
            headers: vec![("CSeq".into(), cseq.to_string())],
            body: String::new(),
        }
    }

    pub fn response(status: u16, reason: &str, cseq: u32) -> Self {
        Message {
            start: StartLine::Response { status, reason: reason.into() },
            headers: vec![("CSeq".into(), cseq.to_string())],
            body: String::new(),
        }
    }

    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Sets a `text/parameters` body (Content-Type/Length are added on serialisation).
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn cseq(&self) -> Option<u32> {
        self.get("CSeq").and_then(|v| v.trim().parse().ok())
    }

    pub fn method(&self) -> Option<&str> {
        match &self.start {
            StartLine::Request { method, .. } => Some(method),
            _ => None,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match &self.start {
            StartLine::Response { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.to_string().into_bytes()
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.start {
            StartLine::Request { method, uri } => write!(f, "{method} {uri} RTSP/1.0\r\n")?,
            StartLine::Response { status, reason } => write!(f, "RTSP/1.0 {status} {reason}\r\n")?,
        }
        for (n, v) in &self.headers {
            if n.eq_ignore_ascii_case("Content-Length") || n.eq_ignore_ascii_case("Content-Type") {
                continue;
            }
            write!(f, "{n}: {v}\r\n")?;
        }
        if !self.body.is_empty() {
            write!(f, "Content-Type: text/parameters\r\nContent-Length: {}\r\n", self.body.len())?;
        }
        write!(f, "\r\n{}", self.body)
    }
}

/// Incremental decoder: feed bytes from the socket, pull complete messages.
#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

const MAX_HEAD: usize = 64 * 1024;
/// WFD parameter bodies are a few hundred bytes; a sink announcing more must not make us buffer it.
const MAX_BODY: usize = 1024 * 1024;

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    /// Returns the next complete message, `Ok(None)` if more bytes are needed.
    pub fn next_message(&mut self) -> Result<Option<Message>, Error> {
        let Some(head_end) = find(&self.buf, b"\r\n\r\n") else {
            return if self.buf.len() > MAX_HEAD { Err(Error::TooLarge(MAX_HEAD)) } else { Ok(None) };
        };
        let head = String::from_utf8_lossy(&self.buf[..head_end]).into_owned();
        let mut lines = head.split("\r\n");
        let start = parse_start(lines.next().unwrap_or_default())?;
        let mut headers = Vec::new();
        for l in lines {
            let (n, v) = l.split_once(':').ok_or_else(|| Error::Header(l.into()))?;
            headers.push((n.trim().to_string(), v.trim().to_string()));
        }
        let len = match headers.iter().find(|(n, _)| n.eq_ignore_ascii_case("Content-Length")) {
            Some((_, v)) => v.parse::<usize>().map_err(|_| Error::ContentLength(v.clone()))?,
            None => 0,
        };
        if len > MAX_BODY {
            return Err(Error::BodyTooLarge(len));
        }
        let total = head_end + 4 + len;
        if self.buf.len() < total {
            return Ok(None);
        }
        let body = String::from_utf8_lossy(&self.buf[head_end + 4..total]).into_owned();
        self.buf.drain(..total);
        Ok(Some(Message { start, headers, body }))
    }
}

fn parse_start(line: &str) -> Result<StartLine, Error> {
    let err = || Error::StartLine(line.into());
    if let Some(rest) = line.strip_prefix("RTSP/1.0 ") {
        let (code, reason) = rest.split_once(' ').unwrap_or((rest, ""));
        let status = code.parse().map_err(|_| err())?;
        return Ok(StartLine::Response { status, reason: reason.into() });
    }
    let mut it = line.split(' ');
    match (it.next(), it.next(), it.next()) {
        (Some(m), Some(u), Some("RTSP/1.0")) if !m.is_empty() => {
            Ok(StartLine::Request { method: m.into(), uri: u.into() })
        }
        _ => Err(err()),
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from the Samsung sink during the 2026-11-08 probe, M1 response + M2 request
    // arriving in one TCP read.
    const M1_M2: &str = "RTSP/1.0 200 OK\r\nCSeq: 1\r\nPublic: OPTIONS, GET_PARAMETER, PAUSE, PLAY, SETUP, SET_PARAMETER, TEARDOWN, org.wfa.wfd1.0\r\nUser-Agent: SVDTZ6_5/SEC-UIBC3\r\nDate: Sun, 08 Nov 2026 15:22:27 GMT\r\n\r\nOPTIONS * RTSP/1.0\r\nCSeq: 1\r\nRequire: org.wfa.wfd1.0\r\nUser-Agent: SVDTZ6_5/SEC-UIBC3\r\nDate: Sun, 08 Nov 2026 15:22:27 GMT\r\n\r\n";

    #[test]
    fn decodes_pipelined_messages() {
        let mut d = Decoder::new();
        d.feed(M1_M2.as_bytes());
        let m1 = d.next_message().unwrap().unwrap();
        assert_eq!(m1.status(), Some(200));
        assert!(m1.get("public").unwrap().contains("org.wfa.wfd1.0"));
        let m2 = d.next_message().unwrap().unwrap();
        assert_eq!(m2.method(), Some("OPTIONS"));
        assert_eq!(m2.get("Require"), Some("org.wfa.wfd1.0"));
        assert_eq!(d.next_message().unwrap(), None);
    }

    #[test]
    fn waits_for_full_body_split_anywhere() {
        let msg = Message::response(200, "OK", 2).with_body("wfd_audio_codecs: AAC 00000001 00\r\n");
        let bytes = msg.to_bytes();
        for cut in 1..bytes.len() {
            let mut d = Decoder::new();
            d.feed(&bytes[..cut]);
            assert_eq!(d.next_message().unwrap(), None, "cut at {cut}");
            d.feed(&bytes[cut..]);
            let got = d.next_message().unwrap().unwrap();
            assert_eq!(got.body, msg.body);
            assert_eq!(got.cseq(), Some(2));
        }
    }

    #[test]
    fn serialises_request() {
        let m = Message::request("GET_PARAMETER", "rtsp://localhost/wfd1.0", 3).with_body("wfd_video_formats\r\n");
        let s = m.to_string();
        assert!(s.starts_with("GET_PARAMETER rtsp://localhost/wfd1.0 RTSP/1.0\r\nCSeq: 3\r\n"));
        assert!(s.contains("Content-Length: 19\r\n\r\nwfd_video_formats\r\n"));
    }

    #[test]
    fn rejects_a_huge_body_before_buffering_it() {
        let mut d = Decoder::new();
        d.feed(b"SET_PARAMETER rtsp://localhost/wfd1.0 RTSP/1.0\r\nCSeq: 1\r\nContent-Length: 999999999\r\n\r\n");
        assert!(matches!(d.next_message(), Err(Error::BodyTooLarge(999_999_999))));
    }

    #[test]
    fn rejects_garbage() {
        let mut d = Decoder::new();
        d.feed(b"HELLO\r\n\r\n");
        assert!(d.next_message().is_err());
    }
}
