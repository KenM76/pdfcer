//! Form-script verbs: `recompute` and `list-scripts`.

use super::*;

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

    print_recompute_plan(&plan);
    let order = recompute_order_token(&plan);
    print_recompute_caveats(input, &plan);

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

fn print_recompute_plan(plan: &pdfcer_core::form_script::recompute::RecomputePlan) {
    use pdfcer_core::form_script::recompute::Skip;
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
}

fn recompute_order_token(
    plan: &pdfcer_core::form_script::recompute::RecomputePlan,
) -> &'static str {
    use pdfcer_core::form_script::recompute::OrderSource;
    match plan.order_source {
        OrderSource::CalculationOrder => "calc_order",
        OrderSource::Mixed => "mixed",
        OrderSource::Derived => "derived",
        OrderSource::Empty => "none",
    }
}

/// Caveats on stderr, before any write: each changes how far the numbers
/// should be trusted, so they precede the summary line they qualify.
fn print_recompute_caveats(
    input: &Path,
    plan: &pdfcer_core::form_script::recompute::RecomputePlan,
) {
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
}

/// `list-scripts`: classify every form-field script.
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
