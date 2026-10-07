//! One entry point for every model kind: [`OcrRunner::load`] takes a
//! discovered model and returns a recogniser, in-process or program.

use pdfcer_core::ocr::addon_manifest::AddonKind;
use pdfcer_core::ocr::addons::{OcrModel, VerifyError};
use pdfcer_core::ocr::{OcrPage, RecognizedWord};

use crate::program::{ProgramEngine, ProgramError, ProgramPolicy, program_status};
use crate::tesseract;

/// What every page is read with.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RunOptions {
    /// Language codes for engines that take them (Tesseract's `eng+deu`).
    /// In-process engines ignore it.
    pub languages: String,
    /// The raster's resolution, for engines that take it.
    pub dpi: f32,
    /// Whether program add-ons may run.
    pub policy: ProgramPolicy,
}

impl RunOptions {
    /// Options with [`ProgramPolicy::Allow`].
    #[must_use]
    pub fn new(languages: impl Into<String>, dpi: f32) -> Self {
        Self {
            languages: languages.into(),
            dpi,
            policy: ProgramPolicy::Allow,
        }
    }
}

/// Why a model cannot be loaded or run.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RunnerError {
    /// A data model for an engine this build lacks (or does not know).
    #[error("OCR model `{name}` is for the `{engine}` engine, which this build does not have")]
    EngineNotInBuild {
        /// The model.
        name: String,
        /// The engine token.
        engine: String,
    },
    /// A data model that lacks a file its engine needs.
    #[error("OCR model `{name}` in {folder} lacks one of {needs}")]
    MissingFile {
        /// The model.
        name: String,
        /// Its folder, for display.
        folder: String,
        /// The required files, comma-separated.
        needs: String,
    },
    /// A Tesseract add-on that is not `kind = program`.
    #[error(
        "OCR model `{name}` is for `tesseract`, which runs as a program; its manifest needs `kind = program`"
    )]
    NeedsProgramKind {
        /// The model.
        name: String,
    },
    /// A data model whose hashed files do not match.
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
    /// A program add-on refused or failed.
    #[error(transparent)]
    Program(#[from] ProgramError),
    /// The engine refused its model files or failed on a page.
    #[error("{0}")]
    Engine(String),
}

/// Whether `model` can run in this build under `policy`, without loading
/// it or hashing anything: the check a model list or drop-down makes.
///
/// # Errors
///
/// The [`RunnerError`] to show beside the model.
pub fn check_runnable(model: &OcrModel, policy: ProgramPolicy) -> Result<(), RunnerError> {
    if model.kind() == AddonKind::Program {
        program_status(model, policy).map_err(ProgramError::from)?;
        return Ok(());
    }
    if model.engine == tesseract::ENGINE {
        return Err(RunnerError::NeedsProgramKind {
            name: model.name.clone(),
        });
    }
    let needs = data_files(&model.engine).ok_or_else(|| RunnerError::EngineNotInBuild {
        name: model.name.clone(),
        engine: model.engine.clone(),
    })?;
    if !model.has_files(&needs) {
        return Err(RunnerError::MissingFile {
            name: model.name.clone(),
            folder: model.folder.display().to_string(),
            needs: needs.join(", "),
        });
    }
    Ok(())
}

/// The files an in-process engine needs, or `None` when it is not built.
fn data_files(engine: &str) -> Option<Vec<&'static str>> {
    match engine {
        #[cfg(feature = "ocrs")]
        "ocrs" => {
            use pdfcer_core::ocr::engine_ocrs::{DETECTION_MODEL, RECOGNITION_MODEL};
            Some(vec![DETECTION_MODEL, RECOGNITION_MODEL])
        }
        #[cfg(feature = "ocrcer")]
        "ocrcer" => Some(vec![pdfcer_core::ocr::engine_ocrcer::MODEL_FILE]),
        #[cfg(feature = "paddle")]
        "paddle" => {
            use pdfcer_core::ocr::engine_paddle::{DETECTION_MODEL, RECOGNITION_MODEL};
            Some(vec![DETECTION_MODEL, RECOGNITION_MODEL])
        }
        #[cfg(feature = "ocr-vl")]
        "paddle-vl" => Some(pdfcer_core::ocr::engine_paddle_vl::REQUIRED_FILES.to_vec()),
        _ => None,
    }
}

/// A loaded recogniser for one model.
pub struct OcrRunner {
    inner: Inner,
    files_verified: usize,
}

enum Inner {
    Program(ProgramEngine),
    #[cfg(feature = "ocrs")]
    Ocrs(pdfcer_core::ocr::engine_ocrs::OcrsEngine),
    #[cfg(feature = "ocrcer")]
    Ocrcer(Box<pdfcer_core::ocr::engine_ocrcer::OcrcerEngine>),
    #[cfg(feature = "paddle")]
    Paddle(Box<pdfcer_core::ocr::engine_paddle::PaddleEngine>),
    /// The engine and the last page's reading, kept for the disclosure.
    #[cfg(feature = "ocr-vl")]
    PaddleVl(
        Box<pdfcer_core::ocr::engine_paddle_vl::PaddleVlEngine>,
        std::sync::Mutex<Option<pdfcer_core::ocr::engine_paddle_vl::RegionReading>>,
    ),
}

impl std::fmt::Debug for OcrRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            Inner::Program(p) => f.debug_tuple("OcrRunner").field(p).finish(),
            #[cfg(feature = "ocrs")]
            Inner::Ocrs(_) => f.write_str("OcrRunner(ocrs)"),
            #[cfg(feature = "ocrcer")]
            Inner::Ocrcer(_) => f.write_str("OcrRunner(ocrcer)"),
            #[cfg(feature = "paddle")]
            Inner::Paddle(_) => f.write_str("OcrRunner(paddle)"),
            #[cfg(feature = "ocr-vl")]
            Inner::PaddleVl(..) => f.write_str("OcrRunner(paddle-vl)"),
        }
    }
}

impl OcrRunner {
    /// Load `model`: verify its hashed files, then load its engine
    /// in-process, or prepare its program.
    ///
    /// # Errors
    ///
    /// [`RunnerError`] naming the refusal, the changed file or the engine's
    /// complaint.
    pub fn load(model: &OcrModel, options: &RunOptions) -> Result<Self, RunnerError> {
        check_runnable(model, options.policy)?;
        if model.kind() == AddonKind::Program {
            let engine = ProgramEngine::from_model(model, options)?;
            return Ok(Self::from_program(engine));
        }
        let files_verified = model.verify().map_err(|source| RunnerError::Verify {
            name: model.name.clone(),
            source,
        })?;
        load_data(model).map(|inner| Self {
            inner,
            files_verified,
        })
    }

    /// Wrap a program prepared some other way (e.g.
    /// [`ProgramEngine::from_operator_folder`]).
    #[must_use]
    pub fn from_program(engine: ProgramEngine) -> Self {
        Self {
            inner: Inner::Program(engine),
            files_verified: 0,
        }
    }

    /// How many of a data model's files [`OcrRunner::load`] checked against
    /// its manifest's SHA-256 lines; 0 for a bare folder or a program (whose
    /// hashed files are [`ProgramEngine::source`]'s to report).
    #[must_use]
    pub fn files_verified(&self) -> usize {
        self.files_verified
    }

    /// The program, when this runner starts one.
    #[must_use]
    pub fn as_program(&self) -> Option<&ProgramEngine> {
        match &self.inner {
            Inner::Program(p) => Some(p),
            // Unreachable when no in-process engine feature is enabled.
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    /// Recognise an 8-bit greyscale image (row-major, top-down). Words are
    /// in image pixels, y-down; `pdfcer_core::ocr::words_to_page_space_on`
    /// maps them onto the page.
    ///
    /// # Errors
    ///
    /// [`RunnerError::Program`] or [`RunnerError::Engine`].
    pub fn recognize(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Vec<RecognizedWord>, RunnerError> {
        #[cfg(any(feature = "ocrs", feature = "ocrcer", feature = "paddle"))]
        use pdfcer_core::ocr::OcrEngine as _;
        match &self.inner {
            Inner::Program(p) => Ok(p.recognize(width, height, pixels)?),
            #[cfg(feature = "ocrs")]
            Inner::Ocrs(e) => e
                .recognize(width, height, pixels)
                .map_err(|e| RunnerError::Engine(e.to_string())),
            #[cfg(feature = "ocrcer")]
            Inner::Ocrcer(e) => e
                .recognize(width, height, pixels)
                .map_err(|e| RunnerError::Engine(e.to_string())),
            #[cfg(feature = "paddle")]
            Inner::Paddle(e) => e
                .recognize(width, height, pixels)
                .map_err(|e| RunnerError::Engine(e.to_string())),
            #[cfg(feature = "ocr-vl")]
            Inner::PaddleVl(e, last) => {
                let reading = e
                    .read_region(width, height, pixels)
                    .map_err(|e| RunnerError::Engine(e.to_string()))?;
                let lines = reading.lines.clone();
                *last
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(reading);
                Ok(lines)
            }
        }
    }

    /// [`Self::recognize`] for an image rasterised at `dpi`. A program engine
    /// is told `dpi` for this page instead of [`RunOptions`]' resolution; the
    /// in-process engines do not take a resolution and ignore it.
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
    ) -> Result<Vec<RecognizedWord>, RunnerError> {
        match &self.inner {
            Inner::Program(p) => Ok(p.recognize_at(width, height, pixels, dpi)?),
            // Unreachable when no in-process engine feature is enabled.
            #[allow(unreachable_patterns)]
            _ => self.recognize(width, height, pixels),
        }
    }

    /// [`Self::recognize`] (`dpi` `None`) or [`Self::recognize_at`] as an
    /// [`OcrPage`]: a program engine (Tesseract) fills `lines` and `blocks`
    /// with its own lines and paragraphs; every other engine leaves them
    /// empty, so the layer writer infers them. Words are image pixels,
    /// y-down; `pdfcer_core::ocr::words_to_page_space_on` keeps their order,
    /// so the indices stay valid after mapping.
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
    ) -> Result<OcrPage, RunnerError> {
        if let Some(p) = self.as_program() {
            return Ok(p.recognize_page(width, height, pixels, dpi)?);
        }
        let words = match dpi {
            Some(dpi) => self.recognize_at(width, height, pixels, dpi)?,
            None => self.recognize(width, height, pixels)?,
        };
        Ok(OcrPage {
            words,
            confidence_available: self.reports_confidence(),
            ..OcrPage::default()
        })
    }

    /// Whether the engine reports a per-word confidence.
    #[must_use]
    pub fn reports_confidence(&self) -> bool {
        #[cfg(any(feature = "ocrs", feature = "ocrcer", feature = "paddle"))]
        use pdfcer_core::ocr::OcrEngine as _;
        match &self.inner {
            // Tesseract's TSV has a `conf` column on every word row.
            Inner::Program(_) => true,
            #[cfg(feature = "ocrs")]
            Inner::Ocrs(e) => e.reports_confidence(),
            #[cfg(feature = "ocrcer")]
            Inner::Ocrcer(e) => e.reports_confidence(),
            #[cfg(feature = "paddle")]
            Inner::Paddle(e) => e.reports_confidence(),
            // The region's mean token probability, on every line.
            #[cfg(feature = "ocr-vl")]
            Inner::PaddleVl(..) => true,
        }
    }

    /// A line naming what the engine chose or inferred on the operator's
    /// behalf (rule 4), for the shell's report; `None` when there is
    /// nothing to disclose. PaddleOCR-VL's names the last page read.
    #[must_use]
    pub fn disclosure(&self) -> Option<String> {
        match &self.inner {
            #[cfg(feature = "paddle")]
            Inner::Paddle(e) => Some(crate::disclosure::paddle_disclosure(e)),
            #[cfg(feature = "ocr-vl")]
            Inner::PaddleVl(_, last) => {
                let last = last
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                Some(crate::disclosure::paddle_vl_disclosure(last.as_ref()))
            }
            // Unreachable when neither engine feature is enabled.
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

#[allow(clippy::unnecessary_wraps)] // every arm is compiled out in a lean build
fn load_data(model: &OcrModel) -> Result<Inner, RunnerError> {
    let failed = |e: &dyn std::fmt::Display| RunnerError::Engine(e.to_string());
    let dir = &model.folder;
    match model.engine.as_str() {
        #[cfg(feature = "ocrs")]
        "ocrs" => pdfcer_core::ocr::engine_ocrs::OcrsEngine::from_model_dir(dir)
            .map(Inner::Ocrs)
            .map_err(|e| failed(&e)),
        #[cfg(feature = "ocrcer")]
        "ocrcer" => {
            use pdfcer_core::ocr::engine_ocrcer::{MODEL_FILE, OcrcerEngine};
            let path = dir.join(MODEL_FILE);
            let bytes =
                std::fs::read(&path).map_err(|e| failed(&format!("{}: {e}", path.display())))?;
            OcrcerEngine::from_bytes(&bytes)
                .map(|e| Inner::Ocrcer(Box::new(e)))
                .map_err(|e| {
                    failed(&format!(
                        "{}: not a usable OCRcer model: {e}",
                        path.display()
                    ))
                })
        }
        #[cfg(feature = "paddle")]
        "paddle" => pdfcer_core::ocr::engine_paddle::PaddleEngine::from_model_dir(dir)
            .map(|e| Inner::Paddle(Box::new(e)))
            .map_err(|e| failed(&e)),
        #[cfg(feature = "ocr-vl")]
        "paddle-vl" => pdfcer_core::ocr::engine_paddle_vl::PaddleVlEngine::from_model_dir(dir)
            .map(|e| Inner::PaddleVl(Box::new(e), std::sync::Mutex::default()))
            .map_err(|e| failed(&e)),
        _ => {
            let _ = (failed, dir);
            Err(RunnerError::EngineNotInBuild {
                name: model.name.clone(),
                engine: model.engine.clone(),
            })
        }
    }
}
