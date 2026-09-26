use super::*;

/// The file-name stem a `{stem}` placeholder and a per-source bookmark
/// both want.
pub(crate) fn stem_of(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".to_owned())
}

/// Implement `pdfcer extract-pages`.
pub(crate) fn cmd_extract_pages(input: &Path, pages: &str, output: &Path) -> u8 {
    let doc = match open_for_read(input) {
        Ok(doc) => doc,
        Err(code) => return code,
    };
    let count = match pdfcer_core::page_tree::pages(&doc) {
        Ok(pages) => pages.len(),
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let selected = match parse_pages(pages, count) {
        Ok(selected) => selected,
        Err(message) => {
            eprintln!("pdfcer: {}: {message}", input.display());
            return exit::EDIT_REFUSED;
        }
    };

    let view = DocumentView::new(&doc, doc.bytes(), doc.version());
    // The operator's persisted §14.11.4 policy. Loaded here rather than
    // taken as a default, because a setting no front end passes is a
    // setting that does nothing.
    let (settings, settings_report) =
        pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
    report_settings(&settings_report);
    let (bytes, report) =
        match pdfcer_core::pageops::extract_with(&view, &selected, settings.separations) {
            Ok(pair) => pair,
            Err(err) => return report_page_op_error(&err),
        };
    if let Err(err) = std::fs::write(output, &bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    println!(
        "extract-pages {} -> {}; {}",
        input.display(),
        output.display(),
        assemble_metrics(&report, bytes.len())
    );
    report_assemble(output, &report);
    exit::SUCCESS
}

/// Implement `pdfcer merge`.
pub(crate) fn cmd_merge(inputs: &[PathBuf], output: &Path, bookmarks: bool) -> u8 {
    if inputs.len() < 2 {
        eprintln!(
            "pdfcer: merge needs at least two input PDFs; got {}.",
            inputs.len()
        );
        return exit::EDIT_REFUSED;
    }
    let mut docs = Vec::with_capacity(inputs.len());
    for path in inputs {
        match open_for_read(path) {
            Ok(doc) => docs.push(doc),
            Err(code) => return code,
        }
    }
    let views: Vec<DocumentView<'_>> = docs
        .iter()
        .map(|doc| DocumentView::new(doc, doc.bytes(), doc.version()))
        .collect();
    // Titles are PDF text strings (§7.9.2), encoded by the engine's own
    // encoder rather than assembled here — pdfcer-core owns the format,
    // the CLI owns only the choice of what to call each source.
    let titles: Vec<Vec<u8>> = if bookmarks {
        inputs
            .iter()
            .map(|path| pdfcer_core::edit::encode_text_string(&stem_of(path)))
            .collect()
    } else {
        Vec::new()
    };

    // The file NAME of each source, so a bookmark that opens another of
    // these files can be re-pointed at it (`Pass 258.3`). Distinct from
    // `titles`, which is what to CALL each source in the generated
    // heading: a `/Launch` says `chapter1.pdf`, not `chapter1`.
    let files: Vec<Vec<u8>> = inputs
        .iter()
        .map(|path| {
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into_bytes()
        })
        .collect();

    let (bytes, report) = match pdfcer_core::pageops::merge(&views, &titles, &files) {
        Ok(pair) => pair,
        Err(err) => return report_page_op_error(&err),
    };
    if let Err(err) = std::fs::write(output, &bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    println!(
        "merge {} files -> {}; {}",
        inputs.len(),
        output.display(),
        assemble_metrics(&report, bytes.len())
    );
    report_assemble(output, &report);
    exit::SUCCESS
}

/// Implement `pdfcer insert-pages`.
///
/// **This calls `pageops::insert`, NOT `EditSession::insert_pages`**, and
/// the two differ in a way an operator can see: this one **merges**
/// `/AcroForm`, outlines, named destinations and page labels and writes a
/// new document; the session verb saves incrementally and merges none of
/// them, so form fields arrive as widgets nobody owns.
///
/// Measured 2026-08-19 on the same 12-field source: this route produces an
/// `/AcroForm` with 13 widgets and `fields_dropped=0`.
///
/// Named here because the two verbs share a name and a reader arriving from
/// either side will assume there is only one.
pub(crate) fn cmd_insert_pages(
    input: &Path,
    source: &Path,
    source_pages: &str,
    before: Option<usize>,
    after: Option<usize>,
    output: &Path,
) -> u8 {
    let (target_doc, source_doc) = match (open_for_read(input), open_for_read(source)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(code), _) | (_, Err(code)) => return code,
    };
    let source_count = match pdfcer_core::page_tree::pages(&source_doc) {
        Ok(pages) => pages.len(),
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", source.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let selected = match parse_pages(source_pages, source_count) {
        Ok(selected) => selected,
        Err(message) => {
            eprintln!("pdfcer: {}: {message}", source.display());
            return exit::EDIT_REFUSED;
        }
    };
    // 1-based on the command line, 0-based in the engine; the conversion
    // happens exactly here. `--before 0` means "at the very start", which
    // is why it saturates rather than erroring.
    let position = match (before, after) {
        (Some(page), _) => InsertPosition::Before(page.saturating_sub(1)),
        (None, Some(page)) => InsertPosition::After(page.saturating_sub(1)),
        (None, None) => InsertPosition::End,
    };

    let target_view = DocumentView::new(&target_doc, target_doc.bytes(), target_doc.version());
    let source_view = DocumentView::new(&source_doc, source_doc.bytes(), source_doc.version());
    let (bytes, report) =
        match pdfcer_core::pageops::insert(&target_view, &source_view, &selected, position) {
            Ok(pair) => pair,
            Err(err) => return report_page_op_error(&err),
        };
    if let Err(err) = std::fs::write(output, &bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    println!(
        "insert-pages {} + {} -> {}; {}",
        input.display(),
        source.display(),
        output.display(),
        assemble_metrics(&report, bytes.len())
    );
    report_assemble(output, &report);
    exit::SUCCESS
}

/// Implement `pdfcer split`.
#[allow(clippy::too_many_arguments)] // one parameter per documented flag
pub(crate) fn cmd_split(
    input: &Path,
    out_dir: &Path,
    every: usize,
    after: Option<&str>,
    bookmarks: bool,
    name_template: &str,
    force: bool,
) -> u8 {
    let doc = match open_for_read(input) {
        Ok(doc) => doc,
        Err(code) => return code,
    };
    let count = match pdfcer_core::page_tree::pages(&doc) {
        Ok(pages) => pages.len(),
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let criterion = if bookmarks {
        SplitCriterion::TopLevelBookmarks
    } else if let Some(spec) = after {
        match parse_pages(spec, count) {
            Ok(points) => SplitCriterion::AfterPages(points),
            Err(message) => {
                eprintln!("pdfcer: {}: {message}", input.display());
                return exit::EDIT_REFUSED;
            }
        }
    } else {
        SplitCriterion::EveryN(every)
    };

    let view = DocumentView::new(&doc, doc.bytes(), doc.version());
    let stem = stem_of(input);
    // The operator's persisted §14.11.4 policy. Loaded here rather than
    // taken as a default, because a setting no front end passes is a
    // setting that does nothing.
    let (settings, settings_report) =
        pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
    report_settings(&settings_report);
    let parts = match pdfcer_core::pageops::split_with(
        &view,
        &criterion,
        name_template,
        &stem,
        settings.separations,
    ) {
        Ok(parts) => parts,
        Err(err) => return report_page_op_error(&err),
    };

    if let Err(err) = std::fs::create_dir_all(out_dir) {
        eprintln!("pdfcer: {}: {err}", out_dir.display());
        return exit::IO_ERROR;
    }
    // Collision check BEFORE anything is written: a split that overwrites
    // half a folder and then fails is worse than one that refuses.
    if !force {
        let existing: Vec<String> = parts
            .iter()
            .map(|(part, _, _)| part.name.clone())
            .filter(|name| out_dir.join(name).exists())
            .collect();
        if !existing.is_empty() {
            eprintln!(
                "pdfcer: {}: {} output name(s) already exist there ({}). Nothing was written; \
pass --force to overwrite.",
                out_dir.display(),
                existing.len(),
                existing.join(", ")
            );
            return exit::EDIT_REFUSED;
        }
    }

    let mut pages_written = 0usize;
    let mut bytes_written = 0usize;
    for (part, bytes, report) in &parts {
        let path = out_dir.join(&part.name);
        if let Err(err) = std::fs::write(&path, bytes) {
            eprintln!("pdfcer: {}: {err}", path.display());
            return exit::IO_ERROR;
        }
        pages_written += report.pages;
        bytes_written += bytes.len();
    }
    println!(
        "split {} -> {}; parts={} pages={} out_bytes={bytes_written}",
        input.display(),
        out_dir.display(),
        parts.len(),
        pages_written
    );
    if let Some((_, _, first)) = parts.first() {
        // The carryover disclosures are the same for every part, so they
        // are printed once rather than N times.
        report_assemble(out_dir, first);
    }
    exit::SUCCESS
}

/// Implement `pdfcer delete-pages`.
pub(crate) fn cmd_delete_pages(
    input: &Path,
    pages: &str,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let count = match session.pages() {
        Ok(pages) => pages.len(),
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let selected = match parse_pages(pages, count) {
        Ok(selected) => selected,
        Err(message) => {
            eprintln!("pdfcer: {}: {message}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    // The operator's persisted §14.11.4 policy. Loaded here rather than
    // taken as a default, because a setting no front end passes is a
    // setting that does nothing.
    let (settings, settings_report) =
        pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
    report_settings(&settings_report);
    let outcome = match session.delete_pages_with(&selected, settings.separations) {
        Ok(outcome) => outcome,
        Err(err) => return report_edit_error(input, &err),
    };
    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };

    println!(
        "delete-pages {} mode={} signature={} -> {}; pages_removed={} objects_freed={} \
dangling_bookmarks={} dangling_links={} dangling_annot_actions={} dangling_dests={} \
page_labels_stale={} {} {}",
        input.display(),
        mode.name(),
        signature_token(outcome.signature),
        output.display(),
        outcome.pages_removed,
        outcome.objects_freed,
        outcome.dangling.outline_items,
        outcome.dangling.links,
        outcome.dangling.non_link_annotations,
        outcome.dangling.named_destinations,
        u32::from(outcome.dangling.page_labels_stale),
        separation_metrics(&outcome.separations),
        edit_metrics(&saved)
    );

    // The two honesty disclosures the UI spec makes mandatory, in their
    // command-line form.
    if !outcome.dangling.is_empty() {
        eprintln!(
            "pdfcer: {}: {} bookmark(s), {} link(s), {} button/annotation action(s) and {} \
named destination(s) pointed at a removed page and now point nowhere. pdfcer reports them and does \
not repair them — repointing one at whatever page now occupies that index would be pdfcer deciding \
what the author meant.",
            input.display(),
            outcome.dangling.outline_items,
            outcome.dangling.links,
            outcome.dangling.non_link_annotations,
            outcome.dangling.named_destinations
        );
    }
    if outcome.dangling.page_labels_stale {
        eprintln!(
            "pdfcer: {}: this document has a page-label tree (/PageLabels). Deleting pages \
does not adjust it, so its numbering is now stale.",
            input.display()
        );
    }
    // The one class above that pdfcer repairs rather than reports — see
    // `DeleteOutcome::separations` for why a structural invariant is
    // repairable where an authorial destination is not.
    report_separations(input, &outcome.separations);
    eprintln!(
        "pdfcer: {}: deletion removes pages from the DOCUMENT, not from the file's bytes. \
The previous revision can still contain them. This is not redaction.",
        output.display()
    );
    report_signature(input, outcome.signature);
    finish_edit(input, &saved)
}

/// Implement `pdfcer reorder-pages`.
pub(crate) fn cmd_reorder_pages(
    input: &Path,
    order: &str,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let count = match session.pages() {
        Ok(pages) => pages.len(),
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let new_order = match parse_pages(order, count) {
        Ok(order) => order,
        Err(message) => {
            eprintln!("pdfcer: {}: {message}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    if let Err(err) = session.reorder_pages(&new_order) {
        return report_edit_error(input, &err);
    }
    let impact = session.signature_impact_of_save(match mode {
        SaveMode::Incremental => CoreSaveMode::Incremental,
        SaveMode::Full => CoreSaveMode::FullRewrite,
    });
    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };
    println!(
        "reorder-pages {} mode={} signature={} -> {}; pages={} {}",
        input.display(),
        mode.name(),
        signature_token(impact),
        output.display(),
        count,
        edit_metrics(&saved)
    );
    report_signature(input, impact);
    finish_edit(input, &saved)
}

/// Implement `pdfcer rotate`.
pub(crate) fn cmd_rotate(
    input: &Path,
    degrees: i32,
    pages: &str,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let count = match session.pages() {
        Ok(pages) => pages.len(),
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let selected = match parse_pages(pages, count) {
        Ok(selected) => selected,
        Err(message) => {
            eprintln!("pdfcer: {}: {message}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    let rotated = match session.rotate_pages(&selected, degrees) {
        Ok(rotated) => rotated,
        Err(err) => return report_edit_error(input, &err),
    };
    let impact = session.signature_impact_of_save(match mode {
        SaveMode::Incremental => CoreSaveMode::Incremental,
        SaveMode::Full => CoreSaveMode::FullRewrite,
    });
    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };
    println!(
        "rotate {} mode={} signature={} -> {}; rotate={} rotated={} {}",
        input.display(),
        mode.name(),
        signature_token(impact),
        output.display(),
        degrees,
        rotated,
        edit_metrics(&saved)
    );
    report_signature(input, impact);
    finish_edit(input, &saved)
}

/// Parse `--pages 1,3,5` into 0-based indices.
pub(crate) fn parse_page_list(raw: &str) -> Result<Vec<usize>, String> {
    let mut out = Vec::new();
    for token in raw.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let n: usize = token
            .parse()
            .map_err(|_| format!("{token:?} is not a page number"))?;
        // 1-based on this side, 0-based in the engine -- the same convention
        // every `--page` flag in this binary uses.
        let index = n
            .checked_sub(1)
            .ok_or_else(|| "--pages is 1-based; 0 is not a page".to_owned())?;
        out.push(index);
    }
    if out.is_empty() {
        return Err("--pages selected nothing".to_owned());
    }
    Ok(out)
}

/// Parse `--at start|end|before:N|after:N` (1-based) into an `InsertPosition`.
pub(crate) fn parse_insert_position(
    raw: &str,
) -> Result<pdfcer_core::pageops::InsertPosition, String> {
    use pdfcer_core::pageops::InsertPosition;
    let raw = raw.trim();
    if raw.eq_ignore_ascii_case("start") {
        return Ok(InsertPosition::Start);
    }
    if raw.eq_ignore_ascii_case("end") {
        return Ok(InsertPosition::End);
    }
    let (word, number) = raw
        .split_once(':')
        .ok_or_else(|| format!("{raw:?} is not start, end, before:N or after:N"))?;
    let n: usize = number
        .trim()
        .parse()
        .map_err(|_| format!("{number:?} is not a page number"))?;
    let index = n
        .checked_sub(1)
        .ok_or_else(|| "--at is 1-based; 0 is not a page".to_owned())?;
    match word.trim().to_ascii_lowercase().as_str() {
        "before" => Ok(InsertPosition::Before(index)),
        "after" => Ok(InsertPosition::After(index)),
        other => Err(format!("{other:?} is not before or after")),
    }
}

/// Borrowed argument bundle for [`cmd_page_copy`].
pub(crate) struct PageCopyArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) pages: &'a str,
    pub(crate) clip: &'a Path,
    pub(crate) cut: Option<&'a Path>,
    pub(crate) mode: SaveMode,
}

/// `page-copy` — whole pages onto a clipboard file, optionally cutting them.
pub(crate) fn cmd_page_copy(args: &PageCopyArgs<'_>) -> u8 {
    let indices = match parse_page_list(args.pages) {
        Ok(v) => v,
        Err(message) => {
            eprintln!("pdfcer: page-copy refused: {message}");
            return exit::EDIT_REFUSED;
        }
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // The COPY runs first either way, so a page set that cannot be carried is
    // refused with nothing deleted.
    let clip = if args.cut.is_some() {
        match session.cut_pages(&indices) {
            Ok(clip) => clip,
            Err(err) => return report_edit_error(args.input, &err),
        }
    } else {
        match session.copy_pages(&indices) {
            Ok(clip) => clip,
            Err(err) => return report_edit_error(args.input, &err),
        }
    };

    if let Err(err) = std::fs::write(args.clip, clip.to_bytes()) {
        eprintln!("pdfcer: {}: {err}", args.clip.display());
        return exit::IO_ERROR;
    }
    if clip.fields_dropped > 0 {
        eprintln!(
            "pdfcer: {}: {} form field(s) were NOT carried -- their widgets are not all on the copied pages, and half a field is not a field.",
            args.input.display(),
            clip.fields_dropped
        );
    }
    println!(
        "{} {} pages={} -> {} ({} bytes); fields_dropped={}",
        if args.cut.is_some() {
            "page-cut"
        } else {
            "page-copy"
        },
        args.input.display(),
        clip.pages,
        args.clip.display(),
        clip.byte_len(),
        clip.fields_dropped,
    );

    let Some(cut_output) = args.cut else {
        return exit::SUCCESS;
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        cut_output,
        args.mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "  cut=1 cut_out={} mode={} changed={} objects={} appended={} out_bytes={}",
        cut_output.display(),
        args.mode.name(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
    );
    finish_edit(args.input, &outcome)
}

/// Borrowed argument bundle for [`cmd_page_paste`].
pub(crate) struct PagePasteArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) clip: &'a Path,
    pub(crate) at: &'a str,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
}

/// `page-paste` — whole pages from a clipboard file into a document.
pub(crate) fn cmd_page_paste(args: &PagePasteArgs<'_>) -> u8 {
    let position = match parse_insert_position(args.at) {
        Ok(p) => p,
        Err(message) => {
            eprintln!("pdfcer: page-paste refused: {message}");
            return exit::EDIT_REFUSED;
        }
    };
    let bytes = match std::fs::read(args.clip) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.clip.display());
            return exit::IO_ERROR;
        }
    };
    let clip = match pdfcer_core::pageops::PageClip::from_bytes(bytes) {
        Ok(clip) => clip,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.clip.display());
            return exit::EDIT_REFUSED;
        }
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let pasted = match session.paste_pages(&clip, position) {
        Ok(outcome) => outcome,
        Err(err) => return report_edit_error(args.input, &err),
    };

    // Off-canvas, on stderr. The widget counters are the ones that are
    // invisible in the result: boxes that draw like form fields and that
    // nothing can fill.
    if pasted.orphaned_widgets > 0 {
        eprintln!(
            "pdfcer: {}: {} form-field box(es) arrived WITHOUT the form that owns them ({} of those cannot be adopted into a field afterwards, because the widget carries no name or type of its own). They draw like fields and nothing can fill them.",
            args.input.display(),
            pasted.orphaned_widgets,
            pasted.orphaned_widgets_unrecoverable,
        );
    }
    if pasted.source_outline_dropped {
        eprintln!(
            "pdfcer: {}: the copied pages' document had an outline (bookmarks); it did not travel. A bookmark tree describes a DOCUMENT, and half of one grafted into another is a claim nobody made.",
            args.input.display()
        );
    }
    if pasted.source_page_labels_dropped || pasted.page_labels_stale {
        eprintln!(
            "pdfcer: {}: page labels -- source_dropped={} this_document_now_stale={}.",
            args.input.display(),
            u32::from(pasted.source_page_labels_dropped),
            u32::from(pasted.page_labels_stale),
        );
    }

    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "page-paste {} clip={} at={} pages={} orphaned_widgets={} orphaned_unrecoverable={} outline_dropped={} mode={} -> {}; changed={} objects={} appended={} out_bytes={}",
        args.input.display(),
        args.clip.display(),
        args.at,
        pasted.pages_inserted,
        pasted.orphaned_widgets,
        pasted.orphaned_widgets_unrecoverable,
        u32::from(pasted.source_outline_dropped),
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
    );
    finish_edit(args.input, &outcome)
}
