//! The ICC profiles installed on this machine.
//!
//! Lists the operating system's profile folders so a shell can offer a
//! printer profile by name ([`crate::icc_output`]). Bounded: one subfolder
//! level, at most [`MAX_FILES`] files read, each under
//! [`crate::icc_output::MAX_PROFILE_BYTES`]; files that do not parse are
//! skipped, not reported as errors.

use std::path::{Path, PathBuf};

use crate::icc_output::{HeaderFacts, ProfileSpace, description, read_capped};

/// The most profile files one listing reads.
pub const MAX_FILES: usize = 2_000;

/// One installed profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledProfile {
    /// Where it is.
    pub path: PathBuf,
    /// Its description, or the file stem when it carries none.
    pub name: String,
    /// Its device colour space.
    pub color_space: ProfileSpace,
    /// Its device class signature (`prtr`, `mntr`, `scnr`, ...).
    pub class: [u8; 4],
}

impl InstalledProfile {
    /// RGB or grey, output or display class — usable by
    /// [`crate::icc_output::PrinterProfile`].
    #[must_use]
    pub fn is_print_destination(&self) -> bool {
        HeaderFacts {
            space: self.color_space,
            class: self.class,
        }
        .is_print_destination()
    }
}

/// The profile folders this OS uses that exist on this machine.
///
/// Windows `%SystemRoot%\System32\spool\drivers\color`; macOS the system,
/// local and user `ColorSync/Profiles`; Linux `/usr/share/color/icc`,
/// `/usr/local/share/color/icc`, `~/.local/share/color/icc`, `~/.color/icc`.
#[must_use]
pub fn profile_folders() -> Vec<PathBuf> {
    candidate_folders()
        .into_iter()
        .filter(|p| p.is_dir())
        .collect()
}

fn candidate_folders() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut out = Vec::new();
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot")
            .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        out.push(root.join(r"System32\spool\drivers\color"));
    } else if cfg!(target_os = "macos") {
        out.push(PathBuf::from("/System/Library/ColorSync/Profiles"));
        out.push(PathBuf::from("/Library/ColorSync/Profiles"));
        out.extend(home.map(|h| h.join("Library/ColorSync/Profiles")));
    } else {
        out.push(PathBuf::from("/usr/share/color/icc"));
        out.push(PathBuf::from("/usr/local/share/color/icc"));
        if let Some(h) = home {
            out.push(h.join(".local/share/color/icc"));
            out.push(h.join(".color/icc"));
        }
    }
    out
}

/// Every parseable profile in [`profile_folders`], sorted by name.
#[must_use]
pub fn installed_profiles() -> Vec<InstalledProfile> {
    installed_profiles_in(&profile_folders())
}

/// Every parseable profile in `folders` and their immediate subfolders,
/// sorted by name (case-insensitive), then path.
#[must_use]
pub fn installed_profiles_in(folders: &[PathBuf]) -> Vec<InstalledProfile> {
    let mut files = Vec::new();
    for folder in folders {
        collect(folder, true, &mut files);
    }
    files.truncate(MAX_FILES);
    let mut out: Vec<InstalledProfile> = files.iter().filter_map(|p| inspect(p)).collect();
    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.path.cmp(&b.path))
    });
    out
}

/// Only the profiles usable as a print destination.
#[must_use]
pub fn print_destinations(list: &[InstalledProfile]) -> Vec<InstalledProfile> {
    list.iter()
        .filter(|p| p.is_print_destination())
        .cloned()
        .collect()
}

/// The profile whose description matches `name`, else whose file name or
/// stem does; case-insensitive.
#[must_use]
pub fn find_installed<'a>(
    list: &'a [InstalledProfile],
    name: &str,
) -> Option<&'a InstalledProfile> {
    let wanted = name.to_lowercase();
    let eq = |s: Option<&str>| s.is_some_and(|s| s.to_lowercase() == wanted);
    list.iter()
        .find(|p| eq(Some(&p.name)))
        .or_else(|| {
            list.iter()
                .find(|p| eq(p.path.file_name().and_then(|s| s.to_str())))
        })
        .or_else(|| {
            list.iter()
                .find(|p| eq(p.path.file_stem().and_then(|s| s.to_str())))
        })
}

fn collect(dir: &Path, descend: bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        if out.len() >= MAX_FILES {
            return;
        }
        if path.is_dir() {
            if descend {
                collect(&path, false, out);
            }
        } else if is_profile_name(&path) {
            out.push(path);
        }
    }
}

fn is_profile_name(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("icc") || e.eq_ignore_ascii_case("icm"))
}

fn inspect(path: &Path) -> Option<InstalledProfile> {
    let bytes = read_capped(path).ok()?;
    let profile = icccer_profile::Profile::parse(&bytes).ok()?;
    let facts = HeaderFacts::of(&profile);
    let mut name = description(&profile);
    if name.is_empty() {
        name = path.file_stem()?.to_string_lossy().into_owned();
    }
    Some(InstalledProfile {
        path: path.to_path_buf(),
        name,
        color_space: facts.space,
        class: facts.class,
    })
}

#[cfg(test)]
mod tests;
