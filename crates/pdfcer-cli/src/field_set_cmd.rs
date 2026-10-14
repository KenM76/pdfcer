//! `copy-fields` / `paste-fields`: several form fields on one clip file,
//! pasted all or nothing as one undo entry.

use super::*;
use pdfcer_core::formclip::{FieldClipSet, FieldSetPastePolicy, PasteTooltip};

/// Borrowed argument bundle for [`cmd_paste_fields`].
pub(crate) struct PasteFieldsArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) clip: &'a Path,
    pub(crate) page: usize,
    pub(crate) at: &'a str,
    pub(crate) as_new: bool,
    pub(crate) as_widgets: bool,
    pub(crate) copy_value: bool,
    pub(crate) copy_actions: bool,
    pub(crate) carry_tooltip: bool,
    pub(crate) no_tooltip: bool,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `copy-fields` — write several fields onto one clip file.
///
/// Prints one `copy-fields …` line; exits [`exit::EDIT_REFUSED`] with
/// nothing written if any field is refused.
pub(crate) fn cmd_copy_fields(input: &Path, names: &[String], output: &Path) -> u8 {
    let (_, session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let fqns: Vec<&str> = names.iter().map(String::as_str).collect();
    let set = match session.copy_fields(&fqns) {
        Ok(set) => set,
        Err(err) => return report_edit_error(input, &err),
    };
    let bytes = set.to_bytes();
    if let Err(err) = write_output(output, &bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    let bbox = set.bbox().map_or_else(
        || "-".to_owned(),
        |r| format!("{},{},{},{}", r.llx, r.lly, r.urx, r.ury),
    );
    println!(
        "copy-fields {} fields={} widgets={} objects={} bbox={bbox} -> {}; bytes={}{}",
        input.display(),
        set.len(),
        set.widget_count(),
        set.object_count(),
        output.display(),
        bytes.len(),
        name_tokens(set.source_names()),
    );
    exit::SUCCESS
}

/// `paste-fields` — paste a `copy-fields` clip, all or nothing.
///
/// Disclosures go to stderr, each once; one `paste-fields …` line goes to
/// stdout ending in one `name=` token per pasted field, in clip order.
pub(crate) fn cmd_paste_fields(args: &PasteFieldsArgs<'_>) -> u8 {
    let Some(page_index) = args.page.checked_sub(1) else {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            args.input.display()
        );
        return exit::EDIT_REFUSED;
    };
    let Some(origin) = parse_point(args.at) else {
        eprintln!(
            "pdfcer: {}: --at needs two numbers as X,Y",
            args.input.display()
        );
        return exit::EDIT_REFUSED;
    };
    let policy = match paste_fields_policy(args) {
        Ok(policy) => policy,
        Err(code) => return code,
    };
    let set = match read_set(args.clip) {
        Ok(set) => set,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let outcome = match session.paste_fields(&set, page_index, origin, &policy) {
        Ok(outcome) => outcome,
        Err(err) => return report_edit_error(args.input, &err),
    };
    for disclosure in &outcome.disclosures {
        eprintln!("pdfcer: {}: {disclosure}", args.input.display());
    }
    let saved = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };
    let r = &saved.report;
    let widgets: usize = outcome.fields.iter().map(|f| f.widget_ids.len()).sum();
    println!(
        "paste-fields {} clip={} policy={} page={} at={},{} fields={} widgets={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}{}",
        args.input.display(),
        args.clip.display(),
        if args.as_new {
            "new-fields"
        } else {
            "additional-widgets"
        },
        args.page,
        origin.0,
        origin.1,
        outcome.names.len(),
        widgets,
        args.mode.name(),
        args.output.display(),
        saved.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(saved.undo_verified),
        u32::from(saved.undo_identical),
        name_tokens(outcome.names.iter().map(String::as_str)),
    );
    finish_edit(args.input, &saved)
}

/// The paste policy the flags chose; refuses when no choice was made, since
/// the two policies yield documents that differ in nothing the page shows.
fn paste_fields_policy(args: &PasteFieldsArgs<'_>) -> Result<FieldSetPastePolicy, u8> {
    if args.as_widgets {
        // clap waives `requires = "as_new"` once `--as-widgets` (which
        // conflicts with `--as-new`) is present, so this is the only check.
        if args.copy_value || args.copy_actions || args.carry_tooltip || args.no_tooltip {
            eprintln!(
                "pdfcer: {}: --copy-value, --copy-actions, --carry-tooltip and --no-tooltip apply to --as-new only; an added widget shares the existing field's value, actions and name.",
                args.input.display()
            );
            return Err(exit::EDIT_REFUSED);
        }
        return Ok(FieldSetPastePolicy::AdditionalWidgets);
    }
    if !args.as_new {
        eprintln!(
            "pdfcer: {}: choose which paste this is — --as-new for independent fields, or --as-widgets for more views of the same-named fields already here. There is no safe default.",
            args.input.display()
        );
        return Err(exit::EDIT_REFUSED);
    }
    let tooltip = match (args.carry_tooltip, args.no_tooltip) {
        (true, _) => PasteTooltip::Carry,
        (false, true) => PasteTooltip::Declined,
        (false, false) => {
            eprintln!(
                "pdfcer: {}: decide about the accessibility names — pass --carry-tooltip to reuse each copied field's, or --no-tooltip to decline them. A screen reader announces them, so they are never defaulted silently (R105).",
                args.input.display()
            );
            return Err(exit::EDIT_REFUSED);
        }
    };
    Ok(FieldSetPastePolicy::NewFields {
        tooltip,
        copy_value: args.copy_value,
        copy_actions: args.copy_actions,
    })
}

fn read_set(path: &Path) -> Result<FieldClipSet, u8> {
    let bytes = std::fs::read(path).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", path.display());
        exit::IO_ERROR
    })?;
    FieldClipSet::from_bytes(&bytes).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", path.display());
        exit::EDIT_REFUSED
    })
}

/// `X,Y` as two finite numbers.
fn parse_point(text: &str) -> Option<(f64, f64)> {
    let (x, y) = text.split_once(',')?;
    let x: f64 = x.trim().parse().ok()?;
    let y: f64 = y.trim().parse().ok()?;
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

/// ` name="A" name="B"`, one token per field.
fn name_tokens<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    names.into_iter().map(|n| format!(" name={n:?}")).collect()
}
