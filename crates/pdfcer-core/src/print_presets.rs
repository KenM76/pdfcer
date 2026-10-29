//! The print presets a document carries in its catalog's
//! `/ViewerPreferences` (ISO 32000-1 §12.2 Table 150; ISO 32000-2 §12.2
//! Tables 147 and 148).
//!
//! Read-only. A malformed entry reads as absent and is named in
//! [`PrintPresets::ignored`](crate::print_presets::PrintPresets::ignored) so a shell can say what it did not use.

use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};

/// `/PrintScaling` (PDF 1.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresetScaling {
    /// `/None`: print at actual size.
    None,
    /// `/AppDefault`, and any unrecognised name (Table 150): the reader's
    /// own default.
    AppDefault,
}

/// `/Duplex` (PDF 1.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresetDuplex {
    /// `/Simplex`.
    Simplex,
    /// `/DuplexFlipShortEdge`.
    FlipShortEdge,
    /// `/DuplexFlipLongEdge`.
    FlipLongEdge,
}

/// A page boundary named by `/PrintArea` or `/PrintClip` (§14.11.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresetBoundary {
    /// `/MediaBox`.
    MediaBox,
    /// `/CropBox`, the default of both entries.
    CropBox,
    /// `/BleedBox`.
    BleedBox,
    /// `/TrimBox`.
    TrimBox,
    /// `/ArtBox`.
    ArtBox,
}

impl PresetBoundary {
    /// The PDF name without its slash.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::MediaBox => "MediaBox",
            Self::CropBox => "CropBox",
            Self::BleedBox => "BleedBox",
            Self::TrimBox => "TrimBox",
            Self::ArtBox => "ArtBox",
        }
    }

    fn from_name(n: &[u8]) -> Option<Self> {
        Some(match n {
            b"MediaBox" => Self::MediaBox,
            b"CropBox" => Self::CropBox,
            b"BleedBox" => Self::BleedBox,
            b"TrimBox" => Self::TrimBox,
            b"ArtBox" => Self::ArtBox,
            _ => return None,
        })
    }
}

/// The print entries of a document's `/ViewerPreferences`. Every field is
/// `None` (or empty) when the document does not set it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PrintPresets {
    /// `/PrintScaling`.
    pub scaling: Option<PresetScaling>,
    /// `/Enforce` names `PrintScaling` (PDF 2.0, Table 148): the operator
    /// should not be able to override [`Self::scaling`]. Only `true` when
    /// `scaling` is `Some(PresetScaling::None)`, the one value Table 148
    /// allows it with.
    pub scaling_enforced: bool,
    /// `/Duplex`.
    pub duplex: Option<PresetDuplex>,
    /// `/PickTrayByPDFSize`.
    pub pick_tray_by_pdf_size: Option<bool>,
    /// `/PrintPageRange` as inclusive 1-based `(first, last)` pairs, in
    /// document order. A pair with `first > last` or a number below 1 is
    /// dropped and reported.
    pub page_ranges: Vec<(u32, u32)>,
    /// `/NumCopies`; a value below 1 is ignored (the standard names no
    /// range, so pdfcer reads "valid" as at least one).
    pub copies: Option<u32>,
    /// `/PrintArea` (deprecated in PDF 2.0).
    pub print_area: Option<PresetBoundary>,
    /// `/PrintClip` (deprecated in PDF 2.0).
    pub print_clip: Option<PresetBoundary>,
    /// Entries present but unusable, each as `Key: why`.
    pub ignored: Vec<String>,
}

impl PrintPresets {
    /// `true` when the document sets none of the print entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.scaling.is_none()
            && self.duplex.is_none()
            && self.pick_tray_by_pdf_size.is_none()
            && self.page_ranges.is_empty()
            && self.copies.is_none()
            && self.print_area.is_none()
            && self.print_clip.is_none()
    }
}

/// Read the print presets of `graph`'s catalog.
///
/// ```no_run
/// use pdfcer_core::document::Document;
/// use pdfcer_core::print_presets::{print_presets, PresetScaling};
///
/// let doc = Document::from_bytes(std::fs::read("drawing.pdf")?)?;
/// let presets = print_presets(&doc);
/// if presets.scaling == Some(PresetScaling::None) {
///     println!("print at actual size");
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn print_presets<G: ObjectGraph + ?Sized>(graph: &G) -> PrintPresets {
    let mut out = PrintPresets::default();
    let Some(vp) = graph
        .catalog_dict()
        .and_then(|c| c.get(b"ViewerPreferences"))
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict)
    else {
        return out;
    };
    let get = |k: &[u8]| vp.get(k).map(|o| graph.resolve(o));
    read_names(&mut out, &get);
    if let Some(o) = get(b"PickTrayByPDFSize") {
        match o {
            Object::Boolean(b) => out.pick_tray_by_pdf_size = Some(*b),
            _ => out.ignored.push("PickTrayByPDFSize: not a boolean".into()),
        }
    }
    if let Some(o) = get(b"NumCopies") {
        match o.as_int() {
            Some(n) if n >= 1 => out.copies = Some(u32::try_from(n).unwrap_or(u32::MAX)),
            Some(n) => out.ignored.push(format!("NumCopies: {n} is below 1")),
            None => out.ignored.push("NumCopies: not an integer".into()),
        }
    }
    if let Some(o) = get(b"PrintPageRange") {
        read_ranges(&mut out, o, graph);
    }
    out.scaling_enforced = out.scaling == Some(PresetScaling::None) && enforces_scaling(vp, graph);
    out
}

fn read_names<'a>(out: &mut PrintPresets, get: &impl Fn(&[u8]) -> Option<&'a Object>) {
    let name = |k: &[u8]| get(k).map(|o| o.as_name().map(|n| n.as_bytes().to_vec()));
    match name(b"PrintScaling") {
        Some(Some(n)) if n == b"None" => out.scaling = Some(PresetScaling::None),
        Some(Some(_)) => out.scaling = Some(PresetScaling::AppDefault),
        Some(None) => out.ignored.push("PrintScaling: not a name".into()),
        None => {}
    }
    match name(b"Duplex") {
        Some(Some(n)) => match n.as_slice() {
            b"Simplex" => out.duplex = Some(PresetDuplex::Simplex),
            b"DuplexFlipShortEdge" => out.duplex = Some(PresetDuplex::FlipShortEdge),
            b"DuplexFlipLongEdge" => out.duplex = Some(PresetDuplex::FlipLongEdge),
            other => out.ignored.push(format!(
                "Duplex: /{} is not a duplex mode",
                String::from_utf8_lossy(other)
            )),
        },
        Some(None) => out.ignored.push("Duplex: not a name".into()),
        None => {}
    }
    for (key, slot) in [
        (&b"PrintArea"[..], &mut out.print_area),
        (&b"PrintClip"[..], &mut out.print_clip),
    ] {
        match name(key) {
            Some(Some(n)) => match PresetBoundary::from_name(&n) {
                Some(b) => *slot = Some(b),
                None => out.ignored.push(format!(
                    "{}: /{} is not a page boundary",
                    String::from_utf8_lossy(key),
                    String::from_utf8_lossy(&n)
                )),
            },
            Some(None) => out
                .ignored
                .push(format!("{}: not a name", String::from_utf8_lossy(key))),
            None => {}
        }
    }
}

fn read_ranges<G: ObjectGraph + ?Sized>(out: &mut PrintPresets, o: &Object, graph: &G) {
    let Some(items) = o.as_array() else {
        out.ignored.push("PrintPageRange: not an array".into());
        return;
    };
    let nums: Vec<Option<i64>> = items.iter().map(|i| graph.resolve(i).as_int()).collect();
    if !nums.len().is_multiple_of(2) {
        out.ignored
            .push("PrintPageRange: odd number of entries; the last is unused".into());
    }
    for pair in nums.chunks_exact(2) {
        let [a, b] = [pair.first(), pair.get(1)].map(|v| v.copied().flatten());
        match (a, b) {
            (Some(a), Some(b)) if a >= 1 && a <= b => {
                let clamp = |v: i64| u32::try_from(v).unwrap_or(u32::MAX);
                out.page_ranges.push((clamp(a), clamp(b)));
            }
            (a, b) => out.ignored.push(format!(
                "PrintPageRange: pair {} {} is not a 1-based range",
                a.map_or_else(|| "?".into(), |v| v.to_string()),
                b.map_or_else(|| "?".into(), |v| v.to_string()),
            )),
        }
    }
}

fn enforces_scaling<G: ObjectGraph + ?Sized>(vp: &Dict, graph: &G) -> bool {
    vp.get(b"Enforce")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)
        .is_some_and(|names| {
            names.iter().any(|n| {
                graph
                    .resolve(n)
                    .as_name()
                    .is_some_and(|n| n.as_bytes() == b"PrintScaling")
            })
        })
}

#[cfg(test)]
mod tests;
