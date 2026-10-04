//! `add-svg` — place an SVG drawing on a page as vector content.

use super::*;

/// How `add-svg` and `add-emf` fit the drawing into `--rect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SvgFit {
    /// Keep the aspect ratio, centred in the rectangle (the default).
    Contain,
    /// Fill the rectangle exactly.
    Stretch,
    /// The drawing's own size (an SVG at 96 px per inch, an EMF's picture
    /// frame), lower-left at the rectangle's.
    Natural,
}

/// The arguments of `add-svg`, borrowed from the parsed command.
// Without `svg-import` the command refuses before reading most fields.
#[cfg_attr(not(feature = "svg-import"), allow(dead_code))]
pub(crate) struct AddSvgArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) svg: &'a Path,
    pub(crate) page: usize,
    pub(crate) rect: &'a str,
    pub(crate) fit: SvgFit,
    pub(crate) stamp: bool,
    pub(crate) markup: StampMarkup<'a>,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `add-svg` — import an SVG/SVGZ and draw it as a Form XObject of vector
/// operators (or, with `--stamp`, as a `/Stamp` annotation's appearance).
///
/// ## Contract
///
/// - Emits one `add-svg …` line on stdout with every disclosure as a field
///   (`not_carried=`, `approximated=`, `distorted=`), the prose form on
///   stderr, then defers the exit code to [`finish_edit`].
/// - A file that will not import (too large, not UTF-8, bad XML, nested too
///   deep, a gzip bomb) is refused with exit 9 before the PDF is opened.
/// - `--page` is 1-based.
pub(crate) fn cmd_add_svg(args: &AddSvgArgs<'_>) -> u8 {
    let (page_index, requested) = match parse_page_and_rect(args.input, args.page, args.rect) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let bytes = match std::fs::read(args.svg) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.svg.display());
            return exit::IO_ERROR;
        }
    };
    place_svg(args, page_index, requested, &bytes)
}

#[cfg(feature = "svg-import")]
fn place_svg(
    args: &AddSvgArgs<'_>,
    page_index: usize,
    requested: pdfcer_core::page_tree::Rect,
    bytes: &[u8],
) -> u8 {
    let svg = match pdfcer_core::svg_import::import(bytes) {
        Ok(svg) => svg,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.svg.display());
            return exit::EDIT_REFUSED;
        }
    };
    let rect = fit_rect(requested, svg.natural_size_pt(), args.fit);
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let result = if args.stamp {
        match args.markup.options(args.input, &session) {
            Ok(options) => session.add_svg_stamp(page_index, rect, &svg, &options),
            Err(code) => return code,
        }
    } else {
        session.add_svg(page_index, rect, &svg)
    };
    let placed = match result {
        Ok(placed) => placed,
        Err(err) => return report_edit_error(args.input, &err),
    };
    if !placed.notes.is_empty() {
        eprintln!(
            "pdfcer: {}: placed, but {}",
            args.svg.display(),
            placed.notes.summary()
        );
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    print_outcome(args, &svg, &placed, &outcome);
    finish_edit(args.input, &outcome)
}

#[cfg(feature = "svg-import")]
fn print_outcome(
    args: &AddSvgArgs<'_>,
    svg: &pdfcer_core::svg_import::ImportedSvg,
    placed: &pdfcer_core::edit::PlacedSvg,
    outcome: &EditOutcome,
) {
    let features = |m: &std::collections::BTreeMap<pdfcer_core::svg_import::SvgFeature, usize>| {
        if m.is_empty() {
            "-".to_owned()
        } else {
            m.iter()
                .map(|(f, n)| format!("{}:{n}", f.name().replace(' ', "_")))
                .collect::<Vec<_>>()
                .join(",")
        }
    };
    let (w, h) = svg.size_px();
    let p = placed.rect;
    let r = &outcome.report;
    let id = |o: Option<pdfcer_core::object::ObjId>| {
        o.map_or_else(
            || "-".to_owned(),
            |id| format!("{} {}", id.num, id.generation),
        )
    };
    println!(
        "add-svg {} svg={} page={} size_px={w}x{h} placed={:.3},{:.3},{:.3},{:.3} \
         fit={} as={} form={} {} content={} annot={} scale={:.4},{:.4} distorted={} \
         svg_objects={} not_carried={} approximated={} mode={} -> {}; changed={} \
         objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.svg.display(),
        args.page,
        p.llx,
        p.lly,
        p.urx,
        p.ury,
        match args.fit {
            SvgFit::Contain => "contain",
            SvgFit::Stretch => "stretch",
            SvgFit::Natural => "natural",
        },
        if args.stamp { "stamp" } else { "content" },
        placed.form_id.num,
        placed.form_id.generation,
        id(placed.content_id),
        id(placed.annot_id),
        placed.scale_x,
        placed.scale_y,
        u32::from(placed.distorted),
        placed.objects_written,
        features(&placed.notes.skipped),
        features(&placed.notes.approximated),
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
}

#[cfg(not(feature = "svg-import"))]
fn place_svg(
    args: &AddSvgArgs<'_>,
    _page_index: usize,
    _requested: pdfcer_core::page_tree::Rect,
    _bytes: &[u8],
) -> u8 {
    eprintln!(
        "pdfcer: {}: this pdfcer was built without the `svg-import` feature; \
         add-svg is unavailable",
        args.svg.display()
    );
    exit::EDIT_REFUSED
}

/// The rectangle the drawing fills: `requested` itself (stretch), the
/// largest `natural`-shaped rectangle centred in it (contain), or the
/// natural size at its lower-left corner.
pub(crate) fn fit_rect(
    requested: pdfcer_core::page_tree::Rect,
    natural: (f64, f64),
    fit: SvgFit,
) -> pdfcer_core::page_tree::Rect {
    use pdfcer_core::page_tree::Rect;
    let r = Rect::from_corners(requested.llx, requested.lly, requested.urx, requested.ury);
    let (nw, nh) = natural;
    match fit {
        SvgFit::Stretch => r,
        SvgFit::Natural => Rect {
            llx: r.llx,
            lly: r.lly,
            urx: r.llx + nw,
            ury: r.lly + nh,
        },
        SvgFit::Contain => {
            let k = (r.width() / nw).min(r.height() / nh);
            if !k.is_finite() || k <= 0.0 {
                return r;
            }
            let (w, h) = (nw * k, nh * k);
            let llx = r.llx + (r.width() - w) / 2.0;
            let lly = r.lly + (r.height() - h) / 2.0;
            Rect {
                llx,
                lly,
                urx: llx + w,
                ury: lly + h,
            }
        }
    }
}
