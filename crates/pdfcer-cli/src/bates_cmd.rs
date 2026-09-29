use super::*;

use pdfcer_core::bates::{BatesNumbering, BatesOutcome, BatesPosition, BatesStamp};

/// `bates-stamp`'s arguments.
pub(crate) struct BatesArgs<'a> {
    pub(crate) inputs: &'a [PathBuf],
    pub(crate) out_dir: &'a Path,
    pub(crate) start: u64,
    pub(crate) prefix: String,
    pub(crate) suffix: String,
    pub(crate) digits: u8,
    pub(crate) position: BatesPositionArg,
    pub(crate) margin: f64,
    pub(crate) size: f64,
    pub(crate) pages: &'a str,
    pub(crate) name: BatesNameArg,
}

/// One stamped file, held in memory until every file has succeeded.
struct Stamped {
    input: PathBuf,
    output: PathBuf,
    bytes: Vec<u8>,
    outcome: BatesOutcome,
    impact: SignatureImpact,
}

const fn position(arg: BatesPositionArg) -> BatesPosition {
    match arg {
        BatesPositionArg::TopLeft => BatesPosition::TopLeft,
        BatesPositionArg::TopCenter => BatesPosition::TopCenter,
        BatesPositionArg::TopRight => BatesPosition::TopRight,
        BatesPositionArg::BottomLeft => BatesPosition::BottomLeft,
        BatesPositionArg::BottomCenter => BatesPosition::BottomCenter,
        BatesPositionArg::BottomRight => BatesPosition::BottomRight,
    }
}

/// The output file name for `input` stamped `first`..=`last`.
fn output_name(input: &Path, name: BatesNameArg, outcome: &BatesOutcome) -> std::ffi::OsString {
    let range = format!("{}-{}", outcome.first_label, outcome.last_label);
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    match name {
        BatesNameArg::Keep => input.file_name().unwrap_or_default().to_owned(),
        BatesNameArg::Range => format!("{range}.pdf").into(),
        BatesNameArg::KeepRange => format!("{stem}_{range}.pdf").into(),
    }
}

/// Implement `pdfcer bates-stamp`: stamp every input in order, carrying the
/// number across files, then write them all. Nothing is written unless every
/// file stamps and every output path is free of the inputs and of each other.
pub(crate) fn cmd_bates_stamp(args: &BatesArgs<'_>) -> u8 {
    use pdfcer_core::writer::SaveOptions;
    let mut stamp = BatesStamp::new(BatesNumbering::new(
        args.prefix.clone(),
        args.digits,
        args.suffix.clone(),
    ));
    stamp.position = position(args.position);
    stamp.margin = args.margin;
    stamp.font_size = args.size;

    let mut next = args.start;
    let mut done: Vec<Stamped> = Vec::new();
    for input in args.inputs {
        let (_source, mut session) = match open_for_edit(input) {
            Ok(pair) => pair,
            Err(code) => return code,
        };
        let count = match session.pages() {
            Ok(pages) => pages.len(),
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", input.display());
                return exit::EDIT_REFUSED;
            }
        };
        stamp.pages = match sign::parse_pages(args.pages, count) {
            Ok(pages) => Some(pages),
            Err(err) => {
                eprintln!("pdfcer: {}: --pages: {err}", input.display());
                return exit::EDIT_REFUSED;
            }
        };
        let outcome = match session.stamp_bates(&stamp, next) {
            Ok(o) => o,
            Err(err) => return report_edit_error(input, &err),
        };
        let impact = session.signature_impact_of_save(CoreSaveMode::Incremental);
        let bytes = match session.to_incremental_bytes(&SaveOptions::identity()) {
            Ok((bytes, _)) => bytes,
            Err(err) => {
                eprintln!("pdfcer: {}: save refused: {err}", input.display());
                return exit::SAVE_REFUSED;
            }
        };
        next = outcome.next;
        let output = args.out_dir.join(output_name(input, args.name, &outcome));
        done.push(Stamped {
            input: input.clone(),
            output,
            bytes,
            outcome,
            impact,
        });
    }

    if let Err(code) = check_outputs(&done, args.inputs) {
        return code;
    }
    if let Err(err) = std::fs::create_dir_all(args.out_dir) {
        eprintln!("pdfcer: {}: {err}", args.out_dir.display());
        return exit::IO_ERROR;
    }
    for file in &done {
        if let Err(err) = std::fs::write(&file.output, &file.bytes) {
            eprintln!("pdfcer: {}: {err}", file.output.display());
            return exit::IO_ERROR;
        }
        println!(
            "stamped {} -> {} pages={} first={} last={} signature={}",
            file.input.display(),
            file.output.display(),
            file.outcome.pages.len(),
            sanitize_token(&file.outcome.first_label),
            sanitize_token(&file.outcome.last_label),
            signature_token(file.impact),
        );
        report_signature(&file.input, file.impact);
    }
    let (first, last) = match (done.first(), done.last()) {
        (Some(a), Some(b)) => (
            a.outcome.first_label.as_str(),
            b.outcome.last_label.as_str(),
        ),
        _ => ("", ""),
    };
    println!(
        "bates-stamp files={} pages={} first={} last={} next={next}",
        done.len(),
        done.iter().map(|f| f.outcome.pages.len()).sum::<usize>(),
        sanitize_token(first),
        sanitize_token(last),
    );
    exit::SUCCESS
}

/// Refuse an output that would replace an input, or two outputs with one
/// path (two inputs of the same name from different folders, under `keep`).
fn check_outputs(done: &[Stamped], inputs: &[PathBuf]) -> Result<(), u8> {
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let inputs: Vec<PathBuf> = inputs.iter().map(|p| canonical(p)).collect();
    let mut seen: Vec<PathBuf> = Vec::new();
    for file in done {
        let out = canonical(&file.output);
        if inputs.contains(&out) {
            eprintln!(
                "pdfcer: {}: the output would replace an input; choose another --out-dir or --name.",
                file.output.display()
            );
            return Err(exit::EDIT_REFUSED);
        }
        let key = file.output.to_string_lossy().to_lowercase();
        if seen
            .iter()
            .any(|s| s.to_string_lossy().to_lowercase() == key)
        {
            eprintln!(
                "pdfcer: {}: two inputs would be written to this one path; use --name range or \
keep-range.",
                file.output.display()
            );
            return Err(exit::EDIT_REFUSED);
        }
        seen.push(file.output.clone());
    }
    Ok(())
}
