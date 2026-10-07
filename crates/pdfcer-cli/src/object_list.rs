//! `object-list`: one page's vector objects, form leaves and headless
//! hit / line-pick queries (read-only).

use super::*;
use pdfcer_core::vector::{
    DocumentImageAlpha, FormLeaf, HitTarget, ImageAlpha, Matrix, NoImageAlpha, PageObjects, Point,
    decompose_page, hit_test_point_all_with, hit_test_point_deep, hit_test_point_deep_with,
    hit_test_subpaths, subpath_bounds,
};

/// Grouped arguments for `object-list` — a struct to keep the handler under
/// clippy's `too_many_arguments` bar, like the editing subcommands.
pub(crate) struct ObjectListArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page_number: u32,
    pub(crate) hit: Option<&'a str>,
    pub(crate) all_hits: bool,
    pub(crate) hit_scope: HitScope,
    pub(crate) line_pick: Option<&'a str>,
    pub(crate) enter: Option<usize>,
    pub(crate) tolerance: f64,
    pub(crate) image_alpha: ImageAlphaArg,
}

/// `object-list` — inventory one page's vector objects in paint order, and
/// optionally answer a headless hit-test query (read-only).
///
/// ## Contract
///
/// - Emits one `object page=… index=… kind=… bbox=… …` line per object, in
///   paint order (index 0 painted first, so the LAST line is topmost).
/// - Emits a `hit …` line iff `--hit` was supplied.
/// - Emits one `hit-candidate page=… ordinal=… index=… kind=…` line per
///   object under the point, front-most first (`ordinal=0` IS the `hit`
///   line's object), iff `--hit` **and** `--all-hits` were supplied. The
///   prefix is `hit-candidate`, deliberately not `hit`, so a script already
///   matching `^hit ` keeps matching exactly one line.
/// - Emits an `object-list …` summary line last. Its `undecoded_colour=`,
///   `invisible_by_alpha=`, `shadings_unmodelled=` and `oc_sections=` count
///   the ways the rows can differ from the rendered page; each non-zero one
///   is also named on stderr.
/// - `--hit` honours image transparency by default (`image_alpha=` on the
///   `hit` line), as the GUI's click does; `--image-alpha ignore` tests
///   geometry only.
/// - Exit `SUCCESS` (0) on a readable page — including when the page has no
///   objects, and including when `--hit` MISSES. A miss is a valid answer,
///   not a failure; scripts read the `index=` field (`none` on a miss)
///   rather than the exit code.
/// - Exit `RUNTIME_ERROR` (1) for an out-of-range/zero `--page`, a
///   malformed `--hit`, an unreadable page tree, or content that will not
///   tokenize. Exit `IO_ERROR`/`NOT_A_PDF` per [`exit_code_for_doc`] for a
///   file that will not load.
///
/// ## Why the hit-test is here and not reimplemented
///
/// It calls [`pdfcer_core::vector::hit_test_point_deep_with`] on the model
/// [`pdfcer_core::vector::decompose_page`] returned — byte for byte the path
/// `pdfce-gui`'s `ObjectModelProvider::hit_test` takes after it converts the
/// pointer out of canvas space. That makes this subcommand a *diagnostic
/// oracle* for GUI selection: if `--hit` reports an index headlessly and a
/// click at the corresponding screen position does not select, the defect is
/// in the GUI's input/coordinate path, not in core's geometry.
///
/// `--all-hits` extends that oracle role to the one GUI behaviour a topmost
/// query cannot explain: click-through cycling. It calls
/// [`pdfcer_core::vector::hit_test_point_all_with`], which is the same function
/// the GUI provider's `hit_test_all` calls and whose head is, by
/// construction, `hit_test_point`'s answer — so `ordinal=0` always names the
/// same object as the `hit` line, and the rest of the list is exactly what
/// repeated Alt+clicks walk through.
pub(crate) fn cmd_object_list(args: ObjectListArgs<'_>) -> u8 {
    let (hit_point, line_pick_point) =
        match parse_queries(args.input, args.hit, args.line_pick, args.tolerance) {
            Ok(points) => points,
            Err(code) => return code,
        };
    let (doc, model) = match load_page_model(args.input, args.page_number) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    let page = args.page_number;
    let counts = print_object_rows(&model, page);
    print_leaf_rows(&model, page);
    if let Some(point) = hit_point {
        let view = doc.view();
        let alpha = DocumentImageAlpha::new(&view);
        let alpha: &dyn ImageAlpha = match args.image_alpha {
            ImageAlphaArg::Honour => &alpha,
            ImageAlphaArg::Ignore => &NoImageAlpha,
        };
        print_hit(&model, page, point, &args, alpha);
    }
    if let Some(point) = line_pick_point {
        print_line_pick(&model, page, point, args.tolerance);
    }
    if let (Some(point), Some(index)) = (hit_point, args.enter) {
        print_subpath_hits(&model, page, point, index, args.tolerance);
    }
    print_summary(args.input, &model, page, counts);
    exit::SUCCESS
}

/// Validate `--hit`, `--line-pick` and `--tolerance` before the document is
/// loaded, so a typo fails identically whether or not the file is readable
/// and no `object` row is printed ahead of a refusal.
fn parse_queries(
    input: &Path,
    hit: Option<&str>,
    line_pick: Option<&str>,
    tolerance: f64,
) -> Result<(Option<Point>, Option<Point>), u8> {
    let parse = |flag: &str, raw: Option<&str>| -> Result<Option<Point>, u8> {
        let Some(raw) = raw else {
            return Ok(None);
        };
        parse_hit_point(raw).map(Some).ok_or_else(|| {
            eprintln!(
                "pdfcer: {}: malformed --{flag} `{raw}` (expected `X,Y` in PDF user space, \
e.g. `--{flag} 200,200`)",
                input.display()
            );
            exit::RUNTIME_ERROR
        })
    };
    let hit_point = parse("hit", hit)?;
    let line_pick_point = parse("line-pick", line_pick)?;
    // NaN or a negative tolerance would make every query a miss, which reads
    // as "hit-testing is broken"; refuse it by name (rule 4).
    if (hit_point.is_some() || line_pick_point.is_some())
        && (!tolerance.is_finite() || tolerance < 0.0)
    {
        eprintln!(
            "pdfcer: {}: --tolerance must be a finite, non-negative number of points \
(got `{tolerance}`)",
            input.display()
        );
        return Err(exit::RUNTIME_ERROR);
    }
    Ok((hit_point, line_pick_point))
}

/// Open `input` and decompose its 1-based page `page_number` under the
/// identity CTM, the initial CTM the GUI provider also passes.
fn load_page_model(input: &Path, page_number: u32) -> Result<(Document, PageObjects), u8> {
    let doc = open_for_read(input)?;
    let pages = pdfcer_core::page_tree::pages(&doc).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", input.display());
        exit::RUNTIME_ERROR
    })?;
    let Some(page) = page_number
        .checked_sub(1)
        .and_then(|i| pages.get(i as usize))
    else {
        eprintln!(
            "pdfcer: {}: page {page_number} is out of range (document has {} page(s), \
numbered 1..={})",
            input.display(),
            pages.len(),
            pages.len()
        );
        return Err(exit::RUNTIME_ERROR);
    };
    let model = decompose_page(&doc.view(), page, Matrix::IDENTITY).map_err(|err| {
        eprintln!("pdfcer: {}: page {page_number}: {err}", input.display());
        exit::RUNTIME_ERROR
    })?;
    Ok((doc, model))
}

/// Per-kind object counts for the summary line.
#[derive(Default, Clone, Copy)]
struct KindCounts {
    paths: usize,
    text: usize,
    images: usize,
    forms: usize,
}

fn print_object_rows(model: &PageObjects, page: u32) -> KindCounts {
    let mut c = KindCounts::default();
    for (index, obj) in model.objects.iter().enumerate() {
        let (kind, detail) = object_detail(obj);
        match kind {
            "path" => c.paths += 1,
            "text" => c.text += 1,
            "form" => c.forms += 1,
            _ => c.images += 1,
        }
        println!(
            "object page={page} index={index} kind={kind} bbox={} oc={} {detail}",
            bbox_token(obj.page_bbox()),
            obj.oc()
                .map_or_else(|| "none".to_owned(), |id| id.num.to_string()),
        );
    }
    c
}

fn containment_token(leaf: &FormLeaf) -> String {
    leaf.containment
        .iter()
        .map(|id| id.num.to_string())
        .collect::<Vec<_>>()
        .join(">")
}

/// The objects inside form XObjects. A separate line type, not more `object`
/// rows: an `object` index is what the editing subcommands take and they
/// write to the PAGE's stream, while a leaf's tokens index the FORM's stream.
/// `editable=` asks the leaf, since the form-scoped verbs can edit some.
fn print_leaf_rows(model: &PageObjects, page: u32) {
    for (index, leaf) in model.leaves.iter().enumerate() {
        let (kind, detail) = object_detail(&leaf.object);
        let p = leaf.placement;
        println!(
            "leaf page={page} index={index} kind={kind} bbox={} containment={} \
paint_order={} in_form_index={} placement={},{},{},{},{},{} editable={} {detail}",
            bbox_token(leaf.object.page_bbox()),
            containment_token(leaf),
            leaf.paint_order,
            leaf.form_object_index,
            p.a,
            p.b,
            p.c,
            p.d,
            p.e,
            p.f,
            u32::from(leaf.is_editable()),
        );
    }
}

/// `(locator, kind, bbox)` for one hit target. `index=` for a page object and
/// `leaf=` for a form leaf: a different key, because a leaf ordinal under
/// `index=` would be in range for the page-editing verbs and corrupt the page.
fn describe_target(model: &PageObjects, t: HitTarget) -> (String, String, String) {
    match t {
        HitTarget::Object(i) => model.objects.get(i).map_or_else(
            || (format!("index={i}"), "none".to_owned(), "none".to_owned()),
            |o| {
                (
                    format!("index={i}"),
                    object_detail(o).0.to_owned(),
                    bbox_token(o.page_bbox()),
                )
            },
        ),
        HitTarget::Leaf(i) => model.leaves.get(i).map_or_else(
            || (format!("leaf={i}"), "none".to_owned(), "none".to_owned()),
            |leaf| {
                (
                    format!(
                        "leaf={i} containment={} paint_order={} editable=false",
                        containment_token(leaf),
                        leaf.paint_order
                    ),
                    format!("leaf:{}", object_detail(&leaf.object).0),
                    bbox_token(leaf.object.page_bbox()),
                )
            },
        ),
    }
}

/// The `hit` line and, with `--all-hits`, its `hit-candidate` rows — from ONE
/// query, so `candidates=` and the rows cannot disagree. `--hit-scope deep`
/// (the default) follows the GUI: forms are never candidates, a `/BBox` being
/// a clipping extent (ISO 32000-1 §8.10.1), not ink. `--image-alpha honour`
/// (the default) also follows the GUI: a click on a fully transparent image
/// sample falls through (§8.9.6, §11.6.5.3).
fn print_hit(
    model: &PageObjects,
    page: u32,
    point: Point,
    args: &ObjectListArgs<'_>,
    alpha: &dyn ImageAlpha,
) {
    let tolerance = args.tolerance;
    let deep = matches!(args.hit_scope, HitScope::Deep);
    let candidates: Vec<HitTarget> = if deep {
        hit_test_point_deep_with(model, point, tolerance, alpha)
    } else {
        hit_test_point_all_with(model, point, tolerance, alpha)
            .into_iter()
            .map(HitTarget::Object)
            .collect()
    };
    let (locator, kind) = candidates.first().map_or_else(
        || ("index=none".to_owned(), "none".to_owned()),
        |&t| {
            let (l, k, _) = describe_target(model, t);
            (l, k)
        },
    );
    println!(
        "hit page={page} at={},{} tolerance={tolerance} scope={} {locator} kind={kind} \
candidates={} image_alpha={}",
        point.x,
        point.y,
        if deep { "deep" } else { "page" },
        candidates.len(),
        args.image_alpha.as_str(),
    );
    if args.all_hits {
        // Front-most first: `ordinal=0` is the `hit` line's target; each
        // higher ordinal is one more Alt+click down the stack.
        for (ordinal, &t) in candidates.iter().enumerate() {
            let (locator, kind, bbox) = describe_target(model, t);
            println!(
                "hit-candidate page={page} at={},{} ordinal={ordinal} {locator} kind={kind} \
bbox={bbox}",
                point.x, point.y,
            );
        }
    }
}

/// Which straight line a click resolves to — the headless twin of the
/// two-line measure gesture, over page objects and form leaves. A miss is an
/// answer (`index=none`, exit 0); `reason=` separates "nothing near" from
/// "only a curve near", which pdfcer never chords into a line.
fn print_line_pick(model: &PageObjects, page: u32, point: Point, tolerance: f64) {
    let Some(line) = pdfcer_core::vector::linepick::pick_line_in_page(model, point, tolerance)
    else {
        let near_curve = !hit_test_point_deep(model, point, tolerance).is_empty();
        println!(
            "line-pick page={page} at={},{} tolerance={tolerance} index=none reason={}",
            point.x,
            point.y,
            if near_curve {
                "nothing-straight-within-tolerance"
            } else {
                "nothing-within-tolerance"
            },
        );
        return;
    };
    let target = match line.target {
        HitTarget::Object(i) => format!("index={i}"),
        HitTarget::Leaf(i) => model.leaves.get(i).map_or_else(
            || format!("leaf={i}"),
            |leaf| {
                format!(
                    "leaf={i} containment={} paint_order={}",
                    containment_token(leaf),
                    leaf.paint_order
                )
            },
        ),
    };
    println!(
        "line-pick page={page} at={},{} tolerance={tolerance} {target} \
subpath={} segment={} start={},{} end={},{} pick={},{} length={}",
        point.x,
        point.y,
        line.subpath,
        line.segment,
        line.start.x,
        line.start.y,
        line.end.x,
        line.end.y,
        line.pick.x,
        line.pick.y,
        line.length(),
    );
}

/// Which subpaths of object `index` the `--hit` point lands on, nearest
/// first. Silent for a non-path or out-of-range index, so a script may pass
/// `--enter` unconditionally.
fn print_subpath_hits(model: &PageObjects, page: u32, point: Point, index: usize, tolerance: f64) {
    for (ordinal, sp) in hit_test_subpaths(model, index, point, tolerance)
        .into_iter()
        .enumerate()
    {
        println!(
            "subpath-hit page={page} object={index} ordinal={ordinal} subpath={sp} bbox={}",
            subpath_bounds(model, index, sp).map_or_else(
                || "none".to_owned(),
                |b| format!("{},{},{},{}", b.min.x, b.min.y, b.max.x, b.max.y)
            ),
        );
    }
}

/// The stable `object-list` summary line, then a stderr note for each way the
/// rows above can differ from what the page renders (rule 4).
fn print_summary(input: &Path, model: &PageObjects, page: u32, c: KindCounts) {
    let d = &model.diagnostics;
    println!(
        "object-list {} page={page} objects={} paths={} text={} images={} forms={} \
dropped_objects={} dropped_nodes={} leaves={} form_cycles={} form_depth_overflows={} \
undecoded_colour={} invisible_by_alpha={} shadings_unmodelled={} oc_sections={}",
        input.display(),
        model.objects.len(),
        c.paths,
        c.text,
        c.images,
        c.forms,
        d.objects_dropped,
        d.nodes_dropped,
        model.leaves.len(),
        d.form_cycles,
        d.form_depth_overflows,
        d.paths_with_undecoded_colour,
        d.paths_invisible_by_alpha,
        d.shadings_unmodelled,
        d.oc_sections,
    );
    if d.form_cycles > 0 || d.form_depth_overflows > 0 {
        eprintln!(
            "pdfcer: {}: the leaf list is INCOMPLETE. {} form invocation(s) were skipped as \
cycles (a form that invokes itself, directly or through a chain -- legal under ISO 32000-1 \
§8.10.1, merely unbounded) and {} because the nesting exceeded {} levels. Objects inside those \
forms are not listed above.",
            input.display(),
            d.form_cycles,
            d.form_depth_overflows,
            pdfcer_core::content::MAX_FORM_DEPTH,
        );
    }
    for (count, what) in [
        (
            d.paths_with_undecoded_colour,
            "path(s) are painted in a colour space this list does not decode; their colour \
fields are not the rendered colour",
        ),
        (
            d.paths_invisible_by_alpha,
            "path(s) are listed but painted invisible by an /ExtGState alpha of 0",
        ),
        (
            d.shadings_unmodelled,
            "`sh` shading fill(s) are painted but have no object row",
        ),
        (
            d.oc_sections,
            "optional-content section(s) are listed whether or not their layer is visible",
        ),
    ] {
        if count > 0 {
            eprintln!("pdfcer: {}: page {page}: {count} {what}.", input.display());
        }
    }
}
