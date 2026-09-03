//! Where OCR models come from: bundled beside the binary, fetched on request,
//! or supplied by the operator.
//!
//! # The decision this encodes
//!
//! The operator chose (2026-08-12) to support more than one OCR engine so that
//! multi-language recognition and the WASM-capable pure-Rust path can both
//! exist. One of those engines' model weights are **CC-BY-SA-4.0**, which is
//! a licence pdfcer would rather not *redistribute* inside an MIT portable
//! folder.
//!
//! ★★ **THE WITHDRAWAL RECORDED HERE IS DEAD, AND THIS MODULE'S OWN TITLE
//! WAS WRONG FOR TWELVE DAYS.** Corrected 2026-08-25.
//!
//! This module header used to say *"and why pdfcer does not download them"*,
//! and argued the point at length. The argument was sound when written on
//! 2026-08-12 and was overturned by the operator the very next day. It is
//! kept below rather than deleted, because a reader who has met the old
//! reasoning elsewhere needs to see it retired rather than vanished:
//!
//! > ~~"The obvious answer — have pdfcer download those models on request —
//! > was proposed and then WITHDRAWN on inspection… `ARCHITECTURE.md` §1.1
//! > states pdfcer contains **no HTTP client and no TLS stack**… A
//! > fail-closed CI job (`no-network`, standing rule R12) refuses any
//! > HTTP/TLS/socket client crate entering any pdfcer crate… §1.1 puts this
//! > posture at the same weight as the GUI-core-separation and round-trip
//! > invariants."~~
//!
//! **Decision 061 (2026-08-13) narrowed exactly that rule, and by the
//! operator's own correction** — he said the no-network rule *"was made too
//! broad"*, that what he meant was the software must not *depend* on a
//! network to function, and that *"it is fine to have download update or
//! download addin capability."*
//!
//! The line is **what the software needs to RUN versus what the operator can
//! ASK it to fetch**:
//!
//! - `pdfcer-core` and `pdfcer-render` remain network-free **permanently and
//!   gate-enforced**. That half was never narrowed and is justified twice
//!   over: the engine must not need a network to parse or render, and both
//!   crates must cross into the wasm32 fork where no native HTTP stack
//!   exists. **This module is in `pdfcer-core`, so it still contains no
//!   network code and never will.**
//! - The **shells** may fetch on the operator's explicit request. That is
//!   what `pdfcer-fetch` is — a separate crate on the far side of the line,
//!   pinning each artefact by URL **and SHA-256** so a substituted or
//!   truncated download is refused rather than installed.
//!
//! So the resolution order below is no longer the only way models arrive; it
//! is where a shell's downloader **puts** them, and where a bundled copy is
//! found. Both remain true, and neither is a workaround for a prohibition
//! that no longer exists.
//!
//! ★ **What decision 061 did NOT relax**, stated because an operator
//! narrowing one clause is not consent to widen its neighbour: no telemetry,
//! no analytics, no crash reporting, no licence callback, and **no startup
//! update check**. Every fetch is one an operator asked for at the moment it
//! happens.
//!
//! # What this module does instead
//!
//! The same thing `--font-dir` already does for fonts, and for the same
//! reason: pdfcer looks in **predictable places** rather than fetching
//! anything. No network client, no decision record, no weakened claim — and a
//! model can live on a share or a stick, which a downloader could not have
//! offered anyway.
//!
//! ## ⚠️ This does NOT mean OCR arrives empty-handed
//!
//! Stated explicitly because an earlier draft of this comment said only "the
//! operator supplies the files", which reads as though nothing ships and OCR
//! does nothing until they go and find a model. That would be a real cost and
//! it is not the design.
//!
//! ★ **The arrangement this paragraph used to predict is not the one that
//! shipped, and the difference is worth stating rather than quietly
//! rewriting.** It said the multi-language engine's permissive weights would
//! be bundled and only the CC-BY-SA-4.0 WASM-capable engine's would be
//! operator-supplied. What actually happened: the multi-language engine was
//! never adopted, and the operator **answered the open question YES** on
//! 2026-08-13 — the share-alike weights **are** bundled, at
//! `crates/pdfcer-core/assets/models/ocrs/`, with their licence recorded in a
//! `PROVENANCE.md` that `tools/check-shipped-assets.py` enforces the
//! existence of.
//!
//! So today:
//!
//! - **The `ocrs` weights are BUNDLED** and found via
//!   [`ModelSource::BesideExecutable`] in the single-folder portable layout.
//!   OCR works out of the box.
//! - **A shell may also fetch them**, pinned by hash, for a build that does
//!   not carry them.
//! - **An operator may still name a directory**, which takes precedence over
//!   both and is never silently overridden.
//!
//! The precedent matters. `embed-font --font-dir` solved an identical shape
//! (pdfcer needs a large licensed asset it should not necessarily ship) and it
//! works. Inventing a second, heavier mechanism for the same problem would be
//! the "two ways to do one thing, one of them worse" defect this codebase
//! keeps refusing.
//!
//! # Resolution order, and why absence is reported rather than guessed at
//!
//! [`resolve_model_dir`] returns the FIRST location that exists, and reports
//! every place it looked when none does. An OCR feature that silently did
//! nothing because a model was missing would be indistinguishable from one
//! that ran and found no text — and those two need completely different
//! actions from the operator.

use std::path::{Path, PathBuf};

/// The subdirectory name an engine's models live under, beside the binary or
/// in an operator-named folder.
///
/// A per-engine name rather than one shared `models/` directory: two engines
/// have differently-named files with different licences, and merging them into
/// one folder would make the `PROVENANCE.md` that
/// `tools/check-shipped-assets.py` requires describe a mixture. Keeping them
/// apart keeps each licence attached to its own files.
pub type EngineDirName = &'static str;

/// Where a model directory was found, so a shell can say so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSource {
    /// An explicit path the operator named (`--model-dir`, or a setting).
    ///
    /// First in the order, always: an operator who names a path has said
    /// something specific and pdfcer must not quietly prefer its own copy.
    OperatorSupplied(PathBuf),
    /// A directory beside the running executable — the single-folder portable
    /// case, where models sit next to `pdfcer.exe`.
    BesideExecutable(PathBuf),
    /// A directory in the platform's user-data location.
    UserData(PathBuf),
}

impl ModelSource {
    /// The directory itself.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::OperatorSupplied(p) | Self::BesideExecutable(p) | Self::UserData(p) => p,
        }
    }

    /// A stable token for a machine-readable report.
    #[must_use]
    pub const fn token(&self) -> &'static str {
        match self {
            Self::OperatorSupplied(_) => "operator-supplied",
            Self::BesideExecutable(_) => "beside-executable",
            Self::UserData(_) => "user-data",
        }
    }
}

/// Why no model directory could be found, carrying everywhere that was tried.
///
/// # Why the searched paths are part of the error
///
/// "OCR models not found" is unactionable. "I looked in these three places"
/// tells the operator exactly where to put the files, and — just as often —
/// reveals that they put them somewhere pdfcer never looks, which is a
/// different problem from not having them at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelsNotFound {
    /// The engine whose models are missing.
    pub engine: EngineDirName,
    /// Every directory that was checked, in the order they were checked.
    pub searched: Vec<PathBuf>,
}

impl std::fmt::Display for ModelsNotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no OCR models for `{}` — looked in: {}",
            self.engine,
            self.searched
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl std::error::Error for ModelsNotFound {}

/// Find the model directory for `engine`.
///
/// Checks, in order: `explicit` if given, a `models/<engine>` directory beside
/// the running executable, then `user_data/models/<engine>`.
///
/// `exe_dir` and `user_data` are parameters rather than being discovered here
/// so this function is testable without touching the real filesystem layout,
/// and so `pdfcer-core` does not acquire an opinion about where a shell keeps
/// its data — that is the shell's business and differs between the CLI and the
/// GUI.
///
/// # Errors
///
/// [`ModelsNotFound`], carrying every path that was tried.
pub fn resolve_model_dir(
    engine: EngineDirName,
    explicit: Option<&Path>,
    exe_dir: Option<&Path>,
    user_data: Option<&Path>,
) -> Result<ModelSource, ModelsNotFound> {
    resolve_model_dir_with(engine, explicit, exe_dir, user_data, &[])
}

/// [`resolve_model_dir`], but a directory only counts if it CONTAINS
/// `required`.
///
/// # Why the directory's existence was not enough
///
/// The original only asked `is_dir()`. An **empty** `models/ocrs` therefore
/// passed resolution, and the failure surfaced much later and in the wrong
/// vocabulary — the engine reported a missing model file after a shell had
/// already told the operator the models were found. Worse, an empty directory
/// SHADOWED a perfectly good one further down the search order: create
/// `models/ocrs` beside the executable and the operator's own `--model-dir`
/// copy would never be reached.
///
/// Passing an empty `required` reproduces the old behaviour exactly, which is
/// what [`resolve_model_dir`] does — a caller that does not know an engine's
/// filenames should not be forced to invent them.
///
/// # Errors
///
/// [`ModelsNotFound`], carrying every path that was tried. A directory that
/// existed but lacked a required file is reported as a searched path, because
/// from the operator's side *"I made that folder and it still says no"* needs
/// the folder named.
pub fn resolve_model_dir_with(
    engine: EngineDirName,
    explicit: Option<&Path>,
    exe_dir: Option<&Path>,
    user_data: Option<&Path>,
    required: &[&str],
) -> Result<ModelSource, ModelsNotFound> {
    let usable = |dir: &Path| dir.is_dir() && required.iter().all(|f| dir.join(f).is_file());
    let mut searched = Vec::new();

    if let Some(dir) = explicit {
        // An operator-named path that does not exist is reported, NOT skipped
        // in favour of a bundled copy. Silently using something else after
        // they named a specific folder is the sneaky half of rule 4: they
        // would believe they were running the model they pointed at.
        searched.push(dir.to_path_buf());
        if usable(dir) {
            return Ok(ModelSource::OperatorSupplied(dir.to_path_buf()));
        }
        return Err(ModelsNotFound { engine, searched });
    }

    if let Some(base) = exe_dir {
        let candidate = base.join("models").join(engine);
        searched.push(candidate.clone());
        if usable(&candidate) {
            return Ok(ModelSource::BesideExecutable(candidate));
        }
    }

    if let Some(base) = user_data {
        let candidate = base.join("models").join(engine);
        searched.push(candidate.clone());
        if usable(&candidate) {
            return Ok(ModelSource::UserData(candidate));
        }
    }

    Err(ModelsNotFound { engine, searched })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir()
            .join("pdfcer-ocr-models-tests")
            .join(name);
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    /// An operator-named directory wins over anything pdfcer would have found.
    #[test]
    fn an_operator_supplied_path_takes_precedence() {
        let explicit = tmp("explicit");
        let exe = tmp("exe");
        std::fs::create_dir_all(exe.join("models").join("demo")).expect("exe models");

        let got = resolve_model_dir("demo", Some(&explicit), Some(&exe), None)
            .expect("the explicit path exists");
        assert_eq!(got, ModelSource::OperatorSupplied(explicit));
    }

    /// ★ A named path that does NOT exist is an error, never a silent fallback.
    ///
    /// Falling back to a bundled copy here would run a different model from
    /// the one the operator pointed at, while reporting success. They would
    /// have no way to tell — the output is text either way. That is exactly
    /// the substitution rule 4 exists to prevent.
    #[test]
    fn a_named_path_that_is_missing_does_not_fall_back() {
        let exe = tmp("exe2");
        std::fs::create_dir_all(exe.join("models").join("demo")).expect("exe models");
        let nonexistent = std::env::temp_dir().join("pdfcer-definitely-not-here-9f2a");

        let err = resolve_model_dir("demo", Some(&nonexistent), Some(&exe), None)
            .expect_err("a named path that does not exist must fail");
        assert_eq!(
            err.searched,
            vec![nonexistent],
            "and must report the path THEY named, not the one it ignored"
        );
    }

    /// Beside the executable is the single-folder portable case.
    #[test]
    fn a_directory_beside_the_executable_is_found() {
        let exe = tmp("exe3");
        let models = exe.join("models").join("demo");
        std::fs::create_dir_all(&models).expect("models");
        let got = resolve_model_dir("demo", None, Some(&exe), None).expect("found");
        assert_eq!(got, ModelSource::BesideExecutable(models));
    }

    /// ★ When nothing is found, EVERY searched path is reported.
    ///
    /// "Models not found" is unactionable. The list is what tells the operator
    /// where to put the files — and often reveals that they put them somewhere
    /// pdfcer never looks, which is a different problem from not having them.
    #[test]
    fn every_searched_path_is_reported_when_nothing_is_found() {
        let exe = tmp("exe4");
        let data = tmp("data4");
        let err =
            resolve_model_dir("demo", None, Some(&exe), Some(&data)).expect_err("nothing exists");
        assert_eq!(err.searched.len(), 2, "both candidates must be reported");
        let text = err.to_string();
        assert!(
            text.contains("models") && text.contains("demo"),
            "the message must name the paths, got {text:?}"
        );
    }

    /// Two engines never share a directory.
    ///
    /// Their model files carry different licences, and a merged folder would
    /// make the `PROVENANCE.md` that `tools/check-shipped-assets.py` requires
    /// describe a mixture rather than a set.
    #[test]
    fn each_engine_resolves_to_its_own_directory() {
        let exe = tmp("exe5");
        std::fs::create_dir_all(exe.join("models").join("alpha")).expect("alpha");
        std::fs::create_dir_all(exe.join("models").join("beta")).expect("beta");

        let a = resolve_model_dir("alpha", None, Some(&exe), None).expect("alpha");
        let b = resolve_model_dir("beta", None, Some(&exe), None).expect("beta");
        assert_ne!(a.path(), b.path(), "two engines must not share a folder");
    }
}
