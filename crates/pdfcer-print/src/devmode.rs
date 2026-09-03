//! # `DEVMODE` — the driver's own settings structure, sourced and amended
//!
//! Every driver-level print setting on Windows — orientation, paper size,
//! input tray, duplex, media type, stapling, and the vendor-private half
//! nobody outside the driver can name — travels in ONE structure:
//! `DEVMODEW`. It is handed to `CreateDC` when the device context is
//! opened, and to `ResetDC` when something must change mid-job.
//!
//! ## ★ Why this module exists at all: pdfcer used to SYNTHESISE one
//!
//! Until 2026-08-18 `pdfcer-print` built its `DEVMODE` from
//! `DEVMODEW::default()` — a zeroed structure — set `dmOrientation` and
//! `dmDuplex` on it, and gave that to `CreateDC`. Its own doc comment
//! said the opposite, in a heading:
//!
//! > *"Why it starts from the driver's own default rather than zeroed …
//! > the driver's current default is fetched first and only the
//! > requested fields are overwritten"*
//!
//! Nothing was fetched. The parameter that would have been needed to
//! fetch it (`_printer_wide`) was present, underscore-prefixed, unused.
//! The consequences were reported from outside by the `pdfcer-gui` shell
//! as three separate defects, which are one defect:
//!
//! | symptom | cause |
//! |---|---|
//! | paper size cannot be chosen | there was no `DEVMODE` to put a `dmPaperSize` in — only a synthetic one with two fields |
//! | the driver's properties dialog is unreachable | `DocumentProperties` RETURNS a populated `DEVMODE`, and there was nowhere to put one |
//! | `pick_tray_by_page_size` did nothing | ditto: no field was written, and nothing read the flag |
//!
//! A zeroed `DEVMODE` is not neutral. `dmFields` names which members are
//! meaningful, so a zeroed structure with `dmFields = DM_ORIENTATION`
//! does tell the driver "orientation only" and the rest of the driver's
//! own configuration survives *in practice* — but the structure also
//! carries a **driver-private tail** of `dmDriverExtra` bytes past
//! `dmSize`, and a synthesised structure has none of it. That tail is
//! where a driver keeps everything Win32 has no field for. Discarding it
//! is how "the operator configured stapling and pdfcer printed unstapled"
//! happens.
//!
//! So: **the base is always the driver's own**, obtained from
//! `DocumentPropertiesW(… DM_OUT_BUFFER)`, and pdfcer overwrites exactly
//! the fields it was asked to and names them in `dmFields`.
//!
//! ## Why this is a byte model rather than a `DEVMODEW` value
//!
//! Three reasons, and the first is the one that forces it:
//!
//! 1. **A `DEVMODE` is variable-length.** Its true length is
//!    `dmSize + dmDriverExtra`, where `dmSize` is the public header the
//!    running Windows defines and `dmDriverExtra` is the driver's
//!    private tail. Holding one as a `DEVMODEW` *value* silently
//!    truncates the tail, which is exactly the loss this module exists
//!    to prevent. It must be held as a buffer.
//! 2. **It has to survive a round trip through a file.** The properties
//!    dialog runs once; the configuration it returns is carried to a
//!    `spool` call that may be a separate `pdfcer` invocation. Opaque
//!    bytes with a validating parser is the only honest shape for that.
//! 3. **It makes the interesting half testable everywhere.** The field
//!    layout below is a stable, documented ABI, so the amend-a-`DEVMODE`
//!    logic — which is where the bugs are — is pure safe Rust with no
//!    `windows` dependency, and its tests run on the Linux and macOS CI
//!    jobs as well as on Windows. Only *acquiring* and *using* a
//!    configuration is Windows-only. This mirrors what the crate already
//!    does with `Printer`, `PrinterCaps` and the placement geometry, and
//!    is the same reasoning `imposition` carries.
//!
//! The obvious risk of a hand-written ABI model is that it drifts from
//! the real structure. That is not left to inspection: a `#[cfg(windows)]`
//! test asserts every offset here against `std::mem::offset_of!` on the
//! real `DEVMODEW`, and every flag against the real constant. If the
//! `windows` crate's layout ever changes, that test fails rather than a
//! printer misbehaving.
//!
//! ## The field layout (ISO-equivalent citation: Microsoft Win32 API,
//! `wingdi.h`, `DEVMODEW`)
//!
//! ```text
//! off  size  member
//!   0    64  dmDeviceName[32]   UTF-16, NUL-padded — the device this belongs to
//!  64     2  dmSpecVersion
//!  66     2  dmDriverVersion
//!  68     2  dmSize             bytes of the PUBLIC portion
//!  70     2  dmDriverExtra      bytes of the PRIVATE tail that follows it
//!  72     4  dmFields           bitmask: which members below are meaningful
//!  76     2  dmOrientation      DMORIENT_PORTRAIT (1) / DMORIENT_LANDSCAPE (2)
//!  78     2  dmPaperSize        a DMPAPER_* form id, or DMPAPER_USER (256)
//!  80     2  dmPaperLength      tenths of a millimetre
//!  82     2  dmPaperWidth       tenths of a millimetre
//!  84     2  dmScale
//!  86     2  dmCopies
//!  88     2  dmDefaultSource    a DMBIN_* value
//!  90     2  dmPrintQuality
//!  92     2  dmColor
//!  94     2  dmDuplex           DMDUP_SIMPLEX/VERTICAL/HORIZONTAL (1/2/3)
//!  96     2  dmYResolution
//!  98     2  dmTTOption
//! 100     2  dmCollate
//! 102    64  dmFormName[32]     UTF-16 form name — an ALTERNATIVE to dmPaperSize
//! …
//! 220        end of the public portion as this Windows defines it
//! ```
//!
//! Offsets 76..92 are a union with the display-settings variant
//! (`dmPosition`/`dmDisplayOrientation`/`dmDisplayFixedOutput`); the
//! printer variant is the one above, and a printer `DEVMODE` never uses
//! the other.
//!
//! ## ★ Two traps that produce a plausible wrong sheet
//!
//! **`dmPaperSize` and `dmFormName` can disagree, and which one wins is
//! not uniform across drivers.** A driver's own `DEVMODE` very often
//! arrives with `DM_FORMNAME` set and `dmFormName` naming the paper. If
//! pdfcer then writes `dmPaperSize` and leaves `DM_FORMNAME` asserted,
//! some drivers honour the name and print the *old* paper — a request
//! that appears to have been accepted and was not. So selecting a form
//! by id CLEARS `DM_FORMNAME`: exactly one of the two is asserted at a
//! time, always.
//!
//! **A custom size must clear the form and vice versa.** `DM_PAPERLENGTH`
//! and `DM_PAPERWIDTH` left asserted from the driver's base would
//! override a form id that was just written. Both directions are handled
//! in [`PrinterConfiguration::apply`], and both are tested.
//!
//! ## What this module deliberately does NOT model
//!
//! Everything else. `dmMediaType`, `dmColor`, `dmPrintQuality`,
//! `dmCollate`, `dmNup` and the driver-private tail are carried through
//! untouched, not interpreted. pdfcer has no operator-facing control for
//! them, and a field this crate cannot describe is a field it must not
//! silently rewrite — the operator reaches those through the driver's
//! own properties dialog, whose entire output is preserved by the
//! carry-the-bytes design above.

use crate::{Duplex, Orientation};

// ---------------------------------------------------------------------------
// The ABI
// ---------------------------------------------------------------------------

/// Byte offsets of the `DEVMODEW` members pdfcer reads or writes.
///
/// Verified against `std::mem::offset_of!` in `devmode_offsets_match_the_real_struct`.
mod offset {
    /// `dmDeviceName[32]`, UTF-16.
    pub(super) const DEVICE_NAME: usize = 0;
    /// `dmSize`, `u16` — the size of the public portion.
    pub(super) const SIZE: usize = 68;
    /// `dmDriverExtra`, `u16` — the size of the driver-private tail.
    pub(super) const DRIVER_EXTRA: usize = 70;
    /// `dmFields`, `u32`.
    pub(super) const FIELDS: usize = 72;
    /// `dmOrientation`, `i16`.
    pub(super) const ORIENTATION: usize = 76;
    /// `dmPaperSize`, `i16`.
    pub(super) const PAPER_SIZE: usize = 78;
    /// `dmPaperLength`, `i16`, tenths of a millimetre.
    pub(super) const PAPER_LENGTH: usize = 80;
    /// `dmPaperWidth`, `i16`, tenths of a millimetre.
    pub(super) const PAPER_WIDTH: usize = 82;
    /// `dmDefaultSource`, `i16`.
    pub(super) const DEFAULT_SOURCE: usize = 88;
    /// `dmDuplex`, `i16`.
    pub(super) const DUPLEX: usize = 94;
    /// `dmFormName[32]`, UTF-16.
    pub(super) const FORM_NAME: usize = 102;
}

/// `dmFields` bits for the members pdfcer writes.
///
/// Verified against the `windows` crate's own constants in
/// `devmode_field_flags_match_the_real_constants`.
mod field {
    /// `DM_ORIENTATION`.
    pub(super) const ORIENTATION: u32 = 0x0000_0001;
    /// `DM_PAPERSIZE`.
    pub(super) const PAPER_SIZE: u32 = 0x0000_0002;
    /// `DM_PAPERLENGTH`.
    pub(super) const PAPER_LENGTH: u32 = 0x0000_0004;
    /// `DM_PAPERWIDTH`.
    pub(super) const PAPER_WIDTH: u32 = 0x0000_0008;
    /// `DM_DEFAULTSOURCE`.
    pub(super) const DEFAULT_SOURCE: u32 = 0x0000_0200;
    /// `DM_DUPLEX`.
    pub(super) const DUPLEX: u32 = 0x0000_1000;
    /// `DM_FORMNAME`.
    pub(super) const FORM_NAME: u32 = 0x0001_0000;
}

/// `DMORIENT_PORTRAIT`.
const DMORIENT_PORTRAIT: i16 = 1;
/// `DMORIENT_LANDSCAPE`.
const DMORIENT_LANDSCAPE: i16 = 2;
/// `DMDUP_SIMPLEX`.
const DMDUP_SIMPLEX: i16 = 1;
/// `DMDUP_VERTICAL` — the LONG-edge flip. See [`Duplex`] for why the
/// Win32 name reads backwards.
const DMDUP_VERTICAL: i16 = 2;
/// `DMDUP_HORIZONTAL` — the SHORT-edge flip.
const DMDUP_HORIZONTAL: i16 = 3;
/// `DMBIN_FORMSOURCE` — "choose the input tray from the form/paper size".
///
/// This is the value that makes a driver do what
/// [`crate::DeviceSettings::pick_tray_by_page_size`] asks for, and it is
/// the driver's own Form-to-Tray Assignment table that answers it. Not
/// `DMBIN_AUTO` (7), which means "the driver's automatic selection" and
/// on most drivers means whichever tray is loaded rather than which tray
/// matches the sheet.
const DMBIN_FORMSOURCE: i16 = 15;
/// `DMPAPER_USER` — "the size is in `dmPaperWidth`/`dmPaperLength`".
// Used by `#[cfg(windows)]` spooling and by tests on every platform;
// see the module note on why the lint is relaxed rather than the item
// gated.
#[cfg_attr(not(windows), allow(dead_code))]
const DMPAPER_USER: i16 = 256;

/// The size of the public `DEVMODEW` portion this Windows defines.
///
/// Used only as a floor for the buffer pdfcer allocates, never as an
/// assumption about what a driver reports: a driver built against an
/// older SDK may report a SMALLER `dmSize`, which is legal, and the
/// buffer is padded to this length so that a `*const DEVMODEW` handed to
/// Win32 is always fully in bounds.
pub(crate) const DEVMODE_PUBLIC_BYTES: usize = 220;

/// The shortest public portion pdfcer will accept from a driver.
///
/// One byte past `dmFormName`, which is the last member this crate reads
/// or writes. A `dmSize` below this means pdfcer would be writing into
/// the driver's private tail, which is corruption rather than
/// configuration — so it is refused by name instead.
const MIN_PUBLIC_BYTES: usize = offset::FORM_NAME + 64;

// ---------------------------------------------------------------------------
// Paper
// ---------------------------------------------------------------------------

/// Points per tenth of a millimetre — `72 / 254`.
///
/// `DEVMODE` measures paper in tenths of a millimetre; every other
/// measurement in this crate is in PDF points. One conversion, in one
/// place, because two would eventually disagree.
const PT_PER_TENTH_MM: f64 = 72.0 / 254.0;

/// One paper size a device offers.
///
/// The three parallel `DeviceCapabilities` queries — `DC_PAPERS`,
/// `DC_PAPERNAMES`, `DC_PAPERSIZE` — are zipped into this by
/// [`crate::printer_forms`]. They are separate calls returning separate
/// arrays that are only related by INDEX, which is the sort of API that
/// silently mismatches when one of them is shorter.
#[derive(Debug, Clone, PartialEq)]
pub struct PaperForm {
    /// The `dmPaperSize` value that selects this form. Pass it to
    /// [`PaperSelection::Form`].
    pub id: u16,
    /// The driver's own name for it — `"A4"`, `"Letter"`, `"ARCH D"`,
    /// `"Roll Paper 24in"`. Operator-facing; not stable across drivers.
    pub name: String,
    /// The sheet in PDF points, converted from the driver's tenths of a
    /// millimetre.
    ///
    /// This is the PHYSICAL sheet, not the printable area — the same
    /// distinction [`crate::PrinterCaps`] makes, and for the same reason:
    /// fitting a page to it would produce a page whose edges the hardware
    /// crops.
    pub size_pt: (f64, f64),
}

/// Which sheet the driver should feed.
///
/// # Why `Custom` is stored in the driver's unit and not in points
///
/// `DEVMODE` measures a custom sheet in tenths of a millimetre, in an
/// `i16`. Storing points here and converting at the boundary would put
/// a lossy conversion on every path that touches this value and let two
/// callers round differently. Storing the driver's own unit makes the
/// value exact and makes the rounding happen exactly once, in
/// [`Self::custom_from_points`], with [`Self::size_pt`] reporting back
/// what was actually requested — which is what a shell discloses to the
/// operator (project rule 4: pdfcer chose a value the operator did not
/// type, so it says so).
///
/// It also keeps [`Eq`] derivable on this type and on
/// [`crate::DeviceSettings`], which a pair of `f64` would not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaperSelection {
    /// Say nothing about paper; whatever the device is configured for.
    #[default]
    DeviceDefault,
    /// A form the driver enumerates — see [`PaperForm::id`].
    Form(u16),
    /// A sheet given by size.
    ///
    /// Build one with [`Self::custom_from_points`] rather than by hand:
    /// the `i16` ceiling below is a real limit that has to be checked.
    Custom {
        /// Sheet width in tenths of a millimetre.
        width_tenths_mm: u16,
        /// Sheet height in tenths of a millimetre.
        height_tenths_mm: u16,
    },
}

/// The largest custom sheet a `DEVMODE` can express, in tenths of a
/// millimetre — `i16::MAX`, i.e. 3276.7 mm, about 3.28 m.
///
/// Named rather than inlined because it is an ABI ceiling a plotter
/// operator can genuinely hit: a roll-fed banner longer than 3.28 m
/// cannot be requested through `dmPaperLength` at all, and pdfcer must
/// say so rather than wrapping the value into a negative number and
/// printing something absurd.
pub const MAX_CUSTOM_SHEET_TENTHS_MM: u16 = i16::MAX as u16;

impl PaperSelection {
    /// A custom sheet from a size in PDF points.
    ///
    /// The single place points become the driver's tenths of a
    /// millimetre. Rounds to nearest; [`Self::size_pt`] reports the
    /// value that was actually stored so a shell can disclose the
    /// difference rather than implying an exactness that is not there.
    ///
    /// # Errors
    ///
    /// `None` when either axis is not positive, or exceeds
    /// [`MAX_CUSTOM_SHEET_TENTHS_MM`] — the `i16` ceiling `DEVMODE`
    /// imposes. Refused rather than clamped: a silently shortened sheet
    /// is a wrong print that looks like a pdfcer scaling bug.
    ///
    /// ```
    /// use pdfcer_print::PaperSelection;
    ///
    /// // A4 is 210 x 297 mm, i.e. 2100 x 2970 tenths.
    /// let a4 = PaperSelection::custom_from_points((595.276, 841.89)).unwrap();
    /// assert_eq!(
    ///     a4,
    ///     PaperSelection::Custom { width_tenths_mm: 2100, height_tenths_mm: 2970 }
    /// );
    /// // Past the DEVMODE ceiling it refuses rather than clamping.
    /// assert!(PaperSelection::custom_from_points((595.0, 20000.0)).is_none());
    /// ```
    #[must_use]
    pub fn custom_from_points(size_pt: (f64, f64)) -> Option<Self> {
        let to_tenths = |pt: f64| -> Option<u16> {
            if !pt.is_finite() || pt <= 0.0 {
                return None;
            }
            let tenths = (pt / PT_PER_TENTH_MM).round();
            if tenths < 1.0 || tenths > f64::from(MAX_CUSTOM_SHEET_TENTHS_MM) {
                return None;
            }
            // The bounds check above puts this in `u16` range; the cast
            // cannot wrap.
            Some(tenths as u16)
        };
        Some(Self::Custom {
            width_tenths_mm: to_tenths(size_pt.0)?,
            height_tenths_mm: to_tenths(size_pt.1)?,
        })
    }

    /// The selected sheet in PDF points, when this selection states one.
    ///
    /// `None` for [`Self::DeviceDefault`] and [`Self::Form`]: a form id
    /// is a name for a size the DRIVER holds, and this crate will not
    /// guess at it — [`crate::printer_forms`] is what resolves an id to
    /// a size, by asking the device.
    #[must_use]
    pub fn size_pt(self) -> Option<(f64, f64)> {
        match self {
            Self::Custom {
                width_tenths_mm,
                height_tenths_mm,
            } => Some((
                f64::from(width_tenths_mm) * PT_PER_TENTH_MM,
                f64::from(height_tenths_mm) * PT_PER_TENTH_MM,
            )),
            Self::DeviceDefault | Self::Form(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// The configuration itself
// ---------------------------------------------------------------------------

/// A driver's own `DEVMODE`, carried opaquely.
///
/// # What a caller can do with one
///
/// Obtain it ([`crate::printer_configuration`]), let the operator edit
/// it in the driver's dialog ([`crate::edit_printer_configuration`]),
/// read a summary of it for disclosure ([`Self::summary`]), store it
/// ([`Self::as_bytes`]) and load it back ([`Self::from_bytes`]), and
/// hand it to [`crate::spool_with_config`]. That is the whole surface,
/// deliberately: the structure is driver-defined and the fields pdfcer
/// does not model must survive untouched, which they cannot do if
/// callers can reach in.
///
/// # Not `cfg(windows)`
///
/// It is a `Vec<u8>` and a length. Only the functions that FILL it and
/// the one that USES it are Windows-only — the same split as
/// [`crate::Printer`] and [`crate::PrinterCaps`], for the same reason:
/// a plain data type gated to one platform breaks every non-Windows
/// build that so much as names it in a stub's signature.
#[derive(Clone, PartialEq, Eq)]
pub struct PrinterConfiguration {
    /// `dmSize + dmDriverExtra` bytes of real structure, zero-padded to
    /// at least [`DEVMODE_PUBLIC_BYTES`] so that a `*const DEVMODEW`
    /// into it is always fully in bounds even when a driver reports a
    /// `dmSize` smaller than this Windows' own.
    bytes: Vec<u8>,
}

/// Deliberately NOT derived: the derived form would dump a couple of
/// hundred bytes of driver-private data into every log line and error
/// message that formats one, which is noise at best and a way to leak a
/// device's configuration into a bug report at worst.
impl core::fmt::Debug for PrinterConfiguration {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PrinterConfiguration")
            .field("device", &self.device_name())
            .field("bytes", &self.bytes.len())
            .field("driver_extra", &self.driver_extra())
            .finish()
    }
}

/// Why a byte buffer was not a usable `DEVMODE`.
///
/// Separate from [`crate::PrintError`] because these are all facts about
/// a buffer rather than about a device, and a caller loading a saved
/// configuration needs to tell "this file is not a `DEVMODE`" from "this
/// printer refused" — the two send an operator somewhere different.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigurationError {
    /// Shorter than a `DEVMODE` header can be.
    #[error(
        "not a printer configuration: {len} bytes, which is shorter than a DEVMODE header ({need} bytes)"
    )]
    TooShort {
        /// The length that was offered.
        len: usize,
        /// The minimum a `DEVMODE` public portion can be for pdfcer to
        /// amend it — see `MIN_PUBLIC_BYTES`.
        need: usize,
    },
    /// `dmSize` is below the last member pdfcer reads or writes, so
    /// amending it would write into the driver's private tail.
    #[error(
        "this driver reports a DEVMODE public portion of {dm_size} bytes, below the {need} pdfcer needs to set orientation and paper without overwriting the driver's private data"
    )]
    PublicPortionTooSmall {
        /// The `dmSize` the buffer declares.
        dm_size: usize,
        /// `MIN_PUBLIC_BYTES`.
        need: usize,
    },
    /// `dmSize + dmDriverExtra` runs past the end of the buffer — the
    /// buffer is truncated, or was never a `DEVMODE`.
    #[error(
        "truncated printer configuration: it declares {declared} bytes (dmSize + dmDriverExtra) but only {len} are present"
    )]
    Truncated {
        /// `dmSize + dmDriverExtra`.
        declared: usize,
        /// The bytes actually present.
        len: usize,
    },
    /// The configuration belongs to a different device.
    ///
    /// A `DEVMODE` is only meaningful to the driver that produced it —
    /// its private tail is that driver's private format — so handing one
    /// printer another's configuration is not a degraded result, it is
    /// undefined behaviour at the driver level.
    #[error(
        "this configuration was saved for the printer {saved:?}, not {requested:?}. A DEVMODE is only meaningful to the driver that produced it"
    )]
    DeviceMismatch {
        /// The device named inside the configuration.
        saved: String,
        /// The device it was about to be used with.
        requested: String,
    },
}

impl PrinterConfiguration {
    /// Parse a buffer that claims to be a `DEVMODE`.
    ///
    /// The validating half of the save/load round trip: a configuration
    /// reaches this from a file an operator named, so every field the
    /// amend logic later trusts is checked here rather than assumed.
    ///
    /// # Errors
    ///
    /// [`ConfigurationError`] — see its variants; each names what was
    /// wrong with the bytes rather than reporting a generic parse
    /// failure.
    ///
    /// ```
    /// use pdfcer_print::{ConfigurationError, PrinterConfiguration};
    ///
    /// assert!(matches!(
    ///     PrinterConfiguration::from_bytes(&[0u8; 8]),
    ///     Err(ConfigurationError::TooShort { .. })
    /// ));
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ConfigurationError> {
        if bytes.len() < MIN_PUBLIC_BYTES {
            return Err(ConfigurationError::TooShort {
                len: bytes.len(),
                need: MIN_PUBLIC_BYTES,
            });
        }
        let dm_size = usize::from(read_u16(bytes, offset::SIZE));
        let extra = usize::from(read_u16(bytes, offset::DRIVER_EXTRA));
        if dm_size < MIN_PUBLIC_BYTES {
            return Err(ConfigurationError::PublicPortionTooSmall {
                dm_size,
                need: MIN_PUBLIC_BYTES,
            });
        }
        let declared = dm_size.saturating_add(extra);
        if declared > bytes.len() {
            return Err(ConfigurationError::Truncated {
                declared,
                len: bytes.len(),
            });
        }
        // Keep exactly the declared structure, then pad to the public
        // size this Windows knows. Padding at the END cannot move the
        // driver's private tail, which sits at `dmSize`, and it
        // guarantees a `*const DEVMODEW` into the buffer is in bounds
        // even for a driver whose `dmSize` predates fields this Windows
        // has.
        let mut kept = bytes.get(..declared).unwrap_or_default().to_vec();
        if kept.len() < DEVMODE_PUBLIC_BYTES {
            kept.resize(DEVMODE_PUBLIC_BYTES, 0);
        }
        Ok(Self { bytes: kept })
    }

    /// The bytes, for storing. Round-trips through [`Self::from_bytes`].
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The device this configuration belongs to, from `dmDeviceName`.
    #[must_use]
    pub fn device_name(&self) -> String {
        let name = self
            .bytes
            .get(offset::DEVICE_NAME..offset::DEVICE_NAME + 64)
            .unwrap_or_default();
        utf16_field(name)
    }

    /// Refuse a configuration that belongs to a different device.
    ///
    /// # Errors
    ///
    /// [`ConfigurationError::DeviceMismatch`] when the names differ.
    /// Compared case-INSENSITIVELY: Windows printer names are not
    /// case-sensitive, and refusing `"HP LaserJet"` against
    /// `"HP Laserjet"` would be a false alarm on a correct file.
    pub fn ensure_device(&self, printer: &str) -> Result<(), ConfigurationError> {
        let saved = self.device_name();
        // A driver that leaves `dmDeviceName` empty cannot be checked, and
        // refusing on that would reject a valid configuration for a fact
        // the driver declined to state. Passing it through is the honest
        // direction: the driver itself rejects a foreign DEVMODE.
        if saved.is_empty() || saved.eq_ignore_ascii_case(printer) {
            return Ok(());
        }
        Err(ConfigurationError::DeviceMismatch {
            saved,
            requested: printer.to_owned(),
        })
    }

    /// The size of the driver's private tail, in bytes.
    ///
    /// Zero means the driver keeps nothing beyond the documented
    /// members — which is true of the Microsoft-supplied drivers and
    /// false of most vendor ones.
    #[must_use]
    pub fn driver_extra(&self) -> usize {
        usize::from(read_u16(&self.bytes, offset::DRIVER_EXTRA))
    }

    /// What this configuration asks the device for, as far as pdfcer
    /// models it.
    ///
    /// The disclosure surface (project rule 4). After the operator edits
    /// a configuration in the driver's own dialog, pdfcer cannot show
    /// what changed inside the driver-private half — but it CAN state
    /// the part it understands, and stating that is better than a shell
    /// reporting only "configured".
    #[must_use]
    pub fn summary(&self) -> ConfigurationSummary {
        let fields = self.fields();
        let asserted = |bit: u32| fields & bit != 0;
        ConfigurationSummary {
            device: self.device_name(),
            orientation: asserted(field::ORIENTATION)
                .then(|| match self.i16_at(offset::ORIENTATION) {
                    DMORIENT_LANDSCAPE => Some(Orientation::Landscape),
                    DMORIENT_PORTRAIT => Some(Orientation::Portrait),
                    _ => None,
                })
                .flatten(),
            paper_form_id: asserted(field::PAPER_SIZE)
                .then(|| u16::try_from(self.i16_at(offset::PAPER_SIZE)).ok())
                .flatten(),
            form_name: asserted(field::FORM_NAME).then(|| {
                utf16_field(
                    self.bytes
                        .get(offset::FORM_NAME..offset::FORM_NAME + 64)
                        .unwrap_or_default(),
                )
            }),
            custom_paper_pt: (asserted(field::PAPER_WIDTH) && asserted(field::PAPER_LENGTH)).then(
                || {
                    (
                        f64::from(self.i16_at(offset::PAPER_WIDTH)) * PT_PER_TENTH_MM,
                        f64::from(self.i16_at(offset::PAPER_LENGTH)) * PT_PER_TENTH_MM,
                    )
                },
            ),
            duplex: asserted(field::DUPLEX)
                .then(|| match self.i16_at(offset::DUPLEX) {
                    DMDUP_VERTICAL => Some(Duplex::LongEdge),
                    DMDUP_HORIZONTAL => Some(Duplex::ShortEdge),
                    DMDUP_SIMPLEX => Some(Duplex::Simplex),
                    _ => None,
                })
                .flatten(),
            input_tray: asserted(field::DEFAULT_SOURCE)
                .then(|| self.i16_at(offset::DEFAULT_SOURCE)),
            picks_tray_by_size: asserted(field::DEFAULT_SOURCE)
                && self.i16_at(offset::DEFAULT_SOURCE) == DMBIN_FORMSOURCE,
            driver_extra: self.driver_extra(),
        }
    }

    /// Overwrite exactly the members `settings` speaks to, and name them
    /// in `dmFields`.
    ///
    /// `orientation` is the RESOLVED orientation — [`Orientation::Auto`]
    /// has already been decided against a page by the caller, because a
    /// `DEVMODE` describes one sheet and cannot carry "it depends".
    /// Passing `Auto` here is treated as portrait, which is what
    /// `resolve_orientation` does for a square page.
    ///
    /// `assert_duplex` gates `DM_DUPLEX` alone, and it is a parameter
    /// rather than being inferred from `settings` because of a defect
    /// this crate already shipped once: a `DEVMODE` naming
    /// `DMDUP_SIMPLEX` OVERRIDES a driver whose own default is duplex,
    /// so an orientation-only turn must not quietly cancel a duplex
    /// default the operator never asked pdfcer to touch. See
    /// `build_devmode`'s own notes and
    /// `D:/dev/rag/rust/a_disturb_nothing_by_default_guard_can_silently_disable_the_default_behaviour_it_is_guarding.md`.
    // Used by `#[cfg(windows)]` spooling and by tests on every platform;
    // see the module note on why the lint is relaxed rather than the item
    // gated.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn apply(
        &mut self,
        orientation: Orientation,
        paper: PaperSelection,
        pick_tray_by_page_size: bool,
        assert_duplex: Option<Duplex>,
    ) {
        let mut fields = self.fields();

        self.set_i16(
            offset::ORIENTATION,
            match orientation {
                Orientation::Landscape => DMORIENT_LANDSCAPE,
                Orientation::Auto | Orientation::Portrait => DMORIENT_PORTRAIT,
            },
        );
        fields |= field::ORIENTATION;
        self.set_fields(fields);

        self.apply_paper(paper);
        let mut fields = self.fields();

        if pick_tray_by_page_size {
            self.set_i16(offset::DEFAULT_SOURCE, DMBIN_FORMSOURCE);
            fields |= field::DEFAULT_SOURCE;
        }

        if let Some(duplex) = assert_duplex {
            self.set_i16(
                offset::DUPLEX,
                match duplex {
                    Duplex::Simplex => DMDUP_SIMPLEX,
                    // Long-edge binding is `VERTICAL` in Win32's
                    // vocabulary and short-edge is `HORIZONTAL`, which
                    // reads backwards until you notice the name describes
                    // the FLIP AXIS rather than the edge the pages are
                    // bound on. Getting these the wrong way round
                    // produces a booklet whose alternate pages are upside
                    // down, and nothing catches it before the paper.
                    Duplex::LongEdge => DMDUP_VERTICAL,
                    Duplex::ShortEdge => DMDUP_HORIZONTAL,
                },
            );
            fields |= field::DUPLEX;
        }

        self.set_fields(fields);
    }

    /// Overwrite the paper members alone, leaving everything else —
    /// orientation included — exactly as it was.
    ///
    /// # Why this is separable from [`Self::apply`]
    ///
    /// [`crate::printer_caps_for`] has to open an information device
    /// context for the sheet a job will use, so that placement is
    /// computed against the right rectangle. It must NOT also apply the
    /// job's orientation: [`crate::DeviceGeometry::for_orientation`] is
    /// the one place rotation is written, it works from the un-turned
    /// geometry, and a second rotation applied here would eventually
    /// disagree with it — which is the defect that function's own docs
    /// exist to describe.
    ///
    /// So the paper amend is a piece on its own, and [`Self::apply`]
    /// calls it rather than repeating it. Two copies of the
    /// clear-the-other-representation logic below would be two places to
    /// get the `DM_FORMNAME` trap wrong.
    // Used by `#[cfg(windows)]` spooling and by tests on every platform;
    // see the module note on why the lint is relaxed rather than the item
    // gated.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn apply_paper(&mut self, paper: PaperSelection) {
        let mut fields = self.fields();
        match paper {
            // Say nothing; whatever the driver's own base holds survives.
            PaperSelection::DeviceDefault => return,
            PaperSelection::Form(id) => {
                self.set_i16(offset::PAPER_SIZE, i16::try_from(id).unwrap_or(0));
                fields |= field::PAPER_SIZE;
                // ★ Exactly one of {form id, explicit size, form NAME} is
                // asserted at a time. See this module's "two traps".
                fields &= !(field::PAPER_LENGTH | field::PAPER_WIDTH | field::FORM_NAME);
            }
            PaperSelection::Custom {
                width_tenths_mm,
                height_tenths_mm,
            } => {
                self.set_i16(offset::PAPER_SIZE, DMPAPER_USER);
                // `custom_from_points` bounds these at `i16::MAX`; a
                // hand-built value past it saturates rather than wrapping
                // into a negative sheet.
                self.set_i16(
                    offset::PAPER_WIDTH,
                    i16::try_from(width_tenths_mm).unwrap_or(i16::MAX),
                );
                self.set_i16(
                    offset::PAPER_LENGTH,
                    i16::try_from(height_tenths_mm).unwrap_or(i16::MAX),
                );
                fields |= field::PAPER_SIZE | field::PAPER_WIDTH | field::PAPER_LENGTH;
                fields &= !field::FORM_NAME;
            }
        }
        self.set_fields(fields);
    }

    /// A minimal, valid configuration for a device, naming nothing.
    ///
    /// The fallback for a driver that will not answer
    /// `DocumentProperties` — which does happen, most often on a
    /// disconnected network printer — and the base for tests. It is
    /// exactly the structure pdfcer used to synthesise for EVERY job:
    /// correct as far as it goes, and carrying none of the driver's own
    /// settings, which is why it is a fallback rather than the base.
    #[must_use]
    pub fn blank(device: &str) -> Self {
        let mut bytes = vec![0u8; DEVMODE_PUBLIC_BYTES];
        // `dmDeviceName` is 32 UTF-16 units INCLUDING the terminator, so
        // 31 characters of name at most. Truncating is right: the field
        // is fixed and a name that does not fit was never going to match
        // the device anyway.
        for (i, unit) in device.encode_utf16().take(31).enumerate() {
            write_u16(&mut bytes, offset::DEVICE_NAME + i * 2, unit);
        }
        write_u16(
            &mut bytes,
            offset::SIZE,
            u16::try_from(DEVMODE_PUBLIC_BYTES).unwrap_or(0),
        );
        // `dmSpecVersion` — 0x0401 is the value every modern Windows
        // driver reports and the one `DocumentProperties` expects to see
        // on input.
        write_u16(&mut bytes, 64, 0x0401);
        Self { bytes }
    }

    /// `dmFields`.
    fn fields(&self) -> u32 {
        read_u32(&self.bytes, offset::FIELDS)
    }

    /// Set `dmFields`.
    // Used by `#[cfg(windows)]` spooling and by tests on every platform;
    // see the module note on why the lint is relaxed rather than the item
    // gated.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn set_fields(&mut self, value: u32) {
        write_u32(&mut self.bytes, offset::FIELDS, value);
    }

    /// Read a signed 16-bit member.
    fn i16_at(&self, at: usize) -> i16 {
        read_u16(&self.bytes, at) as i16
    }

    /// Write a signed 16-bit member.
    // Used by `#[cfg(windows)]` spooling and by tests on every platform;
    // see the module note on why the lint is relaxed rather than the item
    // gated.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn set_i16(&mut self, at: usize, value: i16) {
        write_u16(&mut self.bytes, at, value as u16);
    }

    /// The buffer as 32-bit words, for handing to Win32.
    ///
    /// `DEVMODEW` contains `u32` members and therefore requires 4-byte
    /// alignment, which a `Vec<u8>` does not guarantee. Rather than cast
    /// a possibly-misaligned pointer and rely on x86 tolerating it, the
    /// bytes are copied into a `Vec<u32>` whose allocation IS aligned,
    /// and that is what the pointer is taken from. The copy is a couple
    /// of hundred bytes once per device-context creation.
    #[cfg(windows)]
    pub(crate) fn to_aligned_words(&self) -> Vec<u32> {
        let mut words = vec![0u32; self.bytes.len().div_ceil(4)];
        for (i, chunk) in self.bytes.chunks(4).enumerate() {
            let mut quad = [0u8; 4];
            quad.get_mut(..chunk.len())
                .unwrap_or_default()
                .copy_from_slice(chunk);
            if let Some(slot) = words.get_mut(i) {
                *slot = u32::from_ne_bytes(quad);
            }
        }
        words
    }

    /// Rebuild a configuration from an aligned scratch buffer Win32 just
    /// wrote into.
    ///
    /// `len` is what the API said it needed; the result is re-validated
    /// through [`Self::from_bytes`], because a driver's `DocumentProperties`
    /// output is untrusted input in exactly the way a file is.
    ///
    /// # Errors
    ///
    /// [`ConfigurationError`] if the driver wrote something that is not a
    /// usable `DEVMODE`.
    #[cfg(windows)]
    pub(crate) fn from_aligned_words(
        words: &[u32],
        len: usize,
    ) -> Result<Self, ConfigurationError> {
        let mut bytes = Vec::with_capacity(len);
        for word in words {
            bytes.extend_from_slice(&word.to_ne_bytes());
        }
        bytes.truncate(len);
        Self::from_bytes(&bytes)
    }
}

/// What a [`PrinterConfiguration`] asks for, as far as pdfcer models it.
///
/// Every field is `Option` for the same reason: `dmFields` says which
/// members are meaningful, and a member the driver did not assert is
/// genuinely "no preference stated" rather than a value. Reporting a
/// zeroed `dmDuplex` as `Simplex` would be inventing a claim the
/// structure does not make.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigurationSummary {
    /// `dmDeviceName`.
    pub device: String,
    /// `dmOrientation`, when `DM_ORIENTATION` is asserted.
    pub orientation: Option<Orientation>,
    /// `dmPaperSize`, when `DM_PAPERSIZE` is asserted.
    pub paper_form_id: Option<u16>,
    /// `dmFormName`, when `DM_FORMNAME` is asserted.
    ///
    /// Present ALONGSIDE `paper_form_id` only if a driver asserted both,
    /// which is the disagreement this module's "two traps" note is
    /// about. pdfcer never writes both.
    pub form_name: Option<String>,
    /// `dmPaperWidth`/`dmPaperLength` in points, when both are asserted.
    pub custom_paper_pt: Option<(f64, f64)>,
    /// `dmDuplex`, when `DM_DUPLEX` is asserted.
    pub duplex: Option<Duplex>,
    /// `dmDefaultSource`, when `DM_DEFAULTSOURCE` is asserted. A raw
    /// `DMBIN_*` value: the vendor-defined range above 256 has no
    /// portable meaning, so it is reported rather than interpreted.
    pub input_tray: Option<i16>,
    /// The tray is `DMBIN_FORMSOURCE` — "choose it from the sheet size".
    pub picks_tray_by_size: bool,
    /// Bytes of driver-private tail carried through untouched.
    pub driver_extra: usize,
}

// ---------------------------------------------------------------------------
// Byte helpers — little-endian, which is the ABI on every Windows target
// ---------------------------------------------------------------------------

/// Read a little-endian `u16`. Out of range reads zero, which is the
/// same answer a zeroed member gives and cannot be reached anyway after
/// [`PrinterConfiguration::from_bytes`]' length check.
fn read_u16(bytes: &[u8], at: usize) -> u16 {
    bytes
        .get(at..at + 2)
        .and_then(|s| <[u8; 2]>::try_from(s).ok())
        .map_or(0, u16::from_le_bytes)
}

/// Read a little-endian `u32`.
fn read_u32(bytes: &[u8], at: usize) -> u32 {
    bytes
        .get(at..at + 4)
        .and_then(|s| <[u8; 4]>::try_from(s).ok())
        .map_or(0, u32::from_le_bytes)
}

/// Write a little-endian `u16`. A write past the end is dropped rather
/// than panicking; the buffer is padded to [`DEVMODE_PUBLIC_BYTES`] at
/// construction so every offset this module uses is in range.
fn write_u16(bytes: &mut [u8], at: usize, value: u16) {
    if let Some(slot) = bytes.get_mut(at..at + 2) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
}

/// Write a little-endian `u32`.
// Used by `#[cfg(windows)]` spooling and by tests on every platform;
// see the module note on why the lint is relaxed rather than the item
// gated.
#[cfg_attr(not(windows), allow(dead_code))]
fn write_u32(bytes: &mut [u8], at: usize, value: u32) {
    if let Some(slot) = bytes.get_mut(at..at + 4) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
}

/// Decode a fixed-width, NUL-padded UTF-16 field (`dmDeviceName`,
/// `dmFormName`).
fn utf16_field(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([*c.first().unwrap_or(&0), *c.get(1).unwrap_or(&0)]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{
        ConfigurationError, DEVMODE_PUBLIC_BYTES, MAX_CUSTOM_SHEET_TENTHS_MM, PaperSelection,
        PrinterConfiguration, field, offset, read_u16, read_u32,
    };
    use crate::{Duplex, Orientation};

    /// A base that looks like a real driver's: a form name asserted, a
    /// duplex default the operator never asked pdfcer to touch, and a
    /// private tail.
    fn driver_base() -> PrinterConfiguration {
        let mut bytes = vec![0u8; DEVMODE_PUBLIC_BYTES + 96];
        // dmDeviceName
        for (i, u) in "ET-16600".encode_utf16().enumerate() {
            bytes[i * 2] = u.to_le_bytes()[0];
            bytes[i * 2 + 1] = u.to_le_bytes()[1];
        }
        // dmSize / dmDriverExtra
        bytes[offset::SIZE] = DEVMODE_PUBLIC_BYTES.to_le_bytes()[0];
        bytes[offset::SIZE + 1] = DEVMODE_PUBLIC_BYTES.to_le_bytes()[1];
        bytes[offset::DRIVER_EXTRA] = 96;
        // dmFields: form name + duplex asserted, as a real driver does.
        let fields = field::FORM_NAME | field::DUPLEX;
        bytes[offset::FIELDS..offset::FIELDS + 4].copy_from_slice(&fields.to_le_bytes());
        // dmFormName = "A4"
        for (i, u) in "A4".encode_utf16().enumerate() {
            bytes[offset::FORM_NAME + i * 2] = u.to_le_bytes()[0];
            bytes[offset::FORM_NAME + i * 2 + 1] = u.to_le_bytes()[1];
        }
        // dmDuplex = DMDUP_VERTICAL (2)
        bytes[offset::DUPLEX] = 2;
        // A recognisable private tail.
        for (i, b) in bytes
            .iter_mut()
            .skip(DEVMODE_PUBLIC_BYTES)
            .take(96)
            .enumerate()
        {
            *b = u8::try_from(i % 251).unwrap_or(0);
        }
        PrinterConfiguration::from_bytes(&bytes).expect("the fixture must be a valid DEVMODE")
    }

    /// ★ The property the whole byte-buffer design exists for.
    ///
    /// The driver's private tail — where stapling, media type and every
    /// vendor setting Win32 has no field for actually live — must come
    /// out of an amend byte-identical. A `DEVMODEW`-by-value design
    /// cannot have this property, and losing it is invisible until an
    /// operator's driver preset stops taking effect.
    #[test]
    fn amending_a_configuration_preserves_the_drivers_private_tail() {
        let base = driver_base();
        let before = base.as_bytes()[DEVMODE_PUBLIC_BYTES..].to_vec();
        assert_eq!(before.len(), 96, "the fixture must actually have a tail");
        let mut amended = base.clone();
        amended.apply(
            Orientation::Landscape,
            PaperSelection::Form(9),
            true,
            Some(Duplex::ShortEdge),
        );
        assert_eq!(
            amended.as_bytes()[DEVMODE_PUBLIC_BYTES..],
            before[..],
            "the driver's private tail must survive an amend untouched"
        );
        assert_eq!(amended.driver_extra(), 96);
    }

    /// Orientation is always asserted, and it is the resolved one.
    #[test]
    fn orientation_is_written_and_named_in_dm_fields() {
        let mut c = driver_base();
        c.apply(
            Orientation::Landscape,
            PaperSelection::DeviceDefault,
            false,
            None,
        );
        let s = c.summary();
        assert_eq!(s.orientation, Some(Orientation::Landscape));
        assert_eq!(read_u16(c.as_bytes(), offset::ORIENTATION), 2);
        assert!(read_u32(c.as_bytes(), offset::FIELDS) & field::ORIENTATION != 0);
    }

    /// ★ Request 1: `pick_tray_by_page_size` reaches the driver.
    ///
    /// It was declared, documented, plumbed through `spool` and read
    /// NOWHERE — `spool` returned `Ok` and the paper came out of the
    /// default tray, which is also what a driver that DECLINED the
    /// request would do, so nothing could tell the two apart.
    #[test]
    fn pick_tray_by_page_size_sets_dmbin_formsource() {
        let mut c = driver_base();
        assert!(!c.summary().picks_tray_by_size, "not asked for yet");
        c.apply(
            Orientation::Portrait,
            PaperSelection::DeviceDefault,
            true,
            None,
        );
        let s = c.summary();
        assert_eq!(s.input_tray, Some(15), "DMBIN_FORMSOURCE");
        assert!(s.picks_tray_by_size);
        assert!(read_u32(c.as_bytes(), offset::FIELDS) & field::DEFAULT_SOURCE != 0);
    }

    /// Not asking for it must not assert a tray at all — otherwise pdfcer
    /// would be overriding a tray the operator chose in the driver.
    #[test]
    fn not_picking_a_tray_leaves_dm_defaultsource_alone() {
        let mut c = driver_base();
        c.apply(
            Orientation::Portrait,
            PaperSelection::DeviceDefault,
            false,
            None,
        );
        assert_eq!(c.summary().input_tray, None);
        assert!(read_u32(c.as_bytes(), offset::FIELDS) & field::DEFAULT_SOURCE == 0);
    }

    /// ★ Trap 1: a form id must clear `DM_FORMNAME`.
    ///
    /// The driver's own base arrives with a form NAME asserted. Leaving
    /// it asserted while writing a form ID leaves two contradictory
    /// statements in one structure, and which one a driver honours is
    /// not uniform — so a paper selection that looks accepted prints on
    /// the old sheet.
    #[test]
    fn selecting_a_form_clears_the_form_name_and_any_custom_size() {
        let base = driver_base();
        assert!(
            base.summary().form_name.is_some(),
            "the fixture must start with DM_FORMNAME asserted"
        );
        let mut c = base;
        c.apply(Orientation::Portrait, PaperSelection::Form(9), false, None);
        let s = c.summary();
        assert_eq!(s.paper_form_id, Some(9));
        assert_eq!(s.form_name, None, "DM_FORMNAME must be cleared");
        assert_eq!(s.custom_paper_pt, None);
    }

    /// Trap 2, the other direction: a custom size clears the form name
    /// and is expressed as `DMPAPER_USER` plus width/length.
    #[test]
    fn a_custom_sheet_sets_dmpaper_user_and_both_axes() {
        let mut c = driver_base();
        let paper = PaperSelection::custom_from_points((595.276, 841.89)).expect("A4 fits");
        c.apply(Orientation::Portrait, paper, false, None);
        let s = c.summary();
        assert_eq!(s.paper_form_id, Some(256), "DMPAPER_USER");
        assert_eq!(s.form_name, None);
        let (w, h) = s.custom_paper_pt.expect("a custom sheet states its size");
        assert!((w - 595.27).abs() < 0.1, "width {w}");
        assert!((h - 841.88).abs() < 0.1, "height {h}");
    }

    /// ★ `DM_DUPLEX` stays gated, and the gate is a parameter.
    ///
    /// A `DEVMODE` naming `DMDUP_SIMPLEX` OVERRIDES a driver whose own
    /// default is duplex. The fixture's base is duplex; an
    /// orientation-only amend must leave it that way.
    #[test]
    fn an_orientation_only_amend_does_not_cancel_the_drivers_duplex_default() {
        let mut c = driver_base();
        assert_eq!(c.summary().duplex, Some(Duplex::LongEdge));
        c.apply(
            Orientation::Landscape,
            PaperSelection::DeviceDefault,
            false,
            None,
        );
        assert_eq!(
            c.summary().duplex,
            Some(Duplex::LongEdge),
            "the driver's own duplex default must survive an orientation turn"
        );
    }

    /// …and when duplex IS asserted it overwrites.
    #[test]
    fn asserting_duplex_overwrites_the_drivers_value() {
        let mut c = driver_base();
        c.apply(
            Orientation::Portrait,
            PaperSelection::DeviceDefault,
            false,
            Some(Duplex::Simplex),
        );
        assert_eq!(c.summary().duplex, Some(Duplex::Simplex));
    }

    /// The save/load round trip a properties dialog depends on.
    #[test]
    fn a_configuration_round_trips_through_bytes() {
        let mut c = driver_base();
        c.apply(Orientation::Landscape, PaperSelection::Form(1), true, None);
        let reloaded =
            PrinterConfiguration::from_bytes(c.as_bytes()).expect("its own bytes must parse");
        assert_eq!(reloaded, c);
        assert_eq!(reloaded.summary(), c.summary());
    }

    /// Every malformed shape is named rather than collapsed into one
    /// "invalid" answer, because they send an operator somewhere
    /// different.
    #[test]
    fn malformed_configurations_are_refused_by_name() {
        assert!(matches!(
            PrinterConfiguration::from_bytes(&[0u8; 4]),
            Err(ConfigurationError::TooShort { .. })
        ));
        let mut short_public = vec![0u8; DEVMODE_PUBLIC_BYTES];
        short_public[offset::SIZE] = 100;
        assert!(matches!(
            PrinterConfiguration::from_bytes(&short_public),
            Err(ConfigurationError::PublicPortionTooSmall { .. })
        ));
        let mut truncated = vec![0u8; DEVMODE_PUBLIC_BYTES];
        truncated[offset::SIZE..offset::SIZE + 2]
            .copy_from_slice(&u16::try_from(DEVMODE_PUBLIC_BYTES).unwrap().to_le_bytes());
        truncated[offset::DRIVER_EXTRA] = 200;
        assert!(matches!(
            PrinterConfiguration::from_bytes(&truncated),
            Err(ConfigurationError::Truncated { .. })
        ));
    }

    /// A configuration is only meaningful to the driver that made it.
    #[test]
    fn a_configuration_refuses_a_different_device() {
        let c = driver_base();
        assert!(c.ensure_device("ET-16600").is_ok());
        assert!(
            c.ensure_device("et-16600").is_ok(),
            "names are not case-sensitive"
        );
        assert!(matches!(
            c.ensure_device("SC-F100 Series"),
            Err(ConfigurationError::DeviceMismatch { .. })
        ));
    }

    /// A blank configuration is valid, names its device, and states
    /// nothing.
    #[test]
    fn a_blank_configuration_is_valid_and_states_nothing() {
        let c = PrinterConfiguration::blank("Microsoft Print to PDF");
        assert_eq!(c.device_name(), "Microsoft Print to PDF");
        let s = c.summary();
        assert_eq!(s.orientation, None);
        assert_eq!(s.duplex, None);
        assert_eq!(s.paper_form_id, None);
        assert_eq!(s.driver_extra, 0);
        assert!(
            PrinterConfiguration::from_bytes(c.as_bytes()).is_ok(),
            "a blank configuration must survive the same validation a driver's does"
        );
    }

    /// The custom-sheet ceiling is a real `DEVMODE` limit and is refused
    /// rather than clamped.
    #[test]
    fn a_custom_sheet_past_the_devmode_ceiling_is_refused() {
        let at_ceiling = f64::from(MAX_CUSTOM_SHEET_TENTHS_MM) * super::PT_PER_TENTH_MM;
        assert!(PaperSelection::custom_from_points((100.0, at_ceiling - 1.0)).is_some());
        assert!(PaperSelection::custom_from_points((100.0, at_ceiling + 100.0)).is_none());
        assert!(PaperSelection::custom_from_points((0.0, 100.0)).is_none());
        assert!(PaperSelection::custom_from_points((f64::NAN, 100.0)).is_none());
    }

    /// ★ The guard that keeps this hand-written ABI honest.
    ///
    /// Every offset above is asserted against the real `DEVMODEW`. If
    /// the `windows` crate's layout ever moves, this fails here rather
    /// than as a printer that feeds the wrong tray.
    #[cfg(windows)]
    #[test]
    fn devmode_offsets_match_the_real_struct() {
        use windows::Win32::Graphics::Gdi::{DEVMODEW, DEVMODEW_0, DEVMODEW_0_0};
        assert_eq!(size_of::<DEVMODEW>(), DEVMODE_PUBLIC_BYTES);
        assert_eq!(
            std::mem::offset_of!(DEVMODEW, dmDeviceName),
            offset::DEVICE_NAME
        );
        assert_eq!(std::mem::offset_of!(DEVMODEW, dmSize), offset::SIZE);
        assert_eq!(
            std::mem::offset_of!(DEVMODEW, dmDriverExtra),
            offset::DRIVER_EXTRA
        );
        assert_eq!(std::mem::offset_of!(DEVMODEW, dmFields), offset::FIELDS);
        assert_eq!(std::mem::offset_of!(DEVMODEW, dmDuplex), offset::DUPLEX);
        assert_eq!(
            std::mem::offset_of!(DEVMODEW, dmFormName),
            offset::FORM_NAME
        );
        // The printer half of the union.
        let union_at = std::mem::offset_of!(DEVMODEW, Anonymous1);
        assert_eq!(
            union_at
                + std::mem::offset_of!(DEVMODEW_0, Anonymous1)
                + std::mem::offset_of!(DEVMODEW_0_0, dmOrientation),
            offset::ORIENTATION
        );
        assert_eq!(
            union_at
                + std::mem::offset_of!(DEVMODEW_0, Anonymous1)
                + std::mem::offset_of!(DEVMODEW_0_0, dmPaperSize),
            offset::PAPER_SIZE
        );
        assert_eq!(
            union_at
                + std::mem::offset_of!(DEVMODEW_0, Anonymous1)
                + std::mem::offset_of!(DEVMODEW_0_0, dmPaperLength),
            offset::PAPER_LENGTH
        );
        assert_eq!(
            union_at
                + std::mem::offset_of!(DEVMODEW_0, Anonymous1)
                + std::mem::offset_of!(DEVMODEW_0_0, dmPaperWidth),
            offset::PAPER_WIDTH
        );
        assert_eq!(
            union_at
                + std::mem::offset_of!(DEVMODEW_0, Anonymous1)
                + std::mem::offset_of!(DEVMODEW_0_0, dmDefaultSource),
            offset::DEFAULT_SOURCE
        );
    }

    /// The same guard for every constant.
    #[cfg(windows)]
    #[test]
    fn devmode_field_flags_match_the_real_constants() {
        use windows::Win32::Graphics::Gdi::{
            DM_DEFAULTSOURCE, DM_DUPLEX, DM_FORMNAME, DM_ORIENTATION, DM_PAPERLENGTH, DM_PAPERSIZE,
            DM_PAPERWIDTH, DMBIN_FORMSOURCE, DMDUP_HORIZONTAL, DMDUP_SIMPLEX, DMDUP_VERTICAL,
            DMORIENT_LANDSCAPE, DMORIENT_PORTRAIT, DMPAPER_USER,
        };
        assert_eq!(DM_ORIENTATION.0, field::ORIENTATION);
        assert_eq!(DM_PAPERSIZE.0, field::PAPER_SIZE);
        assert_eq!(DM_PAPERLENGTH.0, field::PAPER_LENGTH);
        assert_eq!(DM_PAPERWIDTH.0, field::PAPER_WIDTH);
        assert_eq!(DM_DEFAULTSOURCE.0, field::DEFAULT_SOURCE);
        assert_eq!(DM_DUPLEX.0, field::DUPLEX);
        assert_eq!(DM_FORMNAME.0, field::FORM_NAME);
        assert_eq!(DMORIENT_PORTRAIT as i16, super::DMORIENT_PORTRAIT);
        assert_eq!(DMORIENT_LANDSCAPE as i16, super::DMORIENT_LANDSCAPE);
        assert_eq!(DMDUP_SIMPLEX.0, super::DMDUP_SIMPLEX);
        assert_eq!(DMDUP_VERTICAL.0, super::DMDUP_VERTICAL);
        assert_eq!(DMDUP_HORIZONTAL.0, super::DMDUP_HORIZONTAL);
        assert_eq!(DMBIN_FORMSOURCE as i16, super::DMBIN_FORMSOURCE);
        assert_eq!(DMPAPER_USER as i16, super::DMPAPER_USER);
    }
}
