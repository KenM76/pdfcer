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

/// Resolution of the render white margins are measured on: 2 px per point.
const MARGIN_SCALE: f32 = 2.0;
/// A channel value at or above this counts as paper, so faint JPEG noise in
/// a scanned margin is not mistaken for content.
const WHITE_THRESHOLD: u8 = 250;

/// Turn `--rect` / `--reset` / `--remove-white-margins` into a request.
pub(crate) fn crop_request(
    rect: Option<&str>,
    reset: bool,
    white_margins: bool,
    margin: Option<f64>,
) -> Result<CropRequest, String> {
    if margin.is_some() && !white_margins {
        // clap's `requires` is satisfied by the flag's implicit `false`.
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
                let Some(found) = content_bounds(session, page)? else {
                    plan.blank += 1;
                    eprintln!(
                        "pdfcer: note: page {}: nothing painted; its {} box is left as is.",
                        index + 1,
                        a.page_box.name()
                    );
                    continue;
                };
                let grown = Rect::from_corners(
                    found.llx - margin,
                    found.lly - margin,
                    found.urx + margin,
                    found.ury + margin,
                );
                let fitted = grown.intersection(&page.crop_box).unwrap_or(found);
                plan.detected += 1;
                eprintln!(
                    "pdfcer: note: page {}: content found within [{:.2} {:.2} {:.2} {:.2}] \
                     (inferred from a {} dpi render); the {} box is set to \
                     [{:.2} {:.2} {:.2} {:.2}].",
                    index + 1,
                    found.llx,
                    found.lly,
                    found.urx,
                    found.ury,
                    (72.0 * MARGIN_SCALE) as u32,
                    a.page_box.name(),
                    fitted.llx,
                    fitted.lly,
                    fitted.urx,
                    fitted.ury
                );
                CropBoxEdit::Set(fitted)
            }
        };
        plan.edits.push(PageBoxEdit::new(index, target, edit));
    }
    Ok(plan)
}

/// The user-space rectangle covering every non-white pixel of `page`'s
/// visible region, one pixel wider on each side; `None` for a blank page.
fn content_bounds(session: &EditSession, page: &Page) -> Result<Option<Rect>, String> {
    let options = pdfcer_render::RenderOptions::default();
    let rendered =
        pdfcer_render::render_page_with_view(&session.view(), page, MARGIN_SCALE, &options)
            .map_err(|e| format!("--remove-white-margins: render failed: {e}"))?;
    let pix = &rendered.pixmap;
    let (w, h) = (pix.width() as usize, pix.height() as usize);
    let Some((x0, y0, x1, y1)) = ink_extent(pix.data(), w, h) else {
        return Ok(None);
    };
    let pad = |v: usize, max: usize, grow: bool| {
        if grow {
            (v + 1).min(max)
        } else {
            v.saturating_sub(1)
        }
    };
    let (x0, y0) = (pad(x0, w, false) as f64, pad(y0, h, false) as f64);
    let (x1, y1) = (pad(x1, w, true) as f64, pad(y1, h, true) as f64);
    let c = page.crop_box;
    let turned = matches!(page.rotate % 360, 90 | 270);
    let s = w as f64 / if turned { c.height() } else { c.width() };
    let (a, b) = (
        device_to_user(c, page.rotate, s, x0, y0),
        device_to_user(c, page.rotate, s, x1, y1),
    );
    Ok(Some(Rect::from_corners(a.0, a.1, b.0, b.1)))
}

/// Bounds `[x0, x1) × [y0, y1)` of the pixels darker than paper in an RGBA
/// buffer, or `None` when every pixel is white.
fn ink_extent(data: &[u8], w: usize, h: usize) -> Option<(usize, usize, usize, usize)> {
    let mut extent: Option<(usize, usize, usize, usize)> = None;
    for (i, px) in data.chunks_exact(4).enumerate().take(w * h) {
        if px[..3].iter().all(|&v| v >= WHITE_THRESHOLD) {
            continue;
        }
        let (x, y) = (i % w, i / w);
        extent = Some(match extent {
            None => (x, y, x + 1, y + 1),
            Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x + 1), d.max(y + 1)),
        });
    }
    extent
}

/// Map a device pixel (top-left origin, `s` px per point) on a page framed
/// by crop box `c` and turned clockwise by `/Rotate` back to user space.
fn device_to_user(c: Rect, rotate: u16, s: f64, x: f64, y: f64) -> (f64, f64) {
    let (u, v) = (x / s, y / s);
    match rotate % 360 {
        90 => (c.llx + v, c.lly + u),
        180 => (c.urx - u, c.lly + v),
        270 => (c.urx - v, c.ury - u),
        _ => (c.llx + u, c.ury - v),
    }
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

    fn rect(a: f64, b: f64, c: f64, d: f64) -> Rect {
        Rect::from_corners(a, b, c, d)
    }

    #[test]
    fn each_rotation_maps_the_top_left_pixel_to_the_right_corner() {
        let c = rect(10.0, 20.0, 110.0, 220.0);
        assert_eq!(device_to_user(c, 0, 2.0, 0.0, 0.0), (10.0, 220.0));
        assert_eq!(device_to_user(c, 90, 2.0, 0.0, 0.0), (10.0, 20.0));
        assert_eq!(device_to_user(c, 180, 2.0, 0.0, 0.0), (110.0, 20.0));
        assert_eq!(device_to_user(c, 270, 2.0, 0.0, 0.0), (110.0, 220.0));
        // One point right, two down, at 2 px per point.
        assert_eq!(device_to_user(c, 0, 2.0, 2.0, 4.0), (11.0, 218.0));
        assert_eq!(device_to_user(c, 90, 2.0, 2.0, 4.0), (12.0, 21.0));
    }

    #[test]
    fn ink_extent_ignores_paper_and_near_white() {
        let (w, h) = (4, 3);
        let mut data = vec![255_u8; w * h * 4];
        assert_eq!(ink_extent(&data, w, h), None);
        data[(w + 2) * 4] = 252; // near-white noise
        assert_eq!(ink_extent(&data, w, h), None);
        data[(2 * w + 1) * 4 + 1] = 0;
        assert_eq!(ink_extent(&data, w, h), Some((1, 2, 2, 3)));
    }

    #[test]
    fn parity_is_one_based() {
        assert_eq!(filter_parity(vec![0, 1, 2, 3], true, false), [0, 2]);
        assert_eq!(filter_parity(vec![0, 1, 2, 3], false, true), [1, 3]);
        assert_eq!(filter_parity(vec![0, 1], false, false), [0, 1]);
    }
}
