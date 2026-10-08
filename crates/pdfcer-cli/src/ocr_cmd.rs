use super::*;

/// `fetch-ocr-models` — download the pinned `ocrs` weights.
///
/// The pins are `pdfcer_core::ocr::models::fetchable_models("ocrs")`, the one
/// list every shell fetches from; why each file comes from the host it does is
/// documented there and in `crates/pdfcer-core/assets/models/ocrs/PROVENANCE.md`.
#[cfg(feature = "download")]
pub(crate) fn cmd_fetch_ocr_models(dir: Option<&Path>) -> u8 {
    use pdfcer_fetch::{PinnedArtifact, fetch_verified};

    let Some(set) = pdfcer_core::ocr::models::fetchable_models("ocrs") else {
        eprintln!("pdfcer: fetch-ocr-models: this build lists no fetchable ocrs models");
        return exit::RUNTIME_ERROR;
    };
    let target = match dir {
        Some(d) => d.to_path_buf(),
        None => match std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
        {
            Some(exe_dir) => exe_dir.join("models").join(set.folder),
            None => {
                eprintln!(
                    "pdfcer: fetch-ocr-models: could not locate this executable's directory \
                     — pass --dir"
                );
                return exit::RUNTIME_ERROR;
            }
        },
    };
    if let Err(err) = std::fs::create_dir_all(&target) {
        eprintln!("pdfcer: {}: {err}", target.display());
        return exit::IO_ERROR;
    }

    let artifacts: Vec<PinnedArtifact> = set
        .files
        .iter()
        .map(|f| PinnedArtifact::new(f.url, f.sha256, f.file_name))
        .collect();

    eprintln!(
        "pdfcer: fetch-ocr-models: downloading {} file(s) to {} — these weights are \
         {}, by {} ({})",
        artifacts.len(),
        target.display(),
        set.licence,
        set.creator,
        set.source
    );
    for art in &artifacts {
        match fetch_verified(art, &target) {
            Ok(path) => println!("fetched {} -> {}", art.url, path.display()),
            Err(err) => {
                eprintln!("pdfcer: fetch-ocr-models: {err}");
                return exit::RUNTIME_ERROR;
            }
        }
    }
    println!(
        "fetch-ocr-models {} files={} verified=sha256",
        target.display(),
        artifacts.len()
    );
    // CC-BY-SA requires attribution, and a fetched copy has no PROVENANCE.md
    // beside it to carry it.
    eprintln!("pdfcer: fetch-ocr-models: {}", set.attribution());
    exit::SUCCESS
}

/// `fetch-ocr-models`, in a build compiled WITHOUT the `download` feature.
///
/// Refuses **by name**, which is the operator's own modularity rule: a
/// stripped capability says what is missing and how to get it back, rather
/// than the subcommand quietly not existing. A missing subcommand reads as a
/// version difference; a named refusal reads as a build choice.
#[cfg(not(feature = "download"))]
pub(crate) fn cmd_fetch_ocr_models(_dir: Option<&Path>) -> u8 {
    eprintln!(
        "pdfcer: fetch-ocr-models: this build was compiled without the `download` feature, \
         so it contains no network code at all and cannot fetch anything. The ocrs weights \
         come as an add-on zip that installs as `models/ocrs` beside the executable; \
         unzip it there, or point \
         `ocr --model-dir` at one"
    );
    exit::UNIMPLEMENTED
}

/// `list-standards` — the render presets, and the provenance of every value.
///
/// # Why the provenance is a COLUMN and not a footnote
///
/// `pdfce-gui` asked pdfcer for this vector and declined to guess it, on the
/// grounds that *"a control labelled `ISO 15930-7` carries that standard's
/// authority whether or not we intended it to."* That is right, and it means
/// the interesting information is not the value — it is how much weight the
/// value can bear. Most of these are `best-effort`: the standards mostly do
/// not legislate this far, and saying so is the honest output.
pub(crate) fn cmd_list_standards(only: Option<&str>) -> u8 {
    use pdfcer_core::settings::presets::{RenderPreset, RenderStandard};

    let wanted: Vec<RenderStandard> = match only {
        None => RenderStandard::all().to_vec(),
        Some(tok) => match RenderStandard::parse(tok) {
            Ok(s) => vec![s],
            Err(bad) => {
                eprintln!(
                    "pdfcer: list-standards: unknown standard {bad:?} — known: {}",
                    RenderStandard::all()
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                return exit::RUNTIME_ERROR;
            }
        },
    };

    for std in wanted {
        let preset = RenderPreset::for_standard(std);
        println!("standard {} title={:?}", std.as_str(), std.title());
        for e in preset.entries() {
            // Formatted by core, deliberately: `PresetAction` is
            // `#[non_exhaustive]`, so a match here would need a wildcard and a
            // seventh variant would print as the fallback while still
            // compiling. See `PresetAction::value_string`.
            let value = e.action.value_string();
            println!(
                "  setting {:<24} value={:<28} evidence={:<15} why={:?}",
                e.key.as_str(),
                value,
                e.evidence.label(),
                e.why
            );
        }
        for line in preset.disclosures() {
            eprintln!("pdfcer: {line}");
        }
    }
    exit::SUCCESS
}

/// Say what a search-driven redaction **could not read**.
///
/// # Why a redaction owes this louder than a search does
///
/// `find-text` reporting zero hits wastes a minute. `redact-mark --search`
/// reporting zero marks, on a document whose text was never recoverable as
/// Unicode, tells an operator that a name is not present when it is on the
/// page in front of them — and the next thing they do is send the file.
///
/// The two populations both **render perfectly**, which is exactly what makes
/// the failure invisible: a Type 3 font with no `/ToUnicode` (ISO 32000-1
/// §9.6.5, glyphs that are content streams named by arbitrary `/CharProcs`
/// keys) and an `Identity-H` font with no `/ToUnicode` (§9.10.2 excludes it
/// from every ladder rung).
///
/// Printed whether or not anything matched, and that is deliberate. A
/// partial match is the more dangerous case, not the safer one: "3 marks
/// authored" reads as success, and the operator has no reason to suspect a
/// fourth occurrence sat in a font the scan could not read.
pub(crate) fn report_unsearchable_redaction(
    input: &Path,
    d: &pdfcer_core::text_extract::TextDiagnostics,
) {
    if d.ladder_failures == 0 {
        return;
    }
    eprintln!(
        "pdfcer: {}: WARNING — {} of {} character code(s) in this document could not be \
         mapped to Unicode, so a search CANNOT have matched them. Marks were authored only \
         where the text was readable",
        input.display(),
        d.ladder_failures,
        d.codes_total
    );
    if d.type3_fonts_without_to_unicode > 0 {
        eprintln!(
            "pdfcer: {}: {} Type 3 font(s) carry no /ToUnicode CMap (ISO 32000-1 §9.6.5) — \
             text set in them renders correctly and cannot be searched or redacted by search",
            input.display(),
            d.type3_fonts_without_to_unicode
        );
    }
    if d.identity_fonts_without_to_unicode > 0 {
        eprintln!(
            "pdfcer: {}: {} font(s) are Identity-H/Adobe-Identity-0 with no /ToUnicode — \
             §9.10.2 excludes them from every ladder rung, so text set in them cannot be \
             searched or redacted by search",
            input.display(),
            d.identity_fonts_without_to_unicode
        );
    }
    eprintln!(
        "pdfcer: {}: DO NOT treat this document as cleared on the strength of a \
         search-driven redaction. Check the unreadable runs by eye, or mark them with --rect",
        input.display()
    );
}
