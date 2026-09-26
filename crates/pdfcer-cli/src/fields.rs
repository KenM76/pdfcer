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

/// `list-fields`: inventory a document's AcroForm fields (Pass 7).
///
/// Read-only. One `field …` line per terminal field, then a `list-fields …`
/// summary line carrying the document-level form disclosures. The value is
/// emitted as a sanitised token so the line stays field-splittable.
pub(crate) fn cmd_list_fields(
    input: &Path,
    fillable_only: bool,
    rich_text: bool,
    widgets: bool,
) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let Some(form) = pdfcer_core::forms::parse_acroform(&doc) else {
        // No form is not an error — report zero and exit clean, so a batch
        // sweep can tally form-bearing vs form-free files.
        //
        // BUT THE ACTIONS ARE STILL REPORTED, and that is the point of
        // `Pass 133.0`. Actions are a DOCUMENT property, not a forms
        // property: a file whose only hazard is a `/Launch` on a bookmark or
        // a `/URI` on a link has no AcroForm at all, and this branch used to
        // return before saying so. Worse, `inspect`'s own hazard line points
        // the operator HERE for the breakdown — so on exactly the documents
        // where the breakdown matters most, the pointer led to one word.
        let js = pdfcer_core::forms::scan_javascript(&doc);
        println!(
            "list-fields {} fields=0 no_acroform=1 js_network_actions={} \
js_launch_actions={} annot_actions={} chained_actions={} page_trigger_actions={} \
outline_actions={} js_actions_anywhere={} actions_scanned={} action_scan_truncated={}",
            input.display(),
            js.network_action_count,
            js.launch_action_count,
            js.annotation_actions,
            js.chained_actions,
            js.page_trigger_actions,
            js.outline_actions,
            js.javascript_actions,
            js.actions_scanned,
            u32::from(js.scan_truncated),
        );
        if js.reaches_outside() {
            eprintln!(
                "pdfcer: {}: this document has NO form, and still carries {} network and {} process-launch action trigger(s) that Adobe Acrobat/Reader would run; pdfcer recognizes them but NEVER executes any (R12/R13/R54).",
                input.display(),
                js.network_action_count,
                js.launch_action_count,
            );
        }
        return exit::SUCCESS;
    };

    let mut fields_with_aa = 0usize;
    let mut shown = 0usize;
    for field in &form.fields {
        if field.has_additional_actions {
            fields_with_aa += 1;
        }
        if fillable_only && !field.is_fillable() {
            continue;
        }
        shown += 1;
        let ty = match field.field_type {
            Some(pdfcer_core::forms::FieldType::Button) => "Btn",
            Some(pdfcer_core::forms::FieldType::Text) => "Tx",
            Some(pdfcer_core::forms::FieldType::Choice) => "Ch",
            Some(pdfcer_core::forms::FieldType::Signature) => "Sig",
            None => "none",
        };
        let button = match field.button_kind {
            Some(pdfcer_core::forms::ButtonKind::Push) => "push",
            Some(pdfcer_core::forms::ButtonKind::Check) => "check",
            Some(pdfcer_core::forms::ButtonKind::Radio) => "radio",
            None => "-",
        };
        // QUOTED, NOT WHITESPACE-MANGLED — and this was a real defect.
        //
        // These three columns carry §7.9.2 TEXT STRINGS (`/T`, `/V`, `/MK`
        // `/CA`), and a text string may contain spaces. They used to run
        // through `sanitize_token`, whose doc comment justified itself with
        // *"names cannot legally contain whitespace (§7.3.5 uses `#20` for a
        // space), so this only fires on pathological input."* §7.3.5 governs
        // **name objects** (`/Foo`). It has nothing to say about `/T`.
        //
        // What that cost, measured on a real form (Arizona courts' Health
        // Care Power of Attorney, 2026-08-09): `/T` values of `Home Phone`,
        // `Address 1_3`, `Cell Phone_2` and eleven more printed as
        // `Home_Phone`, `Address_1_3`, `Cell_Phone_2` — and this verb's own
        // help calls its output *"also how `fill-field` and `list-fields`
        // refer to it"*, while `--name` on every write verb says *"as
        // `list-fields` reports it"*. So for every field whose name contains
        // a space — which on Acrobat-authored forms is most of them, because
        // Acrobat derives field names from nearby label text — the
        // documented discovery path emitted a name that `fill-field`,
        // `rename-field`, `delete-field`, `delete-widget` and `move-widget`
        // all reject with "no fillable form field with the fully-qualified
        // name". Five of that form's six broken fields were unreachable.
        //
        // Debug-quoting is not a new convention: `delete-widget`,
        // `rename-field` and `delete-field` already print `name={:?}` in
        // their own result lines. This verb — the DISCOVERY one, the only
        // one whose output is meant to be fed back in — was the odd one out.
        //
        // The bare sentinels stay bare, so `-` (absent) stays distinguishable
        // from `""` (present and empty), which quoting everything would have
        // merged.
        let name = if field.fully_qualified_name.is_empty() {
            "(unnamed)".to_owned()
        } else {
            format!("{:?}", field.fully_qualified_name)
        };
        let value = {
            let v = field.value.display_text();
            if v.is_empty() {
                "-".to_owned()
            } else {
                format!("{v:?}")
            }
        };
        // `/MK` `/CA`, from the first widget that has one. Appended LAST so
        // a parser reading through `aa=` is unaffected.
        //
        // Worth a column of its own because `value=` cannot carry it: a push
        // button has no `/V` in any state (§12.7.4.2.2), so without this
        // every push button in a form lists identically and the only thing
        // telling *Submit* from *Reset* is a string inside an appearance
        // stream. `-` for a field with no caption, which for a non-button is
        // every one of them.
        let caption = field
            .widgets
            .iter()
            .find_map(|w| w.caption.as_deref())
            .map_or_else(
                || "-".to_owned(),
                |c| format!("{:?}", String::from_utf8_lossy(c)),
            );
        // The field's rich text, parsed once and used for both the row's
        // compact token and the optional detail below.
        //
        // Parsed from `/RV` UNGATED by the RichText flag, matching the
        // export path: a file may legally-ish carry `/RV` with bit 26
        // clear, and that is precisely the case where reporting "no
        // formatting" would hide the only copy of it. A parse failure
        // reports as `rich=unparsed` rather than as absent, because "this
        // field has formatting pdfcer could not read" and "this field has
        // no formatting" are different facts and only one of them is a
        // reason to stop.
        let runs = field.rich_value.as_ref().map(|rv| {
            let ds = field
                .default_style
                .as_ref()
                .map(|d| String::from_utf8_lossy(d).into_owned());
            String::from_utf8(rv.clone())
                .map_err(|_| "not UTF-8".to_owned())
                .and_then(|s| {
                    pdfcer_core::richtext::parse(&s, ds.as_deref()).map_err(|e| e.to_string())
                })
        });
        let rich = match &runs {
            None => "-".to_owned(),
            Some(Ok(r)) => format!("{}runs", r.len()),
            Some(Err(_)) => "unparsed".to_owned(),
        };

        println!(
            "field name={name} type={ty} button={button} flags=0x{:X} value={value} \
widgets={} ap={} fillable={} readonly={} aa={} caption={caption} rich={rich}",
            field.flags.0,
            field.widgets.len(),
            u32::from(field.has_appearance()),
            u32::from(field.is_fillable()),
            u32::from(field.flags.read_only()),
            u32::from(field.has_additional_actions),
        );

        // `Pass 146.0`. Per WIDGET, because the border and the visibility
        // belong to the annotation box rather than to the field.
        if widgets {
            for (i, w) in field.widgets.iter().enumerate() {
                let rect = w.rect.map_or_else(
                    || "-".to_owned(),
                    |r| format!("[{:.1} {:.1} {:.1} {:.1}]", r.llx, r.lly, r.urx, r.ury),
                );
                // `-` is "THE FILE STATES NONE", never "solid 1 pt". See the
                // flag's own help, and `forms::Widget::border`.
                let border = w.border.as_ref().map_or_else(
                    || "-".to_owned(),
                    |b| format!("{}/{:.2}", String::from_utf8_lossy(b.style.name()), b.width),
                );
                // `other` rather than a nearest match: the four are what pdfcer
                // can SET, and a widget outside them is not one of the four
                // with a detail dropped.
                let visibility = match w.visibility {
                    Some(pdfcer_core::edit::Visibility::VisibleAndPrints) => "visible+print",
                    Some(pdfcer_core::edit::Visibility::ScreenOnly) => "screen-only",
                    Some(pdfcer_core::edit::Visibility::PrintOnly) => "print-only",
                    Some(pdfcer_core::edit::Visibility::Hidden) => "hidden",
                    _ => "other",
                };
                let state = w.appearance_state.as_deref().map_or_else(
                    || "-".to_owned(),
                    |n| String::from_utf8_lossy(n).into_owned(),
                );
                // `/MK /R` (Table 189), `Pass 177.0`. `-` means the file is SILENT,
                // which is not the same fact as `0`: Table 189 defaults `/R` to 0, so
                // a silent file renders upright -- but a control seeded from `0` would
                // write that invention back on the first press. The same distinction
                // `border` makes one column to the left.
                let rotation = w.rotation.map_or_else(|| "-".to_owned(), |d| d.to_string());
                // `/MK` `/BG` and `/BC` (Table 189). `-` is the key ABSENT;
                // `none` is the empty array, which the standard defines as
                // stating no colour. Different facts about the file, kept
                // apart by the read model, so kept apart here too — the same
                // distinction `border` and `rotation` already make.
                //
                // `background` had been readable since `Pass 249.1` and was
                // never printed, while `docs/FEATURES.md` claimed `cli [x]`
                // for it. This line is what makes that tick true.
                println!(
                    "  widget {i} obj={} rect={rect} border={border} rotation={rotation} \
visibility={visibility} flags=0x{:X} state={state} merged={} background={} border_color={}",
                    w.id.num,
                    w.annot_flags.0,
                    u32::from(w.merged),
                    mk_colour_token(w.background),
                    mk_colour_token(w.border_color),
                );
            }
        }

        if rich_text {
            match &runs {
                None => {}
                Some(Ok(r)) => {
                    for (i, run) in r.iter().enumerate() {
                        println!(
                            "  run {i} p={} text={:?} style={}",
                            run.paragraph,
                            run.text,
                            describe_style(&run.style),
                        );
                    }
                }
                // Named, not swallowed. A field whose formatting pdfcer
                // cannot read is the one an operator most needs told
                // about, since every downstream decision about it is
                // being made blind.
                Some(Err(e)) => println!("  rich text could not be read: {e}"),
            }
        }
    }

    let xfa = match form.xfa {
        pdfcer_core::forms::XfaPresence::None => "none".to_owned(),
        pdfcer_core::forms::XfaPresence::Stream => "stream".to_owned(),
        pdfcer_core::forms::XfaPresence::PacketArray { packets } => format!("packets:{packets}"),
    };
    // Decision 009 posture-A JavaScript disclosure histogram (recognition
    // only — pdfcer NEVER executes any of it). Network/launch action counts
    // flag the R12/R13 hazards loudly.
    let js = pdfcer_core::forms::scan_javascript(&doc);
    println!(
        "list-fields {} fields={} shown={shown} need_appearances={} sig_flags=0x{:X} \
calc_order={} fields_with_aa={fields_with_aa} xfa={xfa} default_resources={} \
js_calc={} js_format={} js_validate={} js_keystroke={} js_custom={} js_doc_level={} \
open_action_js={} js_network_actions={} js_launch_actions={} annot_actions={} \
chained_actions={} page_trigger_actions={} outline_actions={} js_actions_anywhere={} \
actions_scanned={} action_scan_truncated={}",
        input.display(),
        form.fields.len(),
        u32::from(form.need_appearances),
        form.sig_flags,
        form.calc_order_count,
        u32::from(form.has_default_resources),
        js.fields_with_calculate_script,
        js.fields_with_format_script,
        js.fields_with_validate_script,
        js.fields_with_keystroke_script,
        js.custom_scripts,
        js.doc_level_scripts,
        u32::from(js.open_action_is_javascript),
        js.network_action_count,
        js.launch_action_count,
        // `Pass 133.0`. Appended per the stable-line append-never-reorder
        // rule. The first four are carriers the scan used to miss entirely;
        // the last two are the honesty pair — a hazard count of zero from a
        // truncated scan is not the same fact as one from a complete scan,
        // and a reader with only the first number cannot tell them apart.
        js.annotation_actions,
        js.chained_actions,
        js.page_trigger_actions,
        js.outline_actions,
        // The superset counter: a JavaScript action ANYWHERE, not only the
        // four field-level hooks. A page-open script used to report zero in
        // every script counter this line had.
        js.javascript_actions,
        js.actions_scanned,
        u32::from(js.scan_truncated),
    );
    if js.reaches_outside() {
        eprintln!(
            "pdfcer: {}: this form carries {} network and {} process-launch action trigger(s) \
that Adobe Acrobat/Reader would run; pdfcer recognizes them but NEVER executes any (R12/R13/R54).",
            input.display(),
            js.network_action_count,
            js.launch_action_count,
        );
    }
    exit::SUCCESS
}

/// `recompute`: natively recompute recognised calculation scripts.
///
/// # Plan first, apply second — and that is not a convenience
///
/// Without `--apply` this writes nothing. Decision 009 §5.1 makes a recompute
/// an operator-invoked act rather than a side effect, and project rule 4
/// requires anything pdfcer inferred to be visible before it becomes document
/// state. A recomputed total is an inference: pdfcer read a script it did not
/// run and reproduced what it believes the script means.
///
/// On a batch surface the dry run is doing more work than on a screen. A
/// script that pipes `recompute --apply` across a directory has no operator
/// watching; the plan is what makes it possible to look first.
///
/// # Output contract
///
/// One `change` line per field, one `skip` line per recognised calculation
/// left alone, then a summary. All locale-invariant and stable across runs:
///
/// ```text
/// change field="Total" from="0" to="132.5" op=SUM operands=2 coerced=0
/// skip   field="Bad" reason=refused detail="..."
/// recompute <path> changes=1 skipped=1 order=calc_order applied=0
/// ```
///
/// # Exit status
///
/// A dry run with changes pending still exits `SUCCESS`: it did what it was
/// asked and found work. Distinguishing "nothing to do" from "changes
/// pending" is what the `changes=` count is for — an exit code that varied
/// would make `recompute` unusable in a `set -e` script that only wanted the
/// report.
pub(crate) fn cmd_recompute(
    input: &Path,
    apply: bool,
    output: Option<&Path>,
    mode: SaveMode,
    policy: pdfcer_core::form_script::calc::CommaPolicy,
    verify_undo: bool,
) -> u8 {
    use pdfcer_core::form_script::recompute::{OrderSource, Skip};

    if apply && output.is_none() {
        eprintln!("pdfcer: --apply needs --output");
        return exit::EDIT_REFUSED;
    }

    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let plan = {
        let view = session.view();
        pdfcer_core::form_script::recompute::plan(&view, policy)
    };

    for change in &plan.changes {
        println!(
            "change field={:?} from={:?} to={:?} op={} operands={} coerced={}",
            change.field,
            change.previous,
            change.proposed,
            change.computation.op.code(),
            change.computation.operands.len(),
            change.computation.coerced_operands(),
        );
    }
    for skipped in &plan.skipped {
        let reason = match skipped.reason {
            Skip::Refused(_) => "refused",
            Skip::CircularDependency => "circular",
            Skip::AlreadyCorrect => "already_correct",
            Skip::NotAValueField => "not_a_value_field",
        };
        println!(
            // string-gap-exempt: aligned status column in a machine-readable report
            "skip   field={:?} reason={reason} detail={:?}",
            skipped.field,
            skipped.reason.to_string(),
        );
    }

    let order = match plan.order_source {
        OrderSource::CalculationOrder => "calc_order",
        OrderSource::Mixed => "mixed",
        OrderSource::Derived => "derived",
        OrderSource::Empty => "none",
    };

    // The caveats, on stderr, before any write. Each is a fact that changes
    // how much the numbers should be trusted, and burying them under the
    // summary line would put them after the thing they qualify.
    if plan.order_source.is_pdfcer_choice() {
        eprintln!(
            "pdfcer: {}: this form has {} calculated field(s) its /CO array does not \
list, which ISO 32000-1 requires it to. The standard gives no recovery rule, so pdfcer \
ordered them by their own dependencies — another reader may legitimately compute \
different values.",
            input.display(),
            plan.unlisted_calculations,
        );
    }
    if plan.coerced_operands() > 0 {
        eprintln!(
            "pdfcer: {}: {} operand(s) were blank or non-numeric and counted as zero, \
matching Acrobat. The totals are arithmetically correct for a partly-empty form.",
            input.display(),
            plan.coerced_operands(),
        );
    }
    if plan.not_reproducible > 0 {
        eprintln!(
            "pdfcer: {}: {} script(s) were NOT considered — pdfcer recognises no \
built-in in them, so their fields keep the values last saved. Run list-scripts to see \
which.",
            input.display(),
            plan.not_reproducible,
        );
    }

    if !apply {
        println!(
            "recompute {} changes={} skipped={} order={order} applied=0",
            input.display(),
            plan.changes.len(),
            plan.skipped.len(),
        );
        if !plan.is_empty() {
            eprintln!(
                "pdfcer: {}: nothing was written. Re-run with --apply --output FILE to \
store these values. The source scripts stay in the file either way, so a \
JavaScript-running reader still recomputes independently.",
                input.display()
            );
        }
        return exit::SUCCESS;
    }

    for change in &plan.changes {
        if let Err(err) = session.fill_text_field(&change.field, &change.proposed) {
            return report_edit_error(input, &err);
        }
    }

    let Some(output) = output else {
        // Unreachable: guarded at entry. Handled rather than unwrapped so a
        // future edit to the guard cannot turn this into a panic.
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
    println!(
        "recompute {} changes={} skipped={} order={order} applied={}",
        input.display(),
        plan.changes.len(),
        plan.skipped.len(),
        plan.changes.len(),
    );
    finish_edit(input, &outcome)
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

    // The preview comes from the CORE, not from a second copy of the
    // eligibility rule written here. The GUI panel and this dry run were
    // each deriving it independently until `reset_preview` existed, which is
    // two implementations of one rule free to drift — R171's exact shape,
    // and the drift would have shown only as the CLI and the GUI disagreeing
    // about how many fields a reset touches.
    let preview = session.reset_preview(only.map(<[String]>::as_ref));
    if preview.is_empty() {
        eprintln!(
            "pdfcer: {}: the document has no interactive form",
            input.display()
        );
        return exit::EDIT_REFUSED;
    }
    let mut clearing = 0usize;
    for row in &preview {
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
        // `<removed>` rather than `""`: the clause removes the KEY, and an
        // operator reading `to=""` would reasonably expect an empty string in
        // the file.
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

    if !apply {
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
        return exit::SUCCESS;
    }

    let out = match session.reset_form(only.map(<[String]>::as_ref)) {
        Ok(out) => out,
        Err(err) => return report_edit_error(input, &err),
    };
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
    println!(
        "reset-form {} reset={} defaulted={} removed={} widgets={} skipped={} applied={}",
        input.display(),
        out.fields_reset,
        out.values_defaulted,
        out.values_removed,
        // `Pass 110.0`. Computed since the verb existed and printed by
        // nothing. It is NOT `reset` under another name: a field presented in
        // several places has more widgets than fields, so this is how many
        // CONTROLS changed on the page against how many VALUES changed in the
        // form. An operator reconciling a reset against what he can see needs
        // the first number, and had only the second.
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
    finish_edit(input, &outcome)
}

/// `list-scripts`: classify every form-field script (decision 009 posture B).
///
/// # Why this exists as its own subcommand rather than more columns on
/// `list-fields`
///
/// `list-fields` already prints a posture-A *histogram* — how many fields
/// calculate, how many format, how many are custom. That answers "is this
/// form script-driven?" and stops. The question posture B raises is
/// per-field and has four parts: **which** field, on **which trigger**,
/// recognised as **what**, and **can pdfcer reproduce it**. Four facts per
/// script do not fit on a summary line, and folding them in would make the
/// summary line unstable in width — the property that makes it greppable.
///
/// # Output contract
///
/// One line per script, fields space-separated `key=value`, locale-invariant
/// and stable across runs:
///
/// ```text
/// script field=Total trigger=calculate helper=AFSimple_Calculate reproducible=1 source=string bytes=38
/// ```
///
/// Then one summary line with the histogram. A form with no scripts prints
/// the summary with a zero count rather than nothing at all — silence would
/// be indistinguishable from a failed read, and "this form has no scripts"
/// is a positive finding worth stating (R162's shape: an absence claim is
/// only meaningful once the reader has shown it can find the thing).
///
/// # The non-execution disclaimer is unconditional
///
/// Printed to stderr whenever any script exists, whether or not pdfcer
/// recognised any of them. An operator reading a list of recognised Acrobat
/// built-ins is at their most likely to assume the values on the page are
/// live, and that is exactly the moment to say they are not.
pub(crate) fn cmd_list_scripts(input: &Path, reproducible_only: bool) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let view = pdfcer_core::view::DocumentView::new(&doc, doc.bytes(), doc.version());
    let inv = pdfcer_core::form_script::inventory::inventory(&view);

    let mut shown = 0usize;
    for s in &inv.scripts {
        if reproducible_only && !s.is_reproducible() {
            continue;
        }
        shown += 1;
        let source = match s.source {
            pdfcer_core::form_script::inventory::ScriptSource::LiteralString => "string",
            pdfcer_core::form_script::inventory::ScriptSource::Stream => "stream",
            pdfcer_core::form_script::inventory::ScriptSource::Unreadable => "unreadable",
        };
        // The field name is quoted because a fully-qualified name may
        // contain spaces (`/T` is a text string, not a name object), and an
        // unquoted one would silently break the key=value parse this line
        // promises.
        println!(
            "script field={:?} trigger={} helper={} reproducible={} source={source} bytes={}",
            s.field,
            s.trigger.token(),
            s.class.token(),
            u32::from(s.is_reproducible()),
            s.length,
        );
    }

    let histogram = inv
        .histogram()
        .iter()
        .map(|(token, n)| format!("{token}={n}"))
        .collect::<Vec<_>>()
        .join(" ");
    println!(
        "list-scripts {} scripts={} shown={shown} reproducible={} {histogram}",
        input.display(),
        inv.scripts.len(),
        inv.reproducible().count(),
    );

    if !inv.scripts.is_empty() {
        eprintln!(
            "pdfcer: {}: this form carries {} script(s) that Adobe Acrobat/Reader would \
run. pdfcer NEVER executes any of them (R53/R54). A recognised built-in is read, not run; \
its stored value is shown as last saved and may be stale until you recompute it.",
            input.display(),
            inv.scripts.len(),
        );
    }
    exit::SUCCESS
}

/// `fill-field`: set form-field values and save (Pass 7).
///
/// Each `NAME=VALUE` is dispatched by the field's modelled type: text/choice
/// through [`EditSession::fill_text_field`], check-box/radio through
/// [`EditSession::set_button_state`]. All assignments land in one session
/// (so the save carries them as one incremental revision), then the shared
/// [`save_edited`]/[`finish_edit`] plumbing writes and reports.
///
/// # `downgrade_rich_text`, and why the CLI needed it
///
/// Until this flag existed, a rich-text field was **unfillable from the
/// CLI at all**: this function called [`EditSession::fill_text_field`],
/// which refuses one with [`EditError::FieldIsRichText`], and never
/// exposed [`EditSession::fill_text_field_downgrading_rich_text`]. The GUI
/// had shipped the disclosed downgrade; the CLI had no route to it, and
/// `docs/FEATURES.md` asserted the exact opposite of both facts until
/// `aac321c`.
///
/// The flag is **opt-in and lossy**, which is the whole design. Making it
/// the default would silently discard `/RV` formatting on a plain
/// `fill-field` — the "sneaky" half of rule 4, on a batch surface where
/// nobody is watching a screen. Refusing without any escape leaves a real
/// document permanently unfillable. An explicit flag is the only option
/// that is neither.
///
/// Disclosure is **per field, by name, on stderr** — not a count. A count
/// tells the operator that something lost its formatting; it does not tell
/// them WHICH, and on a scripted run that is the only question worth
/// answering. This is the same reasoning as `R181`, arrived at from the
/// other direction: there, a count described the wrong thing; here, a count
/// would be the wrong SHAPE for the thing.
///
/// # Loop safety (`R179`)
///
/// The assignment loop mutates and returns early on error, which is
/// `R179`'s shape. It is safe here because the early return happens
/// **before** [`save_edited`] — the partially-mutated session is dropped
/// and the output file is never written, so a failed run leaves no partial
/// fill anywhere an operator can observe. Atomicity by not saving, not by
/// rollback.
pub(crate) fn cmd_fill_field(
    input: &Path,
    sets: &[String],
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
    downgrade_rich_text: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // Model the form once up front to know each field's type. The session is
    // still at the base revision here, so the model is the file as loaded;
    // each fill re-reads through the overlay, so later fills see earlier ones.
    let Some(form) = pdfcer_core::forms::parse_acroform(&session.graph()) else {
        eprintln!(
            "pdfcer: {}: the document has no interactive form",
            input.display()
        );
        return exit::EDIT_REFUSED;
    };

    let mut applied = 0usize;
    for set in sets {
        let Some((name, value)) = set.split_once('=') else {
            eprintln!("pdfcer: --set must be NAME=VALUE, got {set:?}");
            return exit::EDIT_REFUSED;
        };
        // Look up the field's type from the model to choose the fill path.
        use pdfcer_core::forms::FieldType;
        let field_type = form.field_by_name(name).and_then(|f| f.field_type);
        let result = match field_type {
            Some(FieldType::Button) => {
                // Convenience aliases for a checkbox's single on-state.
                let state = match value.to_ascii_lowercase().as_str() {
                    "on" | "true" | "1" | "yes" | "checked" => resolve_on_state(&form, name),
                    "off" | "false" | "0" | "" | "unchecked" => "Off".to_owned(),
                    _ => value.to_owned(),
                };
                session.set_button_state(name, &state)
            }
            Some(FieldType::Choice) => {
                // A choice value may name several selections for a
                // multi-select field, `|`-separated (`Red|Blue`).
                let sels: Vec<&str> = value.split('|').collect();
                session
                    .set_choice_value(name, &sels)
                    .map(|out| disclose_fill(name, &out))
            }
            // Text, and the `None`/unmodelled fallback.
            //
            // The lossy verb is taken ONLY for a field the model says is
            // actually rich text, never merely because the flag is set.
            // Routing every text field through it would be wrong twice
            // over: `--downgrade-rich-text` must not change the outcome
            // for a field that has no formatting to lose, and the note
            // below would then have to guess whether anything happened.
            // Asking `is_rich_text()` — which resolves `/FT` first, so it
            // cannot mistake a radio group's bit 26 for RichText
            // (`587e520`) — makes both exact.
            //
            // Without the flag this falls through to `fill_text_field`,
            // which refuses a rich-text field. That refusal is the
            // default and stays the default.
            _ if downgrade_rich_text
                && form
                    .field_by_name(name)
                    .is_some_and(pdfcer_core::forms::Field::is_rich_text) =>
            {
                // Announced BEFORE the write, so an operator watching a
                // batch run sees which field is about to lose formatting
                // even if a later assignment aborts the whole run. stderr,
                // so a script capturing stdout still shows a human.
                eprintln!(
                    "pdfcer: {name}: rich-text formatting discarded \
                     (--downgrade-rich-text) — /RV removed, RichText flag \
                     cleared"
                );
                session
                    .fill_text_field_downgrading_rich_text(name, value)
                    .map(|out| disclose_fill(name, &out))
            }
            _ => session
                .fill_text_field(name, value)
                .map(|out| disclose_fill(name, &out)),
        };
        if let Err(err) = result {
            return report_edit_error(input, &err);
        }
        applied += 1;
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
        "fill-field {} sets={applied} mode={} -> {}; changed={} objects={} verbatim={} \
reserialized={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.objects_verbatim,
        r.objects_reserialized,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

/// Print the disclosures a vector surgery owes, to stderr.
///
/// Stderr, not stdout, because each of these commands prints ONE fixed-shape
/// record line that scripts parse; interleaving a variable-length prose block
/// into that stream would break them. The operator still sees it on a
/// terminal, where both streams land together.
///
/// These say the surgery had to change the *form* of an operator to do what
/// was asked — expand a rectangle whose corner was dragged out of square,
/// write the `m` an implicitly-started subpath never had. The drawing is
/// unchanged; the bytes are not recoverable by reversing the gesture, and
/// rule 4 forbids leaving the operator to discover that from a diff.
pub(crate) fn report_disclosures(disclosures: &[String]) {
    for d in disclosures {
        eprintln!("pdfcer: {d}");
    }
}

/// Print the fuzzy-never-sneaky disclosures a fill owes (an applied
/// auto-size, any unencodable characters) to stderr.
pub(crate) fn disclose_fill(name: &str, out: &pdfcer_core::edit::FillOutcome) {
    // `Pass 110.0`. Computed since the verb existed and emitted by nothing.
    //
    // Stated only ABOVE ONE, deliberately. One widget per field is the
    // unremarkable case and printing it on every fill would be noise that
    // trains an operator to skip the line. More than one means the SAME field
    // is presented in several places on the page (§12.7.3.1 — one field, many
    // widgets), so a fill the operator made in one place just changed
    // something he may not be looking at. That is a fact about the document he
    // cannot get from `sets=1`.
    if out.widgets_updated > 1 {
        eprintln!(
            "pdfcer: field {name:?}: {} widget appearance(s) were regenerated — this field is presented in more than one place, so the value changed everywhere it appears",
            out.widgets_updated
        );
    }
    // Stated FIRST among the caveats, before the cosmetic ones. The others
    // describe how the value was drawn; this one says the value may not be the
    // one a reader sees at all, which is a different order of consequence.
    if out.xfa_may_disagree {
        eprintln!(
            "pdfcer: field {name:?}: this form also carries an XFA packet. pdfcer filled the \
AcroForm half, which most viewers read, but cannot write the XFA half — so an XFA-aware viewer \
may still show the OLD value."
        );
    }
    if let Some(sz) = out.applied_autosize {
        // NAMES THE CONSTRAINT THAT BOUND, at a consuming shell's request:
        // an operator who thinks the text is too small wants to know whether
        // to widen the box or heighten it, and those are different answers.
        // The old wording — "a reviewable pdfcer heuristic" — was accurate when
        // the answer was a flat 12 pt and stopped being the useful thing to
        // say once it became a fit.
        let why = match out.applied_autosize_bound {
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
        eprintln!("pdfcer: field {name:?}: auto-sized to {sz:.3} pt ({why})");
    }
    if out.da_colour_unmodelled {
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
            "pdfcer: field {name:?}: the field's default appearance names a colour space pdfcer cannot emit (Separation/DeviceN/ICCBased/Indexed/Lab), so this appearance was generated in BLACK -- a narrowing, not the colour the file asked for"
        );
    }
    if out.unencodable_chars > 0 {
        eprintln!(
            "pdfcer: field {name:?}: {} character(s) had no WinAnsi code and were substituted \
with '?' (Base-14 Latin only)",
            out.unencodable_chars
        );
    }
    if let Some(ti) = out.top_index {
        eprintln!(
            "pdfcer: field {name:?}: this list box was SCROLLED to option {ti} (/TI) so the \
selection is on screen — the selected option sits below the first visible window at this \
field's size. pdfcer derived the position; nothing about the field's value changed."
        );
    }
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

/// `export-data`: write a filled form's field data to FDF or XFDF (Pass 7.1).
pub(crate) fn cmd_export_data(input: &Path, output: &Path, format: DataFormat) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let session = pdfcer_core::edit::EditSession::new(doc);
    let Some(data) = session.export_form_data() else {
        eprintln!(
            "pdfcer: {}: the document has no interactive form",
            input.display()
        );
        return exit::EDIT_REFUSED;
    };
    let src_hint = input.to_string_lossy();
    let bytes = match format {
        DataFormat::Fdf => data.to_fdf(Some(&src_hint)),
        DataFormat::Xfdf => data.to_xfdf(Some(&src_hint)),
        DataFormat::Csv => {
            let export = pdfcer_core::formcsv::to_csv(&data);
            // Reported BEFORE the success line, because it describes a
            // difference between the CSV and the PDF that an operator
            // comparing the two would otherwise have to explain to
            // themselves.
            if let Some(message) = export.message() {
                eprintln!("pdfcer: {}: {message}", input.display());
            }
            export.csv
        }
    };
    // Rich-text disclosure, on stderr in prose like every other one this
    // binary emits. Counted from the data itself rather than re-derived from
    // the form, so it describes the FILE that was written.
    //
    // Note what it does NOT say. Until Pass 37.3's first slice this export
    // dropped the formatting entirely and the GUI warned about that; the
    // warning is now false there and has been corrected. The CLI never had
    // one at all, which is its own gap — the two shells must not develop
    // different accounts of the same behaviour.
    let rich = data
        .fields
        .iter()
        .filter(|f| f.rich_value.is_some())
        .count();
    if rich > 0 {
        eprintln!(
            "pdfcer: {}: {rich} field(s) hold formatted (rich) text, and the formatting IS in the data file. pdfcer cannot yet apply it on import, though — another reader can, but a round trip back through pdfcer will not restore it.",
            input.display()
        );
    }
    if let Err(err) = std::fs::write(output, &bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    let fmt = match format {
        DataFormat::Fdf => "fdf",
        DataFormat::Xfdf => "xfdf",
        DataFormat::Csv => "csv",
    };
    println!(
        "export-data {} fields={} format={fmt} -> {}; out_bytes={}",
        input.display(),
        data.fields.len(),
        output.display(),
        bytes.len(),
    );
    exit::SUCCESS
}

/// `import-data`: set field values from an FDF/XFDF file and save (Pass 7.1).
/// The format is detected from the data file's content.
pub(crate) fn cmd_import_data(input: &Path, data_path: &Path, output: &Path, mode: SaveMode) -> u8 {
    let data_bytes = match std::fs::read(data_path) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", data_path.display());
            return exit::IO_ERROR;
        }
    };
    // Detect the format by CONTENT rather than by extension: a file named
    // `.txt` that is plainly an XFDF should still import, and an operator
    // who renamed one should not have to know that renaming mattered.
    //
    // The three tests are ordered by how specific their marker is. FDF
    // carries a `%FDF` header, XFDF opens with `<`, and CSV is the residue —
    // which is right, because CSV has no marker of its own and anything that
    // is neither of the other two is at least worth *trying* to read as two
    // columns before giving up.
    let first = data_bytes
        .iter()
        .find(|b| !b.is_ascii_whitespace())
        .copied();
    let looks_pdfish = data_bytes.starts_with(b"%FDF") || first == Some(b'%');
    let parsed = if first == Some(b'<') {
        pdfcer_core::fdf::FormData::parse_xfdf(&data_bytes).map_err(|e| e.to_string())
    } else if looks_pdfish {
        pdfcer_core::fdf::FormData::parse_fdf(&data_bytes).map_err(|e| e.to_string())
    } else {
        pdfcer_core::formcsv::parse_csv(&data_bytes).map_err(|e| e.to_string())
    };
    let data = match parsed {
        Ok(d) => d,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", data_path.display());
            return exit::EDIT_REFUSED;
        }
    };

    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    // Counted BEFORE the import, off the live form, because a rich-text
    // field is skipped and so leaves no trace in the outcome to count after.
    let rich_targets = pdfcer_core::forms::parse_acroform(&session.graph()).map_or(0, |form| {
        data.fields
            .iter()
            .filter(|e| {
                form.field_by_name(&e.name)
                    .is_some_and(pdfcer_core::forms::Field::is_rich_text)
            })
            .count()
    });
    let outcome = match session.import_form_data(&data) {
        Ok(o) => o,
        Err(err) => return report_edit_error(input, &err),
    };
    // WHY a field was skipped, not just that it was. `skipped=1` on the
    // result line is a number an operator cannot act on; this is the
    // sentence that tells them the field still holds what it held, and that
    // pdfcer declined on purpose rather than failed.
    if rich_targets > 0 {
        eprintln!(
            "pdfcer: {}: {rich_targets} rich-text field(s) were left untouched — not even their plain value was applied. Writing plain text beside a field's existing formatting makes conforming readers display the OLD text (ISO 32000-1 §12.7.3.3), so pdfcer leaves such a field alone rather than corrupt what it shows.",
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
        "import-data {} applied={} skipped={} mode={} -> {}; objects={} out_bytes={}",
        input.display(),
        outcome.applied,
        outcome.skipped,
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

/// Map a [`DocError`] (full-document load) to the CLI's exit code.
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
