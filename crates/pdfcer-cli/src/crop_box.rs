//! `pdfcer set-crop-box`: set, reset or trim-to-content a page box on a
//! page selection, through [`pdfcer_core::edit::EditSession::set_page_boxes`].

use super::*;
use pdfcer_core::edit::{
    CropBoxEdit, CropBoxEntry, EditSession, PageBox, PageBoxChange, PageBoxEdit,
};
use pdfcer_core::page_tree::{Page, Rect};

/// Which page boundary `set-crop-box --box` changes (ISO 32000-2 §14.11.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum PageBoxArg {
    /// /CropBox — what every reader shows and prints.
    #[default]
    Crop,
    /// /BleedBox — where production output is clipped.
    Bleed,
    /// /TrimBox — the finished page after trimming.
    Trim,
    /// /ArtBox — the extent of the meaningful content.
    Art,
}

impl PageBoxArg {
    fn to_core(self) -> PageBox {
        match self {
            Self::Crop => PageBox::Crop,
            Self::Bleed => PageBox::Bleed,
            Self::Trim => PageBox::Trim,
            Self::Art => PageBox::Art,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Crop => "crop",
            Self::Bleed => "bleed",
            Self::Trim => "trim",
            Self::Art => "art",
        }
    }
}

/// What to do to each selected page's box.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CropRequest {
    Set(Rect),
    Reset,
    /// Fit the box to the painted content, grown by `margin` points.
    WhiteMargins {
        margin: f64,
    },
}

/// The parsed `set-crop-box` invocation.
pub(crate) struct SetCropBoxArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) pages: &'a str,
    pub(crate) odd: bool,
    pub(crate) even: bool,
    pub(crate) page_box: PageBoxArg,
    pub(crate) request: CropRequest,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// Turn `--rect` / `--reset` / `--remove-white-margins` into a request.
pub(crate) fn crop_request(
    rect: Option<&str>,
    reset: bool,
    white_margins: bool,
    margin: Option<f64>,
) -> Result<CropRequest, String> {
    if margin.is_some() && !white_margins {
        // clap waives `requires` when its target conflicts with a present
        // arg (`--rect`, `--reset`), so this pair is checked here.
        return Err("--margin needs --remove-white-margins".into());
    }
    match (rect, reset, white_margins) {
        (_, _, true) => {
            let margin = margin.unwrap_or(0.0);
            if !margin.is_finite() || margin < 0.0 {
                return Err(format!("--margin {margin} must be zero or more points"));
            }
            Ok(CropRequest::WhiteMargins { margin })
        }
        (_, true, false) => Ok(CropRequest::Reset),
        (Some(raw), false, false) => {
            let nums: Vec<f64> = raw
                .split(',')
                .filter_map(|p| p.trim().parse::<f64>().ok())
                .collect();
            let [x0, y0, x1, y1] = nums[..] else {
                return Err(format!(
                    "--rect: `{raw}` is not four comma-separated numbers `x0,y0,x1,y1`"
                ));
            };
            Ok(CropRequest::Set(Rect::from_corners(x0, y0, x1, y1)))
        }
        (None, false, false) => Err("give --rect, --reset or --remove-white-margins".into()),
    }
}

/// Implement `pdfcer set-crop-box`.
///
/// Prints one machine line, `set-crop-box <in> pages <spec> mode=<m> ->
/// <out>; pages_set=<n> explicit=<n> removed=<n> base_kept=<n>
/// overhang=<n> changed=<0|1> …save counters… box=<crop|bleed|trim|art>
/// detected=<n> blank=<n>`, plus a stderr note per page whose rectangle
/// overhangs its sheet, and with `--remove-white-margins` one per page
/// giving the rectangle it inferred (rule 4).
pub(crate) fn cmd_set_crop_box(a: &SetCropBoxArgs<'_>) -> u8 {
    let (source, mut session) = match open_for_edit(a.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let pages = match session.pages() {
        Ok(pages) => pages,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", a.input.display());
            return exit::EDIT_REFUSED;
        }
    };
    let targets = match parse_pages(a.pages, pages.len()) {
        Ok(list) => filter_parity(list, a.odd, a.even),
        Err(msg) => {
            eprintln!("pdfcer: {}: --pages: {msg}", a.input.display());
            return exit::EDIT_REFUSED;
        }
    };
    if targets.is_empty() {
        eprintln!(
            "pdfcer: {}: no page matches --pages with --odd/--even",
            a.input.display()
        );
        return exit::EDIT_REFUSED;
    }
    let plan = match plan_edits(&session, &pages, &targets, a) {
        Ok(plan) => plan,
        Err(msg) => {
            eprintln!("pdfcer: {}: {msg}", a.input.display());
            return exit::EDIT_REFUSED;
        }
    };
    let changes = match session.set_page_boxes(&plan.edits) {
        Ok(changes) => changes,
        Err(err) => return report_edit_error(a.input, &err),
    };
    let tally = Tally::of(&changes, a.page_box);
    let outcome = match save_edited(
        &mut session,
        &source,
        a.output,
        a.mode,
        ProducerArg::Preserve,
        a.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    print_result(a, &changes, &tally, &plan, &outcome);
    finish_edit(a.input, &outcome)
}

/// Keep only odd (1-based) or even pages when asked.
fn filter_parity(list: Vec<usize>, odd: bool, even: bool) -> Vec<usize> {
    list.into_iter()
        .filter(|&i| match (odd, even) {
            (true, false) => i % 2 == 0,
            (false, true) => i % 2 == 1,
            _ => true,
        })
        .collect()
}

struct Plan {
    edits: Vec<PageBoxEdit>,
    detected: usize,
    blank: usize,
}

fn plan_edits(
    session: &EditSession,
    pages: &[Page],
    targets: &[usize],
    a: &SetCropBoxArgs<'_>,
) -> Result<Plan, String> {
    let target = a.page_box.to_core();
    let mut plan = Plan {
        edits: Vec::with_capacity(targets.len()),
        detected: 0,
        blank: 0,
    };
    for &index in targets {
        let edit = match a.request {
            CropRequest::Set(r) => CropBoxEdit::Set(r),
            CropRequest::Reset => CropBoxEdit::Reset,
            CropRequest::WhiteMargins { margin } => {
                let page = pages.get(index).ok_or("page index out of range")?;
                let found = pdfcer_render::white_margin_crop(&session.view(), page, margin)
                    .map_err(|e| format!("--remove-white-margins: {e}"))?;
                let Some(found) = found else {
                    plan.blank += 1;
                    eprintln!(
                        "pdfcer: note: page {}: nothing painted; its {} box is left as is.",
                        index + 1,
                        a.page_box.name()
                    );
                    continue;
                };
                let (c, f) = (found.content, found.crop);
                plan.detected += 1;
                eprintln!(
                    "pdfcer: note: page {}: content found within [{:.2} {:.2} {:.2} {:.2}] \
                     (inferred from a {} dpi render); the {} box is set to \
                     [{:.2} {:.2} {:.2} {:.2}].",
                    index + 1,
                    c.llx,
                    c.lly,
                    c.urx,
                    c.ury,
                    (72.0 * pdfcer_render::WHITE_MARGIN_SCALE) as u32,
                    a.page_box.name(),
                    f.llx,
                    f.lly,
                    f.urx,
                    f.ury
                );
                CropBoxEdit::Set(f)
            }
        };
        plan.edits.push(PageBoxEdit::new(index, target, edit));
    }
    Ok(plan)
}

#[derive(Default)]
struct Tally {
    explicit: usize,
    removed: usize,
    base_kept: usize,
    overhang: usize,
}

impl Tally {
    fn of(changes: &[PageBoxChange], page_box: PageBoxArg) -> Self {
        let mut t = Self::default();
        for change in changes {
            match change.entry {
                CropBoxEntry::ExplicitWritten => t.explicit += 1,
                CropBoxEntry::BaseSpellingKept => t.base_kept += 1,
                CropBoxEntry::InheritedSoOwnEntryRemoved | CropBoxEntry::Absent => t.removed += 1,
                // `CropBoxEntry` is #[non_exhaustive].
                _ => {}
            }
            if change.overhangs_media_box {
                t.overhang += 1;
                let a = change.after;
                eprintln!(
                    "pdfcer: note: page {}: the {} box reaches past the sheet. It is written as \
                     given; readers use only the part on the sheet, [{:.4} {:.4} {:.4} {:.4}] \
                     (ISO 32000-2 §14.11.2.1).",
                    change.page_index + 1,
                    page_box.name(),
                    a.llx,
                    a.lly,
                    a.urx,
                    a.ury
                );
            }
        }
        t
    }
}

fn print_result(
    a: &SetCropBoxArgs<'_>,
    changes: &[PageBoxChange],
    t: &Tally,
    plan: &Plan,
    outcome: &EditOutcome,
) {
    let r = &outcome.report;
    println!(
        "set-crop-box {} pages {} mode={} -> {}; \
pages_set={} explicit={} removed={} base_kept={} \
overhang={} changed={} objects={} verbatim={} reserialized={} promoted={} \
appended={} out_bytes={} undo_verified={} undo_identical={} delinearized={} \
box={} detected={} blank={}",
        a.input.display(),
        a.pages,
        a.mode.name(),
        a.output.display(),
        changes.len(),
        t.explicit,
        t.removed,
        t.base_kept,
        t.overhang,
        outcome.changed,
        r.objects_written,
        r.objects_verbatim,
        r.objects_reserialized,
        r.promoted.len(),
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
        u32::from(r.delinearized),
        a.page_box.name(),
        plan.detected,
        plan.blank,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parity_is_one_based() {
        assert_eq!(filter_parity(vec![0, 1, 2, 3], true, false), [0, 2]);
        assert_eq!(filter_parity(vec![0, 1, 2, 3], false, true), [1, 3]);
        assert_eq!(filter_parity(vec![0, 1], false, false), [0, 1]);
    }
}
