use super::*;

/// **Print one object's internal structure** (`Pass 193.0`).
///
/// The verb that closes the gap this Pass exists for: before it, an
/// `/ExtGState` compressed inside an object stream was unreachable from any
/// shipped command, and diagnosing a rendering defect that depended on one
/// meant hand-decompressing the file in a throwaway script.
///
/// Read-only. Every bound is disclosed in the output rather than applied
/// silently — see `pdfcer_core::structure` for why each exists.
pub(crate) fn cmd_dump_object(
    input: &Path,
    id: u32,
    generation: u16,
    depth: usize,
    streams: StreamDump,
    max_stream_bytes: usize,
) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let target = pdfcer_core::object::ObjId::new(id, generation);
    let options = pdfcer_core::structure::DumpOptions::default()
        .with_depth(depth)
        .with_streams(streams.into())
        .with_max_stream_bytes(max_stream_bytes);
    // The physical fact first, because it is the one the logical dump hides
    // and the one that explains why an object was invisible to a text search.
    let inv = pdfcer_core::structure::inventory(&doc);
    if let Some(row) = inv.objects.iter().find(|r| r.id == target) {
        println!("% {} — {}", row.id, describe_storage(row.storage));
        if !row.referenced_by.is_empty() {
            let names: Vec<String> = row.referenced_by.iter().map(ToString::to_string).collect();
            println!("% referenced by: {}", names.join(", "));
        } else if inv.trailer_referenced.contains(&target) {
            println!("% referenced by: the trailer");
        } else {
            println!("% referenced by: nothing (see `list-objects --show-unreferenced`)");
        }
    }
    print!(
        "{}",
        pdfcer_core::structure::render_object(&doc, doc.bytes(), target, &options)
    );
    exit::SUCCESS
}

/// **Walk and print a document's object graph** from a chosen root.
pub(crate) fn cmd_dump_structure(
    input: &Path,
    root: &str,
    max_objects: usize,
    streams: StreamDump,
    max_stream_bytes: usize,
) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let start = match resolve_dump_root(&doc, root) {
        Ok(id) => id,
        Err(msg) => {
            eprintln!("pdfcer: {}: {msg}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let options = pdfcer_core::structure::DumpOptions::default()
        .with_max_objects(max_objects)
        .with_streams(streams.into())
        .with_max_stream_bytes(max_stream_bytes);
    println!("% walk from {start}");
    print!(
        "{}",
        pdfcer_core::structure::walk(&doc, doc.bytes(), start, &options)
    );
    exit::SUCCESS
}

/// Turn a `--root` spec into an object id.
///
/// Accepts `catalog`, `page:<n>` (1-based, matching how every reader numbers
/// pages), or a bare `<num>[,<gen>]`. Errors name what was accepted rather than
/// only what was rejected, because a rejected spec is usually a guess at the
/// syntax.
pub(crate) fn resolve_dump_root(
    doc: &pdfcer_core::document::Document,
    spec: &str,
) -> Result<pdfcer_core::object::ObjId, String> {
    use pdfcer_core::object::ObjId;
    let spec = spec.trim();
    if spec.eq_ignore_ascii_case("catalog") {
        return doc
            .view()
            .graph()
            .catalog_id()
            .ok_or_else(|| "this document has no resolvable /Root catalog".to_owned());
    }
    if let Some(rest) = spec.strip_prefix("page:") {
        let n: usize = rest
            .parse()
            .map_err(|_| format!("`page:{rest}` — expected a 1-based page number"))?;
        let pages = pdfcer_core::page_tree::pages(doc).map_err(|e| format!("page tree: {e}"))?;
        let idx = n
            .checked_sub(1)
            .ok_or_else(|| "page numbers are 1-based; `page:0` does not exist".to_owned())?;
        return pages
            .get(idx)
            .map(|p| p.id)
            .ok_or_else(|| format!("page {n} is past the end; the document has {}", pages.len()));
    }
    let (num, generation) = match spec.split_once(',') {
        Some((a, b)) => (a.trim(), b.trim()),
        None => (spec, "0"),
    };
    let num: u32 = num
        .parse()
        .map_err(|_| format!("`{spec}` — expected `catalog`, `page:<n>`, or `<num>[,<gen>]`"))?;
    let generation: u16 = generation
        .parse()
        .map_err(|_| format!("`{spec}` — generation must be a number"))?;
    Ok(ObjId::new(num, generation))
}

/// One object's storage, in the operator's terms.
pub(crate) fn describe_storage(s: pdfcer_core::structure::Storage) -> String {
    use pdfcer_core::structure::Storage;
    match s {
        Storage::AtOffset { offset, generation } => {
            format!("at byte offset {offset} (generation {generation})")
        }
        Storage::InObjectStream { container, index } => format!(
            "COMPRESSED inside object stream {container} at index {index} — this is why a text \
             search of the file cannot find it (ISO 32000-1 §7.5.7)"
        ),
        Storage::Free { generation } => {
            format!("marked FREE by the cross-reference (next generation {generation})")
        }
        Storage::Unindexed => "parsed, but the cross-reference table has no entry for it — \
             expected on a recovered document, whose table was rebuilt by scanning"
            .to_owned(),
    }
}

/// **Inventory every object, and report the file's physical layout.**
pub(crate) fn cmd_list_objects(
    input: &Path,
    filter_type: Option<&str>,
    layout_only: bool,
    show_unreferenced: bool,
) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let layout = pdfcer_core::structure::layout(&doc);
    println!("version={}", layout.version);
    println!("xref_style={}", layout.xref_style);
    println!("startxref={}", layout.startxref);
    println!("objects={}", layout.object_count);
    println!("highest_object_number={}", layout.highest_object_number);
    println!("linearized={}", layout.linearized);
    println!("encrypted={}", layout.encrypted);
    println!("object_streams={}", layout.object_streams.len());
    for (container, members) in &layout.object_streams {
        println!(
            "  objstm {container}: {} object(s) {members:?}",
            members.len()
        );
    }
    if layout.suppressed_by_size > 0 {
        // Not a curiosity: raising /Size — which creating any object does —
        // would expose every one of these.
        println!(
            "suppressed_by_size={}  % the file's /Size HIDES this many cross-reference entries",
            layout.suppressed_by_size
        );
    }
    if let Some(r) = &layout.recovered {
        println!(
            "recovered=yes  % the cross-reference table was rebuilt by scanning; this document \
             cannot be saved incrementally. {r}"
        );
    }

    if layout_only {
        return exit::SUCCESS;
    }

    let inv = pdfcer_core::structure::inventory(&doc);
    println!();
    for row in &inv.objects {
        if let Some(want) = filter_type
            && row.type_name.as_deref() != Some(want)
        {
            continue;
        }
        let ty = row.type_name.as_deref().unwrap_or("-");
        let sub = row.subtype.as_deref().unwrap_or("-");
        let storage = match row.storage {
            pdfcer_core::structure::Storage::AtOffset { offset, .. } => format!("offset:{offset}"),
            pdfcer_core::structure::Storage::InObjectStream { container, index } => {
                format!("objstm:{container}[{index}]")
            }
            pdfcer_core::structure::Storage::Free { .. } => "free".to_owned(),
            pdfcer_core::structure::Storage::Unindexed => "unindexed".to_owned(),
        };
        let bytes = row
            .stream_bytes
            .map_or_else(|| "-".to_owned(), |n| n.to_string());
        println!(
            "{} kind={} type={ty} subtype={sub} {storage} stream_bytes={bytes} refs={}",
            row.id,
            row.kind,
            row.referenced_by.len()
        );
    }

    if show_unreferenced {
        let orphans = inv.unreferenced();
        println!();
        println!(
            "unreferenced={}  % NOT by itself a defect: an incremental update leaves superseded \
             objects behind by design (ISO 32000-1 §7.5.6)",
            orphans.len()
        );
        for id in orphans {
            println!("  {id}");
        }
    }
    exit::SUCCESS
}

/// **Export a PDF's internals to an editable form** (`Pass 194.0`).
///
/// Read-only with respect to the input; writes one new file. The note on stderr
/// is rule 4's off-canvas disclosure: an export is a FULL REWRITE, and an
/// operator who expects pdfcer's usual minimal diff has to be told so at the
/// moment it happens rather than inferring it from a byte count.
pub(crate) fn cmd_export_structure(input: &Path, output: &Path) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let bytes = match pdfcer_core::editable::export(&doc) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    if let Err(err) = std::fs::write(output, &bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    let layout = pdfcer_core::structure::layout(&doc);
    println!(
        "exported {} -> {} objects={} bytes={} objstm_expanded={}",
        input.display(),
        output.display(),
        doc.object_count(),
        bytes.len(),
        layout.object_streams.len()
    );
    eprintln!(
        "pdfcer: note: this export is a FULL REWRITE and is deliberately not byte-identical to the input -- object streams are expanded and stream data is decoded so the file can be read and edited. Compile it back with `import-structure`, which IS minimal-diff: it appends only the objects you changed and leaves the original bytes untouched"
    );
    exit::SUCCESS
}

/// **Compile an edited export back**, appending only what changed
/// (`Pass 194.0`).
///
/// Defaults to an incremental update because that is the capability qpdf lacks:
/// the original bytes stay as an untouched prefix, so a signature over a range
/// the operator did not edit remains valid.
pub(crate) fn cmd_import_structure(
    input: &Path,
    edited_path: &Path,
    output: &Path,
    full: bool,
    dry_run: bool,
) -> u8 {
    let original = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let edited = match open_document(edited_path) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", edited_path.display());
            return exit_code_for_doc(&err);
        }
    };
    let (dirty, report) = pdfcer_core::editable::import(&original, &edited);
    println!(
        "modified={} added={} removed={} unchanged={} streams_matched_after_decode={}",
        report.modified.len(),
        report.added.len(),
        report.removed.len(),
        report.unchanged,
        report.streams_matched_after_decode
    );
    for id in &report.modified {
        println!("  modified {id}");
    }
    for id in &report.added {
        println!("  added    {id}");
    }
    for id in &report.removed {
        println!("  removed  {id}");
    }
    // The refusal `EditSession::check_certification` applies to every edit:
    // an enforced certification (§12.8.4 Table 258) forbids changes, and an
    // arbitrary object import is at least as broad as any session edit.
    let census = pdfcer_core::signature::census(&original);
    if census.forbids_structural_change() && !report.is_empty() {
        eprintln!(
            "pdfcer: {}: refused: the document carries an enforced certification signature (DocMDP P={}) that forbids changes (ISO 32000-1 section 12.8.4)",
            input.display(),
            census.certification_permission.unwrap_or(2)
        );
        return exit::EDIT_REFUSED;
    }
    if dry_run {
        println!("dry-run: nothing written");
        return exit::SUCCESS;
    }
    if report.is_empty() && !full {
        // Worth saying rather than quietly writing a copy: it tells the
        // operator their edit did not take, which is the one failure they
        // cannot otherwise see.
        eprintln!(
            "pdfcer: note: nothing changed between the original and the edited export, so the output is byte-identical to the input"
        );
    }
    // bypass-exempt: an object-level compile-back below the edit model (qpdf
    // QDF parity). One-shot, so there is no undo to record; every changed
    // object id is printed above (rule 4), and the certification refusal
    // above is the one EditSession would apply.
    let opts = pdfcer_core::writer::SaveOptions::identity();
    let saved = if full {
        pdfcer_core::writer::save_full(&original, &dirty, &opts)
    } else {
        pdfcer_core::writer::save_incremental(&original, &dirty, &opts)
    };
    let (bytes, _) = match saved {
        Ok(v) => v,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", output.display());
            return exit::RUNTIME_ERROR;
        }
    };
    if let Err(err) = std::fs::write(output, &bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    println!(
        "wrote {} bytes={} mode={}",
        output.display(),
        bytes.len(),
        if full { "full-rewrite" } else { "incremental" }
    );
    if full {
        eprintln!(
            "pdfcer: note: a full rewrite DESTROYS every existing digital signature (ISO 32000-1 section 12.8.1). The default incremental mode does not"
        );
    }
    exit::SUCCESS
}
