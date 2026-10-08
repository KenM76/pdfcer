//! `list-metadata` and `remove-metadata`.

use super::*;
use pdfcer_core::doc_metadata::{
    DocumentIdAction, MetadataInventory, MetadataItemId, MetadataKind, MetadataRemoval,
    MetadataRemoveOptions,
};
use pdfcer_core::edit::EditSession;

fn open_session(input: &Path) -> Result<EditSession, u8> {
    open_document(input).map(EditSession::new).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", input.display());
        exit_code_for_doc(&err)
    })
}

/// `list-metadata`.
pub(crate) fn cmd_list_metadata(input: &Path, json: bool) -> u8 {
    let session = match open_session(input) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let inventory = session.metadata_inventory();
    if json {
        println!("{}", inventory_json(&inventory));
    } else {
        println!(
            "list-metadata {} items={}",
            input.display(),
            inventory.items.len()
        );
        for item in &inventory.items {
            println!(
                "  {}\t{}\t{}\t{}\t{}",
                item.id,
                item.kind.as_str(),
                item.bytes,
                item.location,
                item.preview
            );
        }
    }
    if inventory.truncated {
        eprintln!("pdfcer: the object walk hit its budget; the list is partial.");
    }
    exit::SUCCESS
}

fn inventory_json(inventory: &MetadataInventory) -> String {
    let rows: Vec<String> = inventory
        .items
        .iter()
        .map(|i| {
            format!(
                "{{\"id\":\"{}\",\"kind\":\"{}\",\"bytes\":{},\"location\":\"{}\",\"preview\":\"{}\"}}",
                json_escape(i.id.as_str()),
                i.kind.as_str(),
                i.bytes,
                json_escape(&i.location),
                json_escape(&i.preview)
            )
        })
        .collect();
    format!(
        "{{\"truncated\":{},\"items\":[{}]}}",
        inventory.truncated,
        rows.join(",")
    )
}

/// The arguments of `remove-metadata`, borrowed from the parsed command.
pub(crate) struct RemoveMetadataArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) items: &'a [String],
    pub(crate) kinds: &'a [String],
    pub(crate) all: bool,
    pub(crate) document_id: DocumentIdArg,
    pub(crate) apply: bool,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
}

/// The ids `args` selects: `--all`, every item of a `--kind`, and each
/// `--item` as given (so an unknown one is reported by the removal).
fn selected(session: &EditSession, args: &RemoveMetadataArgs<'_>) -> Vec<MetadataItemId> {
    let inventory = session.metadata_inventory();
    let mut ids: Vec<MetadataItemId> = inventory
        .items
        .into_iter()
        .filter(|i| args.all || args.kinds.iter().any(|k| k == i.kind.as_str()))
        .map(|i| i.id)
        .collect();
    ids.extend(args.items.iter().map(MetadataItemId::new));
    ids
}

/// `remove-metadata`.
pub(crate) fn cmd_remove_metadata(args: &RemoveMetadataArgs<'_>) -> u8 {
    if !args.all && args.items.is_empty() && args.kinds.is_empty() {
        eprintln!("pdfcer: name what to remove: --item ID, --kind KIND or --all");
        return exit::RUNTIME_ERROR;
    }
    if let Some(bad) = args
        .kinds
        .iter()
        .find(|k| !MetadataKind::ALL.iter().any(|m| m.as_str() == k.as_str()))
    {
        let known: Vec<&str> = MetadataKind::ALL.iter().map(|m| m.as_str()).collect();
        eprintln!("pdfcer: unknown --kind {bad}; one of: {}", known.join(", "));
        return exit::RUNTIME_ERROR;
    }
    let mut session = match open_session(args.input) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let ids = selected(&session, args);
    let action = match args.document_id {
        DocumentIdArg::Regenerate => DocumentIdAction::Regenerate,
        DocumentIdArg::Remove => DocumentIdAction::Remove,
    };
    let options = MetadataRemoveOptions::default().with_document_id(action);
    let report = match session.remove_metadata(&ids, &options) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.input.display());
            return exit::EDIT_REFUSED;
        }
    };
    print_removal(args, &report);
    let incomplete = !report.not_found.is_empty() || !report.not_removed.is_empty();
    let code = if incomplete {
        exit::EDIT_REFUSED
    } else {
        exit::SUCCESS
    };
    if !args.apply {
        eprintln!("pdfcer: dry run — pass --apply with --output to write the file.");
        return code;
    }
    match save_removal(&session, args) {
        exit::SUCCESS => code,
        failed => failed,
    }
}

fn print_removal(args: &RemoveMetadataArgs<'_>, report: &MetadataRemoval) {
    println!(
        "remove-metadata {} removed={} objects_freed={} mode={} applied={}",
        args.input.display(),
        report.removed.len(),
        report.objects_freed,
        mode_token(args.mode),
        u32::from(args.apply)
    );
    for id in &report.removed {
        println!("  removed {id}");
    }
    for id in &report.not_found {
        eprintln!("pdfcer: not found: {id} (run list-metadata for the ids)");
    }
    for miss in &report.not_removed {
        eprintln!("pdfcer: not removed: {}: {}", miss.id, miss.reason);
    }
    for note in &report.disclosures {
        // The always-present full-rewrite note only matters when the save
        // is incremental; a full save is that rewrite.
        if matches!(args.mode, SaveMode::Full) && note.contains("full rewrite") {
            continue;
        }
        eprintln!("pdfcer: {note}");
    }
}

/// A full save unpacks the object streams holding a changed object, so an
/// old value cannot survive inside one; `/Producer` is never stamped.
fn save_removal(session: &EditSession, args: &RemoveMetadataArgs<'_>) -> u8 {
    use pdfcer_core::writer::{ProducerPolicy, SaveOptions};
    let Some(out) = args.output else {
        eprintln!("pdfcer: --apply needs --output <PATH>");
        return exit::RUNTIME_ERROR;
    };
    let saved = match args.mode {
        SaveMode::Incremental => session.to_incremental_bytes(&SaveOptions::identity()),
        SaveMode::Full => session
            .to_full_bytes_decomposing_containers(
                &SaveOptions::default().with_producer(ProducerPolicy::Preserve),
            )
            .map(|(bytes, report, _)| (bytes, report)),
    };
    let (bytes, report) = match saved {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("pdfcer: {}: save refused: {err}", args.input.display());
            hint_recovered_base(&err);
            return exit::SAVE_REFUSED;
        }
    };
    if let Err(err) = write_output(out, &bytes) {
        eprintln!("pdfcer: {}: {err}", out.display());
        return exit::IO_ERROR;
    }
    crate::edit_common::disclose_rc4(out, report.rc4_keystream_reused);
    println!("  wrote {} bytes={}", out.display(), bytes.len());
    exit::SUCCESS
}
