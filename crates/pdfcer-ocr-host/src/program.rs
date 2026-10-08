//! Program add-ons (decision 184): an OCR engine executable in an add-on
//! folder, run as a separate process.
//!
//! Before every run each file the manifest hashes is opened, hashed and
//! compared; a mismatch refuses the run and names the file. On Windows the
//! handles are opened without write or delete sharing and held until the
//! program exits, so no file can be changed between the check and its use.
//! Elsewhere that window stays open: a process that can write the add-on
//! folder can swap a file after it is hashed. Decision 184 records the gap.

use std::fs::File;
use std::path::{Path, PathBuf};

use pdfcer_core::ocr::addon_manifest::AddonKind;
use pdfcer_core::ocr::addons::{OcrModel, VerifyError, check_digest};
use pdfcer_core::ocr::{OcrPage, RecognizedWord};

use crate::runner::RunOptions;
use crate::tesseract::{self, Invocation};

/// Engines whose programs this crate knows how to run.
pub const PROGRAM_ENGINES: [&str; 1] = [tesseract::ENGINE];

/// Whether program add-ons may run at all; settings key
/// `ocr_program_addons`. `Refuse` never starts a process.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ProgramPolicy {
    /// Run a program add-on whose hashes match.
    #[default]
    Allow,
    /// List program add-ons but never run one.
    Refuse,
}

impl ProgramPolicy {
    /// The settings-file spelling: `allow` or `refuse`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Refuse => "refuse",
        }
    }

    /// The stricter of two policies: either one refusing refuses.
    #[must_use]
    pub fn and(self, other: Self) -> Self {
        if self == Self::Refuse || other == Self::Refuse {
            Self::Refuse
        } else {
            Self::Allow
        }
    }
}

/// Why a program add-on will not run. Shown in model lists, so each says
/// which add-on and what to change.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProgramRefusal {
    /// A data add-on or bare folder, not a program.
    #[error("OCR model `{name}` is a data add-on, not a program")]
    NotAProgram {
        /// The model.
        name: String,
    },
    /// [`ProgramPolicy::Refuse`] is in force.
    #[error(
        "OCR model `{name}` runs the program `{program}`, and running OCR programs is turned off"
    )]
    RefusedByPolicy {
        /// The model, or the folder for an operator-named one.
        name: String,
        /// The program file.
        program: String,
    },
    /// The manifest has no `sha256` line for the program.
    #[error(
        "OCR model `{name}` runs the program `{program}`, but its manifest has no `sha256` line for it, so it is not run"
    )]
    NoProgramHash {
        /// The model.
        name: String,
        /// The program file.
        program: String,
    },
    /// No protocol for the engine.
    #[error(
        "OCR model `{name}` is a program for the `{engine}` engine; this pdfcer can run programs for {}",
        PROGRAM_ENGINES.join(", ")
    )]
    NoProtocol {
        /// The model.
        name: String,
        /// The engine token.
        engine: String,
    },
    /// The program file is not in the folder.
    #[error("OCR model `{name}`: {} is not there", path.display())]
    ProgramMissing {
        /// The model.
        name: String,
        /// Where it should be.
        path: PathBuf,
    },
}

/// Why a program could not be prepared or run.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProgramError {
    /// It may not run.
    #[error(transparent)]
    Refused(#[from] ProgramRefusal),
    /// A hashed file is missing, unreadable or changed.
    #[error(
        "OCR model `{name}` refused: {source}. Reinstall the add-on, or delete its folder to uninstall it"
    )]
    Verify {
        /// The model.
        name: String,
        /// The file at fault.
        #[source]
        source: VerifyError,
    },
    /// The folder or the options are unusable (missing language data, bad
    /// language codes, no executable).
    #[error("{0}")]
    Setup(String),
    /// The program failed or produced unreadable output.
    #[error("{0}")]
    Run(String),
}

/// Where a [`ProgramEngine`]'s program came from.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProgramSource {
    /// A program add-on; `hashed_files` are re-verified before every run.
    Addon {
        /// The model name.
        name: String,
        /// How many files the manifest hashes.
        hashed_files: usize,
    },
    /// A folder the operator named for this run, holding a stock install.
    /// Nothing is hashed: the operator chose the program by path.
    OperatorFolder(PathBuf),
}

/// Whether `model` is a program add-on that may run under `policy`; on
/// success, the program's path. Reads no file contents.
///
/// # Errors
///
/// The [`ProgramRefusal`] to show beside the model.
pub fn program_status(model: &OcrModel, policy: ProgramPolicy) -> Result<PathBuf, ProgramRefusal> {
    let manifest = match &model.manifest {
        Some(m) if m.kind == AddonKind::Program => m,
        _ => {
            return Err(ProgramRefusal::NotAProgram {
                name: model.name.clone(),
            });
        }
    };
    let program = manifest.program.clone().unwrap_or_default();
    if policy == ProgramPolicy::Refuse {
        let name = model.name.clone();
        return Err(ProgramRefusal::RefusedByPolicy { name, program });
    }
    if !PROGRAM_ENGINES.contains(&model.engine.as_str()) {
        return Err(ProgramRefusal::NoProtocol {
            name: model.name.clone(),
            engine: model.engine.clone(),
        });
    }
    if manifest.program_digest().is_none() {
        let name = model.name.clone();
        return Err(ProgramRefusal::NoProgramHash { name, program });
    }
    let path = model.file_path(&program);
    if !path.is_file() {
        let name = model.name.clone();
        return Err(ProgramRefusal::ProgramMissing { name, path });
    }
    Ok(path)
}

/// A program ready to run, plus the files to re-verify before each run.
#[derive(Debug, Clone)]
pub struct ProgramEngine {
    program: PathBuf,
    pinned: Vec<(PathBuf, [u8; 32])>,
    source: ProgramSource,
    invocation: Invocation,
}

impl ProgramEngine {
    /// Prepare a program add-on: check [`program_status`], verify every
    /// hashed file once, and check the requested languages are present.
    ///
    /// # Errors
    ///
    /// [`ProgramError`] naming the refusal, the changed file or the missing
    /// language data.
    pub fn from_model(model: &OcrModel, options: &RunOptions) -> Result<Self, ProgramError> {
        let program = program_status(model, options.policy)?;
        let manifest = model.manifest.as_ref().ok_or(ProgramRefusal::NotAProgram {
            name: model.name.clone(),
        })?;
        let pinned: Vec<(PathBuf, [u8; 32])> = manifest
            .files
            .iter()
            .map(|f| (model.file_path(&f.file), f.sha256))
            .collect();
        let source = ProgramSource::Addon {
            name: model.name.clone(),
            hashed_files: pinned.len(),
        };
        let invocation = invocation(model, manifest.data.as_deref(), options)?;
        let engine = Self {
            program,
            pinned,
            source,
            invocation,
        };
        drop(engine.hold_verified()?);
        Ok(engine)
    }

    /// Prepare the stock Tesseract layout in a folder the operator named
    /// (`tesseract[.exe]` beside `tessdata/`). No manifest and no hashes:
    /// the operator picked the program by path. [`ProgramPolicy::Refuse`]
    /// still refuses it.
    ///
    /// # Errors
    ///
    /// [`ProgramError`] for the policy, a missing executable or missing
    /// language data.
    pub fn from_operator_folder(dir: &Path, options: &RunOptions) -> Result<Self, ProgramError> {
        let program = dir.join(tesseract::EXE_FILE);
        if options.policy == ProgramPolicy::Refuse {
            return Err(ProgramRefusal::RefusedByPolicy {
                name: dir.display().to_string(),
                program: tesseract::EXE_FILE.to_owned(),
            }
            .into());
        }
        if !program.is_file() {
            return Err(ProgramError::Setup(format!(
                "{}: no Tesseract executable here",
                program.display()
            )));
        }
        let invocation = Invocation::new(
            dir.join(tesseract::TESSDATA_DIR),
            &options.languages,
            options.dpi,
            &options.dictionaries,
        )
        .map_err(ProgramError::Setup)?;
        Ok(Self {
            program,
            pinned: Vec::new(),
            source: ProgramSource::OperatorFolder(dir.to_path_buf()),
            invocation,
        })
    }

    /// The executable that runs.
    #[must_use]
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Where the program came from.
    #[must_use]
    pub fn source(&self) -> &ProgramSource {
        &self.source
    }

    /// The language codes passed to the program, `+`-joined.
    #[must_use]
    pub fn languages(&self) -> &str {
        self.invocation.langs()
    }

    /// Which word lists the program is told to use, and how many user
    /// words: for the run report.
    #[must_use]
    pub fn dictionary_note(&self) -> String {
        self.invocation.dictionary_note()
    }

    /// Re-verify every hashed file, then run the program on an 8-bit
    /// greyscale image (row-major, top-down). Words are in image pixels,
    /// y-down.
    ///
    /// # Errors
    ///
    /// [`ProgramError::Verify`] when a file changed since the manifest was
    /// written; [`ProgramError::Run`] when the program fails.
    pub fn recognize(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Vec<RecognizedWord>, ProgramError> {
        self.run(width, height, pixels, None).map(|page| page.words)
    }

    /// [`Self::recognize`] for an image rasterised at `dpi`, which the program
    /// is told instead of the resolution it was loaded with. Tesseract uses it
    /// to size its text-height expectations; a wrong value costs accuracy on
    /// small or large type.
    ///
    /// # Errors
    ///
    /// As [`Self::recognize`].
    pub fn recognize_at(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
        dpi: f32,
    ) -> Result<Vec<RecognizedWord>, ProgramError> {
        self.run(width, height, pixels, Some(dpi))
            .map(|page| page.words)
    }

    /// [`Self::recognize_at`] with Tesseract's own lines and paragraphs
    /// ([`pdfcer_core::ocr::tesseract_tsv::parse_tsv_page`]); `dpi` `None`
    /// uses the load-time resolution. Words are image pixels, y-down.
    ///
    /// # Errors
    ///
    /// As [`Self::recognize`].
    pub fn recognize_page(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
        dpi: Option<f32>,
    ) -> Result<OcrPage, ProgramError> {
        self.run(width, height, pixels, dpi)
    }

    fn run(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
        dpi: Option<f32>,
    ) -> Result<OcrPage, ProgramError> {
        let held = self.hold_verified()?;
        let words = self
            .invocation
            .run(&self.program, width, height, pixels, dpi)
            .map_err(ProgramError::Run);
        drop(held);
        words
    }

    /// Open and hash every pinned file, returning the open handles.
    fn hold_verified(&self) -> Result<Vec<File>, ProgramError> {
        let name = match &self.source {
            ProgramSource::Addon { name, .. } => name.clone(),
            ProgramSource::OperatorFolder(dir) => dir.display().to_string(),
        };
        let mut held = Vec::with_capacity(self.pinned.len());
        for (path, sha256) in &self.pinned {
            let verified = open_shared_read(path)
                .map_err(|e| VerifyError::Unreadable {
                    path: path.clone(),
                    reason: e.to_string(),
                })
                .and_then(|mut f| check_digest(path, &mut f, sha256).map(|()| f));
            match verified {
                Ok(f) => held.push(f),
                Err(source) => {
                    return Err(ProgramError::Verify {
                        name: name.clone(),
                        source,
                    });
                }
            }
        }
        Ok(held)
    }
}

/// Open for reading while letting others only read: write, rename and
/// delete fail until the handle closes.
#[cfg(windows)]
fn open_shared_read(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt as _;
    const FILE_SHARE_READ: u32 = 0x1;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
}

#[cfg(not(windows))]
fn open_shared_read(path: &Path) -> std::io::Result<File> {
    File::open(path)
}

/// The program's command for `options`, reading the language data under
/// `data` (default [`tesseract::TESSDATA_DIR`]) and any user word files;
/// nothing is hashed or started.
fn invocation(
    model: &OcrModel,
    data: Option<&str>,
    options: &RunOptions,
) -> Result<Invocation, ProgramError> {
    let data = data.unwrap_or(tesseract::TESSDATA_DIR);
    Invocation::new(
        model.file_path(data),
        &options.languages,
        options.dpi,
        &options.dictionaries,
    )
    .map_err(ProgramError::Setup)
}

/// Whether a program add-on can honour `options` (languages present, word
/// files readable), without hashing or starting it. A model with no
/// manifest passes: [`crate::check_runnable`] is the one to refuse it.
pub(crate) fn check_program_options(
    model: &OcrModel,
    options: &RunOptions,
) -> Result<(), ProgramError> {
    match &model.manifest {
        Some(m) => invocation(model, m.data.as_deref(), options).map(drop),
        None => Ok(()),
    }
}
