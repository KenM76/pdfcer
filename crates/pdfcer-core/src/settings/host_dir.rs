//! A settings directory the host application supplies, for platforms where
//! pdfcer cannot find a writable folder by itself (Android: the executable
//! is the read-only `app_process64` and no config environment is set).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::StoreLocation;

/// The directory [`set_store_dir`] supplied.
static HOST_DIR: OnceLock<PathBuf> = OnceLock::new();
/// [`super::resolve_store`]'s one answer per process.
pub(super) static RESOLVED: OnceLock<StoreLocation> = OnceLock::new();

/// Why [`set_store_dir`] refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StoreDirError {
    /// The store was already resolved; every caller in a process gets one
    /// answer, so a later directory cannot take effect.
    #[error(
        "the settings store was already resolved to {resolved:?}; set the directory before anything loads settings"
    )]
    AlreadyResolved {
        /// Where it resolved.
        resolved: Option<PathBuf>,
    },
    /// A different directory was supplied earlier.
    #[error("a settings directory was already supplied: {0}")]
    AlreadySet(PathBuf),
}

/// Supplies the directory settings (and anything else joined onto
/// [`StoreLocation::directory`], such as a recent-files list) live in.
///
/// Call it once, at start-up, before the first [`super::resolve_store`] or
/// [`super::Settings::load`]. When the directory is writable (it is created
/// if missing), [`super::resolve_store`] answers it with
/// [`super::StoreKind::HostSupplied`] ahead of the portable and platform
/// locations; when it is not, resolution falls through to those as before.
/// Supplying the same directory again is `Ok`.
///
/// # Errors
///
/// [`StoreDirError::AlreadyResolved`] if the store was resolved first;
/// [`StoreDirError::AlreadySet`] if a different directory was supplied.
///
/// ```
/// let dir = std::env::temp_dir().join("pdfcer-doc-host-dir");
/// pdfcer_core::settings::set_store_dir(&dir)?;
/// let store = pdfcer_core::settings::resolve_store();
/// assert_eq!(store.directory(), Some(dir.as_path()));
/// # Ok::<(), pdfcer_core::settings::StoreDirError>(())
/// ```
pub fn set_store_dir(dir: &Path) -> Result<(), StoreDirError> {
    if let Some(resolved) = RESOLVED.get() {
        return Err(StoreDirError::AlreadyResolved {
            resolved: resolved.directory().map(Path::to_path_buf),
        });
    }
    let set = HOST_DIR.get_or_init(|| dir.to_path_buf());
    if set == dir {
        Ok(())
    } else {
        Err(StoreDirError::AlreadySet(set.clone()))
    }
}

/// The supplied directory, if any.
pub(super) fn host_dir() -> Option<&'static Path> {
    HOST_DIR.get().map(PathBuf::as_path)
}
