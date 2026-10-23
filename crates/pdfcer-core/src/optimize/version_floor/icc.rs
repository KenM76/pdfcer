//! ICC profile header versions (ICC.1 §7.2.4) and the strict mapping
//! from them to the PDF version whose ICCBased table first names that
//! edition (1.7 §8.6.5.5 Table 67). ISO 32000-2 erratum #181 removes the
//! PDF-version column, so the mapping is a setting, not a rule.

use crate::PdfVersion;
use crate::object::ObjId;

/// How an embedded ICC profile's version counts toward the floor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum IccVersionGate {
    /// Report each profile's version; raise nothing. Under ISO 32000-2
    /// with its errata the ICC version is not a PDF-version gate, and a
    /// reader that cannot use a profile substitutes its `/Alternate`.
    #[default]
    Permissive,
    /// Raise the floor to the first PDF version whose ICCBased table names
    /// the profile's edition (header 2.2 needs 1.4, 4.1 needs 1.6, 4.3
    /// needs 2.0). A version no table names (4.4 and later) raises nothing
    /// and is reported as such.
    Strict,
}

/// One embedded ICC profile and the version its header declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct IccProfile {
    /// The profile stream.
    pub object: ObjId,
    /// Header major version (byte 8).
    pub major: u8,
    /// Header minor version (high nibble of byte 9).
    pub minor: u8,
    /// The PDF version [`IccVersionGate::Strict`] maps it to; `None` when
    /// no PDF version names it.
    pub strict: Option<PdfVersion>,
}

/// The header version of profile bytes `data`, decoded from the nibbles.
pub(super) fn header(data: &[u8]) -> Option<(u8, u8)> {
    let major = *data.get(8)?;
    let minor = *data.get(9)? >> 4;
    // Byte 8 is BCD; anything else is not an ICC header.
    (major >> 4 <= 9 && major & 0xF <= 9 && major != 0)
        .then(|| (10 * (major >> 4) + (major & 0xF), minor))
}

/// The strict PDF version code for header `major.minor`.
pub(super) fn strict(major: u8, minor: u8) -> Option<u8> {
    match (major, minor) {
        (0..=1, _) | (2, 0..=1) => Some(13),
        (2, 2..=3) => Some(14),
        (2, _) | (4, 0) => Some(15),
        (4, 1) => Some(16),
        (4, 2) => Some(17),
        (4, 3) => Some(20),
        _ => None,
    }
}
