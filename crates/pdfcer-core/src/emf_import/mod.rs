//! Windows Enhanced Metafile import as vector content ([MS-EMF] v18.0).
//!
//! [`import`] plays an EMF's GDI records into one PDF content stream in the
//! picture's own frame, placed by
//! [`EditSession::add_emf`](crate::edit::EditSession::add_emf). Nothing is
//! rasterised: paths stay paths, clips stay clips, bitmaps the file carries
//! become image XObjects and text becomes text in a standard-14 face.
//!
//! Every record that is not drawn, and every one drawn approximately, is
//! counted by name in [`EmfImportNotes`] — the disclosure a shell shows.
//! EMF+ ([MS-EMFPLUS]) is not read: a dual-mode file is drawn from its EMF
//! records and says so; an EMF+-only file is refused, because its EMF
//! records are not a rendering of the picture.
//!
//! Ceilings (refused by name in a pre-scan, before any drawing):
//! [`MAX_INPUT_BYTES`], [`MAX_RECORDS`], [`MAX_POINTS`],
//! [`MAX_BITMAP_PIXELS`], [`MAX_SAVE_DEPTH`], [`MAX_CONTENT_BYTES`].

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::fontdata::Std14;
use crate::image_import::ImportedImage;

mod dc;
mod draw;
mod objects;
mod raster;
mod reader;
mod shapes;
mod text;

/// Largest EMF accepted, bytes.
pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
/// Most records one EMF may hold.
pub const MAX_RECORDS: usize = 1_000_000;
/// Most points, summed over every poly record of the file.
pub const MAX_POINTS: usize = 8_000_000;
/// Most pixels one embedded bitmap may declare.
pub const MAX_BITMAP_PIXELS: usize = 64 * 1024 * 1024;
/// Deepest EMR_SAVEDC nesting accepted.
pub const MAX_SAVE_DEPTH: usize = 1024;
/// Largest content stream the import may produce, bytes (uncompressed).
pub const MAX_CONTENT_BYTES: usize = 128 * 1024 * 1024;

/// Why an EMF could not be imported. Each variant names the refusal.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EmfImportError {
    /// The bytes do not start with an EMR_HEADER carrying the " EMF"
    /// signature ([MS-EMF] §2.2.9).
    #[error("this is not an EMF file (no EMR_HEADER with the \" EMF\" signature)")]
    NotEmf,
    /// The record stream is malformed.
    #[error("this EMF file is damaged: {detail}")]
    Corrupt {
        /// What did not parse.
        detail: String,
    },
    /// The file holds only EMF+ drawing ([MS-EMFPLUS] §2.3.3.3, header flag
    /// D clear). Its EMF records are GDI fall-through, not the picture.
    #[error(
        "this metafile is EMF+ only (no EMF fallback); pdfcer reads EMF records, not EMF+, and placed nothing"
    )]
    EmfPlusOnly,
    /// The picture frame and the bounds are both empty: it has no size.
    #[error("this EMF declares an empty picture frame and empty bounds")]
    EmptyFrame,
    /// Over [`MAX_INPUT_BYTES`].
    #[error("this EMF is larger than pdfcer's {limit}-byte ceiling")]
    TooLarge {
        /// The ceiling.
        limit: usize,
    },
    /// Over [`MAX_RECORDS`].
    #[error("this EMF has more than {limit} records")]
    TooManyRecords {
        /// The ceiling.
        limit: usize,
    },
    /// Over [`MAX_POINTS`].
    #[error("this EMF's poly records hold more than {limit} points")]
    TooManyPoints {
        /// The ceiling.
        limit: usize,
    },
    /// A bitmap over [`MAX_BITMAP_PIXELS`].
    #[error("this EMF embeds a bitmap larger than {limit} pixels")]
    BitmapTooLarge {
        /// The ceiling.
        limit: usize,
    },
    /// EMR_SAVEDC nested deeper than [`MAX_SAVE_DEPTH`].
    #[error("this EMF nests EMR_SAVEDC deeper than {limit}")]
    SaveDepth {
        /// The ceiling.
        limit: usize,
    },
    /// The generated content would exceed [`MAX_CONTENT_BYTES`].
    #[error("this EMF would produce more than {limit} bytes of PDF content")]
    TooComplex {
        /// The ceiling.
        limit: usize,
    },
}

/// What the import left out or drew approximately.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmfImportNotes {
    /// Records or features not drawn, by name (`EMR_ARC`, `EMR_GRADIENTFILL`,
    /// `RGN_OR clip`, …) → count.
    pub skipped: BTreeMap<String, usize>,
    /// Drawn, but not exactly as GDI would (`hatched brush (drawn solid)`,
    /// `text without Dx (standard widths)`, …) → count.
    pub approximated: BTreeMap<String, usize>,
    /// Each EMF font face drawn in a standard-14 substitute → its `BaseFont`.
    pub fonts_substituted: BTreeMap<String, String>,
    /// Characters with no WinAnsi code, drawn as `?`.
    pub characters_replaced: usize,
    /// The file carried EMF+ records as well (dual mode); they were ignored
    /// and the picture was drawn from its EMF records.
    pub emf_plus_ignored: bool,
}

impl EmfImportNotes {
    /// Nothing skipped, approximated or substituted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// One line for a status bar or a CLI report; empty when
    /// [`Self::is_empty`].
    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        let list = |m: &BTreeMap<String, usize>| {
            m.iter()
                .map(|(k, n)| format!("{k} \u{d7}{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        if !self.skipped.is_empty() {
            parts.push(format!("not drawn: {}", list(&self.skipped)));
        }
        if !self.approximated.is_empty() {
            parts.push(format!("approximated: {}", list(&self.approximated)));
        }
        if !self.fonts_substituted.is_empty() {
            let mut s = String::from("fonts substituted: ");
            for (i, (face, to)) in self.fonts_substituted.iter().enumerate() {
                let sep = if i == 0 { "" } else { ", " };
                let _ = write!(s, "{sep}{face} \u{2192} {to}");
            }
            parts.push(s);
        }
        if self.characters_replaced > 0 {
            parts.push(format!(
                "{} characters outside WinAnsi drawn as '?'",
                self.characters_replaced
            ));
        }
        if self.emf_plus_ignored {
            parts.push("EMF+ records ignored (drawn from the EMF fallback)".to_owned());
        }
        parts.join("; ")
    }

    /// Count one record or feature not drawn.
    pub(crate) fn skip(&mut self, what: &str) {
        *self.skipped.entry(what.to_owned()).or_default() += 1;
    }

    /// Count one record or feature drawn approximately.
    pub(crate) fn approximate(&mut self, what: &str) {
        *self.approximated.entry(what.to_owned()).or_default() += 1;
    }
}

/// An EMF converted into one content stream plus the resources it names.
///
/// Produced by [`import`]; pure, touches no document. The content is in
/// the picture's frame: points, origin at the frame's lower-left corner,
/// `[0 0 w h]` with `(w, h)` = [`Self::natural_size_pt`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ImportedEmf {
    pub(crate) content: Vec<u8>,
    pub(crate) size_pt: (f64, f64),
    /// Image resources: name (`Im1`, …) and image.
    pub(crate) images: Vec<(String, ImportedImage)>,
    /// Font resources: name (`F1`, …) and face.
    pub(crate) fonts: Vec<(String, Std14)>,
    pub(crate) notes: EmfImportNotes,
    pub(crate) records: usize,
}

impl ImportedEmf {
    /// The picture frame's size in points (1/72 inch): [MS-EMF] §2.2.9
    /// `Frame`, in 0.01 mm.
    #[must_use]
    pub fn natural_size_pt(&self) -> (f64, f64) {
        self.size_pt
    }

    /// What was skipped or approximated.
    #[must_use]
    pub fn notes(&self) -> &EmfImportNotes {
        &self.notes
    }

    /// Records read, EMR_HEADER and EMR_EOF included.
    #[must_use]
    pub fn record_count(&self) -> usize {
        self.records
    }

    /// Bitmaps written as image XObjects.
    #[must_use]
    pub fn image_count(&self) -> usize {
        self.images.len()
    }
}

/// Parse `data` as an EMF and play its records into PDF content.
///
/// # Errors
///
/// [`EmfImportError`]: not an EMF, damaged framing, EMF+ only, an empty
/// frame, or a ceiling.
///
/// # Examples
///
/// ```
/// # fn demo(emf: &[u8]) -> Result<(), pdfcer_core::emf_import::EmfImportError> {
/// let picture = pdfcer_core::emf_import::import(emf)?;
/// let (w, h) = picture.natural_size_pt();
/// assert!(w > 0.0 && h > 0.0);
/// if !picture.notes().is_empty() {
///     eprintln!("{}", picture.notes().summary());
/// }
/// # Ok(())
/// # }
/// ```
pub fn import(data: &[u8]) -> Result<ImportedEmf, EmfImportError> {
    let scan = reader::scan(data)?;
    let mut player = draw::Player::new(&scan.header)?;
    if scan.emf_plus_dual {
        player.notes.emf_plus_ignored = true;
    }
    for rec in reader::records(data) {
        player.play(&rec)?;
    }
    Ok(player.finish(scan.records))
}
