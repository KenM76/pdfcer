//! `print`'s merge of the operator's options with the document's own print
//! presets (ISO 32000-1 §12.2 Table 150; ISO 32000-2 Tables 147-148).
//!
//! A preset fills only an option the operator left unset, and every preset
//! used is reported. The one exception is `/Enforce [/PrintScaling]`: the
//! document's actual-size setting then wins over `--scale` too, unless
//! `--ignore-document-presets` is given, and either way it is reported.

use pdfcer_core::print_presets::{PresetBoundary, PresetDuplex, PresetScaling, PrintPresets};

use crate::print::{DuplexArg, PrintScaleArg};

/// The options a document preset can fill, as the operator gave them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Requested {
    pub scale: Option<PrintScaleArg>,
    pub scale_percent: Option<u32>,
    pub pages: Option<String>,
    pub copies: Option<u16>,
    pub duplex: Option<DuplexArg>,
    pub pick_tray: bool,
}

/// What the job uses, and the lines saying where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Effective {
    pub scale: PrintScaleArg,
    pub scale_percent: Option<u32>,
    pub pages: String,
    pub copies: u16,
    pub duplex: DuplexArg,
    pub pick_tray: bool,
    pub notes: Vec<String>,
}

/// The most copies `print --copies` accepts.
const MAX_COPIES: u16 = 999;

/// Merge `req` with `presets` for a document of `page_count` pages.
/// `ignore` drops every preset, enforced or not.
// Consumed only by the `#[cfg(windows)]` `cmd_print`; the merge is pure,
// so it is compiled and tested everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn merge(
    req: Requested,
    presets: &PrintPresets,
    page_count: usize,
    ignore: bool,
) -> Effective {
    let mut notes = Vec::new();
    let mut eff = Effective {
        scale: req.scale.unwrap_or(PrintScaleArg::Fit),
        scale_percent: req.scale_percent,
        pages: req.pages.clone().unwrap_or_else(|| "all".to_owned()),
        copies: req.copies.unwrap_or(1),
        duplex: req.duplex.unwrap_or(DuplexArg::Simplex),
        pick_tray: req.pick_tray,
        notes: Vec::new(),
    };
    if ignore {
        if !presets.is_empty() {
            notes.push(
                "the document's own print settings were ignored (--ignore-document-presets)."
                    .to_owned(),
            );
        }
        if presets.scaling_enforced {
            notes.push(
                "the document requires actual size (/Enforce); that was overridden too.".to_owned(),
            );
        }
        eff.notes = notes;
        return eff;
    }
    scaling(&req, presets, &mut eff, &mut notes);
    if req.pages.is_none()
        && let Some(spec) = ranges(&presets.page_ranges, page_count, &mut notes)
    {
        notes.push(format!("pages {spec}, from the document's print range."));
        eff.pages = spec;
    }
    if req.copies.is_none()
        && let Some(n) = presets.copies
    {
        let asked = n;
        let n = u16::try_from(n).unwrap_or(u16::MAX).min(MAX_COPIES);
        if u32::from(n) < asked {
            notes.push(format!(
                "the document asks for {asked} copies; pdfcer prints at most {MAX_COPIES}."
            ));
        }
        notes.push(format!("{n} copies, from the document's print settings."));
        eff.copies = n;
    }
    if req.duplex.is_none()
        && let Some(d) = presets.duplex
    {
        eff.duplex = match d {
            PresetDuplex::Simplex => DuplexArg::Simplex,
            PresetDuplex::FlipLongEdge => DuplexArg::LongEdge,
            PresetDuplex::FlipShortEdge => DuplexArg::ShortEdge,
        };
        notes.push(format!(
            "{}, from the document's print settings.",
            duplex_words(eff.duplex)
        ));
    }
    if !req.pick_tray && presets.pick_tray_by_pdf_size == Some(true) {
        eff.pick_tray = true;
        notes.push("tray chosen by page size, from the document's print settings.".to_owned());
    }
    boundaries(presets, &mut notes);
    notes.extend(
        presets
            .ignored
            .iter()
            .map(|i| format!("unusable print setting ignored: {i}.")),
    );
    eff.notes = notes;
    eff
}

fn scaling(req: &Requested, presets: &PrintPresets, eff: &mut Effective, notes: &mut Vec<String>) {
    if presets.scaling != Some(PresetScaling::None) {
        return;
    }
    let operator_set = req.scale.is_some() || req.scale_percent.is_some();
    if !operator_set {
        eff.scale = PrintScaleArg::Actual;
        notes.push("actual size, from the document's print settings.".to_owned());
    } else if presets.scaling_enforced {
        eff.scale = PrintScaleArg::Actual;
        eff.scale_percent = None;
        notes.push(
            "actual size: the document requires it (/Enforce), so your scale was not used. \
             Add --ignore-document-presets to use it."
                .to_owned(),
        );
    }
}

/// `ranges` as a `--pages` spec, clipped to the document's pages.
fn ranges(ranges: &[(u32, u32)], page_count: usize, notes: &mut Vec<String>) -> Option<String> {
    if ranges.is_empty() {
        return None;
    }
    let last = u32::try_from(page_count).unwrap_or(u32::MAX);
    let mut parts = Vec::new();
    for &(a, b) in ranges {
        if a > last {
            notes.push(format!(
                "the document's print range {a}-{b} is past its last page ({last}); skipped."
            ));
            continue;
        }
        let b = b.min(last);
        parts.push(if a == b {
            a.to_string()
        } else {
            format!("{a}-{b}")
        });
    }
    (!parts.is_empty()).then(|| parts.join(","))
}

fn boundaries(presets: &PrintPresets, notes: &mut Vec<String>) {
    for (key, b) in [
        ("PrintArea", presets.print_area),
        ("PrintClip", presets.print_clip),
    ] {
        if let Some(b) = b
            && b != PresetBoundary::CropBox
        {
            notes.push(format!(
                "the document's /{key} asks for its {}; pdfcer prints the page as it displays.",
                b.name()
            ));
        }
    }
}

const fn duplex_words(d: DuplexArg) -> &'static str {
    match d {
        DuplexArg::Simplex => "one-sided",
        DuplexArg::LongEdge => "two-sided, flipped on the long edge",
        DuplexArg::ShortEdge => "two-sided, flipped on the short edge",
    }
}

#[cfg(test)]
mod tests;
