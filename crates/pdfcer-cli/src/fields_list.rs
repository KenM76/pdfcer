//! `list-fields`: the read-only AcroForm inventory verb.

use super::*;

use pdfcer_core::forms::{AcroForm, Field, FormJavaScript};
use std::collections::HashMap;

type RichRuns = Option<Result<Vec<pdfcer_core::richtext::Run>, String>>;

/// `list-fields`: inventory a document's AcroForm fields (Pass 7).
///
/// Read-only. One `field …` line per terminal field, then a `list-fields …`
/// summary line carrying the document-level form disclosures. Text-string
/// columns are Debug-quoted so the line stays field-splittable.
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
    let js = pdfcer_core::forms::scan_javascript(&doc);
    // No form is not an error, and actions are a document property: a file
    // whose only hazard is a `/Launch` on a bookmark still gets its breakdown.
    let Some(form) = pdfcer_core::forms::parse_acroform(&doc) else {
        print_no_form_summary(input, &js);
        return exit::SUCCESS;
    };
    let actions = push_button_actions(input, &form);
    let mut shown = 0usize;
    for field in &form.fields {
        if fillable_only && !field.is_fillable() {
            continue;
        }
        shown += 1;
        let runs = rich_runs(field);
        print_field_line(field, &runs, &actions);
        if widgets {
            print_widget_lines(field);
        }
        if rich_text {
            print_run_lines(&runs);
        }
    }
    let fields_with_aa = form
        .fields
        .iter()
        .filter(|f| f.has_additional_actions)
        .count();
    print_form_summary(input, &form, &js, shown, fields_with_aa);
    exit::SUCCESS
}

fn print_no_form_summary(input: &Path, js: &FormJavaScript) {
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
}

/// `action=` tokens for the push buttons of `form`, keyed by fully-qualified
/// name: `none`, a modelled subtype (`ResetForm`, `SubmitForm`, `GoTo`,
/// `Hide`, `Show`, `Named`, `URI`), `unmodelled:S` (a subtype pdfcer writes,
/// in a shape it does not decode) or `foreign:S` (one it never writes, such
/// as `JavaScript`). Read from each button's first widget, as
/// `EditSession::button_action` does. Empty when the form has no push button.
fn push_button_actions(input: &Path, form: &AcroForm) -> HashMap<String, String> {
    use pdfcer_core::edit::ButtonActionState;
    let mut out = HashMap::new();
    let push = |f: &&Field| {
        f.button_kind == Some(pdfcer_core::forms::ButtonKind::Push)
            && !f.fully_qualified_name.is_empty()
    };
    if !form.fields.iter().any(|f| push(&f)) {
        return out;
    }
    let Ok(doc) = open_document(input) else {
        return out;
    };
    let session = pdfcer_core::edit::EditSession::new(doc);
    for field in form.fields.iter().filter(push) {
        let token = match session.button_action(&field.fully_qualified_name) {
            Ok(ButtonActionState::None) => "none".to_owned(),
            Ok(ButtonActionState::Known(a)) => button_action_label(&a).to_owned(),
            Ok(ButtonActionState::Unmodelled(s)) => format!("unmodelled:{}", sanitize_token(&s)),
            Ok(ButtonActionState::Foreign(s)) => format!("foreign:{}", sanitize_token(&s)),
            Ok(_) | Err(_) => "unread".to_owned(),
        };
        out.insert(field.fully_qualified_name.clone(), token);
    }
    out
}

fn rich_runs(field: &Field) -> RichRuns {
    field.rich_value.as_ref().map(|rv| {
        let ds = field
            .default_style
            .as_ref()
            .map(|d| String::from_utf8_lossy(d).into_owned());
        String::from_utf8(rv.clone())
            .map_err(|_| "not UTF-8".to_owned())
            .and_then(|s| {
                pdfcer_core::richtext::parse(&s, ds.as_deref()).map_err(|e| e.to_string())
            })
    })
}

/// Debug-quotes a §7.9.2 text string (it may contain spaces, and this verb's
/// output is what `--name` on every write verb takes back), leaving the bare
/// sentinel for an absent one so `-` stays distinct from `""`.
fn quoted_or(s: &str, absent: &str) -> String {
    if s.is_empty() {
        absent.to_owned()
    } else {
        format!("{s:?}")
    }
}

fn field_type_tokens(field: &Field) -> (&'static str, &'static str) {
    use pdfcer_core::forms::{ButtonKind, FieldType};
    let ty = match field.field_type {
        Some(FieldType::Button) => "Btn",
        Some(FieldType::Text) => "Tx",
        Some(FieldType::Choice) => "Ch",
        Some(FieldType::Signature) => "Sig",
        None => "none",
    };
    let button = match field.button_kind {
        Some(ButtonKind::Push) => "push",
        Some(ButtonKind::Check) => "check",
        Some(ButtonKind::Radio) => "radio",
        None => "-",
    };
    (ty, button)
}

fn print_field_line(field: &Field, runs: &RichRuns, actions: &HashMap<String, String>) {
    let (ty, button) = field_type_tokens(field);
    let name = quoted_or(&field.fully_qualified_name, "(unnamed)");
    let value = quoted_or(&field.value.display_text(), "-");
    let caption = field
        .widgets
        .iter()
        .find_map(|w| w.caption.as_deref())
        .map_or_else(
            || "-".to_owned(),
            |c| format!("{:?}", String::from_utf8_lossy(c)),
        );
    let rich = match runs {
        None => "-".to_owned(),
        Some(Ok(r)) => format!("{}runs", r.len()),
        Some(Err(_)) => "unparsed".to_owned(),
    };
    println!(
        "field name={name} type={ty} button={button} flags=0x{:X} value={value} \
widgets={} ap={} fillable={} readonly={} aa={} caption={caption} rich={rich} action={}",
        field.flags.0,
        field.widgets.len(),
        u32::from(field.has_appearance()),
        u32::from(field.is_fillable()),
        u32::from(field.flags.read_only()),
        u32::from(field.has_additional_actions),
        actions
            .get(&field.fully_qualified_name)
            .map_or("-", String::as_str),
    );
}

fn print_widget_lines(field: &Field) {
    use pdfcer_core::edit::Visibility;
    for (i, w) in field.widgets.iter().enumerate() {
        let rect = w.rect.map_or_else(
            || "-".to_owned(),
            |r| format!("[{:.1} {:.1} {:.1} {:.1}]", r.llx, r.lly, r.urx, r.ury),
        );
        let border = w.border.as_ref().map_or_else(
            || "-".to_owned(),
            |b| format!("{}/{:.2}", String::from_utf8_lossy(b.style.name()), b.width),
        );
        let visibility = match w.visibility {
            Some(Visibility::VisibleAndPrints) => "visible+print",
            Some(Visibility::ScreenOnly) => "screen-only",
            Some(Visibility::PrintOnly) => "print-only",
            Some(Visibility::Hidden) => "hidden",
            _ => "other",
        };
        let state = w.appearance_state.as_deref().map_or_else(
            || "-".to_owned(),
            |n| String::from_utf8_lossy(n).into_owned(),
        );
        let rotation = w.rotation.map_or_else(|| "-".to_owned(), |d| d.to_string());
        let dash = w.border_dash.as_ref().map_or_else(
            || "-".to_owned(),
            |d| {
                d.pattern()
                    .iter()
                    .map(f64::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            },
        );
        println!(
            "  widget {i} obj={} rect={rect} border={border} rotation={rotation} \
visibility={visibility} flags=0x{:X} state={state} merged={} background={} border_color={} border_dash={dash}",
            w.id.num,
            w.annot_flags.0,
            u32::from(w.merged),
            mk_colour_token(w.background),
            mk_colour_token(w.border_color),
        );
    }
}

fn print_run_lines(runs: &RichRuns) {
    match runs {
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
        Some(Err(e)) => println!("  rich text could not be read: {e}"),
    }
}

fn print_form_summary(
    input: &Path,
    form: &AcroForm,
    js: &FormJavaScript,
    shown: usize,
    fields_with_aa: usize,
) {
    let xfa = match form.xfa {
        pdfcer_core::forms::XfaPresence::None => "none".to_owned(),
        pdfcer_core::forms::XfaPresence::Stream => "stream".to_owned(),
        pdfcer_core::forms::XfaPresence::PacketArray { packets } => format!("packets:{packets}"),
    };
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
            "pdfcer: {}: this form carries {} network and {} process-launch action trigger(s) \
that Adobe Acrobat/Reader would run; pdfcer recognizes them but NEVER executes any (R12/R13/R54).",
            input.display(),
            js.network_action_count,
            js.launch_action_count,
        );
    }
}
