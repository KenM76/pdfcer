//! The ICC profile header fields PDF/A constrains (ICC.1:2001-04 §6.1 and
//! ICC.1:2010 §7.2: version at byte 8, device class at 12, data colour
//! space at 16), read from a profile stream's decoded data.

use crate::object::Stream;
use crate::preflight::cos::decoded;
use crate::view::DocumentView;

/// The header of one ICC profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Header {
    /// Major version (byte 8).
    pub(super) major: u8,
    /// Profile/device class signature (`prtr`, `mntr`, `scnr`, `spac`, ...).
    pub(super) class: [u8; 4],
    /// Data colour space signature (`GRAY`, `RGB `, `CMYK`, `Lab `, ...).
    pub(super) space: [u8; 4],
}

impl Header {
    /// Parse the first 20 bytes of `data`; `None` when it is too short.
    pub(super) fn parse(data: &[u8]) -> Option<Self> {
        let sig = |at: usize| -> Option<[u8; 4]> { data.get(at..at + 4)?.try_into().ok() };
        Some(Self {
            major: *data.get(8)?,
            class: sig(12)?,
            space: sig(16)?,
        })
    }

    /// The header of `stream`'s profile; `None` when it cannot be decoded or
    /// is too short.
    pub(super) fn of(view: &DocumentView<'_>, stream: &Stream) -> Option<Self> {
        Self::parse(&decoded(view, stream)?)
    }

    /// Channels the data colour space has, for the spaces PDF/A allows.
    pub(super) fn channels(&self) -> Option<i64> {
        match &self.space {
            b"GRAY" => Some(1),
            b"RGB " | b"Lab " => Some(3),
            b"CMYK" => Some(4),
            _ => None,
        }
    }

    /// The class and space as text, for messages (`prtr CMYK v4`).
    pub(super) fn describe(&self) -> String {
        format!(
            "{} {} v{}",
            String::from_utf8_lossy(&self.class).trim_end(),
            String::from_utf8_lossy(&self.space).trim_end(),
            self.major
        )
    }
}
