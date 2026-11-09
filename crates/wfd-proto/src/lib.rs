//! Wi-Fi Display (Miracast R1) protocol pieces without I/O: RTSP message codec,
//! `text/parameters` bodies and the WFD capability parameters a source needs.

pub mod audio;
pub mod device_info;
pub mod params;
pub mod rtsp;
pub mod video;

pub use params::Params;
pub use rtsp::{Decoder, Message};
