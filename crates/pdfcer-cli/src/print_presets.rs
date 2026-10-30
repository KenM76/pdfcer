//! `print`'s use of the engine's preset merge
//! ([`pdfcer_core::print_presets::merge_print_presets`]): the operator's
//! options in, the job and its notes out, each note worded for the CLI.

use pdfcer_core::print_presets::{
    EffectivePrint, MAX_PRINT_COPIES, PresetDuplex, PresetNote, PrintPresets, PrintRequest,
    PrintScale,
};

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
    let mut core = PrintRequest::default();
    core.scale = req.scale.map(scale_to_core);
    core.scale_percent = req.scale_percent;
    core.pages = req.pages;
    core.copies = req.copies;
    core.duplex = req.duplex.map(duplex_to_core);
    core.pick_tray = req.pick_tray;
    let EffectivePrint {
        scale,
        scale_percent,
        pages,
        copies,
        duplex,
        pick_tray,
        notes,
        ..
    } = pdfcer_core::print_presets::merge_print_presets(core, presets, page_count, ignore);
    Effective {
        scale: scale_from_core(scale),
        scale_percent,
        pages,
        copies,
        duplex: duplex_from_core(duplex),
        pick_tray,
        notes: notes.iter().map(note_text).collect(),
    }
}

/// `note` as the line `print` writes for it.
fn note_text(note: &PresetNote) -> String {
    match note {
        PresetNote::AllIgnored => {
            "the document's own print settings were ignored (--ignore-document-presets).".into()
        }
        PresetNote::EnforcedOverridden => {
            "the document requires actual size (/Enforce); that was overridden too.".into()
        }
        PresetNote::ActualSize { enforced: false } => {
            "actual size, from the document's print settings.".into()
        }
        PresetNote::ActualSize { enforced: true } => {
            "actual size: the document requires it (/Enforce), so your scale was not used. \
             Add --ignore-document-presets to use it."
                .into()
        }
        PresetNote::Pages(spec) => format!("pages {spec}, from the document's print range."),
        PresetNote::RangePastEnd {
            first,
            last,
            page_count,
        } => format!(
            "the document's print range {first}-{last} is past its last page ({page_count}); skipped."
        ),
        PresetNote::CopiesClamped { asked } => format!(
            "the document asks for {asked} copies; pdfcer prints at most {MAX_PRINT_COPIES}."
        ),
        PresetNote::Copies(n) => format!("{n} copies, from the document's print settings."),
        PresetNote::Duplex(d) => format!(
            "{}, from the document's print settings.",
            duplex_words(duplex_from_core(*d))
        ),
        PresetNote::PickTrayBySize => {
            "tray chosen by page size, from the document's print settings.".into()
        }
        PresetNote::PrintAreaNotHonoured(b) => boundary_text("PrintArea", b.name()),
        PresetNote::PrintClipNotHonoured(b) => boundary_text("PrintClip", b.name()),
        PresetNote::Unusable(why) => format!("unusable print setting ignored: {why}."),
        // `PresetNote` is `#[non_exhaustive]`; a variant this build does not
        // know is still disclosed rather than dropped.
        other => format!("document print setting: {other:?}."),
    }
}

fn boundary_text(key: &str, boundary: &str) -> String {
    format!("the document's /{key} asks for its {boundary}; pdfcer prints the page as it displays.")
}

const fn scale_to_core(s: PrintScaleArg) -> PrintScale {
    match s {
        PrintScaleArg::Fit => PrintScale::Fit,
        PrintScaleArg::Actual => PrintScale::Actual,
        PrintScaleArg::Shrink => PrintScale::Shrink,
    }
}

const fn scale_from_core(s: PrintScale) -> PrintScaleArg {
    match s {
        PrintScale::Fit => PrintScaleArg::Fit,
        PrintScale::Actual => PrintScaleArg::Actual,
        PrintScale::Shrink => PrintScaleArg::Shrink,
    }
}

const fn duplex_to_core(d: DuplexArg) -> PresetDuplex {
    match d {
        DuplexArg::Simplex => PresetDuplex::Simplex,
        DuplexArg::LongEdge => PresetDuplex::FlipLongEdge,
        DuplexArg::ShortEdge => PresetDuplex::FlipShortEdge,
    }
}

const fn duplex_from_core(d: PresetDuplex) -> DuplexArg {
    match d {
        PresetDuplex::Simplex => DuplexArg::Simplex,
        PresetDuplex::FlipLongEdge => DuplexArg::LongEdge,
        PresetDuplex::FlipShortEdge => DuplexArg::ShortEdge,
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
