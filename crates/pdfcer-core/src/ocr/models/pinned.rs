//! The OCR model files a shell may download on the operator's request: URL,
//! SHA-256 and local name per file, plus the licence a fetched copy carries.
//!
//! Plain data with no network code, so every shell fetches the same measured
//! bytes from one list (`pdfcer-fetch` does the fetching). A file is pinned by
//! hash because the same model's copies on different hosts differ; see
//! `crates/pdfcer-core/assets/models/ocrs/PROVENANCE.md`.

use super::EngineDirName;

/// One downloadable model file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct PinnedModelFile {
    /// The exact HTTPS URL to fetch.
    pub url: &'static str,
    /// Lowercase hex SHA-256 the downloaded bytes must equal.
    pub sha256: &'static str,
    /// The name to write it under, inside [`FetchableModels::folder`].
    pub file_name: &'static str,
}

/// One engine's downloadable model set and the terms it comes under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct FetchableModels {
    /// The engine name, as `ocr --ocr-engine` takes it.
    pub engine: &'static str,
    /// The sub-folder of `models/` the files go in, which is where the
    /// engine's model resolution looks ([`super::resolve_model_dir`]).
    pub folder: EngineDirName,
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
    /// creator, source and obligation in one line. CC-BY-SA requires it, and
    /// a fetched copy has no `PROVENANCE.md` beside it to carry it.
    #[must_use]
    pub fn attribution(&self) -> String {
        format!(
            "licence {} <{}>, creator {}, source {}. {}",
            self.licence, self.licence_url, self.creator, self.source, self.obligation
        )
    }
}

/// The `ocrs` engine's weights. The detection file comes from the author's S3
/// bucket because the Hugging Face copy does not work with `ocrs` 0.12.2; the
/// recognition file is fine on Hugging Face. Do not move them onto one host.
const OCRS: FetchableModels = FetchableModels {
    engine: "ocrs",
    folder: "ocrs",
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

/// Every engine whose models can be downloaded. Engines whose models ship in
/// the portable folder or as add-on zips are not listed.
pub const FETCHABLE_MODELS: &[FetchableModels] = &[OCRS];

/// The downloadable model set for `engine` (`ocr --ocr-engine` spelling), or
/// `None` when that engine's models are not fetchable.
#[must_use]
pub fn fetchable_models(engine: &str) -> Option<&'static FetchableModels> {
    FETCHABLE_MODELS.iter().find(|m| m.engine == engine)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

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
            }
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

    #[test]
    fn an_engine_without_a_download_is_none() {
        assert!(fetchable_models("paddle").is_none());
        assert!(fetchable_models("").is_none());
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
