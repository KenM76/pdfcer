//! Form-data exchange: `export-data` and `import-data` (FDF, XFDF, CSV).

use super::*;

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
    if let Err(err) = write_output(output, &bytes) {
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
    let data = match read_form_data(data_path) {
        Ok(d) => d,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    // Counted BEFORE the import: a skipped rich-text field leaves no trace
    // in the outcome to count after.
    let rich_targets = count_rich_text_targets(&session, &data);
    let outcome = match session.import_form_data(&data) {
        Ok(o) => o,
        Err(err) => return report_edit_error(input, &err),
    };
    disclose_import_skips(input, rich_targets, &outcome);
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

/// Reads and parses an FDF, XFDF or CSV data file, detected by content.
///
/// Ordered by how specific the marker is: FDF has a `%FDF` header, XFDF
/// opens with `<`, and CSV, having no marker, is the residue.
fn read_form_data(data_path: &Path) -> Result<pdfcer_core::fdf::FormData, u8> {
    let data_bytes = match std::fs::read(data_path) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", data_path.display());
            return Err(exit::IO_ERROR);
        }
    };
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
    parsed.map_err(|err| {
        eprintln!("pdfcer: {}: {err}", data_path.display());
        exit::EDIT_REFUSED
    })
}

/// How many entries in `data` name a rich-text field of the live form.
fn count_rich_text_targets(
    session: &pdfcer_core::edit::EditSession,
    data: &pdfcer_core::fdf::FormData,
) -> usize {
    pdfcer_core::forms::parse_acroform(&session.graph()).map_or(0, |form| {
        data.fields
            .iter()
            .filter(|e| {
                form.field_by_name(&e.name)
                    .is_some_and(pdfcer_core::forms::Field::is_rich_text)
            })
            .count()
    })
}

/// Says WHY fields were skipped or withheld, which `skipped=N` cannot.
fn disclose_import_skips(
    input: &Path,
    rich_targets: usize,
    outcome: &pdfcer_core::edit::ImportOutcome,
) {
    if rich_targets > 0 {
        eprintln!(
            "pdfcer: {}: {rich_targets} rich-text field(s) were left untouched — not even their plain value was applied. Writing plain text beside a field's existing formatting makes conforming readers display the OLD text (ISO 32000-1 §12.7.3.3), so pdfcer leaves such a field alone rather than corrupt what it shows.",
            input.display()
        );
    }
    if outcome.password_values_withheld > 0 {
        eprintln!(
            "pdfcer: {}: {} password field(s) were drawn as asterisks and their values NOT saved (ISO 32000 §12.7.4.3)",
            input.display(),
            outcome.password_values_withheld
        );
    }
}
