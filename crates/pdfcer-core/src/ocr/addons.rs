//! OCR model add-ons: discovery and verification (decision 182).
//!
//! An add-on is a folder holding one engine's data files plus a
//! [`MANIFEST_FILE`]. Installing is dropping the folder under a search root;
//! uninstalling is deleting it. There is no registry and no other state.
//!
//! The caller supplies the search roots in priority order; this module never
//! reads a settings file and never touches the network. It reads directory
//! listings and manifests; model bytes are read only by
//! [`OcrModel::verify`].
//!
//! A folder named after a built-in engine (`ocrs`, `ocrcer`, `paddle`,
//! `tesseract`) directly under a root, with no manifest, is a *bare* model of
//! that engine named after it: the layout every pdfcer before decision 182
//! used.

use std::collections::HashSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use sha2::Digest as _;

pub use super::addon_manifest::MANIFEST_FILE;
use super::addon_manifest::{FileDigest, ManifestError, OcrModelManifest, parse_manifest_bytes};

/// Folder names that are a bare model of the engine of the same name.
pub const BARE_ENGINE_FOLDERS: [&str; 4] = ["ocrs", "ocrcer", "paddle", "tesseract"];

/// Folder levels below a root searched for add-ons (`ARCHITECTURE.md` §10).
pub const MAX_ADDON_DEPTH: usize = 3;

/// Ceiling on folders visited across all roots.
pub const MAX_FOLDERS_VISITED: usize = 2_000;

/// Ceiling on models registered across all roots.
pub const MAX_MODELS: usize = 256;

/// One discovered model folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrModel {
    /// Unique id: the manifest's `name`, or the engine for a bare folder.
    pub name: String,
    /// The engine token.
    pub engine: String,
    /// The model folder.
    pub folder: PathBuf,
    /// The search root it was found under.
    pub root: PathBuf,
    /// The manifest; `None` for a bare engine folder.
    pub manifest: Option<OcrModelManifest>,
}

impl OcrModel {
    /// The manifest's label, if any.
    #[must_use]
    pub fn label(&self) -> Option<&str> {
        self.manifest.as_ref().and_then(|m| m.label.as_deref())
    }

    /// The manifest's language tags; empty when unstated.
    #[must_use]
    pub fn languages(&self) -> &[String] {
        self.manifest.as_ref().map_or(&[], |m| &m.languages)
    }

    /// The manifest's licence, if stated.
    #[must_use]
    pub fn licence(&self) -> Option<&str> {
        self.manifest.as_ref().and_then(|m| m.licence.as_deref())
    }

    /// Whether every file in `required` (relative to the folder) exists.
    #[must_use]
    pub fn has_files(&self, required: &[&str]) -> bool {
        required.iter().all(|f| self.folder.join(f).is_file())
    }

    /// Check every `sha256` line of the manifest against the file on disk.
    /// Returns how many files were checked (0 for a bare folder or a
    /// manifest without digests).
    ///
    /// # Errors
    ///
    /// [`VerifyError`] for the first file that is missing, unreadable or
    /// does not match.
    pub fn verify(&self) -> Result<usize, VerifyError> {
        let Some(manifest) = &self.manifest else {
            return Ok(0);
        };
        for digest in &manifest.files {
            verify_file(&self.folder, digest)?;
        }
        Ok(manifest.files.len())
    }
}

/// Why [`OcrModel::verify`] refused a model.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum VerifyError {
    /// A listed file could not be read.
    #[error("{}: {reason}", path.display())]
    Unreadable {
        /// The file.
        path: PathBuf,
        /// The I/O error, as text.
        reason: String,
    },
    /// A listed file's SHA-256 differs from the manifest's.
    #[error("{}: SHA-256 is {actual}, the manifest says {expected}", path.display())]
    Mismatch {
        /// The file.
        path: PathBuf,
        /// Manifest value, lower-case hex.
        expected: String,
        /// Computed value, lower-case hex.
        actual: String,
    },
}

fn verify_file(folder: &Path, digest: &FileDigest) -> Result<(), VerifyError> {
    let path = digest
        .file
        .split('/')
        .fold(folder.to_path_buf(), |p, c| p.join(c));
    let unreadable = |e: std::io::Error| VerifyError::Unreadable {
        path: path.clone(),
        reason: e.to_string(),
    };
    let mut file = std::fs::File::open(&path).map_err(unreadable)?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf).map_err(unreadable)?;
        if n == 0 {
            break;
        }
        hasher.update(buf.get(..n).unwrap_or_default());
    }
    let actual: [u8; 32] = hasher.finalize().into();
    if actual == digest.sha256 {
        return Ok(());
    }
    Err(VerifyError::Mismatch {
        path,
        expected: super::addon_manifest::to_hex(&digest.sha256),
        actual: super::addon_manifest::to_hex(&actual),
    })
}

/// Something discovery skipped or chose; each is a line a shell should show.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DiscoveryNote {
    /// A search root does not exist or is not a folder.
    #[error("OCR model folder {} does not exist", .0.display())]
    RootMissing(PathBuf),
    /// A folder's manifest did not parse; the folder was skipped.
    #[error("skipped {}: {MANIFEST_FILE}: {error}", folder.display())]
    BadManifest {
        /// The add-on folder.
        folder: PathBuf,
        /// Why.
        error: ManifestError,
    },
    /// A folder or manifest could not be read; it was skipped.
    #[error("skipped {}: {reason}", path.display())]
    Unreadable {
        /// The path.
        path: PathBuf,
        /// The I/O error, as text.
        reason: String,
    },
    /// A second model with an already-seen name; the first one wins.
    #[error("OCR model `{name}` in {} is shadowed by the one in {}", ignored.display(), kept.display())]
    Shadowed {
        /// The shared name.
        name: String,
        /// The folder in use.
        kept: PathBuf,
        /// The folder ignored.
        ignored: PathBuf,
    },
    /// A manifest carries keys this build does not know; they were ignored.
    #[error("{}: ignored unknown manifest key(s): {}", folder.display(), keys.join(", "))]
    UnknownKeys {
        /// The add-on folder.
        folder: PathBuf,
        /// The keys.
        keys: Vec<String>,
    },
    /// A ceiling stopped the walk; later folders were not searched.
    #[error("stopped searching for OCR models at {}: {limit}", at.display())]
    Ceiling {
        /// Where it stopped.
        at: PathBuf,
        /// Which ceiling.
        limit: String,
    },
}

/// What [`discover_ocr_models`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OcrModelDiscovery {
    /// Models in priority order: root order, then sorted folder order.
    pub models: Vec<OcrModel>,
    /// Everything skipped or shadowed, in walk order.
    pub notes: Vec<DiscoveryNote>,
}

/// The result of [`OcrModelDiscovery::for_engine`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineMatch<'a> {
    /// The first model of the engine holding every required file.
    pub chosen: Option<&'a OcrModel>,
    /// Earlier models of the engine passed over for lacking a required file.
    pub incomplete: Vec<&'a OcrModel>,
}

impl OcrModelDiscovery {
    /// The model named `name`.
    #[must_use]
    pub fn by_name(&self, name: &str) -> Option<&OcrModel> {
        self.models.iter().find(|m| m.name == name)
    }

    /// The first model for `engine` that holds every file in `required`,
    /// plus the ones passed over on the way (a shell discloses those).
    #[must_use]
    pub fn for_engine(&self, engine: &str, required: &[&str]) -> EngineMatch<'_> {
        let mut out = EngineMatch::default();
        for m in self.models.iter().filter(|m| m.engine == engine) {
            if m.has_files(required) {
                out.chosen = Some(m);
                break;
            }
            out.incomplete.push(m);
        }
        out
    }
}

/// Find every OCR model under `roots`, earlier roots first.
///
/// Each root is either itself an add-on folder (it holds a manifest) or a
/// folder whose subfolders, up to [`MAX_ADDON_DEPTH`] levels down, are
/// searched. A folder with a manifest is a model and is not descended into;
/// a bare engine folder directly under a root is a model; anything else is
/// descended into. Symlink cycles are cut by canonical path. On a duplicate
/// name the first model found wins and a [`DiscoveryNote::Shadowed`] is
/// recorded.
#[must_use]
pub fn discover_ocr_models(roots: &[PathBuf]) -> OcrModelDiscovery {
    let mut walk = Walk::default();
    for root in roots {
        if walk.stopped {
            break;
        }
        if !root.is_dir() {
            walk.out
                .notes
                .push(DiscoveryNote::RootMissing(root.clone()));
            continue;
        }
        if !walk.enter(root) {
            continue;
        }
        if root.join(MANIFEST_FILE).is_file() {
            walk.manifest_folder(root, root);
        } else {
            walk.children(root, root, 1);
        }
    }
    walk.out
}

#[derive(Default)]
struct Walk {
    out: OcrModelDiscovery,
    seen_dirs: HashSet<PathBuf>,
    visited: usize,
    stopped: bool,
}

impl Walk {
    /// Record `dir` as visited; false on a cycle, a revisit or a ceiling.
    fn enter(&mut self, dir: &Path) -> bool {
        if self.stopped {
            return false;
        }
        if self.visited >= MAX_FOLDERS_VISITED {
            self.stop(dir, format!("{MAX_FOLDERS_VISITED} folders visited"));
            return false;
        }
        self.visited += 1;
        let key = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        self.seen_dirs.insert(key)
    }

    fn stop(&mut self, at: &Path, limit: String) {
        self.stopped = true;
        self.out.notes.push(DiscoveryNote::Ceiling {
            at: at.to_path_buf(),
            limit,
        });
    }

    fn children(&mut self, root: &Path, dir: &Path, depth: usize) {
        let entries = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) => {
                self.out.notes.push(DiscoveryNote::Unreadable {
                    path: dir.to_path_buf(),
                    reason: e.to_string(),
                });
                return;
            }
        };
        let mut subdirs: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subdirs.sort();
        for sub in subdirs {
            if !self.enter(&sub) {
                continue;
            }
            if sub.join(MANIFEST_FILE).is_file() {
                self.manifest_folder(root, &sub);
            } else if let Some(engine) = bare_engine(&sub).filter(|_| depth == 1) {
                self.register(root, &sub, engine.to_owned(), engine.to_owned(), None);
            } else if depth < MAX_ADDON_DEPTH {
                self.children(root, &sub, depth + 1);
            }
        }
    }

    fn manifest_folder(&mut self, root: &Path, folder: &Path) {
        let path = folder.join(MANIFEST_FILE);
        let read = std::fs::File::open(&path).and_then(|f| {
            let mut bytes = Vec::new();
            let cap = super::addon_manifest::MAX_MANIFEST_BYTES as u64 + 1;
            f.take(cap).read_to_end(&mut bytes).map(|_| bytes)
        });
        let bytes = match read {
            Ok(b) => b,
            Err(e) => {
                let reason = e.to_string();
                self.out
                    .notes
                    .push(DiscoveryNote::Unreadable { path, reason });
                return;
            }
        };
        match parse_manifest_bytes(&bytes) {
            Ok(m) => {
                if !m.unknown_keys.is_empty() {
                    self.out.notes.push(DiscoveryNote::UnknownKeys {
                        folder: folder.to_path_buf(),
                        keys: m.unknown_keys.clone(),
                    });
                }
                self.register(root, folder, m.name.clone(), m.engine.clone(), Some(m));
            }
            Err(error) => self.out.notes.push(DiscoveryNote::BadManifest {
                folder: folder.to_path_buf(),
                error,
            }),
        }
    }

    fn register(
        &mut self,
        root: &Path,
        folder: &Path,
        name: String,
        engine: String,
        manifest: Option<OcrModelManifest>,
    ) {
        if let Some(kept) = self.out.by_name(&name) {
            let note = DiscoveryNote::Shadowed {
                kept: kept.folder.clone(),
                ignored: folder.to_path_buf(),
                name,
            };
            self.out.notes.push(note);
            return;
        }
        if self.out.models.len() >= MAX_MODELS {
            self.stop(folder, format!("{MAX_MODELS} models found"));
            return;
        }
        self.out.models.push(OcrModel {
            name,
            engine,
            folder: folder.to_path_buf(),
            root: root.to_path_buf(),
            manifest,
        });
    }
}

fn bare_engine(dir: &Path) -> Option<&'static str> {
    let name = dir.file_name()?.to_str()?;
    BARE_ENGINE_FOLDERS.into_iter().find(|e| *e == name)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("pdfcer-ocr-addons-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn addon(dir: &Path, manifest: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
    }

    #[test]
    fn bare_engine_folders_and_manifest_addons_are_found_in_root_order() {
        let a = scratch("order-a");
        let b = scratch("order-b");
        std::fs::create_dir_all(a.join("paddle")).unwrap();
        std::fs::create_dir_all(a.join("notes")).unwrap();
        addon(
            &a.join("vendor").join("ja"),
            "name = pp-ja\nengine = paddle\n",
        );
        addon(&b.join("x"), "name = big\nengine = ocrs\nlicence = MIT\n");
        let found = discover_ocr_models(&[a.clone(), b.clone()]);
        let names: Vec<&str> = found.models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["paddle", "pp-ja", "big"]);
        assert!(found.models[0].manifest.is_none());
        assert_eq!(found.models[1].root, a);
        assert_eq!(found.models[2].licence(), Some("MIT"));
        assert!(found.notes.is_empty(), "{:?}", found.notes);
    }

    #[test]
    fn a_bare_engine_name_counts_only_directly_under_a_root() {
        let r = scratch("bare-depth");
        std::fs::create_dir_all(r.join("group").join("paddle")).unwrap();
        assert!(discover_ocr_models(&[r]).models.is_empty());
    }

    #[test]
    fn a_root_that_is_itself_an_addon_is_one_model() {
        let r = scratch("self");
        addon(&r, "name = solo\nengine = tesseract\n");
        addon(&r.join("inner"), "name = inner\nengine = ocrs\n");
        let found = discover_ocr_models(std::slice::from_ref(&r));
        assert_eq!(found.models.len(), 1);
        assert_eq!(found.models[0].folder, r);
    }

    #[test]
    fn the_first_name_wins_and_the_shadow_is_reported() {
        let a = scratch("shadow-a");
        let b = scratch("shadow-b");
        std::fs::create_dir_all(a.join("ocrs")).unwrap();
        addon(&b.join("mine"), "name = ocrs\nengine = ocrs\n");
        let found = discover_ocr_models(&[a.clone(), b.clone()]);
        assert_eq!(found.models.len(), 1);
        assert_eq!(found.models[0].folder, a.join("ocrs"));
        assert_eq!(
            found.notes,
            [DiscoveryNote::Shadowed {
                name: "ocrs".into(),
                kept: a.join("ocrs"),
                ignored: b.join("mine"),
            }]
        );
    }

    #[test]
    fn a_bad_manifest_skips_only_its_folder_and_names_it() {
        let r = scratch("bad");
        addon(&r.join("broken"), "name = x\n");
        addon(&r.join("good"), "name = g\nengine = paddle\nfuture = 1\n");
        let missing = r.join("nope");
        let found = discover_ocr_models(&[r.clone(), missing.clone()]);
        assert_eq!(found.models.len(), 1);
        let text: Vec<String> = found.notes.iter().map(ToString::to_string).collect();
        assert!(
            text[0].contains("broken") && text[0].contains("no `engine` line"),
            "{text:?}"
        );
        assert!(text[1].contains("future"), "{text:?}");
        assert_eq!(found.notes[2], DiscoveryNote::RootMissing(missing));
    }

    #[test]
    fn depth_is_bounded() {
        let r = scratch("deep");
        addon(
            &r.join("a").join("b").join("c"),
            "name = ok\nengine = ocrs\n",
        );
        addon(
            &r.join("a").join("b").join("c2").join("d"),
            "name = deep\nengine = ocrs\n",
        );
        let names: Vec<String> = discover_ocr_models(&[r])
            .models
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert_eq!(names, ["ok"]);
    }

    #[test]
    fn a_root_listed_twice_is_walked_once() {
        let r = scratch("twice");
        addon(&r.join("m"), "name = m\nengine = ocrs\n");
        let found = discover_ocr_models(&[r.clone(), r.join(".").join("")]);
        assert_eq!(found.models.len(), 1);
        assert!(found.notes.is_empty(), "{:?}", found.notes);
    }

    #[test]
    fn for_engine_skips_and_reports_incomplete_folders() {
        let a = scratch("engine-a");
        let b = scratch("engine-b");
        std::fs::create_dir_all(a.join("paddle")).unwrap();
        addon(&b.join("full"), "name = full\nengine = paddle\n");
        std::fs::write(b.join("full").join("det.onnx"), b"x").unwrap();
        let found = discover_ocr_models(&[a.clone(), b.clone()]);
        let m = found.for_engine("paddle", &["det.onnx"]);
        assert_eq!(m.chosen.map(|m| m.name.as_str()), Some("full"));
        assert_eq!(m.incomplete.len(), 1);
        assert_eq!(m.incomplete[0].folder, a.join("paddle"));
        assert!(found.for_engine("ocrs", &[]).chosen.is_none());
        assert_eq!(
            found.by_name("paddle").map(|m| m.engine.as_str()),
            Some("paddle")
        );
    }

    #[test]
    fn verify_checks_every_listed_file() {
        let r = scratch("verify");
        let dir = r.join("m");
        // SHA-256("abc").
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        addon(
            &dir,
            &format!("name = m\nengine = ocrs\nsha256 = sub/f.bin {abc}\n"),
        );
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("f.bin"), b"abc").unwrap();
        let found = discover_ocr_models(std::slice::from_ref(&r));
        assert_eq!(found.models[0].verify().unwrap(), 1);

        std::fs::write(dir.join("sub").join("f.bin"), b"abd").unwrap();
        let err = found.models[0].verify().unwrap_err().to_string();
        assert!(
            err.contains("the manifest says") && err.contains(abc),
            "{err}"
        );

        std::fs::remove_file(dir.join("sub").join("f.bin")).unwrap();
        assert!(matches!(
            found.models[0].verify(),
            Err(VerifyError::Unreadable { .. })
        ));
    }
}
