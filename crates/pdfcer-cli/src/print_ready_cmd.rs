use super::*;

use pdfcer_core::edit::PrintReadyError;

/// `print-ready`: a copy for a print service, with what prints burned in
/// (pdfcer-gui request G273). The invocation commits, so everything inferred
/// or left out is printed.
pub(crate) fn cmd_print_ready(
    input: &Path,
    output: &Path,
    free_text_c: Option<crate::viewer_preview::FreeTextCArg>,
) -> u8 {
    let (_source, session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let free_text_c = match free_text_c {
        Some(arg) => arg.into(),
        None => {
            let (settings, report) =
                pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
            report_settings(&report);
            settings.free_text_c
        }
    };
    let copy = match session.to_print_ready_bytes(free_text_c) {
        Ok(copy) => copy,
        Err(PrintReadyError::Edit(err)) => return report_edit_error(input, &err),
        Err(err) => {
            eprintln!("error: {}: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    if let Err(err) = write_output(output, &copy.bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    let r = &copy.report;
    println!(
        "burned={} not_printing={} lost={} drawn={} fields_generated={} output={}",
        r.burned,
        r.not_printing,
        r.lost.len(),
        r.appearances_drawn,
        r.fields_generated,
        output.display()
    );
    for line in &r.disclosures {
        println!("disclosure: {line}");
    }
    exit::SUCCESS
}
