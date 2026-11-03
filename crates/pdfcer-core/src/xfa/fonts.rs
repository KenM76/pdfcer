//! Faces a shell supplies for XFA text, in place of the standard-14
//! approximation (XFA 3.3 `font` pp.741-744: `typeface` names a font the
//! application finds on the system; Ch.2 p.58 best match).
//!
//! `pdfcer-core` parses no font program (R21): the shell reads each face and
//! hands over its WinAnsi advance widths, metrics and program bytes. Text is
//! drawn as a simple font with `/WinAnsiEncoding` (ISO 32000-1 §9.6.6.1)
//! whose `/BaseFont` is the face's PostScript name, so the same content
//! stream draws whether the program is embedded or referenced.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::fontinfo::{EmbeddingPermission, FsTypeBits};
use crate::vartext::encode_winansi;

/// The outline format of a supplied face's program, which decides how it
/// is embedded (ISO 32000-1 §9.9 Table 126).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FontProgramKind {
    /// An sfnt with `glyf` outlines: a `/TrueType` font with `/FontFile2`.
    TrueType,
    /// An sfnt (`OTTO`) whose `CFF ` table has no CIDFont operators: a
    /// `/Type1` font with `/FontFile3 /Subtype /OpenType` (Table 126 row
    /// OT-3; the whole program is embedded, so its `cmap` is kept).
    OpenTypeCff,
}

/// A supplied face's whole program, as read from its file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct XfaFontProgram {
    /// The file's bytes (one face, not a collection).
    pub bytes: Arc<[u8]>,
    /// Its outline format.
    pub kind: FontProgramKind,
    /// Its `OS/2.fsType`, `None` when the face has no `OS/2` table.
    pub fs_type: Option<FsTypeBits>,
}

impl XfaFontProgram {
    /// A program of `kind` with `fs_type`.
    #[must_use]
    pub fn new(bytes: Arc<[u8]>, kind: FontProgramKind, fs_type: Option<FsTypeBits>) -> Self {
        Self {
            bytes,
            kind,
            fs_type,
        }
    }

    /// Why `fsType` forbids embedding this program whole, if it does
    /// (decision 053 as applied to a new document: Restricted and ambiguous
    /// usage bits refuse, bitmap-only embedding cannot carry outlines;
    /// no-subsetting is honoured by embedding the whole program).
    #[must_use]
    pub const fn embedding_refusal(&self) -> Option<&'static str> {
        let Some(bits) = &self.fs_type else {
            return None;
        };
        match bits.permission {
            EmbeddingPermission::Restricted => Some("Restricted License embedding"),
            EmbeddingPermission::Ambiguous => Some("ambiguous embedding bits"),
            _ if bits.bitmap_only => Some("bitmap-only embedding"),
            _ => None,
        }
    }
}

/// One supplied face, measured.
///
/// Widths are in glyph space (1000 units per em) per `WinAnsiEncoding` code;
/// `None` marks a code the face has no glyph for, which is drawn as the
/// face's `.notdef` and disclosed.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct XfaFace {
    /// The `/BaseFont` written for the face: its PostScript name, which the
    /// renderer's font environment must resolve to the same face.
    pub base_font: String,
    /// Advance per WinAnsi code, 256 entries.
    pub widths: Vec<Option<u16>>,
    /// Ascent, descent (negative) and cap height, glyph space.
    pub ascent: f64,
    /// See [`Self::ascent`].
    pub descent: f64,
    /// See [`Self::ascent`].
    pub cap_height: f64,
    /// The font bounding box, glyph space.
    pub bbox: [f64; 4],
    /// The italic angle in degrees (negative leans right).
    pub italic_angle: f64,
    /// Whether the face is bold, italic, serif and fixed-pitch, for the
    /// descriptor's `/Flags` and `/StemV` (§9.8.2).
    pub bold: bool,
    /// See [`Self::bold`].
    pub italic: bool,
    /// See [`Self::bold`].
    pub serif: bool,
    /// See [`Self::bold`].
    pub fixed_pitch: bool,
    /// The program to embed; `None` references the face by name only.
    pub program: Option<XfaFontProgram>,
}

impl XfaFace {
    /// A face named `base_font` whose advance for a character is
    /// `advance(c)` (glyph space, `None` for no glyph), with Helvetica-like
    /// metrics until [`Self::with_metrics`] sets the face's own.
    #[must_use]
    pub fn new(base_font: impl Into<String>, advance: impl Fn(char) -> Option<u16>) -> Self {
        let widths = winansi_chars()
            .iter()
            .map(|c| c.and_then(&advance))
            .collect();
        Self {
            base_font: base_font.into(),
            widths,
            ascent: 718.0,
            descent: -207.0,
            cap_height: 718.0,
            bbox: [-166.0, -225.0, 1000.0, 931.0],
            italic_angle: 0.0,
            bold: false,
            italic: false,
            serif: false,
            fixed_pitch: false,
            program: None,
        }
    }

    /// With ascent, descent (negative), cap height and bounding box.
    #[must_use]
    pub const fn with_metrics(
        mut self,
        ascent: f64,
        descent: f64,
        cap_height: f64,
        bbox: [f64; 4],
    ) -> Self {
        self.ascent = ascent;
        self.descent = descent;
        self.cap_height = cap_height;
        self.bbox = bbox;
        self
    }

    /// With the style flags and italic angle.
    #[must_use]
    pub const fn with_style(
        mut self,
        bold: bool,
        italic: bool,
        serif: bool,
        fixed_pitch: bool,
        italic_angle: f64,
    ) -> Self {
        self.bold = bold;
        self.italic = italic;
        self.serif = serif;
        self.fixed_pitch = fixed_pitch;
        self.italic_angle = italic_angle;
        self
    }

    /// With the program to embed.
    #[must_use]
    pub fn with_program(mut self, program: XfaFontProgram) -> Self {
        self.program = Some(program);
        self
    }

    /// The advance of WinAnsi `code`, `None` when the face lacks it.
    #[must_use]
    pub fn width(&self, code: u8) -> Option<u16> {
        self.widths.get(usize::from(code)).copied().flatten()
    }
}

/// The character each WinAnsi code stands for (§D.2), `None` for an
/// unassigned code.
#[must_use]
pub fn winansi_chars() -> &'static [Option<char>; 256] {
    static TABLE: OnceLock<[Option<char>; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut out = [None; 256];
        // Every WinAnsi character lies at or below U+2122 (TRADE MARK SIGN).
        for c in (0..=0x2122u32).filter_map(char::from_u32) {
            let mut buf = [0u8; 4];
            let (codes, miss) = encode_winansi(c.encode_utf8(&mut buf));
            if let ([code], 0) = (codes.as_slice(), miss)
                && let Some(slot) = out.get_mut(usize::from(*code))
                && slot.is_none()
            {
                *slot = Some(c);
            }
        }
        out
    })
}

/// Where XFA text finds a face for a `font` element's `typeface`.
///
/// Implemented by a shell over the fonts it has read; the renderer's font
/// environment must resolve each returned [`XfaFace::base_font`] to the
/// same face, or a rendered page and a written one differ.
pub trait XfaFaceSource: Send + Sync {
    /// The face for `typeface` in the requested style, `None` to fall back
    /// to the standard 14.
    fn face(&self, typeface: &str, bold: bool, italic: bool) -> Option<XfaFace>;
}

type FaceKey = (String, bool, bool);

/// The faces XFA layout and painting set text in: the standard 14 only
/// ([`XfaFonts::new`]), or a shell's [`XfaFaceSource`] first. Pass the same
/// value to layout and to painting, so text is measured as it is drawn.
///
/// # Examples
///
/// ```
/// use pdfcer_core::xfa::{XfaFace, XfaFaceSource, XfaFonts};
///
/// struct Wide;
/// impl XfaFaceSource for Wide {
///     fn face(&self, typeface: &str, _bold: bool, _italic: bool) -> Option<XfaFace> {
///         (typeface == "Wide").then(|| XfaFace::new("Wide-Regular", |_| Some(1000)))
///     }
/// }
/// let fonts = XfaFonts::with_source(Wide);
/// assert_eq!(fonts.face("Wide", false, false).unwrap().width(b'A'), Some(1000));
/// assert!(fonts.face("Myriad Pro", false, false).is_none());
/// ```
#[derive(Default)]
pub struct XfaFonts {
    source: Option<Box<dyn XfaFaceSource>>,
    memo: Mutex<HashMap<FaceKey, Option<Arc<XfaFace>>>>,
    fallbacks: Mutex<BTreeMap<String, String>>,
}

impl XfaFonts {
    /// The standard 14 only.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `source` first, then the standard 14.
    #[must_use]
    pub fn with_source(source: impl XfaFaceSource + 'static) -> Self {
        Self {
            source: Some(Box::new(source)),
            ..Self::default()
        }
    }

    /// The supplied face for `typeface`, asked of the source once per style.
    #[must_use]
    pub fn face(&self, typeface: &str, bold: bool, italic: bool) -> Option<Arc<XfaFace>> {
        let source = self.source.as_ref()?;
        let key = (typeface.trim().to_owned(), bold, italic);
        let mut memo = self.memo.lock().unwrap_or_else(PoisonError::into_inner);
        memo.entry(key)
            .or_insert_with_key(|k| source.face(&k.0, bold, italic).map(Arc::new))
            .clone()
    }

    /// Each typeface text was set in a standard-14 face for, with that
    /// face's `/BaseFont`, in name order: the disclosure a shell prints
    /// (rule 4) after laying out or painting.
    #[must_use]
    pub fn fallbacks(&self) -> Vec<(String, String)> {
        let map = self
            .fallbacks
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    pub(crate) fn record_fallback(&self, typeface: &str, base_font: impl FnOnce() -> String) {
        let mut map = self
            .fallbacks
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        map.entry(typeface.trim().to_owned())
            .or_insert_with(base_font);
    }
}

impl fmt::Debug for XfaFonts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("XfaFonts")
            .field("supplied", &self.source.is_some())
            .finish_non_exhaustive()
    }
}
