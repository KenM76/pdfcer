//! Field-value verbs: `fill-field` and the password-value commands.

use super::*;

/// `fill-field`: set form-field values and save (Pass 7).
///
/// Each `NAME=VALUE` is dispatched by the field's modelled type: text/choice
/// through `EditSession::fill_text_field`, check-box/radio through
/// `EditSession::set_button_state`. All assignments land in one session
/// (so the save carries them as one incremental revision), then the shared
/// [`save_edited`]/[`finish_edit`] plumbing writes and reports.
///
/// # `downgrade_rich_text`, and why the CLI needed it
///
/// Until this flag existed, a rich-text field was **unfillable from the
/// CLI at all**: this function called `EditSession::fill_text_field`,
/// which refuses one with `EditError::FieldIsRichText`, and never
/// exposed `EditSession::fill_text_field_downgrading_rich_text`. The GUI
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
    store_password_values: bool,
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

    let policy = TextFillPolicy {
        downgrade_rich_text,
        store_password_values,
    };
    let mut withheld_password_fields: Vec<String> = Vec::new();
    for set in sets {
        let Some((name, value)) = set.split_once('=') else {
            eprintln!("pdfcer: --set must be NAME=VALUE, got {set:?}");
            return exit::EDIT_REFUSED;
        };
        match fill_one(&mut session, &form, name, value, policy) {
            Ok(true) => withheld_password_fields.push(name.to_owned()),
            Ok(false) => {}
            Err(err) => return report_edit_error(input, &err),
        }
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
    print_fill_field_summary(input, sets.len(), mode, output, &outcome);
    if matches!(mode, SaveMode::Incremental) {
        disclose_password_history(&source, &withheld_password_fields);
    }
    finish_edit(input, &outcome)
}

/// How `fill-field` writes a text value; both default to the refusing path.
#[derive(Clone, Copy)]
struct TextFillPolicy {
    downgrade_rich_text: bool,
    store_password_values: bool,
}

/// Applies one `NAME=VALUE`, choosing the fill path from the field's type.
/// `Ok(true)` means a password field's value was withheld from the file.
fn fill_one(
    session: &mut pdfcer_core::edit::EditSession,
    form: &pdfcer_core::forms::AcroForm,
    name: &str,
    value: &str,
    policy: TextFillPolicy,
) -> Result<bool, pdfcer_core::edit::EditError> {
    use pdfcer_core::forms::FieldType;
    let field = form.field_by_name(name);
    match field.and_then(|f| f.field_type) {
        Some(FieldType::Button) => {
            // Convenience aliases for a checkbox's single on-state.
            let state = match value.to_ascii_lowercase().as_str() {
                "on" | "true" | "1" | "yes" | "checked" => resolve_on_state(form, name),
                "off" | "false" | "0" | "no" | "" | "unchecked" => "Off".to_owned(),
                _ => value.to_owned(),
            };
            session.set_button_state(name, &state).map(|()| false)
        }
        Some(FieldType::Choice) => {
            // A multi-select value is `|`-separated (`Red|Blue`).
            let sels: Vec<&str> = value.split('|').collect();
            let out = session.set_choice_value(name, &sels)?;
            disclose_fill(name, &out);
            Ok(false)
        }
        // The lossy verb only for a field the model says IS rich text
        // (`is_rich_text` resolves `/FT` first, so a radio group's bit 26
        // cannot pass): the flag must not change a field with nothing to
        // lose. Without the flag, `fill_text_field` refuses rich text.
        _ if policy.downgrade_rich_text
            && field.is_some_and(pdfcer_core::forms::Field::is_rich_text) =>
        {
            // Announced before the write, so a batch that later aborts still
            // shows which field was about to lose formatting.
            eprintln!(
                "pdfcer: {name}: rich-text formatting discarded \
                 (--downgrade-rich-text) — /RV removed, RichText flag \
                 cleared"
            );
            let out = session.fill_text_field_downgrading_rich_text(name, value)?;
            disclose_fill(name, &out);
            Ok(false)
        }
        _ if policy.store_password_values => {
            let out = session.fill_text_field_storing_password(name, value)?;
            disclose_fill(name, &out);
            Ok(false)
        }
        _ => {
            let out = session.fill_text_field(name, value)?;
            disclose_fill(name, &out);
            Ok(out.password_value_withheld)
        }
    }
}

fn print_fill_field_summary(
    input: &Path,
    applied: usize,
    mode: SaveMode,
    output: &Path,
    outcome: &EditOutcome,
) {
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
}

/// `password-values`: where the file stores password-field values.
pub(crate) fn cmd_password_values(input: &Path) -> u8 {
    let bytes = match std::fs::read(input) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::IO_ERROR;
        }
    };
    let scan =
        pdfcer_core::password_history::scan_stored_password_values(&bytes, open_document_bytes);
    let last = scan.revisions.saturating_sub(1);
    for s in &scan.stored {
        println!(
            "stored field={} revision={} latest={}",
            sanitize_token(&s.field),
            s.revision,
            u8::from(s.revision == last)
        );
    }
    let latest = scan.in_latest().count();
    let superseded = scan.in_superseded().count();
    println!(
        "password-values {} revisions={} unreadable_revisions={} latest={latest} superseded={superseded}",
        input.display(),
        scan.revisions,
        scan.unreadable_revisions,
    );
    if superseded > 0 {
        eprintln!(
            "pdfcer: {superseded} password value(s) are in earlier revisions: a reader does not show them, but the file's bytes still hold them. Only a full rewrite removes an earlier revision."
        );
    }
    if scan.unreadable_revisions > 0 {
        eprintln!(
            "pdfcer: {} revision(s) could not be opened on their own, so their fields were not checked.",
            scan.unreadable_revisions
        );
    }
    0
}

/// `purge-password-values`: purge in the session, then always the decomposing
/// full rewrite, because an incremental save or a verbatim container copy would
/// keep the value. The output is re-scanned so the exit code reports what the
/// file actually holds, not what the purge intended.
pub(crate) fn cmd_purge_password_values(
    input: &Path,
    output: &Path,
    invalidate_signatures: bool,
) -> u8 {
    use pdfcer_core::writer::SaveOptions;
    let (_source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let impact = session.signature_impact_of_save(CoreSaveMode::FullRewrite);
    if session.signature_census().any() && !invalidate_signatures {
        eprintln!(
            "pdfcer: {}: this document is signed. Removing the values needs a full rewrite, which \
invalidates every signature; pass --invalidate-signatures to do it anyway.",
            input.display()
        );
        return exit::EDIT_REFUSED;
    }
    let outcome = match session.purge_password_values() {
        Ok(o) => o,
        Err(err) => return report_edit_error(input, &err),
    };
    let (bytes, _report, decomposition) =
        match session.to_full_bytes_decomposing_containers(&SaveOptions::default()) {
            Ok(saved) => saved,
            Err(err) => {
                eprintln!("pdfcer: save refused: {err}");
                return exit::SAVE_REFUSED;
            }
        };
    if let Err(err) = write_output(output, &bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    for name in &outcome.fields_purged {
        println!(
            "purged field={} read_only={}",
            sanitize_token(name),
            u8::from(outcome.read_only_purged.contains(name))
        );
    }
    for name in &outcome.inherited_not_removed {
        eprintln!(
            "pdfcer: field \"{name}\": its value is inherited from a parent field, which may hold \
other fields' values too, so it was not removed and is still in the output."
        );
    }
    if !outcome.read_only_purged.is_empty() {
        eprintln!(
            "pdfcer: {} read-only password field(s) were purged as well.",
            outcome.read_only_purged.len()
        );
    }
    let rescan =
        pdfcer_core::password_history::scan_stored_password_values(&bytes, open_document_bytes);
    let remaining = rescan.stored.len();
    println!(
        "purge-password-values {} -> {} purged={} read_only={} inherited={} appearances_removed={} \
containers_unpacked={} remaining={remaining} signature={}",
        input.display(),
        output.display(),
        outcome.fields_purged.len(),
        outcome.read_only_purged.len(),
        outcome.inherited_not_removed.len(),
        outcome.appearance_objects_removed,
        decomposition.containers,
        signature_token(impact),
    );
    report_signature(input, impact);
    if remaining > 0 {
        eprintln!(
            "pdfcer: {}: {remaining} stored password value(s) remain in the output; run \
password-values on it to see which fields.",
            output.display()
        );
        return exit::EDIT_REFUSED;
    }
    exit::SUCCESS
}

/// After an incremental fill of a password field, state which of the filled
/// fields still have a value in the input's revisions: the save appended, so
/// every one of them is still in the output's bytes.
fn disclose_password_history(source: &[u8], password_fields: &[String]) {
    if password_fields.is_empty() {
        return;
    }
    let scan =
        pdfcer_core::password_history::scan_stored_password_values(source, open_document_bytes);
    for name in password_fields {
        let revisions: Vec<String> = scan
            .stored
            .iter()
            .filter(|s| &s.field == name)
            .map(|s| s.revision.to_string())
            .collect();
        if !revisions.is_empty() {
            eprintln!(
                "pdfcer: field {name:?}: an earlier value of this password field is still in the file (input revision {}). An incremental save keeps every earlier revision; save with --mode full to drop them.",
                revisions.join(", ")
            );
        }
    }
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
    print_layout_disclosure(
        &format!("field {name:?}"),
        out.applied_autosize,
        out.applied_autosize_bound,
        out.da_colour_unmodelled,
        out.unencodable_chars,
    );
    if let Some(limit) = out.exceeds_max_len {
        eprintln!(
            "pdfcer: field {name:?}: the value is longer than the field's limit of {limit} characters (/MaxLen); it was stored in full, and a comb field shows only the first {limit}"
        );
    }
    if out.password_value_withheld {
        eprintln!(
            "pdfcer: field {name:?}: password field -- drawn as asterisks and its value was NOT saved (ISO 32000 §12.7.4.3); pass --store-password-values to store it in plain text"
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
