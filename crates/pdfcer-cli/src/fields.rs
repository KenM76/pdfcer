use super::*;

/// `find-text` — locate every occurrence of a string in the page text.
///
/// # Why the geometry is in the output and not just the page number
///
/// "page 3" is not an answer when a word appears six times on it. Each
/// hit reports its bounding box in unrotated page space, which is what a
/// caller needs to draw a box, crop an image, or hand a coordinate to
/// `mark-redaction`. It is the SAME quad `mark-redaction --search` would
/// cover, because both come from one scan in core — so a script that
/// finds first and redacts second cannot get two different rectangles.
///
/// # Exit code
///
/// `0` whether or not anything matched. Finding nothing is a successful
/// search, not a failure, and a non-zero exit would make "no hits"
/// indistinguishable from "could not read the file" in a shell pipeline.
/// The count is on the summary line for a caller that wants to branch.
pub(crate) fn cmd_find_text(input: &Path, needle: &str, ignore_case: bool) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    // `search_text`, not `find_text`: the same scan, but it also hands
    // back what the extraction underneath could NOT read. See the
    // "what a zero means" block below — that is the whole reason.
    let found = session.search_text(
        needle,
        &pdfcer_core::edit::TextSearchOptions::default()
            .with_case_insensitive(ignore_case)
            .with_wildcards(true),
    );
    let hits = &found.matches;

    for h in hits {
        // Bounds over all FOUR corners rather than reading `ll`/`ur`.
        // Today every quad here comes from `Quad::from_rect` and is
        // axis-aligned, so the two agree — but `Quad` is a general
        // quadrilateral (§12.5.6.10 `/QuadPoints`), and a corner-pair
        // shortcut would silently under-report the box the day a rotated
        // one arrives.
        let xs = [h.quad.ul.0, h.quad.ur.0, h.quad.ll.0, h.quad.lr.0];
        let ys = [h.quad.ul.1, h.quad.ur.1, h.quad.ll.1, h.quad.lr.1];
        let min = |v: [f64; 4]| v.iter().copied().fold(f64::INFINITY, f64::min);
        let max = |v: [f64; 4]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        // 1-based page, matching every other page-addressing surface in
        // this CLI. The extraction is 0-based and the operator is not.
        println!(
            "match page={} text={:?} rect={:.2},{:.2},{:.2},{:.2}",
            h.page_index + 1,
            h.text,
            min(xs),
            min(ys),
            max(xs),
            max(ys),
        );
    }
    println!(
        "find-text {} needle={needle:?} ignore_case={} matches={} unreadable_codes={} \
type3_no_tounicode={} identity_no_tounicode={}",
        input.display(),
        u32::from(ignore_case),
        hits.len(),
        // Appended `Pass 127.0`, per the stable-line append-never-reorder
        // rule. See `report_unsearchable_text` for why a search result
        // that omits these is not a result.
        found.diagnostics.ladder_failures,
        found.diagnostics.type3_fonts_without_to_unicode,
        found.diagnostics.identity_fonts_without_to_unicode,
    );
    report_unsearchable_text(&found.diagnostics);
    exit::SUCCESS
}

/// Say, on stderr, what this document's text search **could not read**.
///
/// # Why a search command has to do this at all
///
/// `matches=0` has two causes and one appearance: the needle is not in
/// the document, or the document's text was never recoverable as Unicode
/// so no needle could have matched it. A search that prints only the
/// first reading is not merely terse — it is **wrong** about the second,
/// and wrong in the direction that ends with an operator concluding a
/// word is absent from a page they can see it on.
///
/// The two named populations are both fonts that **render perfectly**,
/// which is exactly what makes the failure invisible:
///
/// * **Type 3 with no `/ToUnicode`** (ISO 32000-1 §9.6.5). A Type 3
///   glyph is a content stream named by an arbitrary `/CharProcs` key —
///   `/g13` means nothing outside the one document — so §9.10.2 method 2's
///   precondition is false by construction and rung 1 is the font's only
///   route to Unicode. Acrobat is gated on the identical entry; this is
///   parity, not a pdfcer shortfall.
/// * **`Identity-H`/`Adobe-Identity-0` with no `/ToUnicode`**, the
///   composite twin, which §9.10.2 excludes from every rung.
///
/// `unreadable_codes` (`ladder_failures`) is the per-code total across
/// every cause including unnamed ones, and is reported whether or not
/// either font-level counter fired.
///
/// # Why stderr, and why unconditionally on the summary line
///
/// The counters ride the machine-readable summary line so a script can
/// branch on them without parsing prose; the prose goes to stderr so it
/// never contaminates a `find-text > hits.txt` capture. Project rule 4
/// ("fuzzy, never sneaky") makes the disclosure obligatory, and in
/// `pdfcer` the invocation is the commit — there is no session in
/// which to review it later, so it is printed on the way past.
pub(crate) fn report_unsearchable_text(d: &pdfcer_core::text_extract::TextDiagnostics) {
    if d.type3_fonts_without_to_unicode > 0 {
        eprintln!(
            "pdfcer: find-text: {} Type 3 font(s) in this document carry NO /ToUnicode CMap \
             (ISO 32000-1 §9.6.5) — their glyphs are content streams named by arbitrary \
             /CharProcs keys, so text set in them RENDERS correctly and cannot be searched or \
             copied. Acrobat is gated on the same entry",
            d.type3_fonts_without_to_unicode
        );
    }
    if d.identity_fonts_without_to_unicode > 0 {
        eprintln!(
            "pdfcer: find-text: {} font(s) are Identity-H/Adobe-Identity-0 with NO /ToUnicode \
             — ISO 32000-1 §9.10.2 excludes them from every ladder rung, so text set in them \
             cannot be searched or copied",
            d.identity_fonts_without_to_unicode
        );
    }
    if d.ladder_failures > 0 {
        eprintln!(
            "pdfcer: find-text: {} of {} character code(s) could not be mapped to Unicode and \
             are U+FFFD in the searched text — a needle covering them cannot match. A zero match \
             count for this document is therefore not evidence the needle is absent",
            d.ladder_failures, d.codes_total
        );
    }
}

/// One line describing a resolved rich-text run style.
///
/// # Why only the SET properties appear
///
/// [`pdfcer_core::richtext::Style`] uses `None` for "neither the run nor
/// `/DS` specified this", which is deliberately not the same as "the
/// default". Printing `weight=none` for every plain run would bury the
/// handful of properties that are actually set, and — worse — would read
/// as an assertion about the field that the file does not make. What is
/// absent here is absent from the document.
///
/// `unstyled` rather than an empty string when nothing is set, because a
/// blank tail in a `key=value` line reads as truncated output.
pub(crate) fn describe_style(s: &pdfcer_core::richtext::Style) -> String {
    use pdfcer_core::richtext::{Align, Stretch};
    let mut parts: Vec<String> = Vec::new();
    if let Some(f) = s.size_pt {
        parts.push(format!("{f}pt"));
    }
    if !s.family.is_empty() {
        parts.push(s.family.join("/"));
    }
    // Reported as the number the spec normalises to, with the familiar
    // keyword alongside for the two values that have one — an operator
    // reading `700` should not have to remember that it means bold.
    if let Some(w) = s.weight {
        parts.push(match w {
            400 => "weight=400(normal)".to_owned(),
            700 => "weight=700(bold)".to_owned(),
            other => format!("weight={other}"),
        });
    }
    if let Some(i) = s.italic {
        parts.push(if i { "italic" } else { "upright" }.to_owned());
    }
    if s.underline == Some(true) {
        parts.push("underline".to_owned());
    }
    if s.strikethrough == Some(true) {
        parts.push("strikethrough".to_owned());
    }
    if let Some([r, g, b]) = s.color {
        // Back to the #rrggbb the file wrote. The model holds DeviceRGB
        // 0.0-1.0 because RT-M12 requires that conversion, but three
        // decimals are not what an operator recognises as "the red one".
        let byte = |v: f64| (v * 255.0).round().clamp(0.0, 255.0) as u8;
        parts.push(format!("#{:02X}{:02X}{:02X}", byte(r), byte(g), byte(b)));
    }
    if let Some(a) = s.align {
        parts.push(
            match a {
                Align::Left => "align=left",
                Align::Center => "align=center",
                Align::Right => "align=right",
            }
            .to_owned(),
        );
    }
    if let Some(v) = s.baseline_shift_pt {
        // Named by what it MEANS, not by its sign. Table 225's convention
        // is positive-is-superscript, which is the opposite of the
        // intuition a reader brings from CSS's `vertical-align`.
        let kind = if v > 0.0 { "superscript" } else { "subscript" };
        parts.push(format!("{kind}({v:+}pt)"));
    }
    // `Normal` is suppressed rather than printed: it is the width every
    // font already has, so naming it adds a token to every run without
    // distinguishing any of them.
    if let Some(st) = s.stretch.filter(|st| *st != Stretch::Normal) {
        parts.push(format!("stretch={st:?}"));
    }
    if parts.is_empty() {
        "unstyled".to_owned()
    } else {
        parts.join(",")
    }
}

/// `reset-form`: restore form fields to their defaults (§12.7.5.3).
///
/// # Why this shows the damage before doing it
///
/// A reset DISCARDS what the operator typed, and unlike a fill it does so to
/// many fields at once. `fill-field` writes one named value and the operator
/// can see what they asked for; `reset-form` with no arguments touches
/// everything, and "everything" is exactly the scope where a wrong guess is
/// unrecoverable from the command line.
///
/// So the default lists the fields it would clear and writes nothing. That is
/// the same shape as `recompute`, and for a stronger reason: recompute's
/// mistake is a wrong number, this one's is lost data.
///
/// # Output contract
///
/// One line per field, then a summary, all locale-invariant:
///
/// ```text
/// reset  field="Keep" from="typed" to="factory" source=default
/// reset  field="Drop" from="typed" to=<removed> source=none
/// skip   field="Push" reason=pushbutton
/// reset-form <path> reset=2 defaulted=1 removed=1 skipped=1 applied=0
/// ```
///
/// `to=<removed>` rather than `to=""` on purpose: the clause removes the key,
/// and an operator reading `to=""` would reasonably expect an empty string in
/// the file.
pub(crate) fn cmd_reset_form(
    input: &Path,
    fields: &[String],
    apply: bool,
    output: Option<&Path>,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    if apply && output.is_none() {
        eprintln!("pdfcer: --apply needs --output");
        return exit::EDIT_REFUSED;
    }
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let only = (!fields.is_empty()).then_some(fields);

    // The preview comes from the core so the dry run and the GUI cannot
    // drift on which fields a reset touches.
    let preview = session.reset_preview(only.map(<[String]>::as_ref));
    if preview.is_empty() {
        eprintln!(
            "pdfcer: {}: the document has no interactive form",
            input.display()
        );
        return exit::EDIT_REFUSED;
    }
    let clearing = print_reset_preview(&preview);

    if !apply {
        print_reset_dry_run_summary(input, &preview, clearing);
        return exit::SUCCESS;
    }

    let out = match session.reset_form(only.map(<[String]>::as_ref)) {
        Ok(out) => out,
        Err(err) => return report_edit_error(input, &err),
    };
    print_layout("reset-form", &out.layout);
    let Some(output) = output else {
        eprintln!("pdfcer: --apply needs --output");
        return exit::EDIT_REFUSED;
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
    print_reset_applied(input, &out);
    finish_edit(input, &outcome)
}

/// Prints one line per previewed field and returns how many would change.
fn print_reset_preview(preview: &[pdfcer_core::edit::ResetPreviewRow]) -> usize {
    let mut clearing = 0usize;
    for row in preview {
        if let Some(reason) = row.ineligible {
            // string-gap-exempt: aligned status column in a machine-readable report
            println!("skip   field={:?} reason={}", row.field, reason.token());
            continue;
        }
        if !row.would_change {
            // string-gap-exempt: aligned status column in a machine-readable report
            println!("ok     field={:?} reason=already_default", row.field);
            continue;
        }
        clearing += 1;
        // `<removed>` rather than `""`: the clause removes the KEY, and
        // `to=""` would read as an empty string in the file.
        let to = if row.would_remove {
            "<removed>".to_owned()
        } else {
            format!("{:?}", row.target)
        };
        println!(
            "reset  field={:?} from={:?} to={to} source={}",
            row.field,
            row.current,
            if row.would_remove { "none" } else { "default" },
        );
    }
    clearing
}

fn print_reset_dry_run_summary(
    input: &Path,
    preview: &[pdfcer_core::edit::ResetPreviewRow],
    clearing: usize,
) {
    println!(
        "reset-form {} reset={clearing} defaulted={} removed={} skipped={} applied=0",
        input.display(),
        preview
            .iter()
            .filter(|r| r.would_change && !r.would_remove)
            .count(),
        preview
            .iter()
            .filter(|r| r.would_change && r.would_remove)
            .count(),
        preview.iter().filter(|r| r.ineligible.is_some()).count(),
    );
    eprintln!(
        "pdfcer: {}: nothing was written. The lines above are what a reset WOULD \
clear. Re-run with --apply --output FILE to perform it.",
        input.display()
    );
}

/// `widgets` is controls redrawn on the page, `reset` values changed in
/// the form; a field shown in several places makes them differ.
fn print_reset_applied(input: &Path, out: &pdfcer_core::edit::ResetOutcome) {
    println!(
        "reset-form {} reset={} defaulted={} removed={} widgets={} skipped={} applied={}",
        input.display(),
        out.fields_reset,
        out.values_defaulted,
        out.values_removed,
        out.widgets_updated,
        out.skipped_pushbuttons + out.skipped_signatures + out.skipped_read_only,
        out.fields_reset,
    );
    if out.skipped_signatures > 0 {
        eprintln!(
            "pdfcer: {}: {} signature field(s) were left alone — a signature's value IS \
the signature, and removing it would destroy it.",
            input.display(),
            out.skipped_signatures,
        );
    }
}

/// `promote-dr-fonts`: `EditSession::promote_inline_dr_fonts`, then save.
pub(crate) fn cmd_promote_dr_fonts(
    input: &Path,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let promoted = match session.promote_inline_dr_fonts() {
        Ok(n) => n,
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
    println!(
        "promote-dr-fonts {} -> {} promoted={promoted}",
        input.display(),
        output.display()
    );
    finish_edit(input, &outcome)
}

/// `regenerate-appearances`: rebuild widget appearances and clear
/// /NeedAppearances (Pass 7.1, R51).
pub(crate) fn cmd_regenerate_appearances(input: &Path, output: &Path, mode: SaveMode) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let outcome = match session.regenerate_appearances() {
        Ok(o) => o,
        Err(err) => return report_edit_error(input, &err),
    };
    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(o) => o,
        Err(code) => return code,
    };
    println!(
        "regenerate-appearances {} regenerated={} need_appearances_cleared={} mode={} -> {}; \
objects={} out_bytes={}",
        input.display(),
        outcome.regenerated,
        u32::from(outcome.need_appearances_cleared),
        mode.name(),
        output.display(),
        saved.report.objects_written,
        saved.report.bytes_written,
    );
    finish_edit(input, &saved)
}

/// `flatten`: burn form fields into page content and remove them (Pass 7.1,
/// R48). `--full-rewrite` maps to a single-revision full save that removes
/// even the prior-revision-recoverable pre-flatten data.
pub(crate) fn cmd_flatten(
    input: &Path,
    fields: &[String],
    output: &Path,
    full_rewrite: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let names: Option<Vec<&str>> = if fields.is_empty() {
        None
    } else {
        Some(fields.iter().map(String::as_str).collect())
    };
    let outcome = match session.flatten_fields(names.as_deref()) {
        Ok(o) => o,
        Err(err) => return report_edit_error(input, &err),
    };
    let mode = if full_rewrite {
        SaveMode::Full
    } else {
        SaveMode::Incremental
    };
    if !full_rewrite {
        eprintln!(
            "pdfcer: {}: flatten saved incrementally — the pre-flatten field values remain \
recoverable in the prior revision. Re-run with --full-rewrite to remove them physically (R48).",
            input.display()
        );
    }
    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(o) => o,
        Err(code) => return code,
    };
    println!(
        "flatten {} fields_flattened={} widgets_burned={} pages_touched={} mode={} -> {}; \
objects={} out_bytes={}",
        input.display(),
        outcome.fields_flattened,
        outcome.widgets_burned,
        outcome.pages_touched,
        mode.name(),
        output.display(),
        saved.report.objects_written,
        saved.report.bytes_written,
    );
    finish_edit(input, &saved)
}

/// The on-state name a check-box/radio convenience alias (`on`/`true`/…)
/// selects: the field's first widget's first non-`Off` on-state, or `Yes`
/// (the §12.7.4.2.3 convention) when none is discoverable.
pub(crate) fn resolve_on_state(form: &pdfcer_core::forms::AcroForm, name: &str) -> String {
    form.field_by_name(name)
        .and_then(|f| f.widgets.iter().find_map(|w| w.on_states.first()))
        .map_or_else(
            || "Yes".to_owned(),
            |s| String::from_utf8_lossy(s).into_owned(),
        )
}

/// Classify one modelled annotation for `list-annotations`, returning
/// `(disposition, ap_shape)` in the render path's precedence order (Popup
/// → suppressed → appearance selection). Model-level only — the degenerate
/// -placement refusal needs the appearance stream's geometry and is a
/// `render-page` concern.
pub(crate) fn classify_for_listing(
    annot: &pdfcer_core::annot::Annotation,
) -> (&'static str, &'static str) {
    use pdfcer_core::annot::Appearance;
    if annot.is_popup {
        return ("popup", "none");
    }
    if annot.flags.suppressed_on_screen() {
        // Still report the appearance shape it *would* have, so a hidden
        // annotation's nature is disclosed (R50).
        let shape = match annot.appearance {
            Appearance::Normal { .. } => "stream",
            Appearance::StateUnresolved => "state-dict",
            Appearance::None => "none",
        };
        return ("suppressed", shape);
    }
    match annot.appearance {
        Appearance::Normal { .. } => {
            if annot.rect.is_some() {
                ("paint-ready", "stream")
            } else {
                ("no-rect", "stream")
            }
        }
        Appearance::None => ("no-ap", "none"),
        Appearance::StateUnresolved => ("state-missing", "state-dict"),
    }
}

/// Replace ASCII whitespace in a token with `_` so a stable stdout line
/// stays splittable on spaces.
///
/// # ⚠ ONLY for tokens that are genuinely PDF NAME OBJECTS (§7.3.5)
///
/// Annotation subtypes (`/Widget`, `/Redact`) are name objects, and §7.3.5
/// writes a space in one as `#20`, so a decoded subtype containing raw
/// whitespace really is pathological and mangling it costs nothing.
///
/// **It must never be applied to a §7.9.2 TEXT STRING.** This function's
/// doc comment used to assert the §7.3.5 rule as though it covered every
/// caller, and on that reasoning it was applied to a form field's `/T`, its
/// `/V` and a widget's `/MK` `/CA` — none of which are name objects and all
/// of which may legally contain spaces.
///
/// The cost, measured on a real government form (2026-08-09): `list-fields`
/// printed `Home_Phone` for a field whose `/T` is `Home Phone`, while every
/// write verb's `--name` documents itself as taking the name *"as
/// `list-fields` reports it"*. The discovery path emitted names the write
/// path rejected, for the majority of fields on any Acrobat-authored form —
/// Acrobat derives field names from nearby label text, so spaces are the
/// norm rather than the exception. Those columns are now debug-quoted; see
/// the comment at their construction.
pub(crate) fn sanitize_token(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .collect()
}

/// Map a [`PdfError`] to the CLI's documented exit code. The wildcard arm
/// exists because `PdfError` is `#[non_exhaustive]` (later Passes add
/// variants) — unmapped future errors report the generic runtime code
/// rather than failing to compile.
pub(crate) fn exit_code_for(err: &PdfError) -> u8 {
    match err {
        PdfError::Io(_) => exit::IO_ERROR,
        PdfError::MissingHeader { .. } | PdfError::MalformedVersion { .. } => exit::NOT_A_PDF,
        _ => exit::RUNTIME_ERROR,
    }
}

/// Map a `DocError` (full-document load) to the CLI's exit code.
///
/// Only two cases are more specific than "something went wrong": the file
/// was unreadable ([`exit::IO_ERROR`]) and the file is not a PDF at all
/// ([`exit::NOT_A_PDF`], delegated to [`exit_code_for`] since the header
/// probe's own error type carries that distinction).
///
/// Everything else — a broken cross-reference chain, an object that does
/// not match its xref entry, a missing `/Root`, and the deliberate
/// xref-stream / hybrid-reference *"not yet supported"* refusals — is
/// [`exit::RUNTIME_ERROR`]. That last group is worth naming: those files
/// are perfectly valid PDFs that this build declines to open, and the
/// distinction is currently carried by the **stderr message**, not by the
/// exit code. If a script ever needs to branch on "unsupported structure"
/// versus "corrupt file", that earns a new, dedicated exit code rather
/// than a broadened meaning for an existing one.
pub(crate) fn exit_code_for_doc(err: &pdfcer_core::document::DocError) -> u8 {
    use pdfcer_core::document::DocError;
    match err {
        DocError::Io(_) => exit::IO_ERROR,
        DocError::Header(inner) => exit_code_for(inner),
        _ => exit::RUNTIME_ERROR,
    }
}

/// Print what a regenerated text/choice appearance decided on the operator's
/// behalf (rule 4): auto-size, a narrowed `/DA` colour, unencodable
/// characters. `subject` opens each line, e.g. `field "a.b"`.
pub(crate) fn print_layout_disclosure(
    subject: &str,
    autosize: Option<f64>,
    bound: Option<pdfcer_core::vartext::AutoFitBound>,
    colour_unmodelled: bool,
    unencodable_chars: usize,
) {
    if let Some(sz) = autosize {
        // NAMES THE CONSTRAINT THAT BOUND, at a consuming shell's request:
        // an operator who thinks the text is too small wants to know whether
        // to widen the box or heighten it, and those are different answers.
        // The old wording — "a reviewable pdfcer heuristic" — was accurate when
        // the answer was a flat 12 pt and stopped being the useful thing to
        // say once it became a fit.
        let why = match bound {
            Some(pdfcer_core::vartext::AutoFitBound::Height) => {
                "fitted to the field's HEIGHT; make the box taller to change it"
            }
            Some(pdfcer_core::vartext::AutoFitBound::Width) => {
                "shrunk to fit the field's WIDTH — the text was too long at the \
height-derived size"
            }
            Some(pdfcer_core::vartext::AutoFitBound::Floor) => {
                "held at pdfcer's legibility FLOOR — the box is too small for the text, \
which will overflow"
            }
            // A multiline field still takes the older height-only route, so
            // there is no bound to name and claiming one would be a fact pdfcer
            // did not establish.
            None => "a reviewable pdfcer heuristic; §12.7.3.3 mandates no formula",
            // `AutoFitBound` is `#[non_exhaustive]`, so a catch-all is forced
            // from outside `pdfcer-core` and the compile-time guarantee this
            // match wanted is unavailable. It resolves to the SAME wording as
            // the no-bound case rather than inventing a description of a
            // constraint this binary has never heard of — saying nothing is
            // the honest failure here, and saying "height" would be a guess
            // presented as a measurement.
            Some(_) => "a reviewable pdfcer heuristic; §12.7.3.3 mandates no formula",
        };
        eprintln!("pdfcer: {subject}: auto-sized to {sz:.3} pt ({why})");
    }
    if colour_unmodelled {
        // Rule 4: pdfcer substituted a colour the FILE DID NOT ASK FOR into
        // an appearance it wrote into the document. The `/DA` named a
        // `/Separation`, `/DeviceN`, `/ICCBased`, `/Indexed` or `/Lab`
        // colour, none of which this generator can emit, so the text was
        // painted in §8.6.8's default black.
        //
        // Before `Pass 221.0` the parser aliased that onto "the /DA set no
        // colour" -- which ALREADY meant "render black" -- so the narrowing
        // was indistinguishable from the file's own instruction and nothing
        // could report it.
        eprintln!(
            "pdfcer: {subject}: the field's default appearance names a colour space pdfcer cannot emit (Separation/DeviceN/ICCBased/Indexed/Lab), so this appearance was generated in BLACK -- a narrowing, not the colour the file asked for"
        );
    }
    if unencodable_chars > 0 {
        eprintln!(
            "pdfcer: {subject}: {} character(s) had no WinAnsi code and were substituted \
with '?' (Base-14 Latin only)",
            unencodable_chars
        );
    }
}

/// [`print_layout_disclosure`] for a verb that redrew as a side effect.
pub(crate) fn print_layout(subject: &str, l: &pdfcer_core::edit::LayoutDisclosure) {
    print_layout_disclosure(
        subject,
        l.applied_autosize,
        l.applied_autosize_bound,
        l.da_colour_unmodelled,
        l.unencodable_chars,
    );
}
