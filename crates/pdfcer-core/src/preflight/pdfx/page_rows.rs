//! Page-level PDF/X rows: colour (ISO 15930-1 §6.3.1), pre-separation
//! (15930-1 §6.2, 15930-3 §6.1, 15930-4 §6.1), transparency (15930-4
//! §6.16, 15930-3 Annex D), page boxes (15930-4 §6.8, 15930-7 §6.12) and
//! annotations (15930-4 §6.13, 15930-7 §6.17).

use crate::annot::{AnnotFlags, Annotation, page_annotations};
use crate::object::Object;
use crate::page_tree::{BoxResolution, Page, Rect};
use crate::preflight::checks::{self, Sink};
use crate::preflight::{Check, ColourFamily, FindingTarget, ProfileRule};

use super::{Ctx, PageCtx, PdfxLevel};

/// Spaces outside CMYK, gray and spot colour (ISO 15930-1 §6.3.1).
const NOT_X1A: [ColourFamily; 8] = [
    ColourFamily::DeviceRgb,
    ColourFamily::CalGray,
    ColourFamily::CalRgb,
    ColourFamily::Lab,
    ColourFamily::IccGray,
    ColourFamily::IccRgb,
    ColourFamily::IccCmyk,
    ColourFamily::IccOther,
];

/// Annotation subtypes ISO 15930-7 §6.17 excludes.
const MEDIA: [&[u8]; 5] = [b"Sound", b"Movie", b"Screen", b"RichMedia", b"3D"];

/// The per-page rows: colour, transparency, page boxes and annotations.
pub(super) fn check(ctx: &Ctx<'_>, p: &PageCtx<'_, '_>, sink: &mut Sink) {
    let (page, number) = (p.page, p.number);
    match ctx.row.id {
        "PXQ-CS-1" => {
            let check = Check::ColourSpaceUsed {
                families: NOT_X1A.to_vec(),
            };
            let rule = ProfileRule::new(ctx.rule.id.clone(), check, ctx.rule.severity);
            ctx.relay(&rule, sink, |r, s| checks::colour(r, p.events, number, s));
        }
        "PXQ-CS-10" => {
            let separated = p
                .view
                .graph()
                .value(page.id)
                .and_then(Object::as_dict)
                .is_some_and(|d| d.get(b"SeparationInfo").is_some());
            if separated {
                let m = "The page is one plate of a pre-separated job (/SeparationInfo).";
                sink.push(ctx.found(Some(number), FindingTarget::Page, m));
            }
        }
        "PXQ-TR-1" | "PXQ-TR-2" => ctx.relay(ctx.rule, sink, |r, s| {
            checks::transparency_used(r, p.view, page, number, s);
        }),
        "PXQ-BX-2" => {
            ctx.relay(ctx.rule, sink, |r, s| {
                checks::no_trim_box(r, page, number, s)
            });
            if ctx.level.is_2003()
                && written(page.trim_box_resolution)
                && written(page.art_box_resolution)
            {
                let m = "The page has both a TrimBox and an ArtBox.";
                sink.push(ctx.found(Some(number), FindingTarget::Page, m));
            }
        }
        "PXQ-BX-3" => inside(ctx, page, number, page.bleed_box, "bleed box", sink),
        "PXQ-BX-4" => inside(ctx, page, number, page.crop_box, "crop box", sink),
        "PXQ-BX-5" if written(page.art_box_resolution) && !written(page.trim_box_resolution) => {
            let m = "The page has an ArtBox but no TrimBox; a TrimBox is preferred.";
            sink.push(ctx.found(Some(number), FindingTarget::Page, m));
        }
        "PXQ-AN-1" | "PXQ-AN-2" | "PXQ-AN-3" => {
            for annot in page_annotations(p.view.graph(), page.id) {
                annotation(ctx, page, number, &annot, sink);
            }
        }
        _ => {}
    }
}

fn written(r: BoxResolution) -> bool {
    r != BoxResolution::Defaulted
}

/// The finished page: the trim box, else the art box; `None` when the
/// file wrote neither.
fn finished(page: &Page) -> Option<(Rect, BoxResolution, &'static str)> {
    if written(page.trim_box_resolution) {
        Some((page.trim_box, page.trim_box_resolution, "trim box"))
    } else if written(page.art_box_resolution) {
        Some((page.art_box, page.art_box_resolution, "art box"))
    } else {
        None
    }
}

/// The finished page lies inside `outer`.
fn inside(
    ctx: &Ctx<'_>,
    page: &Page,
    number: usize,
    outer: Rect,
    outer_name: &str,
    sink: &mut Sink,
) {
    let Some((rect, resolution, name)) = finished(page) else {
        return;
    };
    let message = if resolution == BoxResolution::Clipped {
        format!("The {name} extends beyond the media box, and so beyond the {outer_name}.")
    } else if !outer.contains(&rect) {
        format!("The {name} extends beyond the {outer_name}.")
    } else {
        return;
    };
    sink.push(crate::preflight::Finding {
        rect: Some(rect),
        ..ctx.found(Some(number), FindingTarget::Page, &message)
    });
}

fn annotation(ctx: &Ctx<'_>, page: &Page, number: usize, annot: &Annotation, sink: &mut Sink) {
    let subtype = String::from_utf8_lossy(&annot.subtype).into_owned();
    let mark = annot.subtype == b"PrinterMark";
    let target = annot
        .id
        .map_or(FindingTarget::Page, FindingTarget::Annotation);
    let (region, region_name) = match ctx.row.id {
        "PXQ-AN-3" => {
            if MEDIA.contains(&annot.subtype.as_slice()) {
                let m = format!("A /{subtype} annotation is not allowed.");
                sink.push(ctx.found(Some(number), target, &m));
            }
            return;
        }
        "PXQ-AN-2" if mark => match finished(page) {
            Some((rect, _, name)) => (rect, name),
            None => (page.trim_box, "trim box"),
        },
        "PXQ-AN-1" if !mark && ctx.level == PdfxLevel::X4 => match finished(page) {
            Some((rect, _, _)) => (rect, "trim or art box (geometry NOT SOURCED for PDF/X-4)"),
            None => (page.trim_box, "trim box (geometry NOT SOURCED for PDF/X-4)"),
        },
        "PXQ-AN-1" if !mark => (page.bleed_box, "bleed box"),
        _ => return,
    };
    let Some(rect) = annot.rect else {
        let m = format!("a /{subtype} annotation has no usable /Rect");
        sink.push(ctx.unread(Some(number), &m));
        return;
    };
    if rect.intersection(&region).is_none() {
        return;
    }
    let flags = annot.flags.0;
    let printing = if flags & AnnotFlags::HIDDEN != 0 {
        "it is hidden"
    } else if flags & AnnotFlags::PRINT != 0 {
        "it is set to print"
    } else {
        "it is not set to print"
    };
    let m = format!("A /{subtype} annotation lies inside the {region_name}; {printing}.");
    sink.push(crate::preflight::Finding {
        rect: Some(rect),
        ..ctx.found(Some(number), target, &m)
    });
}
