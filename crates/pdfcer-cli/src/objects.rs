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
/// `(dx, dy)` via content-stream surgery (`EditSession::move_object`).
/// Only the edited content stream changes.
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

/// Arguments for [`cmd_object_move_each`].
pub(crate) struct ObjectMoveEachArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    /// `--move` values, unparsed (`INDEX,DX,DY`).
    pub(crate) moves: &'a [String],
    pub(crate) leaf: bool,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// Parse one `--move INDEX,DX,DY`.
fn parse_move(s: &str) -> Option<(usize, f64, f64)> {
    let mut parts = s.split(',').map(str::trim);
    let index = parts.next()?.parse().ok()?;
    let dx: f64 = parts.next()?.parse().ok()?;
    let dy: f64 = parts.next()?.parse().ok()?;
    (parts.next().is_none() && dx.is_finite() && dy.is_finite()).then_some((index, dx, dy))
}

/// `object-move-each` — move several objects, each by its own page-space
/// offset, as one edit (`EditSession::move_objects_each`, or
/// `move_objects_each_in_form` under `--leaf`).
pub(crate) fn cmd_object_move_each(args: &ObjectMoveEachArgs<'_>) -> u8 {
    let mut moves = Vec::with_capacity(args.moves.len());
    for raw in args.moves {
        let Some(m) = parse_move(raw) else {
            eprintln!(
                "pdfcer: --move {raw:?} is not INDEX,DX,DY (a 0-based index and two finite numbers)"
            );
            return exit::EDIT_REFUSED;
        };
        moves.push(m);
    }
    let page_index = (args.page.max(1) - 1) as usize;
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let reach = if args.leaf {
        match session.move_objects_each_in_form(page_index, &moves) {
            Err(err) => return report_edit_error(args.input, &err),
            Ok(out) => format!(" invocations={} pages={}", out.invocations, out.pages),
        }
    } else {
        match session.move_objects_each(page_index, &moves) {
            Err(err) => return report_edit_error(args.input, &err),
            Ok(disclosures) => {
                report_disclosures(&disclosures);
                String::new()
            }
        }
    };
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
        "object-move-each {} page {} moved={} leaf={}{reach} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page,
        moves.len(),
        u32::from(args.leaf),
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

/// Arguments for [`cmd_object_transform_each`].
pub(crate) struct ObjectTransformEachArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    /// `--transform` values, unparsed (`INDEX,A,B,C,D,E,F`).
    pub(crate) transforms: &'a [String],
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// Parse one `--transform INDEX,A,B,C,D,E,F`.
fn parse_indexed_matrix(s: &str) -> Option<(usize, pdfcer_core::vector::Matrix)> {
    let mut parts = s.split(',').map(str::trim);
    let index = parts.next()?.parse().ok()?;
    let v: Vec<f64> = parts.map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let [a, b, c, d, e, f] = v.as_slice() else {
        return None;
    };
    v.iter().all(|x| x.is_finite()).then_some((
        index,
        pdfcer_core::vector::Matrix {
            a: *a,
            b: *b,
            c: *c,
            d: *d,
            e: *e,
            f: *f,
        },
    ))
}

/// `object-transform-each` — transform several objects, each by its own
/// page-space matrix, as one edit (`EditSession::transform_objects_each`).
pub(crate) fn cmd_object_transform_each(args: &ObjectTransformEachArgs<'_>) -> u8 {
    let mut transforms = Vec::with_capacity(args.transforms.len());
    for raw in args.transforms {
        let Some(t) = parse_indexed_matrix(raw) else {
            eprintln!(
                "pdfcer: --transform {raw:?} is not INDEX,A,B,C,D,E,F (a 0-based index and six finite numbers)"
            );
            return exit::EDIT_REFUSED;
        };
        transforms.push(t);
    }
    let page_index = (args.page.max(1) - 1) as usize;
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let out = match session.transform_objects_each(
        page_index,
        &transforms,
        pdfcer_core::vector::TransformOptions::default(),
    ) {
        Ok(out) => out,
        Err(err) => return report_edit_error(args.input, &err),
    };
    report_disclosures(&out.disclosures);
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
        "object-transform-each {} page {} transformed={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page,
        out.objects_transformed,
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

/// Arguments for [`cmd_object_paste`], grouped so the handler stays under the
/// clippy `too_many_arguments` bound.
pub(crate) struct ObjectPasteArgs<'a> {
    pub(crate) input: &'a Path,
    /// `--layer` / `--layer-id` (`Pass 358.5`): the layer what is added
    /// goes on, or `None` for none.
    pub(crate) layer: Option<LayerPick>,
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

    let layer = match resolve_add_layer(args.input, &session, args.layer.as_ref()) {
        Ok(layer) => layer,
        Err(code) => return code,
    };
    if args.preview {
        match session.paste_preview(page_index, &clip, at) {
            Err(err) => return report_edit_error(args.input, &err),
            Ok(outcome) => {
                report_disclosures(&outcome.disclosures);
                println!(
                    "object-paste {} page {} clip={} PREVIEW; would_paste={} annotations={} replies_unthreaded={} resources={} bbox={:.2},{:.2},{:.2},{:.2}",
                    args.input.display(),
                    args.page,
                    args.clip.display(),
                    outcome.objects_pasted,
                    outcome.annotations_pasted,
                    outcome.replies_unthreaded,
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

    let pasted = match session.paste_objects_on_layer(page_index, &clip, at, layer) {
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
        "object-paste {} page {} clip={} mode={} -> {}; pasted={} annotations={} replies_unthreaded={} resources={} \
bbox={:.2},{:.2},{:.2},{:.2} changed={} objects_written={} appended={} out_bytes={} \
undo_verified={} undo_identical={}",
        args.input.display(),
        args.page,
        args.clip.display(),
        args.mode.name(),
        output.display(),
        pasted.objects_pasted,
        pasted.annotations_pasted,
        pasted.replies_unthreaded,
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
    if let Some(o) = outcome {
        report_reach(o.form.num, o.invocations, o.pages);
    }
}

/// [`report_form_reach`] from the reach's three numbers.
pub(crate) fn report_reach(form: u32, invocations: usize, pages: usize) {
    if invocations <= 1 && pages <= 1 {
        return;
    }
    eprintln!(
        "pdfcer: ★ this object is inside form XObject {form} 0 R, which is drawn {invocations} \
         time(s) across {pages} page(s). A form has ONE set of bytes, so this edit changed every \
         one of them. Run `unshare-form` first if you wanted only this page's copy to change."
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
///.
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
