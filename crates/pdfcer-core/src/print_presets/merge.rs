//! The merge of a print job's operator options with the document's own
//! print presets (ISO 32000-1 §12.2 Table 150; ISO 32000-2 Tables 147-148).
//!
//! A preset fills only an option the operator left unset, and every preset
//! used is reported as a [`PresetNote`]. The one exception is
//! `/Enforce [/PrintScaling]`: the document's actual-size setting then wins
//! over an explicit scale too, unless every preset is ignored.

use super::{PresetBoundary, PresetDuplex, PresetScaling, PrintPresets};

/// How a page is scaled onto the sheet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum PrintScale {
    /// Scale each page up or down to fill the printable area.
    #[default]
    Fit,
    /// Print at 100%.
    Actual,
    /// Scale down only a page larger than the printable area.
    Shrink,
}

/// The most copies [`merge_print_presets`] takes from a document's
/// `/NumCopies`.
pub const MAX_PRINT_COPIES: u16 = 999;

/// The options a document preset can fill, as the operator gave them.
/// `None` (or `false`) means "not chosen", so a preset may fill it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PrintRequest {
    /// The scale mode.
    pub scale: Option<PrintScale>,
    /// A custom scale in percent; counts as the operator choosing a scale.
    pub scale_percent: Option<u32>,
    /// A page spec in `print --pages` form (`"all"`, `"1-3,5"`; 1-based).
    pub pages: Option<String>,
    /// Copies.
    pub copies: Option<u16>,
    /// Sides.
    pub duplex: Option<PresetDuplex>,
    /// Pick the tray by page size.
    pub pick_tray: bool,
}

/// What the job uses, and where each preset-filled value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EffectivePrint {
    /// The scale mode.
    pub scale: PrintScale,
    /// A custom scale in percent, `None` when the scale mode applies alone.
    pub scale_percent: Option<u32>,
    /// The page spec, `"all"` when neither side named one.
    pub pages: String,
    /// Copies, at least 1.
    pub copies: u16,
    /// Sides.
    pub duplex: PresetDuplex,
    /// Pick the tray by page size.
    pub pick_tray: bool,
    /// One entry per preset used, enforced, ignored or not honoured, in
    /// the order the job applies them.
    pub notes: Vec<PresetNote>,
}

/// What became of one of the document's print presets.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PresetNote {
    /// Every preset was ignored at the operator's request.
    AllIgnored,
    /// `/Enforce [/PrintScaling]` was overridden by ignoring every preset.
    EnforcedOverridden,
    /// `/PrintScaling /None` set actual size. `enforced`: the operator had
    /// chosen a scale, and `/Enforce` replaced it.
    ActualSize {
        /// The operator's own scale was replaced.
        enforced: bool,
    },
    /// `/PrintPageRange` set the pages, as this spec.
    Pages(String),
    /// A `/PrintPageRange` pair starts past the last page and was skipped.
    RangePastEnd {
        /// First page of the pair, 1-based.
        first: u32,
        /// Last page of the pair, 1-based.
        last: u32,
        /// The document's page count.
        page_count: u32,
    },
    /// `/NumCopies` asked for more than [`MAX_PRINT_COPIES`]; followed by
    /// [`Self::Copies`] with the clamped count.
    CopiesClamped {
        /// The count the document asked for.
        asked: u32,
    },
    /// `/NumCopies` set this many copies.
    Copies(u16),
    /// `/Duplex` set the sides.
    Duplex(PresetDuplex),
    /// `/PickTrayByPDFSize true` set tray selection by page size.
    PickTrayBySize,
    /// `/PrintArea` names a boundary other than the crop box; the page is
    /// printed as it displays.
    PrintAreaNotHonoured(PresetBoundary),
    /// `/PrintClip` names a boundary other than the crop box; the page is
    /// printed as it displays.
    PrintClipNotHonoured(PresetBoundary),
    /// An entry present but unusable, as [`PrintPresets::ignored`] words it.
    Unusable(String),
}

/// Merge `req` with `presets` for a document of `page_count` pages.
/// `ignore` drops every preset, enforced or not, and says so.
///
/// ```
/// use pdfcer_core::print_presets::{
///     merge_print_presets, PresetNote, PresetScaling, PrintPresets, PrintRequest, PrintScale,
/// };
///
/// let mut presets = PrintPresets::default();
/// presets.scaling = Some(PresetScaling::None);
/// let job = merge_print_presets(PrintRequest::default(), &presets, 3, false);
/// assert_eq!(job.scale, PrintScale::Actual);
/// assert_eq!(job.notes, [PresetNote::ActualSize { enforced: false }]);
/// ```
#[must_use]
pub fn merge_print_presets(
    req: PrintRequest,
    presets: &PrintPresets,
    page_count: usize,
    ignore: bool,
) -> EffectivePrint {
    let mut eff = EffectivePrint {
        scale: req.scale.unwrap_or_default(),
        scale_percent: req.scale_percent,
        pages: req.pages.clone().unwrap_or_else(|| "all".to_owned()),
        copies: req.copies.unwrap_or(1),
        duplex: req.duplex.unwrap_or(PresetDuplex::Simplex),
        pick_tray: req.pick_tray,
        notes: Vec::new(),
    };
    if ignore {
        if !presets.is_empty() {
            eff.notes.push(PresetNote::AllIgnored);
        }
        if presets.scaling_enforced {
            eff.notes.push(PresetNote::EnforcedOverridden);
        }
        return eff;
    }
    scaling(&req, presets, &mut eff);
    if req.pages.is_none()
        && let Some(spec) = ranges(&presets.page_ranges, page_count, &mut eff.notes)
    {
        eff.notes.push(PresetNote::Pages(spec.clone()));
        eff.pages = spec;
    }
    if req.copies.is_none()
        && let Some(asked) = presets.copies
    {
        let n = u16::try_from(asked)
            .unwrap_or(u16::MAX)
            .min(MAX_PRINT_COPIES);
        if u32::from(n) < asked {
            eff.notes.push(PresetNote::CopiesClamped { asked });
        }
        eff.notes.push(PresetNote::Copies(n));
        eff.copies = n;
    }
    if req.duplex.is_none()
        && let Some(d) = presets.duplex
    {
        eff.duplex = d;
        eff.notes.push(PresetNote::Duplex(d));
    }
    if !req.pick_tray && presets.pick_tray_by_pdf_size == Some(true) {
        eff.pick_tray = true;
        eff.notes.push(PresetNote::PickTrayBySize);
    }
    boundaries(presets, &mut eff.notes);
    eff.notes
        .extend(presets.ignored.iter().cloned().map(PresetNote::Unusable));
    eff
}

fn scaling(req: &PrintRequest, presets: &PrintPresets, eff: &mut EffectivePrint) {
    if presets.scaling != Some(PresetScaling::None) {
        return;
    }
    let operator_set = req.scale.is_some() || req.scale_percent.is_some();
    if !operator_set {
        eff.scale = PrintScale::Actual;
        eff.notes.push(PresetNote::ActualSize { enforced: false });
    } else if presets.scaling_enforced {
        eff.scale = PrintScale::Actual;
        eff.scale_percent = None;
        eff.notes.push(PresetNote::ActualSize { enforced: true });
    }
}

/// `ranges` as a page spec, clipped to the document's pages.
fn ranges(ranges: &[(u32, u32)], page_count: usize, notes: &mut Vec<PresetNote>) -> Option<String> {
    let last = u32::try_from(page_count).unwrap_or(u32::MAX);
    let mut parts = Vec::new();
    for &(a, b) in ranges {
        if a > last {
            notes.push(PresetNote::RangePastEnd {
                first: a,
                last: b,
                page_count: last,
            });
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

fn boundaries(presets: &PrintPresets, notes: &mut Vec<PresetNote>) {
    if let Some(b) = presets.print_area
        && b != PresetBoundary::CropBox
    {
        notes.push(PresetNote::PrintAreaNotHonoured(b));
    }
    if let Some(b) = presets.print_clip
        && b != PresetBoundary::CropBox
    {
        notes.push(PresetNote::PrintClipNotHonoured(b));
    }
}

#[cfg(test)]
mod tests;
