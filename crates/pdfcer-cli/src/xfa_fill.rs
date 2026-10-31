//! `xfa-fill`: write values into an XFA form's data and save.

use super::*;

/// `xfa-fill`: apply each `PATH=VALUE` through
/// `EditSession::fill_xfa_field` in one session, then save. A refused set
/// returns before the save, so a failed run writes nothing.
pub(crate) fn cmd_xfa_fill(
    input: &Path,
    sets: &[String],
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
    opts: &pdfcer_core::xfa::MergeOptions,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    for set in sets {
        let Some((path, value)) = set.split_once('=') else {
            eprintln!("pdfcer: --set must be PATH=VALUE, got {set:?}");
            return exit::EDIT_REFUSED;
        };
        let Some(path) = parse_dotted(path) else {
            eprintln!("pdfcer: --set path must be dotted numbers (0.1.2), got {path:?}");
            return exit::EDIT_REFUSED;
        };
        match session.fill_xfa_field(&path, value, opts) {
            Ok(out) => print_set(&path, &out),
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
    let r = &outcome.report;
    println!(
        "xfa-fill {} sets={} mode={} -> {}; changed={} objects={} verbatim={} \
reserialized={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        sets.len(),
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

/// `0.1.2` as a path; `None` when any part is not a number.
/// `xfa-fill --data-creation` (gaps G-B12, G-B16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum DataCreationArg {
    /// Create the missing data node and disclose it as created=.
    #[default]
    Create,
    /// Refuse a field bound to no data node.
    Refuse,
}

impl From<DataCreationArg> for pdfcer_core::xfa::DataCreation {
    fn from(v: DataCreationArg) -> Self {
        match v {
            DataCreationArg::Create => Self::Create,
            DataCreationArg::Refuse => Self::Refuse,
        }
    }
}

fn parse_dotted(text: &str) -> Option<Vec<usize>> {
    text.split('.').map(|p| p.trim().parse().ok()).collect()
}

fn print_set(path: &[usize], out: &pdfcer_core::xfa::XfaFillOutcome) {
    let shared = if out.shared_with.is_empty() {
        "-".to_owned()
    } else {
        out.shared_with
            .iter()
            .map(|p| dotted(p))
            .collect::<Vec<_>>()
            .join(",")
    };
    let created = if out.created.is_empty() {
        "-".to_owned()
    } else {
        out.created.join("/")
    };
    println!(
        "xfa-set path={} data={} stream={} shared_with={shared} created={created}",
        dotted(path),
        dotted(&out.data),
        out.stream.num
    );
}
