//! The OCR model files a shell may download on the operator's request: URL,
//! SHA-256 and local name per file, plus the licence a fetched copy carries
//! and the add-on manifest to write beside it.
//!
//! Plain data with no network code, so every shell fetches the same measured
//! bytes from one list (`pdfcer-fetch` does the fetching). A file is pinned by
//! hash because the same model's copies on different hosts differ; see
//! `crates/pdfcer-core/assets/models/ocrs/PROVENANCE.md`. Hugging Face URLs
//! name a commit, not a branch, so a pin cannot move.
//!
//! Every set is a flat folder. Engines absent from the list, and why, are in
//! `docs/core-api/` under "Downloadable OCR models".

use super::EngineDirName;

/// One downloadable model file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct PinnedModelFile {
    /// The exact HTTPS URL to fetch.
    pub url: &'static str,
    /// Lowercase hex SHA-256 the downloaded bytes must equal.
    pub sha256: &'static str,
    /// The name to write it under, inside [`FetchableModels::folder`]; a bare
    /// file name, never a path.
    pub file_name: &'static str,
}

/// One downloadable model set and the terms it comes under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct FetchableModels {
    /// The add-on name its manifest declares (`ocr --ocr-model NAME`);
    /// unique in [`FETCHABLE_MODELS`].
    pub name: &'static str,
    /// The engine name, as `ocr --ocr-engine` takes it.
    pub engine: &'static str,
    /// The sub-folder of a model root the files go in; add-on discovery finds
    /// it there by its manifest, and for `ocrs` it is also the folder
    /// [`super::resolve_model_dir`] looks in.
    pub folder: EngineDirName,
    /// Human-readable label, as the manifest's `label`.
    pub label: &'static str,
    /// Comma-separated language tags, as the manifest's `languages`; empty
    /// when the set does not state them.
    pub languages: &'static str,
    /// The set's own version, as the manifest's `version`.
    pub version: &'static str,
    /// Every file the engine needs, all of which must be fetched.
    pub files: &'static [PinnedModelFile],
    /// SPDX licence identifier of the files.
    pub licence: &'static str,
    /// Where the licence text is published.
    pub licence_url: &'static str,
    /// Who made the files.
    pub creator: &'static str,
    /// Where they come from.
    pub source: &'static str,
    /// What redistributing a fetched copy obliges, in one sentence.
    pub obligation: &'static str,
}

impl FetchableModels {
    /// The attribution a shell shows once the files are fetched: licence,
    /// creator, source and obligation in one line. A fetched copy has no
    /// `PROVENANCE.md` beside it to carry it.
    #[must_use]
    pub fn attribution(&self) -> String {
        format!(
            "licence {} <{}>, creator {}, source {}. {}",
            self.licence, self.licence_url, self.creator, self.source, self.obligation
        )
    }

    /// The `pdfcer-ocr-model.txt` (decision 182) to write into
    /// [`Self::folder`] once every file has been fetched and verified, so
    /// that `OcrModel::verify` can re-check the folder later exactly as it
    /// checks an installed add-on. It carries a `sha256` line per file and
    /// the [`Self::attribution`] as a comment.
    ///
    /// ```
    /// use pdfcer_core::ocr::addon_manifest::parse_manifest;
    /// use pdfcer_core::ocr::models::fetchable_model;
    ///
    /// let set = fetchable_model("ocrcer").expect("pinned");
    /// let manifest = parse_manifest(&set.manifest())?;
    /// assert_eq!(manifest.name, "ocrcer");
    /// assert_eq!(manifest.files.len(), set.files.len());
    /// # Ok::<(), pdfcer_core::ocr::addon_manifest::ManifestError>(())
    /// ```
    #[must_use]
    pub fn manifest(&self) -> String {
        let mut text = format!(
            "# pdfcer OCR model add-on manifest (decision 182), written on download.\n\
             # {}\n\
             name = {}\nengine = {}\nlabel = \"{}\"\n",
            self.attribution(),
            self.name,
            self.engine,
            self.label
        );
        if !self.languages.is_empty() {
            text.push_str(&format!("languages = {}\n", self.languages));
        }
        text.push_str(&format!(
            "version = {}\nlicence = {}\n",
            self.version, self.licence
        ));
        for f in self.files {
            text.push_str(&format!("sha256 = {} {}\n", f.file_name, f.sha256));
        }
        text
    }
}

/// The `ocrs` engine's weights. The detection file comes from the author's S3
/// bucket because the Hugging Face copy does not work with `ocrs` 0.12.2; the
/// recognition file is fine on Hugging Face. Do not move them onto one host.
const OCRS: FetchableModels = FetchableModels {
    name: "ocrs",
    engine: "ocrs",
    folder: "ocrs",
    label: "ocrs (Latin script)",
    languages: "en",
    version: "1",
    files: &[
        PinnedModelFile {
            url: "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten",
            sha256: "f15cfb56bd02c4bf478a20343986504a1f01e1665c2b3a0ad66340f054b1b5ca",
            file_name: "text-detection.rten",
        },
        PinnedModelFile {
            url: "https://huggingface.co/robertknight/ocrs/resolve/main/text-rec-checkpoint-s52qdbqt.rten",
            sha256: "606d9a0414c6b73c99df75b707c11c70d1c8b12e1d4f900922e185fc37bfca65",
            file_name: "text-rec-checkpoint.rten",
        },
    ],
    licence: "CC-BY-SA-4.0",
    licence_url: "https://creativecommons.org/licenses/by-sa/4.0/",
    creator: "Robert Knight",
    source: "the ocrs project",
    obligation: "Redistributing these files carries that licence's attribution and share-alike terms",
};

/// PaddlePaddle's own PP-OCRv5 mobile exports with the Latin recogniser
/// (English and French), the set pdfcer-gui bundles. Taken as published: the
/// opset-7 recogniser is upgraded on load (`ocr::onnx_upgrade`) and the
/// dictionary is read from `inference.yml`.
const PADDLE_LATIN: FetchableModels = FetchableModels {
    name: "ppocrv5-latin",
    engine: "paddle",
    folder: "ppocrv5-latin",
    label: "PP-OCRv5 Latin (English, French)",
    languages: "en, fr",
    version: "5",
    files: &[
        PinnedModelFile {
            url: "https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_det_onnx/resolve/e6f4fa85f00e168c862bc462aebca69eef9b3d3d/inference.onnx",
            sha256: "a431985659dc921974177a95adcfbb90fd9e51989a5e04d70d0b75f597b6e61d",
            file_name: "det.onnx",
        },
        PinnedModelFile {
            url: "https://huggingface.co/PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx/resolve/89d3a50e2c27e2e7cceeab0e944c25c807d5db4f/inference.onnx",
            sha256: "7888113072263cb471b93f66dd5e2ad70548dc526fa1ace760d0d973dd121498",
            file_name: "rec.onnx",
        },
        PinnedModelFile {
            url: "https://huggingface.co/PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx/resolve/89d3a50e2c27e2e7cceeab0e944c25c807d5db4f/inference.yml",
            sha256: "0bbe984570f597af3638e50bdf2e8276f3ab26a61966096538b3b0d1849f5c84",
            file_name: "inference.yml",
        },
    ],
    licence: "Apache-2.0",
    licence_url: "https://www.apache.org/licenses/LICENSE-2.0",
    creator: "the PaddleOCR authors (PaddlePaddle)",
    source: "Hugging Face PaddlePaddle/PP-OCRv5_mobile_det_onnx and latin_PP-OCRv5_mobile_rec_onnx",
    obligation: "Redistributing these files carries that licence's copy-of-licence and notice terms",
};

/// The PP-OCRv4 Chinese + English set the pdfcer portable folder bundles,
/// byte-identical to `crates/pdfcer-core/assets/models/paddle/`; the
/// dictionary is embedded in `rec.onnx`.
const PADDLE_CH_EN: FetchableModels = FetchableModels {
    name: "ppocrv4-ch-en",
    engine: "paddle",
    folder: "ppocrv4-ch-en",
    label: "PP-OCRv4 Chinese + English",
    languages: "zh, en",
    version: "4",
    files: &[
        PinnedModelFile {
            url: "https://huggingface.co/SWHL/RapidOCR/resolve/1cfba2e90fc938db55889873735088de210cc173/PP-OCRv4/ch_PP-OCRv4_det_infer.onnx",
            sha256: "d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9",
            file_name: "det.onnx",
        },
        PinnedModelFile {
            url: "https://huggingface.co/SWHL/RapidOCR/resolve/1cfba2e90fc938db55889873735088de210cc173/PP-OCRv4/ch_PP-OCRv4_rec_infer.onnx",
            sha256: "48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b",
            file_name: "rec.onnx",
        },
    ],
    licence: "Apache-2.0",
    licence_url: "https://www.apache.org/licenses/LICENSE-2.0",
    creator: "the PaddleOCR authors (PaddlePaddle), ONNX conversion by the RapidOCR authors",
    source: "Hugging Face SWHL/RapidOCR",
    obligation: "Redistributing these files carries that licence's copy-of-licence and notice terms",
};

/// OCRcer's recogniser, from its own release; the hash is the
/// `model-sha256` recorded in `vendor/ocrcer-core/VENDORED`, so the download
/// is the model the vendored reader was synced with.
const OCRCER: FetchableModels = FetchableModels {
    name: "ocrcer",
    engine: "ocrcer",
    folder: "ocrcer",
    label: "OCRcer v0.1.0 (Latin script)",
    languages: "",
    version: "0.1.0",
    files: &[PinnedModelFile {
        url: "https://github.com/KenM76/ocrcer/releases/download/v0.1.0/ocrcer.ocrw",
        sha256: "1aed06b27ca842fb1fe922849ac5929adf278c95a38bf3eda251ccc0adb5cd04",
        file_name: "ocrcer.ocrw",
    }],
    licence: "MIT",
    licence_url: "https://github.com/KenM76/ocrcer/blob/main/LICENSE",
    creator: "Ken Mantle (the OCRcer project)",
    source: "the OCRcer v0.1.0 release",
    obligation: "Redistributing this file carries that licence's copyright-notice term",
};

/// Every downloadable model set, in the order a shell lists them. The first
/// set for an engine is the one [`fetchable_models`] answers.
pub const FETCHABLE_MODELS: &[FetchableModels] = &[OCRS, PADDLE_LATIN, PADDLE_CH_EN, OCRCER];

/// The first downloadable model set for `engine` (`ocr --ocr-engine`
/// spelling), or `None` when that engine has none.
#[must_use]
pub fn fetchable_models(engine: &str) -> Option<&'static FetchableModels> {
    FETCHABLE_MODELS.iter().find(|m| m.engine == engine)
}

/// The downloadable model set whose manifest name is `name`, or `None`.
#[must_use]
pub fn fetchable_model(name: &str) -> Option<&'static FetchableModels> {
    FETCHABLE_MODELS.iter().find(|m| m.name == name)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::ocr::addon_manifest::{parse_manifest, to_hex};

    #[test]
    fn every_pin_is_https_and_a_lowercase_sha256() {
        for set in FETCHABLE_MODELS {
            assert!(!set.files.is_empty(), "{}", set.engine);
            for f in set.files {
                assert!(f.url.starts_with("https://"), "{}", f.url);
                assert_eq!(f.sha256.len(), 64, "{}", f.file_name);
                assert!(
                    f.sha256
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
                    "{}",
                    f.sha256
                );
                assert!(!f.file_name.contains(['/', '\\']), "{}", f.file_name);
                if f.url.starts_with("https://huggingface.co/") && set.name != "ocrs" {
                    let rev = f.url.split("/resolve/").nth(1).expect("resolve URL");
                    assert_eq!(rev.find('/'), Some(40), "{} names a commit", f.url);
                }
            }
        }
    }

    #[test]
    fn names_are_unique_and_each_manifest_parses_back_to_its_set() {
        for (i, set) in FETCHABLE_MODELS.iter().enumerate() {
            assert!(
                FETCHABLE_MODELS.iter().take(i).all(|s| s.name != set.name),
                "{} twice",
                set.name
            );
            let m = parse_manifest(&set.manifest()).expect(set.name);
            assert_eq!((m.name.as_str(), m.engine.as_str()), (set.name, set.engine));
            assert_eq!(m.label.as_deref(), Some(set.label));
            assert_eq!(m.licence.as_deref(), Some(set.licence));
            assert!(m.unknown_keys.is_empty(), "{:?}", m.unknown_keys);
            let digests: Vec<_> = m
                .files
                .iter()
                .map(|d| (d.file.clone(), to_hex(&d.sha256)))
                .collect();
            let pins: Vec<_> = set
                .files
                .iter()
                .map(|f| (f.file_name.to_owned(), f.sha256.to_owned()))
                .collect();
            assert_eq!(digests, pins, "{}", set.name);
        }
    }

    #[cfg(feature = "ocrs")]
    #[test]
    fn the_ocrs_set_is_what_the_engine_loads() {
        use crate::ocr::engine_ocrs::{DETECTION_MODEL, MODEL_DIR, RECOGNITION_MODEL};
        let set = fetchable_models("ocrs").expect("ocrs is fetchable");
        assert_eq!(set.folder, MODEL_DIR);
        let names: Vec<_> = set.files.iter().map(|f| f.file_name).collect();
        assert_eq!(names, [DETECTION_MODEL, RECOGNITION_MODEL]);
    }

    #[cfg(feature = "paddle")]
    #[test]
    fn the_paddle_sets_are_what_the_engine_loads() {
        use crate::ocr::engine_paddle::{DETECTION_MODEL, INFERENCE_CONFIG, RECOGNITION_MODEL};
        let names = |n| -> Vec<_> {
            let set = fetchable_model(n).expect(n);
            assert_eq!(set.engine, "paddle");
            set.files.iter().map(|f| f.file_name).collect()
        };
        assert_eq!(
            names("ppocrv5-latin"),
            [DETECTION_MODEL, RECOGNITION_MODEL, INFERENCE_CONFIG]
        );
        assert_eq!(names("ppocrv4-ch-en"), [DETECTION_MODEL, RECOGNITION_MODEL]);
    }

    #[test]
    fn the_bundled_paddle_set_matches_its_download() {
        let bundled = include_str!("../../../assets/models/paddle/pdfcer-ocr-model.txt");
        let bundled = parse_manifest(bundled).expect("bundled manifest");
        let fetched = parse_manifest(&fetchable_model(&bundled.name).expect("pinned").manifest())
            .expect("fetched manifest");
        assert_eq!(bundled.files, fetched.files);
    }

    #[test]
    fn the_ocrcer_pin_is_the_vendored_release_model() {
        let vendored = include_str!("../../../../../vendor/ocrcer-core/VENDORED");
        let file = OCRCER
            .files
            .first()
            .expect("the ocrcer set names its model");
        let line = format!("model-sha256 = {}", file.sha256);
        assert!(vendored.lines().any(|l| l.trim() == line), "{line}");
    }

    #[test]
    fn an_unknown_engine_or_name_is_none() {
        assert!(fetchable_models("paddle-vl").is_none());
        assert!(fetchable_models("tesseract").is_none());
        assert!(fetchable_models("").is_none());
        assert!(fetchable_model("paddle").is_none());
        assert_eq!(
            fetchable_models("paddle").map(|s| s.name),
            Some("ppocrv5-latin")
        );
    }

    #[test]
    fn the_attribution_names_licence_creator_and_source() {
        let line = fetchable_models("ocrs").map(FetchableModels::attribution);
        assert_eq!(
            line.as_deref(),
            Some(
                "licence CC-BY-SA-4.0 <https://creativecommons.org/licenses/by-sa/4.0/>, \
                 creator Robert Knight, source the ocrs project. Redistributing these files \
                 carries that licence's attribution and share-alike terms"
            )
        );
    }
}
