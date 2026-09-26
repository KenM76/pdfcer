use super::*;

// =====================================================================
// `object-list` — paint-order object inventory + headless hit-test
// =====================================================================
//
// WHY this subcommand exists: `object-move`, `object-delete` and
// `node-move` all address an object by its 0-based paint-order index, and
// before this there was NO way — CLI or GUI — to discover that index. The
// editing subcommands' own help text pointed at "`object-list`-style
// tooling" that did not exist, so the three edits were effectively
// unusable outside a debugger. This closes that gap.
//
// It is deliberately read-only and deliberately thin: every number it
// prints comes from `pdfcer_core::vector::decompose_page` and
// `pdfcer_core::vector::hit_test_point` — the SAME two functions the GUI's
// `ObjectModelProvider` calls — so the listing cannot drift from what the
// edits address or from what a click in the GUI selects. Re-deriving
// either here would recreate exactly the "two decompositions quietly
// diverge" failure decision 011 Z2 warns against.

/// Default `--tolerance` for `object-list --hit`, in page-space points.
///
/// Chosen to equal `pdfce-gui`'s `object_provider::FALLBACK_SELECT_TOLERANCE`
/// (3.0), which is the canvas-space catch radius a click falls back to. At
/// 100% zoom the canvas is distance-preserving against page space (the
/// `page_device_geometry` scale-1.0 map is a pure rotation + Y-flip +
/// translation), so 3.0 pt here reproduces the GUI's 100%-zoom behaviour.
/// The GUI's *live* tolerance additionally scales as `1 / zoom` to hold the
/// on-screen radius constant; the CLI has no zoom, so it takes the value
/// literally and the operator overrides it when reproducing a zoomed click.
pub(crate) const HIT_TOLERANCE_PT: f64 = 3.0;

/// A stable one-token name for a path's paint disposition (ISO 32000-1
/// §8.5.3): what actually marks the page, which is also what decides how
/// [`pdfcer_core::vector::hit_test_point`] tests it — a filled path is hit
/// by its interior under its winding rule, a stroke-only path only by
/// proximity to its outline, and a `n` no-op/clip path only within the bare
/// tolerance. Printing it makes an otherwise-baffling hit-test result
/// ("I clicked inside it and missed") self-explaining.
pub(crate) fn paint_token(style: pdfcer_core::vector::PaintStyle) -> &'static str {
    use pdfcer_core::vector::FillRule;
    match (style.fill, style.stroke) {
        (Some(FillRule::NonZero), true) => "fill-nonzero+stroke",
        (Some(FillRule::NonZero), false) => "fill-nonzero",
        (Some(FillRule::EvenOdd), true) => "fill-evenodd+stroke",
        (Some(FillRule::EvenOdd), false) => "fill-evenodd",
        (None, true) => "stroke",
        // An `n` path: constructed, painted by nothing (a clip or a
        // discarded path). Still selectable, but only precisely.
        (None, false) => "none",
    }
}

/// How a text object's bbox was built, as a stable token — the CLI half of
/// ui-spec §E.3's requirement that a box's *provenance* be recoverable
/// wherever the box is shown.
///
/// `approximate=1` alone cannot answer the question a script (or an
/// operator diagnosing a missed click) actually has, because it is `1` for
/// every text object. These four tokens can:
///
/// | Token | Meaning |
/// |---|---|
/// | `font-metrics` | Advances from the font's own width table, height from its `/FontDescriptor`. The box is where a conforming reader lays the run out. |
/// | `metric-advances-nominal-height` | Advances real; no ascent/descent available, so the height is a nominal em (the Type 3 case). |
/// | `estimated-advances` | The font carried no width source at all, so the advances are estimated from metrically-similar Helvetica (§9.6.2.2 does not permit such a font; real files ship them). |
/// | `em-box` | No font resolved for at least one show operator: that part of the box is the legacy square around the run's START position, which reaches into blank paper before the text and stops short of its end. |
pub(crate) fn bounds_basis_token(basis: pdfcer_core::vector::TextBoundsBasis) -> &'static str {
    use pdfcer_core::vector::TextBoundsBasis;
    match basis {
        TextBoundsBasis::FontMetrics => "font-metrics",
        TextBoundsBasis::MetricAdvancesNominalHeight => "metric-advances-nominal-height",
        TextBoundsBasis::EstimatedAdvances => "estimated-advances",
        TextBoundsBasis::EmBox => "em-box",
    }
}

/// A page-space [`Bounds`](pdfcer_core::vector::Bounds) as the stable
/// `minx,miny,maxx,maxy` token, or `none` for a box that enclosed no finite
/// point.
///
/// A **zero-width or zero-height box is NOT `none`** — a horizontal rule or
/// a vertical rule legitimately has one degenerate axis (`min.y == max.y`),
/// and `Bounds::is_empty` is `min > max`, not `min == max`. Reporting such a
/// box as `none` would have made exactly the thin geometry this tool exists
/// to find look unlocatable.
/// Coordinates are printed at **four decimal places with trailing zeros
/// trimmed**, so `50.0` prints as `50` (as it always has) and a
/// metrics-derived text edge at `70.46000272035599` prints as `70.46`
/// rather than as seventeen digits of `f32`-widening artefact. Four
/// decimals is 1/10 000 of a PDF point — four orders of magnitude finer
/// than the hit-test tolerance that consumes these numbers, so the
/// rounding cannot change any answer this tool gives.
pub(crate) fn bbox_token(b: pdfcer_core::vector::Bounds) -> String {
    if b.is_empty() {
        "none".to_owned()
    } else {
        format!(
            "{},{},{},{}",
            coord_token(b.min.x),
            coord_token(b.min.y),
            coord_token(b.max.x),
            coord_token(b.max.y)
        )
    }
}

/// One page-space coordinate, at four decimal places with trailing zeros
/// (and a trailing `.`) trimmed — see [`bbox_token`].
pub(crate) fn coord_token(v: f64) -> String {
    let s = format!("{v:.4}");
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    // `-0` is the one output the trim can produce that reads as a defect
    // rather than as a number; it is a real f64 value, and `0` is the same
    // point.
    if trimmed == "-0" {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Quote a decoded string as a single `key="…"` token, so a value
/// containing spaces cannot be mistaken for the next field.
///
/// Every other field on an `object …` line is `key=value` with no quoting,
/// because every other value is a number or a fixed token. A text preview is
/// neither: it can contain spaces, quotes, backslashes, newlines and
/// arbitrary Unicode. The escaping is therefore stated exactly, so a script
/// can reverse it without guessing:
///
/// - `\` → `\\`, `"` → `\"` (the two characters that would otherwise break
///   the token's own delimiters);
/// - any character below U+0020, plus U+007F → `\xNN` with two lowercase hex
///   digits (a literal newline inside a line-oriented format is not
///   recoverable at all, and an invisible control character in a value a
///   human reads is worse than an escape they can see);
/// - everything else passes through as UTF-8, including non-ASCII text —
///   the CLI's output is UTF-8 and mangling `é` into an escape would make
///   the common non-English case unreadable for no safety gain.
pub(crate) fn quoted_token(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A text object's `text=` field: a quoted preview, or one of two bare
/// tokens that mean genuinely different things.
///
/// `none` and `undecodable` are unquoted, which is what makes them
/// unambiguous against a quoted string — a document really could contain the
/// literal text `undecodable`, and it would print as `text="undecodable"`.
///
/// The three answers, from
/// [`TextPreview`](pdfcer_core::vector::TextPreview):
///
/// | Field | Meaning |
/// |---|---|
/// | `text="…"` | Decoded. `lossy=1` on the same line if some codes still failed and are shown as U+FFFD. |
/// | `text=undecodable` | Codes were shown and **not one** could be mapped — ISO 32000-1 §9.10.2's failure clause for every code (the `Identity-H`-without-`/ToUnicode` case). A document fact, not a pdfcer limitation. |
/// | `text=none` | Nothing was shown, or no font resolver was in scope. |
pub(crate) fn text_preview_fields(preview: &pdfcer_core::vector::TextPreview) -> String {
    use pdfcer_core::vector::TextPreview;
    match preview {
        TextPreview::Decoded {
            text,
            truncated,
            lossy,
        } => format!(
            "text={} truncated={} lossy={}",
            quoted_token(text),
            u32::from(*truncated),
            u32::from(*lossy),
        ),
        TextPreview::Undecodable => "text=undecodable truncated=0 lossy=1".to_owned(),
        TextPreview::Unavailable | TextPreview::Empty => "text=none truncated=0 lossy=0".to_owned(),
    }
}

/// A text object's `font=`/`size=` fields.
///
/// `font=` is the **typeface** (`/BaseFont`, §9.6.2.1 Table 111) when the
/// font dictionary resolves, since that is what identifies the object to a
/// human; `resource=` always carries the `Tf` name (`F1`), which is the
/// handle a script or a later edit needs. Both, because they answer
/// different questions and neither substitutes for the other.
///
/// `size=` is the `Tf` size **operand as the file states it** — a text-space
/// quantity, not scaled by `Tm`/`cm`. See
/// [`TextFont::size`](pdfcer_core::vector::TextFont::size) for why folding
/// the matrices in would be a confident number that disagrees with the
/// content stream.
pub(crate) fn font_fields(font: Option<&pdfcer_core::vector::TextFont>) -> String {
    match font {
        None => "font=none resource=none size=none".to_owned(),
        Some(f) => format!(
            "font={} resource={} size={}",
            f.base_font
                .as_deref()
                .map_or_else(|| "none".to_owned(), quoted_token),
            quoted_token(&f.resource),
            f.size,
        ),
    }
}

/// One `object …` line's kind + kind-specific detail fields, for the object
/// at paint-order `index`.
///
/// Kinds are `path` / `text` / `image` / `form`. `image` and `form` are the
/// same [`VectorObject::Image`](pdfcer_core::vector::VectorObject) arm
/// discriminated by its [`ImageSource`](pdfcer_core::vector::ImageSource):
/// a Form XObject is reported separately because it is a *container* whose
/// contents were flattened into this same paint-order list, which materially
/// changes what deleting it does.
pub(crate) fn object_detail(obj: &pdfcer_core::vector::VectorObject) -> (&'static str, String) {
    use pdfcer_core::vector::{ImageSource, VectorObject};
    match obj {
        VectorObject::Path(p) => {
            // `anchors` is the count `node-move --node` indexes into: every
            // subpath's start plus each segment endpoint, in decomposition
            // order. Equal to `vector::anchor_count` by construction (that
            // function's own doc comment), derived here from the geometry so
            // no second content-stream walk is needed for a listing.
            let anchors: usize = p.subpaths.iter().map(|sp| sp.anchors().count()).sum();
            let closed = p.subpaths.iter().filter(|sp| sp.closed).count();
            (
                "path",
                format!(
                    "subpaths={} anchors={anchors} closed={closed} paint={} line_width={}",
                    p.subpaths.len(),
                    paint_token(p.style),
                    p.line_width,
                ),
            )
        }
        // `approximate=1` means the bbox is not measured glyph ink. It is
        // `1` for every text object and always will be until pdfcer reads
        // glyph outlines, so on its own it does not distinguish the good
        // case from the bad one — which is what `bounds=` is for.
        //
        // The `text=`/`font=`/`resource=`/`size=` fields are the CLI half of
        // ui-spec §B.4 #1 (rule 11): the GUI's object row and this line
        // describe one object from one `decompose_page` walk, so a script
        // and an operator looking at the same file read the same facts.
        VectorObject::Text(t) => (
            "text",
            format!(
                // `runs=` is the headless oracle for per-run hit-testing.
                // `bounds=` reports the ENCLOSING rectangle, which for a
                // producer that puts many labels in one BT..ET can span the
                // whole page while the ink covers almost none of it — so the
                // bounds field alone cannot tell an operator whether
                // selection will behave. The run count can: `runs=0` means
                // selection falls back to that enclosing box, `runs=N` means
                // it tests N real extents.
                "approximate={} bounds={} runs={} {} {}",
                u32::from(t.approximate),
                bounds_basis_token(t.bounds_basis),
                t.runs.len(),
                font_fields(t.font.as_ref()),
                text_preview_fields(&t.preview),
            ),
        ),
        VectorObject::Image(i) => {
            let kind = match i.source {
                ImageSource::Form => "form",
                ImageSource::Inline | ImageSource::XObject => "image",
            };
            let source = match i.source {
                ImageSource::Inline => "inline",
                ImageSource::XObject => "xobject",
                ImageSource::Form => "form",
            };
            // `pixels=WxH` is the SAMPLE count from `/Width`/`/Height`
            // (§8.9.5 Table 89) — not a size on the page, which is what
            // `bbox=` on the same line already gives. The pair is what lets
            // a script compute effective placed resolution. `none` for a
            // form XObject (no samples) and for a malformed image.
            let pixels = i
                .pixel_size
                .map_or_else(|| "none".to_owned(), |(w, h)| format!("{w}x{h}"));
            (kind, format!("source={source} pixels={pixels}"))
        }
    }
}

/// Parse a `--hit X,Y` operand into a page-space point.
///
/// Deliberately strict — a silently-misparsed coordinate would report a
/// confident wrong answer about which object a click selects, which is worse
/// than a refusal (rule 4: fuzzy, never sneaky). `None` on anything but
/// exactly two finite comma-separated numbers.
pub(crate) fn parse_hit_point(s: &str) -> Option<pdfcer_core::vector::Point> {
    let (x, y) = s.split_once(',')?;
    let x: f64 = x.trim().parse().ok()?;
    let y: f64 = y.trim().parse().ok()?;
    (x.is_finite() && y.is_finite()).then(|| pdfcer_core::vector::Point::new(x, y))
}

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
/// - Emits an `object-list …` summary line last.
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
/// It calls [`pdfcer_core::vector::hit_test_point`] on the model
/// [`pdfcer_core::vector::decompose_page`] returned — byte for byte the path
/// `pdfce-gui`'s `ObjectModelProvider::hit_test` takes after it converts the
/// pointer out of canvas space. That makes this subcommand a *diagnostic
/// oracle* for GUI selection: if `--hit` reports an index headlessly and a
/// click at the corresponding screen position does not select, the defect is
/// in the GUI's input/coordinate path, not in core's geometry.
///
/// `--all-hits` extends that oracle role to the one GUI behaviour a topmost
/// query cannot explain: click-through cycling. It calls
/// [`pdfcer_core::vector::hit_test_point_all`], which is the same function
/// the GUI provider's `hit_test_all` calls and whose head is, by
/// construction, `hit_test_point`'s answer — so `ordinal=0` always names the
/// same object as the `hit` line, and the rest of the list is exactly what
/// repeated Alt+clicks walk through.
pub(crate) fn cmd_object_list(args: ObjectListArgs<'_>) -> u8 {
    use pdfcer_core::vector::{
        HitTarget, Matrix, decompose_page, hit_test_point_all, hit_test_point_deep,
        hit_test_subpaths, subpath_bounds,
    };

    let ObjectListArgs {
        input,
        page_number,
        hit,
        all_hits,
        hit_scope,
        line_pick,
        enter,
        tolerance,
    } = args;

    // Validate the query operands BEFORE loading the document: a typo
    // should fail immediately and identically whether or not the file
    // happens to be readable, and — critically — before any `object` rows
    // are printed, so a refusal never leaves half an answer on stdout.
    let hit_point = match hit {
        None => None,
        Some(raw) => match parse_hit_point(raw) {
            Some(p) => Some(p),
            None => {
                eprintln!(
                    "pdfcer: {}: malformed --hit `{raw}` (expected `X,Y` in PDF user space, \
e.g. `--hit 200,200`)",
                    input.display()
                );
                return exit::RUNTIME_ERROR;
            }
        },
    };
    let line_pick_point = match line_pick {
        None => None,
        Some(raw) => match parse_hit_point(raw) {
            Some(p) => Some(p),
            None => {
                eprintln!(
                    "pdfcer: {}: malformed --line-pick `{raw}` (expected `X,Y` in PDF user space, e.g. `--line-pick 200,200`)",
                    input.display()
                );
                return exit::RUNTIME_ERROR;
            }
        },
    };
    // `--tolerance` is parsed by clap as a bare f64, so `nan` and negatives
    // both arrive intact. Either would make EVERY query a miss, which reads
    // as "hit-testing is broken" rather than "you passed nonsense" — refuse
    // by name instead (rule 4: fuzzy, never sneaky).
    if (hit_point.is_some() || line_pick_point.is_some())
        && (!tolerance.is_finite() || tolerance < 0.0)
    {
        eprintln!(
            "pdfcer: {}: --tolerance must be a finite, non-negative number of points \
(got `{tolerance}`)",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }

    let doc = match open_for_read(input) {
        Ok(doc) => doc,
        Err(code) => return code,
    };
    let pages = match pdfcer_core::page_tree::pages(&doc) {
        Ok(pages) => pages,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    // 1-based → 0-based, matching every other `--page` subcommand.
    // `checked_sub` absorbs `--page 0` without wrapping; `get` absorbs
    // past-the-end. Both land on one message, as `render-page` does.
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
        return exit::RUNTIME_ERROR;
    };

    // `Matrix::IDENTITY` is the initial CTM the GUI provider also passes, so
    // the coordinates printed here are the page space every other page-space
    // operand in this CLI uses.
    let model = match decompose_page(&doc.view(), page, Matrix::IDENTITY) {
        Ok(model) => model,
        Err(err) => {
            eprintln!("pdfcer: {}: page {page_number}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };

    let (mut paths, mut text, mut images, mut forms) = (0usize, 0usize, 0usize, 0usize);
    for (index, obj) in model.objects.iter().enumerate() {
        let (kind, detail) = object_detail(obj);
        match kind {
            "path" => paths += 1,
            "text" => text += 1,
            "form" => forms += 1,
            _ => images += 1,
        }
        println!(
            "object page={page_number} index={index} kind={kind} bbox={} {detail}",
            bbox_token(obj.page_bbox()),
        );
    }

    // THE OBJECTS INSIDE FORM XOBJECTS, which the rows above cannot show.
    //
    // A form is emitted above as ONE object bounded by its `/BBox`, so on a
    // page whose body is wrapped in a form -- a CAD sheet's orthographic view,
    // a print panel -- every `object` row above is furniture and everything
    // the operator came for is in here.
    //
    // A SEPARATE line type, not more `object` rows, and the reason is not
    // cosmetic: an `object` index is what the editing subcommands take, and
    // every one of them writes to the PAGE's content stream. A leaf's tokens
    // index the FORM's stream -- a different buffer. Printing leaves as
    // `object` rows would hand a script indices that corrupt the page when
    // used, silently, because they are in range. `editable=false` says so on
    // every row.
    for (index, leaf) in model.leaves.iter().enumerate() {
        let (kind, detail) = object_detail(&leaf.object);
        let containment = leaf
            .containment
            .iter()
            .map(|id| id.num.to_string())
            .collect::<Vec<_>>()
            .join(">");
        // `editable=` used to be the literal `false`, which was true when this
        // row was written and became a FALSE CLAIM IN SHIPPED OUTPUT the day
        // `Pass 188.0` gave the geometry verbs form-scoped twins. A hard-coded
        // field is the kind that goes stale silently: nothing type-checks a
        // string, and the row kept printing an answer the engine had stopped
        // giving. It asks the leaf now.
        //
        // `in_form_index=` and `placement=` are printed because they are what
        // `--leaf` addressing is built on, and an operator who cannot see them
        // cannot check that a refused edit was refused for the reason claimed.
        let p = leaf.placement;
        println!(
            "leaf page={page_number} index={index} kind={kind} bbox={} containment={containment} \
paint_order={} in_form_index={} placement={},{},{},{},{},{} editable={} {detail}",
            bbox_token(leaf.object.page_bbox()),
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

    if let Some(point) = hit_point {
        // The tolerance is passed through verbatim so the operator can
        // reproduce any zoom's catch radius; a non-finite or negative value
        // would make every query a miss, which reads as "hit-testing is
        // broken", so refuse it by name instead.
        if !tolerance.is_finite() || tolerance < 0.0 {
            eprintln!(
                "pdfcer: {}: --tolerance must be a finite, non-negative number of points",
                input.display()
            );
            return exit::RUNTIME_ERROR;
        }
        // ONE query answers both lines. `hit_test_point` is defined as this
        // list's head (see `pdfcer_core::vector::hit`), so calling it as well
        // would be a second scan that could only ever agree — and a second
        // scan that CAN disagree is exactly the divergence decision 011 §Z2
        // names. `candidates=` on the `hit` line is therefore always
        // consistent with the `hit-candidate` lines below it.
        // DEEP BY DEFAULT SINCE `Pass 138.0`, AND THAT IS A BEHAVIOUR
        // CHANGE THIS BLOCK OWES THE READER AN ACCOUNT OF.
        //
        // This flag's own help text promised the answer was "authoritative
        // for the GUI's behaviour rather than a second implementation of
        // it". That sentence was true when written and became FALSE the day
        // `hit_test_point_deep` shipped and the shell consumed it -- measured
        // by the consuming project on a composite conformance page, where
        // `--hit` returned two candidates, BOTH forms, at a point where the
        // shell selects the path actually painted there.
        //
        // A diagnostic that disagrees with the thing it diagnoses is worse
        // than no diagnostic: it confirms defects that are not there and
        // fails to confirm ones that are. So the default follows the shell,
        // and `--hit-scope page` keeps the old query for whoever wants it.
        //
        // The visible consequence, which a script will meet first: FORMS
        // DISAPPEAR FROM THE CANDIDATE LIST. A `/BBox` is a clipping extent
        // (ISO 32000-1 8.10.1), not ink, so a page-sized form is not a
        // page-sized hit target. What is inside it appears instead, as
        // `kind=leaf:*` rows carrying `leaf=` rather than `index=`.
        let deep = matches!(hit_scope, HitScope::Deep);
        let kind_of = |i: usize| -> String {
            model
                .objects
                .get(i)
                .map_or("none", |obj| object_detail(obj).0)
                .to_owned()
        };
        // ONE list for both modes, so `candidates=` on the `hit` line and the
        // `hit-candidate` rows below it cannot disagree -- the identity the
        // shallow path already relied on, extended rather than forked.
        let candidates: Vec<HitTarget> = if deep {
            hit_test_point_deep(&model, point, tolerance)
        } else {
            hit_test_point_all(&model, point, tolerance)
                .into_iter()
                .map(HitTarget::Object)
                .collect()
        };
        // `index=` for a page object, `leaf=` for a form leaf. Deliberately a
        // DIFFERENT KEY rather than one `index=` over two namespaces: an
        // `index=` is what the editing subcommands take, and every one of
        // them writes to the PAGE's content stream, so handing a script a
        // leaf ordinal under that key would give it a number that is in range
        // and corrupts the page. The `leaf` rows above make the same choice
        // for the same reason.
        let describe = |t: HitTarget| -> (String, String, String) {
            match t {
                HitTarget::Object(i) => (
                    format!("index={i}"),
                    kind_of(i),
                    model
                        .objects
                        .get(i)
                        .map_or_else(|| "none".to_owned(), |o| bbox_token(o.page_bbox())),
                ),
                HitTarget::Leaf(i) => model.leaves.get(i).map_or_else(
                    || (format!("leaf={i}"), "none".to_owned(), "none".to_owned()),
                    |leaf| {
                        let containment = leaf
                            .containment
                            .iter()
                            .map(|id| id.num.to_string())
                            .collect::<Vec<_>>()
                            .join(">");
                        (
                            format!(
                                "leaf={i} containment={containment} paint_order={} editable=false",
                                leaf.paint_order
                            ),
                            format!("leaf:{}", object_detail(&leaf.object).0),
                            bbox_token(leaf.object.page_bbox()),
                        )
                    },
                ),
            }
        };
        let (locator, kind) = candidates.first().map_or_else(
            || ("index=none".to_owned(), "none".to_owned()),
            |&t| {
                let (l, k, _) = describe(t);
                (l, k)
            },
        );
        println!(
            "hit page={page_number} at={},{} tolerance={tolerance} scope={} {locator} kind={kind} candidates={}",
            point.x,
            point.y,
            if deep { "deep" } else { "page" },
            candidates.len(),
        );
        if all_hits {
            // Front-most first: `ordinal=0` is the target the `hit` line
            // names and the one a plain click selects; each higher ordinal is
            // one more Alt+click down the stack, wrapping back to 0 after the
            // last.
            for (ordinal, &t) in candidates.iter().enumerate() {
                let (locator, kind, bbox) = describe(t);
                println!(
                    "hit-candidate page={page_number} at={},{} ordinal={ordinal} {locator} kind={kind} bbox={bbox}",
                    point.x, point.y,
                );
            }
        }
    }

    // WHICH STRAIGHT LINE A CLICK WOULD RESOLVE TO, `Pass 138.0`.
    //
    // The headless twin of the two-line measure gesture. It exists for two
    // reasons and the second is the load-bearing one:
    //
    // 1. `--hit` answers "which OBJECT", and an object on a CAD sheet is
    //    routinely a whole orthographic view -- one measured export holds
    //    1194 subpaths in a single object. "Which object" is not an answer to
    //    "which line did I click", and the measure tool needs the second.
    //
    // 2. Until now `pdfcer_core::vector::linepick::pick_line_in_page` had NO
    //    CLI caller at all, so the only way to observe it was to run the GUI
    //    and place a dimension. A core verb with no headless surface cannot
    //    be regression-tested by a script, cannot be diagnosed on an operator's
    //    file without a screen, and -- as this project has had to write down
    //    twice -- tends to sit callable-and-uncalled while everyone assumes
    //    somebody exercises it.
    //
    // Deliberately read-only and deliberately NOT wired into `dimension-add`:
    // that subcommand takes explicit coordinates, which is the right contract
    // for a script, and giving it a pick would make a batch run depend on the
    // geometry it happens to find. This reports; it does not author.
    if let Some(point) = line_pick_point {
        match pdfcer_core::vector::linepick::pick_line_in_page(&model, point, tolerance) {
            Some(line) => {
                // `target=` names WHICH LIST, in the same two-key vocabulary
                // the `hit` rows use, because the answer genuinely can come
                // from either and a bare index would be ambiguous.
                let target = match line.target {
                    HitTarget::Object(i) => format!("index={i}"),
                    HitTarget::Leaf(i) => model.leaves.get(i).map_or_else(
                        || format!("leaf={i}"),
                        |leaf| {
                            format!(
                                "leaf={i} containment={} paint_order={}",
                                leaf.containment
                                    .iter()
                                    .map(|id| id.num.to_string())
                                    .collect::<Vec<_>>()
                                    .join(">"),
                                leaf.paint_order
                            )
                        },
                    ),
                };
                println!(
                    "line-pick page={page_number} at={},{} tolerance={tolerance} {target} \
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
            None => {
                // A miss is an ANSWER. Exit stays 0 and the row still prints,
                // so a script branches on `index=none` rather than on the exit
                // code -- the same contract `--hit` states, kept identical on
                // purpose so the two flags cannot need different handling.
                //
                // `reason=` distinguishes the two ways to miss, because they
                // call for opposite responses: nothing near the point (move
                // the click, or widen `--tolerance`) versus something near it
                // that is a CURVE, which `pick_line` skips deliberately rather
                // than chording -- dimensioning "the line" of a Bezier would
                // measure something the drawing does not contain.
                let near_curve = !hit_test_point_deep(&model, point, tolerance).is_empty();
                println!(
                    "line-pick page={page_number} at={},{} tolerance={tolerance} index=none \
reason={}",
                    point.x,
                    point.y,
                    if near_curve {
                        "nothing-straight-within-tolerance"
                    } else {
                        "nothing-within-tolerance"
                    },
                );
            }
        }
    }

    // The level BELOW the object: which subpath of an entered object the same
    // point lands on. Printed after the `hit`/`hit-candidate` lines because it
    // refines them — a reader sees which object was named, then which of its
    // parts. Silent without `--hit`, and silent for a non-path or out-of-range
    // `--enter`, so a script may pass `--enter` unconditionally.
    if let (Some(point), Some(index)) = (hit_point, enter) {
        for (ordinal, sp) in hit_test_subpaths(&model, index, point, tolerance)
            .into_iter()
            .enumerate()
        {
            println!(
                "subpath-hit page={page_number} object={index} ordinal={ordinal} subpath={sp} \
bbox={}",
                subpath_bounds(&model, index, sp).map_or_else(
                    || "none".to_owned(),
                    |b| format!("{},{},{},{}", b.min.x, b.min.y, b.max.x, b.max.y)
                ),
            );
        }
    }

    let d = &model.diagnostics;
    println!(
        "object-list {} page={page_number} objects={} paths={paths} text={text} images={images} \
forms={forms} dropped_objects={} dropped_nodes={} leaves={} form_cycles={} \
form_depth_overflows={}",
        input.display(),
        model.objects.len(),
        d.objects_dropped,
        d.nodes_dropped,
        model.leaves.len(),
        d.form_cycles,
        d.form_depth_overflows,
    );
    // Rule 4. A truncated leaf list is INVISIBLE in the rows above -- it looks
    // exactly like a page with fewer objects. Both counts are on the stable
    // line for a script, and named here for a human.
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
    exit::SUCCESS
}

/// Grouped arguments for `object-move` (Pass 9c-min) — a struct to keep the
/// handler under clippy's `too_many_arguments` bar, like the other editing
/// subcommands.
pub(crate) struct ObjectMoveArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    pub(crate) object: usize,
    pub(crate) dx: f64,
    pub(crate) dy: f64,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `object-move` — translate a path or text object by a page-space
/// `(dx, dy)` via content-stream surgery ([`EditSession::move_object`]).
/// Only the edited content stream changes (R46/§5.7).
pub(crate) fn cmd_object_move(args: &ObjectMoveArgs<'_>) -> u8 {
    let page_index = (args.page.max(1) - 1) as usize;
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    match session.move_object(page_index, args.object, args.dx, args.dy) {
        Err(err) => return report_edit_error(args.input, &err),
        Ok(disclosures) => report_disclosures(&disclosures),
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
    let r = &outcome.report;
    println!(
        "object-move {} page {} object={} dx={} dy={} mode={} -> {}; changed={} objects={} \
appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page,
        args.object,
        args.dx,
        args.dy,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// Arguments for [`cmd_object_transform`], grouped so the handler stays under
/// the clippy `too_many_arguments` bound (the `EditTextArgs` pattern).
pub(crate) struct ObjectTransformArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    /// `--objects`, unparsed.
    pub(crate) objects: &'a str,
    pub(crate) scale: Option<&'a str>,
    /// Degrees, counter-clockwise.
    pub(crate) rotate: Option<f64>,
    pub(crate) translate: Option<&'a str>,
    pub(crate) pivot: Option<&'a str>,
    pub(crate) on_mixed: &'a str,
    pub(crate) on_singular: &'a str,
    pub(crate) preview: bool,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// Parse a comma-separated list of 0-based object indices.
///
/// # Errors
///
/// The operator-facing message, ready to print.
pub(crate) fn parse_object_indices(raw: &str) -> Result<Vec<usize>, String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<usize>()
                .map_err(|_| format!("--objects: {s:?} is not a 0-based object index"))
        })
        .collect()
}

/// Parse an `X,Y` pair, or a single number meaning both.
///
/// # Errors
///
/// The operator-facing message, ready to print.
pub(crate) fn parse_pair(flag: &str, raw: &str) -> Result<(f64, f64), String> {
    let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
    let num = |s: &str| {
        s.parse::<f64>()
            .map_err(|_| format!("{flag}: {s:?} is not a number"))
    };
    match parts.as_slice() {
        [one] => {
            let v = num(one)?;
            Ok((v, v))
        }
        [x, y] => Ok((num(x)?, num(y)?)),
        _ => Err(format!("{flag}: expected `N` or `X,Y`, got {raw:?}")),
    }
}

/// `object-transform` — scale/rotate/shear/move a selection by one page-space
/// matrix (Pass 113.0/113.1/113.2).
///
/// # Why the CLI composes the matrix rather than taking six numbers
///
/// A `--matrix a,b,c,d,e,f` flag would be a faithful mirror of the API and a
/// poor command line: nobody types a rotation matrix, and the pivot — which is
/// what makes a scale or a rotation land where the objects are rather than
/// flying toward the page origin — would have to be pre-composed by the
/// caller. So the flags name the gesture and the composition happens here,
/// through the same `Matrix::about` the shell uses.
///
/// # `--preview` is the same body, not a dry run
///
/// It calls `EditSession::transform_preview`, which shares one planner with
/// the verb. A preview that said yes where the verb refuses is not a reachable
/// state, which is the whole point of the preflight the consuming shell asked
/// for three times.
pub(crate) fn cmd_object_transform(args: &ObjectTransformArgs<'_>) -> u8 {
    use pdfcer_core::vector::{Matrix, MixedSelection, Point, SingularPolicy, TransformOptions};

    let page_index = (args.page.max(1) - 1) as usize;
    let indices = match parse_object_indices(args.objects) {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => {
            eprintln!("pdfcer: object-transform refused: --objects named no objects");
            return exit::EDIT_REFUSED;
        }
        Err(message) => {
            eprintln!("pdfcer: object-transform refused: {message}");
            return exit::EDIT_REFUSED;
        }
    };
    if !args.preview && args.output.is_none() {
        eprintln!("pdfcer: object-transform refused: --output is required unless --preview");
        return exit::EDIT_REFUSED;
    }

    let mixed = match args.on_mixed {
        "whole" => MixedSelection::TransformWhole,
        "refuse" => MixedSelection::RefuseHeterogeneous,
        other => {
            eprintln!(
                "pdfcer: object-transform refused: --on-mixed {other:?} is not whole or refuse"
            );
            return exit::EDIT_REFUSED;
        }
    };
    let singular = if args.on_singular == "refuse" {
        SingularPolicy::Refuse
    } else if let Some(min) = args.on_singular.strip_prefix("clamp:") {
        match min.parse::<f64>() {
            Ok(min) if min > 0.0 => SingularPolicy::Clamp { min },
            _ => {
                eprintln!(
                    "pdfcer: object-transform refused: --on-singular clamp:MIN needs a positive MIN, got {min:?}"
                );
                return exit::EDIT_REFUSED;
            }
        }
    } else {
        eprintln!(
            "pdfcer: object-transform refused: --on-singular {:?} is not refuse or clamp:MIN",
            args.on_singular
        );
        return exit::EDIT_REFUSED;
    };
    let options = TransformOptions::default()
        .with_mixed(mixed)
        .with_singular(singular);

    let scale = match args.scale.map(|raw| parse_pair("--scale", raw)) {
        Some(Ok(pair)) => Some(pair),
        Some(Err(message)) => {
            eprintln!("pdfcer: object-transform refused: {message}");
            return exit::EDIT_REFUSED;
        }
        None => None,
    };
    let translate = match args.translate.map(|raw| parse_pair("--translate", raw)) {
        Some(Ok(pair)) => Some(pair),
        Some(Err(message)) => {
            eprintln!("pdfcer: object-transform refused: {message}");
            return exit::EDIT_REFUSED;
        }
        None => None,
    };
    let pivot_arg = match args.pivot.map(|raw| parse_pair("--pivot", raw)) {
        Some(Ok(pair)) => Some(pair),
        Some(Err(message)) => {
            eprintln!("pdfcer: object-transform refused: {message}");
            return exit::EDIT_REFUSED;
        }
        None => None,
    };
    if scale.is_none() && args.rotate.is_none() && translate.is_none() {
        eprintln!(
            "pdfcer: object-transform refused: nothing to do -- give at least one of --scale, --rotate, --translate"
        );
        return exit::EDIT_REFUSED;
    }

    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // The pivot defaults to the SELECTION's own centre, computed from the
    // session's current decomposition. A default of the page origin would send
    // a scaled object flying off the sheet, which is not a gesture anybody
    // makes; the shell chooses its own pivot and this is the CLI's equivalent.
    let pivot = match pivot_arg {
        Some((x, y)) => Point::new(x, y),
        None => match selection_centre(&session, page_index, &indices) {
            Ok(p) => p,
            Err(code) => return code,
        },
    };

    let mut matrix = Matrix::IDENTITY;
    if let Some((sx, sy)) = scale {
        matrix = matrix.post_concat(Matrix::scale(sx, sy).about(pivot));
    }
    if let Some(degrees) = args.rotate {
        matrix = matrix.post_concat(Matrix::rotate(degrees.to_radians()).about(pivot));
    }
    if let Some((dx, dy)) = translate {
        matrix = matrix.post_concat(Matrix::translate(dx, dy));
    }

    if args.preview {
        match session.transform_preview(page_index, &indices, matrix, options) {
            Err(err) => return report_edit_error(args.input, &err),
            Ok(outcome) => {
                report_disclosures(&outcome.disclosures);
                println!(
                    "object-transform {} page {} objects={} PREVIEW; would_transform={} clamped={}",
                    args.input.display(),
                    args.page,
                    indices.len(),
                    outcome.objects_transformed,
                    u32::from(outcome.clamped),
                );
                return exit::SUCCESS;
            }
        }
    }

    let transformed = match session.transform_objects(page_index, &indices, matrix, options) {
        Err(err) => return report_edit_error(args.input, &err),
        Ok(outcome) => {
            report_disclosures(&outcome.disclosures);
            outcome
        }
    };
    let Some(output) = args.output else {
        eprintln!("pdfcer: object-transform refused: --output is required unless --preview");
        return exit::EDIT_REFUSED;
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "object-transform {} page {} objects={} mode={} -> {}; transformed={} clamped={} \
changed={} objects_written={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page,
        indices.len(),
        args.mode.name(),
        output.display(),
        transformed.objects_transformed,
        u32::from(transformed.clamped),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// The page-space bounding-box centre of a selection — `object-transform`'s
/// default pivot.
///
/// # Errors
///
/// An exit code, already reported to stderr.
pub(crate) fn selection_centre(
    session: &pdfcer_core::edit::EditSession,
    page_index: usize,
    indices: &[usize],
) -> Result<pdfcer_core::vector::Point, u8> {
    use pdfcer_core::vector::{Bounds, Point};
    let view = session.view();
    let pages = match pdfcer_core::page_tree::pages_in(&view) {
        Ok(pages) => pages,
        Err(err) => {
            eprintln!("pdfcer: object-transform: {err}");
            return Err(exit::RUNTIME_ERROR);
        }
    };
    let Some(page) = pages.get(page_index) else {
        eprintln!("pdfcer: object-transform: no page {}", page_index + 1);
        return Err(exit::EDIT_REFUSED);
    };
    let model = match pdfcer_core::vector::decompose_page(
        &view,
        page,
        pdfcer_core::vector::Matrix::IDENTITY,
    ) {
        Ok(model) => model,
        Err(err) => {
            eprintln!("pdfcer: object-transform: {err}");
            return Err(exit::RUNTIME_ERROR);
        }
    };
    let mut bounds = Bounds::EMPTY;
    for &i in indices {
        let Some(obj) = model.objects.get(i) else {
            // Out of range is the VERB's refusal to raise, by name, with the
            // page's real object count -- not this helper's. Falling back to
            // the origin here keeps one refusal in one place.
            return Ok(Point::new(0.0, 0.0));
        };
        bounds = bounds.union(obj.page_bbox());
    }
    if bounds.min.x > bounds.max.x {
        return Ok(Point::new(0.0, 0.0));
    }
    Ok(Point::new(
        f64::midpoint(bounds.min.x, bounds.max.x),
        f64::midpoint(bounds.min.y, bounds.max.y),
    ))
}

/// Arguments for [`cmd_object_copy`].
pub(crate) struct ObjectCopyArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    /// `--objects`, unparsed.
    pub(crate) objects: &'a str,
    /// `--annotations`, unparsed.
    pub(crate) annotations: &'a str,
    pub(crate) clip: &'a Path,
    pub(crate) pdf: Option<&'a Path>,
    pub(crate) cut: Option<&'a Path>,
    pub(crate) mode: SaveMode,
}

/// `object-copy` — write a selection to a self-contained clipboard payload
/// (Pass 120.0/120.1), optionally cutting it from a copy of the document.
///
/// # Why a FILE rather than the system clipboard
///
/// The requesting shell asked for exactly this split: *"I am not asking you to
/// touch the OS clipboard. That is mine. What I need from you is `to_bytes`."*
/// A one-shot CLI has no session to hold a clip in either, so a file is the
/// only place a payload can live between two invocations — which makes this
/// subcommand the CLI's whole clipboard, not a debug affordance.
pub(crate) fn cmd_object_copy(args: &ObjectCopyArgs<'_>) -> u8 {
    let (input, page, clip_path, cut, mode) =
        (args.input, args.page, args.clip, args.cut, args.mode);
    let page_index = (page.max(1) - 1) as usize;
    let indices = match parse_object_indices(args.objects) {
        Ok(v) => v,
        Err(message) => {
            eprintln!("pdfcer: object-copy refused: {message}");
            return exit::EDIT_REFUSED;
        }
    };
    let annots = match parse_object_indices(args.annotations) {
        Ok(v) => v,
        Err(message) => {
            eprintln!("pdfcer: object-copy refused: {message}");
            return exit::EDIT_REFUSED;
        }
    };
    if indices.is_empty() && annots.is_empty() {
        eprintln!(
            "pdfcer: object-copy refused: nothing selected -- give --objects, --annotations, or both"
        );
        return exit::EDIT_REFUSED;
    }
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    // COPY FIRST, always -- so a selection that cannot be copied is refused
    // with nothing deleted. Reversed, a cut whose copy half failed would take
    // the objects away with nothing on the clipboard, which is the one outcome
    // the operator cannot recover from by pasting.
    //
    // THE CUT PATH GOES THROUGH `cut_selection`, and it did not used to.
    // This handler called `copy_selection` and then `delete_objects` -- which
    // takes OBJECT indices only. `--annotations 0 --cut out.pdf` therefore
    // copied the annotation, left it on the page, and printed `cut=1`. The
    // core had no annotation-aware cut to call until `Pass 168.0`; now it
    // does, and it also folds every deletion into ONE undo entry and refuses
    // a selection holding an annotation the clipboard cannot carry back.
    // NOTE: `--cut` with `--annotations` was REFUSED between `Pass 168.0`
    // and `Pass 169.0`, because the clip FILE dropped annotations and the CLI
    // only ever has the file -- so that cut would have destroyed them. The
    // clip format carries them as of version 2, so the refusal is gone. See
    // the disclosure further down for the state of the branch it guarded.
    let clip = if cut.is_some() {
        match session.cut_selection(page_index, &indices, &annots) {
            Ok(clip) => clip,
            Err(err) => return report_edit_error(input, &err),
        }
    } else {
        match session.copy_selection(page_index, &indices, &annots) {
            Ok(clip) => clip,
            Err(err) => return report_edit_error(input, &err),
        }
    };
    let payload = clip.to_bytes();
    if let Err(err) = std::fs::write(clip_path, &payload) {
        eprintln!("pdfcer: {}: {err}", clip_path.display());
        return exit::IO_ERROR;
    }

    // The interchange export, if asked for -- a second write of the same
    // read, not a second traversal.
    let mut pdf_note = String::from("pdf=0");
    if let Some(path) = args.pdf {
        let exported = clip.to_pdf();
        if exported.size_substituted {
            eprintln!(
                "pdfcer: object-copy: the selection has no area in one direction, so the exported page was given a minimum size of {:.2}x{:.2} pt -- a zero-area /MediaBox produces a file readers refuse to open.",
                exported.size.0, exported.size.1
            );
        }
        if let Err(err) = std::fs::write(path, &exported.bytes) {
            eprintln!("pdfcer: {}: {err}", path.display());
            return exit::IO_ERROR;
        }
        pdf_note = format!(
            "pdf=1 pdf_out={} pdf_size={:.2}x{:.2} pdf_size_substituted={}",
            path.display(),
            exported.size.0,
            exported.size.1,
            u32::from(exported.size_substituted)
        );
    }

    // DEAD TODAY, DELIBERATELY KEPT, and the claim is checked rather than
    // asserted here: `ObjectClip::annotations_survive_serialisation` returns
    // a constant `true` as of `Pass 169.0`, and
    // `crates/pdfcer-core/tests/object_clipboard.rs`'s
    // `a_clip_says_whether_serialisation_would_lose_anything` is what pins
    // that -- so if the answer ever becomes conditional again, a test fails
    // and leads here.
    //
    // The comment this replaces said "it no longer fires" and gave a
    // narrative reason. That was true, and it was the wrong SHAPE: a prose
    // claim about what a branch does is exactly the thing that quietly stops
    // being true when the code under it moves. Naming the test that holds the
    // invariant makes it checkable by someone who does not believe me.
    //
    // Kept rather than deleted because it is the honest place for the
    // disclosure: if a future clip kind is added that the FILE cannot hold,
    // this is where the operator must be told, and rebuilding the branch
    // later is how it gets forgotten.
    if !clip.annotations_survive_serialisation() {
        eprintln!(
            "pdfcer: object-copy: {} annotation(s) were copied but are NOT carried by the clipboard file, so a paste from this file will place the content and not the annotations.",
            clip.annotation_count()
        );
    }

    let mut cut_note = String::from("cut=0");
    if let Some(output) = cut {
        // The deletion already happened, inside `cut_selection` above -- and
        // it had to, because a cut is one gesture and therefore one undo
        // entry. What is left here is the save.
        let outcome = match save_edited(
            &mut session,
            &source,
            output,
            mode,
            ProducerArg::Preserve,
            false,
        ) {
            Ok(outcome) => outcome,
            Err(code) => return code,
        };
        cut_note = format!("cut=1 cut_out={}", output.display());
        let code = finish_edit(input, &outcome);
        if code != exit::SUCCESS {
            return code;
        }
    }

    println!(
        "object-copy {} page {} objects={} -> {} ({} bytes); kinds={} resources={} {cut_note}",
        input.display(),
        page,
        clip.len(),
        clip_path.display(),
        payload.len(),
        clip.kinds().join("+"),
        clip.resource_count(),
    );
    println!(
        "  annotations={} annotations_serialise={}",
        clip.annotation_count(),
        u32::from(clip.annotations_survive_serialisation())
    );
    println!("  {pdf_note}");
    exit::SUCCESS
}

/// Arguments for [`cmd_object_paste`], grouped so the handler stays under the
/// clippy `too_many_arguments` bound.
pub(crate) struct ObjectPasteArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    pub(crate) clip: &'a Path,
    pub(crate) translate: Option<&'a str>,
    pub(crate) scale: Option<&'a str>,
    /// Degrees, counter-clockwise.
    pub(crate) rotate: Option<f64>,
    pub(crate) preview: bool,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `object-paste` — place a clipboard payload onto a page (Pass 120.0/120.1).
///
/// The scale and rotation pivot on the **clip's own centre**, not the page
/// origin: a pasted selection scaled about the origin flies off the sheet,
/// which is not a gesture anybody makes. Same reasoning as
/// `object-transform`'s default pivot, and the clip already carries the bounds
/// to compute it from.
pub(crate) fn cmd_object_paste(args: &ObjectPasteArgs<'_>) -> u8 {
    use pdfcer_core::vector::{Matrix, ObjectClip, Point};

    let page_index = (args.page.max(1) - 1) as usize;
    if !args.preview && args.output.is_none() {
        eprintln!("pdfcer: object-paste refused: --output is required unless --preview");
        return exit::EDIT_REFUSED;
    }
    let payload = match std::fs::read(args.clip) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.clip.display());
            return exit::IO_ERROR;
        }
    };
    let clip = match ObjectClip::from_bytes(&payload) {
        Ok(clip) => clip,
        Err(err) => {
            eprintln!("pdfcer: object-paste refused: {err}");
            return exit::EDIT_REFUSED;
        }
    };

    let scale = match args.scale.map(|raw| parse_pair("--scale", raw)) {
        Some(Ok(pair)) => Some(pair),
        Some(Err(message)) => {
            eprintln!("pdfcer: object-paste refused: {message}");
            return exit::EDIT_REFUSED;
        }
        None => None,
    };
    let translate = match args.translate.map(|raw| parse_pair("--translate", raw)) {
        Some(Ok(pair)) => Some(pair),
        Some(Err(message)) => {
            eprintln!("pdfcer: object-paste refused: {message}");
            return exit::EDIT_REFUSED;
        }
        None => None,
    };

    let bbox = clip.bbox();
    let pivot = if bbox.min.x > bbox.max.x {
        Point::new(0.0, 0.0)
    } else {
        Point::new(
            f64::midpoint(bbox.min.x, bbox.max.x),
            f64::midpoint(bbox.min.y, bbox.max.y),
        )
    };
    let mut at = Matrix::IDENTITY;
    if let Some((sx, sy)) = scale {
        at = at.post_concat(Matrix::scale(sx, sy).about(pivot));
    }
    if let Some(degrees) = args.rotate {
        at = at.post_concat(Matrix::rotate(degrees.to_radians()).about(pivot));
    }
    if let Some((dx, dy)) = translate {
        at = at.post_concat(Matrix::translate(dx, dy));
    }

    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    if args.preview {
        match session.paste_preview(page_index, &clip, at) {
            Err(err) => return report_edit_error(args.input, &err),
            Ok(outcome) => {
                report_disclosures(&outcome.disclosures);
                println!(
                    "object-paste {} page {} clip={} PREVIEW; would_paste={} annotations={} resources={} bbox={:.2},{:.2},{:.2},{:.2}",
                    args.input.display(),
                    args.page,
                    args.clip.display(),
                    outcome.objects_pasted,
                    outcome.annotations_pasted,
                    outcome.resources_added,
                    outcome.bbox.min.x,
                    outcome.bbox.min.y,
                    outcome.bbox.max.x,
                    outcome.bbox.max.y,
                );
                return exit::SUCCESS;
            }
        }
    }

    let pasted = match session.paste_objects(page_index, &clip, at) {
        Err(err) => return report_edit_error(args.input, &err),
        Ok(outcome) => {
            report_disclosures(&outcome.disclosures);
            outcome
        }
    };
    let Some(output) = args.output else {
        eprintln!("pdfcer: object-paste refused: --output is required unless --preview");
        return exit::EDIT_REFUSED;
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "object-paste {} page {} clip={} mode={} -> {}; pasted={} annotations={} resources={} \
bbox={:.2},{:.2},{:.2},{:.2} changed={} objects_written={} appended={} out_bytes={} \
undo_verified={} undo_identical={}",
        args.input.display(),
        args.page,
        args.clip.display(),
        args.mode.name(),
        output.display(),
        pasted.objects_pasted,
        pasted.annotations_pasted,
        pasted.resources_added,
        pasted.bbox.min.x,
        pasted.bbox.min.y,
        pasted.bbox.max.x,
        pasted.bbox.max.y,
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// `unshare-form`: copy-on-write a shared form XObject onto one page.
///
/// The "option" half of the shared-form edit default — see the subcommand's
/// own documentation for why breaking the sharing is a separate act rather
/// than a mode of the edit verbs.
pub(crate) fn cmd_unshare_form(
    input: &Path,
    page: u32,
    form: u32,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let page_index = (page.max(1) - 1) as usize;
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let report = match session.unshare_form(page_index, pdfcer_core::object::ObjId::new(form, 0)) {
        Ok(report) => report,
        Err(err) => return report_edit_error(input, &err),
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "unshare-form {} page {page} form={form} copy={} refs_moved={} mode={} -> {}; \
changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        report.copy.num,
        report.references_moved,
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        outcome.undo_verified,
        outcome.undo_identical,
    );
    // Rule 4: the operator asked for ONE page to stop sharing, and what he
    // cannot see from the output is that the other invocation sites are still
    // sharing the original. Said on the way past rather than left to be
    // inferred from an object number.
    eprintln!(
        "pdfcer: unshare-form: page {page} now names object {} — a private copy. Every OTHER \
page or invocation that used object {form} still names object {form} and is unchanged. An edit \
to the copy from here on affects this page only; an edit to {form} still affects all of them.",
        report.copy.num
    );
    finish_edit(input, &outcome)
}

/// `object-delete` — remove a vector object's construction + painting
/// operators from the content stream via surgery (Pass 9c-min). NOT
/// redaction (it removes a drawing object, not covered content for
/// security).
pub(crate) fn cmd_object_delete(
    input: &Path,
    page: u32,
    object: usize,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let page_index = (page.max(1) - 1) as usize;
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    if let Err(err) = session.delete_object(page_index, object) {
        return report_edit_error(input, &err);
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "object-delete {} page {page} object={object} mode={} -> {}; changed={} objects={} \
appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

/// Grouped arguments for `subpath-move` (Pass 28.0).
pub(crate) struct SubpathMoveArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    /// The page paint-order index, when addressing a page object.
    pub(crate) object: Option<usize>,
    /// The index into this page's form leaves, when addressing an
    /// object INSIDE a form XObject (`Pass 188.0`). Exactly one of
    /// this and `object` is set; `object_or_leaf` enforces it.
    pub(crate) leaf: Option<usize>,
    pub(crate) subpath: usize,
    pub(crate) dx: f64,
    pub(crate) dy: f64,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// How the machine-readable line names the object that was edited.
///
/// `object=N` or `leaf=N`, never a bare number: the two index different lists
/// and a script that could not tell them apart would reconstruct the wrong
/// command. Replaced the old unconditional `object=` field, which would have
/// printed a page paint-order index for an edit that never used one.
pub(crate) fn target_token(object: Option<usize>, leaf: Option<usize>) -> String {
    match (object, leaf) {
        (Some(o), _) => format!("object={o}"),
        (None, Some(l)) => format!("leaf={l}"),
        (None, None) => "object=?".to_owned(),
    }
}

/// State how far a form edit reached, on stderr, before the machine-readable
/// stdout line (`Pass 188.0`).
///
/// The CLI has no session and no undo: the invocation IS the commit, so rule 4
/// is satisfied by printing what pdfcer decided **on the way past** rather than
/// by any confirm step. What it decided here is that one edit changed several
/// places, which the operator did not ask for and cannot see.
pub(crate) fn report_form_reach(outcome: Option<&pdfcer_core::edit::FormSurgeryOutcome>) {
    let Some(o) = outcome else { return };
    if o.invocations <= 1 && o.pages <= 1 {
        return;
    }
    eprintln!(
        "pdfcer: ★ this object is inside form XObject {} 0 R, which is drawn {} time(s) across \
         {} page(s). A form has ONE set of bytes, so this edit changed every one of them. Run \
         `unshare-form` first if you wanted only this page's copy to change.",
        o.form.num, o.invocations, o.pages
    );
}

/// Which object a geometry subcommand is addressing — a page paint-order
/// index or a form leaf (`Pass 188.0`).
///
/// `--object` and `--leaf` name **different lists**, and a page has both. An
/// operator who passes neither is asking for nothing; one who passes both is
/// asking for two different objects. Both are refused by name here rather than
/// resolved by a precedence rule, because a precedence rule silently picks one
/// of the two things they meant.
pub(crate) enum GeometryTarget {
    Page(usize),
    Leaf(usize),
}

/// Resolve the `--object` / `--leaf` pair, or print the refusal and return the
/// exit code.
pub(crate) fn object_or_leaf(
    input: &Path,
    object: Option<usize>,
    leaf: Option<usize>,
) -> Result<GeometryTarget, u8> {
    match (object, leaf) {
        (Some(o), None) => Ok(GeometryTarget::Page(o)),
        (None, Some(l)) => Ok(GeometryTarget::Leaf(l)),
        (Some(_), Some(_)) => {
            eprintln!(
                "pdfcer: {}: --object and --leaf name different lists (page paint order vs \
                 the objects inside form XObjects) — pass exactly one. `object-list` prints both, \
                 as `object index=` and `leaf index=` rows.",
                input.display()
            );
            Err(exit::RUNTIME_ERROR)
        }
        (None, None) => {
            eprintln!(
                "pdfcer: {}: pass --object N to address a page object, or --leaf N to address \
                 one inside a form XObject. `object-list` prints both.",
                input.display()
            );
            Err(exit::RUNTIME_ERROR)
        }
    }
}

/// `subpath-move` — translate ONE subpath of a path object.
pub(crate) fn cmd_subpath_move(args: &SubpathMoveArgs<'_>) -> u8 {
    let page_index = (args.page.max(1) - 1) as usize;
    let target = match object_or_leaf(args.input, args.object, args.leaf) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let result = match target {
        GeometryTarget::Page(object) => session
            .move_subpath(page_index, object, args.subpath, args.dx, args.dy)
            .map(|d| (d, None)),
        GeometryTarget::Leaf(leaf) => session
            .move_subpath_in_form(page_index, leaf, args.subpath, args.dx, args.dy)
            .map(|o| (o.disclosures.clone(), Some(o))),
    };
    match result {
        Err(err) => return report_edit_error(args.input, &err),
        Ok((disclosures, form)) => {
            report_disclosures(&disclosures);
            report_form_reach(form.as_ref());
        }
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
    let r = &outcome.report;
    println!(
        "subpath-move {} page {} {} subpath={} dx={} dy={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page,
        target_token(args.object, args.leaf),
        args.subpath,
        args.dx,
        args.dy,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// Grouped arguments for `subpath-delete` (Pass 25.2) — a struct to keep the
/// handler under clippy's `too_many_arguments` bar, like its siblings.
pub(crate) struct SubpathDeleteArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    pub(crate) object: usize,
    pub(crate) subpath: usize,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `subpath-delete` — remove ONE subpath of a path object via content-stream
/// surgery, leaving the object's other subpaths byte-verbatim.
///
/// ## Contract
///
/// - Emits one `subpath-delete …` line naming the page, object, subpath and
///   the usual save-report fields, then defers the exit code to
///   [`finish_edit`] like every other editing subcommand.
/// - Every refusal — clipping path, structure mismatch, out-of-range index,
///   non-path object — happens before any mutation and is reported through
///   [`report_edit_error`], so the refusal vocabulary and exit codes are the
///   same ones the GUI surfaces. The operator gets the same answer whichever
///   shell they came through, which is the point of having one core.
pub(crate) fn cmd_subpath_delete(args: &SubpathDeleteArgs<'_>) -> u8 {
    let page_index = (args.page.max(1) - 1) as usize;
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    match session.delete_subpath(page_index, args.object, args.subpath) {
        Err(err) => return report_edit_error(args.input, &err),
        Ok(disclosures) => report_disclosures(&disclosures),
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
    let r = &outcome.report;
    println!(
        "subpath-delete {} page {} object={} subpath={} mode={} -> {}; changed={} objects={} \
appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page,
        args.object,
        args.subpath,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// Grouped arguments for `node-delete` (Pass 36.1).
pub(crate) struct NodeDeleteArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    pub(crate) object: usize,
    pub(crate) node: usize,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `node-delete` — remove ONE anchor of a path object via content-stream
/// surgery, leaving every other object and every sibling subpath
/// byte-verbatim (Pass 36.1).
///
/// ## Contract
///
/// - Emits one `node-delete …` line naming the page, object, node and the
///   usual save-report fields, then defers the exit code to [`finish_edit`]
///   like every other editing subcommand.
/// - Disclosures — currently "a curve went with the point" — go to **stderr**
///   via [`report_disclosures`], so a script's stdout record stays
///   machine-parseable while the operator-facing consequence is still stated.
/// - Every refusal happens before any mutation and is reported through
///   [`report_edit_error`], so the refusal vocabulary and exit codes match the
///   GUI's exactly. Same core, same answer, whichever shell asked.
pub(crate) fn cmd_node_delete(args: &NodeDeleteArgs<'_>) -> u8 {
    let page_index = (args.page.max(1) - 1) as usize;
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    match session.delete_node(page_index, args.object, args.node) {
        Err(err) => return report_edit_error(args.input, &err),
        Ok(disclosures) => report_disclosures(&disclosures),
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
    let r = &outcome.report;
    println!(
        "node-delete {} page {} object={} node={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page,
        args.object,
        args.node,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// Grouped arguments for `node-move` (Pass 9c-min).
pub(crate) struct NodeMoveArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    /// The page paint-order index, when addressing a page object.
    pub(crate) object: Option<usize>,
    /// The index into this page's form leaves, when addressing an
    /// object INSIDE a form XObject (`Pass 188.0`). Exactly one of
    /// this and `object` is set; `object_or_leaf` enforces it.
    pub(crate) leaf: Option<usize>,
    pub(crate) node: usize,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// Parse one `--move NODE,X,Y` triple.
///
/// # Why a custom parser rather than three repeated flags
///
/// The alternative — `--node 0 --x 200 --y 80 --node 1 --x 280 --y 80` —
/// relies on three independent repeated lists staying the same length and
/// in the same order. A dropped value silently pairs every anchor with the
/// wrong point from there on, and the command succeeds. Keeping the three
/// numbers in ONE token makes that unrepresentable.
///
/// # Errors
///
/// A message naming the offending token and what was expected. Negative
/// coordinates are legal (the flag carries `allow_hyphen_values`), so
/// `1,-5,-5` parses; a negative NODE does not, because an anchor index is
/// a position in a list.
pub(crate) fn parse_node_move(token: &str) -> Result<(usize, pdfcer_core::vector::Point), String> {
    let parts: Vec<&str> = token.split(',').collect();
    let [n, x, y] = parts[..] else {
        return Err(format!(
            "--move {token:?}: expected NODE,X,Y (three comma-separated values), got {} \
             value(s)",
            parts.len()
        ));
    };
    let node: usize = n
        .trim()
        .parse()
        .map_err(|_| format!("--move {token:?}: {n:?} is not a 0-based anchor index"))?;
    let x: f64 = x
        .trim()
        .parse()
        .map_err(|_| format!("--move {token:?}: {x:?} is not a number"))?;
    let y: f64 = y
        .trim()
        .parse()
        .map_err(|_| format!("--move {token:?}: {y:?} is not a number"))?;
    Ok((node, pdfcer_core::vector::Point::new(x, y)))
}

/// `nodes-move` — move several anchors of one path object as ONE surgery
/// (`Pass 23.3`).
///
/// ## Contract
///
/// - One `nodes-move …` line carrying `object=`, `nodes=` (how many were
///   moved) and the usual save-report fields, then the exit code from
///   [`finish_edit`].
/// - Disclosures to **stderr**, like `node-move`'s, so the stdout record
///   stays a fixed shape. De-duplicated by core: three rewritten rectangles
///   say so once.
/// - Every refusal — a malformed `--move` token, no anchors, a duplicated
///   anchor, an out-of-range index — happens **before** any mutation, and
///   the argument parsing is done up front for the same reason: a batch
///   whose fourth token is malformed must not apply its first three.
pub(crate) fn cmd_nodes_move(
    input: &Path,
    page: u32,
    object: usize,
    moves: &[String],
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    // ALL tokens parsed before the document is even opened. A partial parse
    // followed by a partial edit is the failure this ordering removes.
    let mut parsed = Vec::with_capacity(moves.len());
    for token in moves {
        match parse_node_move(token) {
            Ok(m) => parsed.push(m),
            Err(msg) => {
                eprintln!("pdfcer: {}: {msg}", input.display());
                return exit::RUNTIME_ERROR;
            }
        }
    }

    let page_index = (page.max(1) - 1) as usize;
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    match session.move_nodes(page_index, object, &parsed) {
        Err(err) => return report_edit_error(input, &err),
        Ok(disclosures) => report_disclosures(&disclosures),
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "nodes-move {} page {} object={} nodes={} mode={} -> {}; changed={} objects={} \
appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        page.max(1),
        object,
        parsed.len(),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

/// `node-move` — move one anchor to a page-space point via surgery
/// (Pass 9c-min, decision 011 §2.5).
///
/// An `re` rectangle corner and the implicit reused start of an `h`-reopened
/// subpath have no operand of their own; both are handled by materializing one
/// (Pass 30.0) and both DISCLOSE that they did, on stderr so a script's stdout
/// record stays machine-parseable.
pub(crate) fn cmd_node_move(args: &NodeMoveArgs<'_>) -> u8 {
    use pdfcer_core::vector::Point;
    let page_index = (args.page.max(1) - 1) as usize;
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let target = match object_or_leaf(args.input, args.object, args.leaf) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let result = match target {
        GeometryTarget::Page(object) => session
            .move_node(page_index, object, args.node, Point::new(args.x, args.y))
            .map(|d| (d, None)),
        GeometryTarget::Leaf(leaf) => session
            .move_node_in_form(page_index, leaf, args.node, Point::new(args.x, args.y))
            .map(|o| (o.disclosures.clone(), Some(o))),
    };
    match result {
        Err(err) => return report_edit_error(args.input, &err),
        // stderr, not stdout: the stdout line is a fixed-shape record other
        // tools parse, and a variable-length prose block in the middle of it
        // would break them.
        Ok((disclosures, form)) => {
            report_disclosures(&disclosures);
            report_form_reach(form.as_ref());
        }
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
    let r = &outcome.report;
    println!(
        "node-move {} page {} {} node={} to=({},{}) mode={} -> {}; changed={} objects={} \
appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page,
        target_token(args.object, args.leaf),
        args.node,
        args.x,
        args.y,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// Grouped arguments for `handle-move` (Pass 30.1).
pub(crate) struct HandleMoveArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    pub(crate) object: usize,
    pub(crate) node: usize,
    pub(crate) side: HandleArg,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `handle-move` — move one Bézier control point, leaving its on-curve node
/// where it is (Pass 30.1).
///
/// The operation that changes a curve's SHAPE; `node-move` can only move the
/// points a curve passes through. A `v`/`y` segment whose requested handle is
/// implied by another point is re-spelled as `c`, disclosed on stderr so the
/// stdout record stays machine-parseable.
pub(crate) fn cmd_handle_move(args: &HandleMoveArgs<'_>) -> u8 {
    use pdfcer_core::vector::Point;
    let page_index = (args.page.max(1) - 1) as usize;
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    match session.move_handle(
        page_index,
        args.object,
        args.node,
        args.side.to_core(),
        Point::new(args.x, args.y),
    ) {
        Err(err) => return report_edit_error(args.input, &err),
        Ok(disclosures) => report_disclosures(&disclosures),
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
    let r = &outcome.report;
    println!(
        "handle-move {} page {} object={} node={} side={} to=({},{}) mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page,
        args.object,
        args.node,
        args.side.token(),
        args.x,
        args.y,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}
