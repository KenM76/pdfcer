//! The portable settings file and opt-in OS font-folder discovery (decision
//! 176).
//!
//! Contract: with no settings file and no `--settings`, nothing here changes
//! any output — [`font_dirs`] is empty and [`workarounds`] is
//! [`Workarounds::Offer`]. An active file is always named on stderr, once
//! per invocation, so a forgotten file cannot silently change batch output.
//! The walk reads only directory listings; font bytes are read by
//! [`build_font_environment`] and handed to `pdfcer-render`/`pdfcer-core`,
//! which stay filesystem-free.

use super::*;
use std::collections::HashSet;
use std::sync::OnceLock;

/// The file looked for beside the executable.
pub(crate) const SETTINGS_FILE_NAME: &str = "pdfcer-settings.txt";

/// Largest settings file read; a bigger one is refused, not truncated.
pub(crate) const MAX_SETTINGS_BYTES: u64 = 64 * 1024;

/// Folder levels searched below each font folder (`ARCHITECTURE.md` §10).
/// Linux's `/usr/share/fonts/truetype/<family>` is two.
pub(crate) const MAX_FOLDER_DEPTH: usize = 8;

/// Default ceiling on font files the walk will register (`font_file_limit`).
pub(crate) const DEFAULT_MAX_FONT_FILES: usize = 10_000;

/// Highest `font_file_limit` accepted.
pub(crate) const MAX_FONT_FILE_LIMIT: usize = 1_000_000;

/// Ceiling on folders visited, so a font folder set to a drive root stops.
pub(crate) const MAX_FOLDERS_VISITED: usize = 20_000;

/// The `workarounds` setting, read by `edit-text` when a text edit is
/// refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Workarounds {
    /// Refuse, naming the workaround on offer (the default).
    #[default]
    Offer,
    /// Apply the workaround without being asked, disclosing it.
    Always,
}

impl Workarounds {
    /// The spelling the settings file and the stderr line use.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Offer => "offer",
            Self::Always => "always",
        }
    }
}

/// One parsed settings file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Settings {
    /// The file these came from; `None` is the built-in default.
    pub(crate) source: Option<PathBuf>,
    /// `workarounds = always|offer`.
    pub(crate) workarounds: Workarounds,
    /// `system_fonts = on|off`: search the OS font folders.
    pub(crate) system_fonts: bool,
    /// `font_folder = PATH` lines, in file order, resolved against the
    /// settings file's folder.
    pub(crate) font_folders: Vec<PathBuf>,
    /// `font_file_limit = N`.
    pub(crate) max_font_files: usize,
    /// `ocr_folder = PATH` lines, in file order, resolved like `font_folder`:
    /// OCR model add-on roots searched after `models/` beside the executable
    /// (decision 182).
    pub(crate) ocr_folders: Vec<PathBuf>,
    /// `ocr_program_addons = allow|refuse`: whether OCR add-ons that carry
    /// an engine program may run (decision 184).
    pub(crate) ocr_program_addons: pdfcer_ocr_host::ProgramPolicy,
}

impl Settings {
    /// The settings in force with no file: nothing enabled.
    pub(crate) const DEFAULT: Self = Self {
        source: None,
        workarounds: Workarounds::Offer,
        system_fonts: false,
        font_folders: Vec::new(),
        max_font_files: DEFAULT_MAX_FONT_FILES,
        ocr_folders: Vec::new(),
        ocr_program_addons: pdfcer_ocr_host::ProgramPolicy::Allow,
    };

    /// Whether any font folder will be searched.
    pub(crate) fn searches_fonts(&self) -> bool {
        self.system_fonts || !self.font_folders.is_empty()
    }
}

static ACTIVE: OnceLock<Settings> = OnceLock::new();
static NO_FILE: Settings = Settings::DEFAULT;
static FONT_DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();

/// The settings this process runs with; [`Settings::DEFAULT`] before
/// [`install`] or without a file.
pub(crate) fn active() -> &'static Settings {
    ACTIVE.get().unwrap_or(&NO_FILE)
}

/// The `workarounds` value the edit-text path reads.
pub(crate) fn workarounds() -> Workarounds {
    active().workarounds
}

/// Locate, read and parse the settings file, store it for the process, and
/// print the one stderr line naming it.
///
/// Precedence: `--no-settings` reads nothing; `--settings PATH` reads PATH,
/// which must exist; otherwise [`SETTINGS_FILE_NAME`] beside the executable
/// is read if present.
///
/// # Errors
///
/// The exit code after printing why: `IO_ERROR` for a missing or unreadable
/// file, `RUNTIME_ERROR` for one that does not parse.
pub(crate) fn install(explicit: Option<&Path>, disabled: bool) -> Result<(), u8> {
    let path = match (disabled, explicit) {
        (true, _) => return Ok(()),
        (false, Some(p)) => p.to_path_buf(),
        (false, None) => match beside_executable() {
            Some(p) => p,
            None => return Ok(()),
        },
    };
    let settings = load(&path)?;
    eprintln!("pdfcer: {}", summary_line(&settings));
    let _ = ACTIVE.set(settings);
    Ok(())
}

/// `<executable's folder>/pdfcer-settings.txt`, when that is a file.
fn beside_executable() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path = exe.parent()?.join(SETTINGS_FILE_NAME);
    path.is_file().then_some(path)
}

fn load(path: &Path) -> Result<Settings, u8> {
    let io = |e: std::io::Error| {
        eprintln!(
            "pdfcer: settings file {}: {e} (pass --no-settings to run without it)",
            path.display()
        );
        exit::IO_ERROR
    };
    let len = std::fs::metadata(path).map_err(io)?.len();
    if len > MAX_SETTINGS_BYTES {
        eprintln!(
            "pdfcer: settings file {}: {len} bytes is over the {MAX_SETTINGS_BYTES}-byte limit",
            path.display()
        );
        return Err(exit::IO_ERROR);
    }
    let bytes = std::fs::read(path).map_err(io)?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let parsed = std::str::from_utf8(&bytes)
        .map_err(|_| "the file is not UTF-8 text".to_owned())
        .and_then(|text| parse(text, base));
    let mut settings = parsed.map_err(|msg| {
        eprintln!(
            "pdfcer: settings file {}: {msg} (pass --no-settings to run without it)",
            path.display()
        );
        exit::RUNTIME_ERROR
    })?;
    settings.source = Some(path.to_path_buf());
    Ok(settings)
}

/// Parse settings text. Relative `font_folder`/`ocr_folder` paths resolve
/// against `base`.
///
/// Format: one `key = value` per line; blank lines and lines starting `#`
/// or `;` are ignored; a value may be wrapped in double quotes. Keys:
/// `workarounds` (`always`|`offer`), `system_fonts` (`on`|`off`),
/// `font_folder` (a path; repeatable), `font_file_limit` (1 to
/// [`MAX_FONT_FILE_LIMIT`]), `ocr_folder` (a path; repeatable),
/// `ocr_program_addons` (`allow`|`refuse`). An unknown key, a repeated
/// single-valued key or a bad value is an error naming the line: a typo must not be ignored.
///
/// # Errors
///
/// The message to print, naming the line.
pub(crate) fn parse(text: &str, base: &Path) -> Result<Settings, String> {
    let mut out = Settings::DEFAULT;
    let mut seen: HashSet<&str> = HashSet::new();
    for (index, raw) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let at = |msg: String| format!("line {}: {msg}", index + 1);
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| at(format!("{line:?} is not `key = value`")))?;
        let key = key.trim();
        let value = unquote(value.trim());
        if value.is_empty() {
            return Err(at(format!("{key} has no value")));
        }
        let repeatable = matches!(key, "font_folder" | "ocr_folder");
        if !repeatable && !seen.insert(key) {
            return Err(at(format!("{key} is set twice")));
        }
        apply(&mut out, key, value, base).map_err(at)?;
    }
    Ok(out)
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
}

fn apply(out: &mut Settings, key: &str, value: &str, base: &Path) -> Result<(), String> {
    match key {
        "workarounds" => {
            out.workarounds = match value {
                "always" => Workarounds::Always,
                "offer" => Workarounds::Offer,
                _ => return Err(format!("workarounds is {value:?}; use always or offer")),
            };
        }
        "system_fonts" => {
            out.system_fonts = match value {
                "on" => true,
                "off" => false,
                _ => return Err(format!("system_fonts is {value:?}; use on or off")),
            };
        }
        "font_folder" => out.font_folders.push(resolve_folder(value, base)),
        "ocr_folder" => out.ocr_folders.push(resolve_folder(value, base)),
        "ocr_program_addons" => {
            out.ocr_program_addons = match value {
                "allow" => pdfcer_ocr_host::ProgramPolicy::Allow,
                "refuse" => pdfcer_ocr_host::ProgramPolicy::Refuse,
                _ => {
                    return Err(format!(
                        "ocr_program_addons is {value:?}; use allow or refuse"
                    ));
                }
            };
        }
        "font_file_limit" => {
            out.max_font_files = value
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=MAX_FONT_FILE_LIMIT).contains(n))
                .ok_or_else(|| {
                    format!("font_file_limit is {value:?}; use a whole number from 1 to {MAX_FONT_FILE_LIMIT}")
                })?;
        }
        _ => {
            return Err(format!(
                "unknown key {key:?}; the keys are workarounds, system_fonts, font_folder, font_file_limit, ocr_folder and ocr_program_addons"
            ));
        }
    }
    Ok(())
}

/// `~/x` against the home folder, a relative path against `base`.
fn resolve_folder(value: &str, base: &Path) -> PathBuf {
    let home_rest = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"));
    if let (Some(rest), Some(home)) = (home_rest, home_dir()) {
        return home.join(rest);
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// The one stderr line naming an active file and what it enabled.
pub(crate) fn summary_line(s: &Settings) -> String {
    let source = s
        .source
        .as_ref()
        .map_or_else(|| "(built-in)".to_owned(), |p| p.display().to_string());
    format!(
        "settings: using {source}: workarounds={} system_fonts={} font_folders={} \
         font_file_limit={} ocr_folders={} ocr_program_addons={} \
         (pass --no-settings to ignore it)",
        s.workarounds.as_str(),
        if s.system_fonts { "on" } else { "off" },
        s.font_folders.len(),
        s.max_font_files,
        s.ocr_folders.len(),
        s.ocr_program_addons.as_str(),
    )
}

/// The OS font folders `system_fonts = on` searches, in this order.
#[cfg(windows)]
pub(crate) fn system_font_folders() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(win) = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot")) {
        out.push(PathBuf::from(win).join("Fonts"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        out.push(PathBuf::from(local).join(r"Microsoft\Windows\Fonts"));
    }
    out
}

/// The OS font folders `system_fonts = on` searches, in this order.
#[cfg(target_os = "macos")]
pub(crate) fn system_font_folders() -> Vec<PathBuf> {
    let mut out = vec![
        PathBuf::from("/System/Library/Fonts"),
        PathBuf::from("/Library/Fonts"),
    ];
    if let Some(home) = home_dir() {
        out.push(home.join("Library/Fonts"));
    }
    out
}

/// The OS font folders `system_fonts = on` searches, in this order.
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn system_font_folders() -> Vec<PathBuf> {
    let mut out = vec![
        PathBuf::from("/usr/share/fonts"),
        PathBuf::from("/usr/local/share/fonts"),
    ];
    if let Some(home) = home_dir() {
        out.push(home.join(".local/share/fonts"));
        out.push(home.join(".fonts"));
    }
    out
}

/// No OS font folders are known for this target.
#[cfg(not(any(windows, unix)))]
pub(crate) fn system_font_folders() -> Vec<PathBuf> {
    Vec::new()
}

/// The folders the settings file adds to every `--font-dir` consumer, walked
/// once per process on first use, in search order: OS folders, then
/// `font_folder` lines. Empty without an active file that enables fonts.
pub(crate) fn font_dirs() -> &'static [PathBuf] {
    FONT_DIRS.get_or_init(|| {
        let s = active();
        if !s.searches_fonts() {
            return Vec::new();
        }
        let mut roots: Vec<(PathBuf, bool)> = Vec::new();
        if s.system_fonts {
            roots.extend(system_font_folders().into_iter().map(|p| (p, false)));
        }
        roots.extend(s.font_folders.iter().map(|p| (p.clone(), true)));
        let walk = walk_font_folders(&roots, s.max_font_files);
        for note in &walk.notes {
            eprintln!("pdfcer: settings: {note}");
        }
        eprintln!(
            "pdfcer: settings: font search found {} font files in {} folders",
            walk.font_files,
            walk.dirs.len()
        );
        walk.dirs
    })
}

/// What a font-folder walk found.
#[derive(Debug, Default)]
pub(crate) struct FontFolderWalk {
    /// Every folder holding at least one font-extension file, in search
    /// order; each is read flat by [`build_font_environment`].
    pub(crate) dirs: Vec<PathBuf>,
    /// Font-extension files in `dirs`.
    pub(crate) font_files: usize,
    /// Skips and stops, for stderr.
    pub(crate) notes: Vec<String>,
}

/// Walk `roots` (`(folder, named)`; a `named` folder that is missing is
/// noted, an absent OS default is not), recursing at most
/// [`MAX_FOLDER_DEPTH`] levels, visiting at most [`MAX_FOLDERS_VISITED`]
/// folders and counting at most `max_font_files` font files. A folder that
/// would pass a ceiling is not searched, nor is any after it, and a note
/// says so; folders already found are kept. Each real folder is visited
/// once, which also ends a symlink cycle.
pub(crate) fn walk_font_folders(
    roots: &[(PathBuf, bool)],
    max_font_files: usize,
) -> FontFolderWalk {
    let mut walker = Walker {
        out: FontFolderWalk::default(),
        seen: HashSet::new(),
        visited: 0,
        max_font_files,
        stopped: false,
    };
    for (root, named) in roots {
        if walker.stopped {
            break;
        }
        if !root.is_dir() {
            if *named {
                walker.out.notes.push(format!(
                    "font_folder {} is not a folder; skipped",
                    root.display()
                ));
            }
            continue;
        }
        walker.visit(root, 0);
    }
    walker.out
}

struct Walker {
    out: FontFolderWalk,
    seen: HashSet<PathBuf>,
    visited: usize,
    max_font_files: usize,
    stopped: bool,
}

impl Walker {
    fn visit(&mut self, dir: &Path, depth: usize) {
        let key = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        if self.stopped || !self.seen.insert(key) {
            return;
        }
        if self.visited >= MAX_FOLDERS_VISITED {
            return self.stop(format!(
                "stopped before {}: the walk reached its limit of {MAX_FOLDERS_VISITED} \
                 folders; it and every folder after it were not searched",
                dir.display()
            ));
        }
        self.visited += 1;
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(err) => return self.out.notes.push(format!("{}: {err}", dir.display())),
        };
        let (mut files, mut subdirs) = (0usize, Vec::new());
        for path in entries.flatten().map(|e| e.path()) {
            if path.is_dir() {
                subdirs.push(path);
            } else if has_font_extension(&path) {
                files += 1;
            }
        }
        if self.out.font_files + files > self.max_font_files {
            return self.stop(format!(
                "stopped at {}: its {files} font files would pass the limit of {} \
                 (font_file_limit); it and every folder after it were not searched",
                dir.display(),
                self.max_font_files
            ));
        }
        self.out.font_files += files;
        if files > 0 {
            self.out.dirs.push(dir.to_path_buf());
        }
        subdirs.sort();
        for sub in subdirs {
            if depth + 1 > MAX_FOLDER_DEPTH {
                self.out.notes.push(format!(
                    "skipped {}: more than {MAX_FOLDER_DEPTH} folder levels deep",
                    sub.display()
                ));
                continue;
            }
            self.visit(&sub, depth + 1);
        }
    }

    fn stop(&mut self, note: String) {
        self.stopped = true;
        self.out.notes.push(note);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            r"C:\portable"
        } else {
            "/portable"
        })
    }

    #[test]
    fn every_key_parses_and_relative_folders_resolve_beside_the_file() {
        let s = parse(
            "\u{feff}# comment\n; also a comment\n\nworkarounds = always\n\
             system_fonts=on\nfont_folder = fonts\nfont_folder = \"more fonts\"\n\
             font_file_limit = 50\n",
            &base(),
        )
        .unwrap();
        assert_eq!(s.workarounds, Workarounds::Always);
        assert!(s.system_fonts);
        assert_eq!(
            s.font_folders,
            vec![base().join("fonts"), base().join("more fonts")]
        );
        assert_eq!(s.max_font_files, 50);
    }

    #[test]
    fn ocr_folder_repeats_and_resolves_beside_the_file() {
        let s = parse("ocr_folder = ocr\nocr_folder = \"more ocr\"\n", &base()).unwrap();
        assert_eq!(
            s.ocr_folders,
            vec![base().join("ocr"), base().join("more ocr")]
        );
        assert!(s.font_folders.is_empty());
        assert!(summary_line(&s).contains("ocr_folders=2"));
    }

    #[test]
    fn ocr_program_addons_defaults_to_allow_and_parses_refuse() {
        use pdfcer_ocr_host::ProgramPolicy;
        assert_eq!(Settings::DEFAULT.ocr_program_addons, ProgramPolicy::Allow);
        let s = parse("ocr_program_addons = refuse\n", &base()).unwrap();
        assert_eq!(s.ocr_program_addons, ProgramPolicy::Refuse);
        assert!(summary_line(&s).contains("ocr_program_addons=refuse"));
        let err = parse("ocr_program_addons = maybe", &base()).unwrap_err();
        assert!(err.contains("use allow or refuse"), "{err}");
    }

    #[test]
    fn an_empty_file_is_the_default() {
        let s = parse("", &base()).unwrap();
        assert_eq!(s, Settings::DEFAULT);
        assert!(!s.searches_fonts());
    }

    #[test]
    fn mistakes_are_errors_naming_the_line() {
        for (text, needle) in [
            ("workaround = always", "line 1: unknown key"),
            ("\nworkarounds = sometimes", "line 2: workarounds is"),
            ("system_fonts = yes", "use on or off"),
            (
                "system_fonts = on\nsystem_fonts = off",
                "line 2: system_fonts is set twice",
            ),
            ("font_file_limit = 0", "font_file_limit is"),
            ("font_folder =", "font_folder has no value"),
            ("ocr_folder =", "ocr_folder has no value"),
            ("just words", "is not `key = value`"),
        ] {
            let err = parse(text, &base()).unwrap_err();
            assert!(err.contains(needle), "{text:?} -> {err}");
        }
    }

    #[test]
    fn workarounds_defaults_to_offer_without_a_file() {
        assert_eq!(Settings::DEFAULT.workarounds, Workarounds::Offer);
        assert_eq!(Workarounds::Always.as_str(), "always");
    }

    #[test]
    fn the_walk_recurses_sorted_and_honours_both_ceilings() {
        let root =
            std::env::temp_dir().join(format!("pdfcer-settings-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut deep = root.clone();
        for level in 0..=MAX_FOLDER_DEPTH + 1 {
            deep = deep.join(format!("d{level}"));
            std::fs::create_dir_all(&deep).unwrap();
            std::fs::write(deep.join("f.ttf"), b"x").unwrap();
        }
        std::fs::write(root.join("notes.txt"), b"x").unwrap();
        let walk = walk_font_folders(&[(root.clone(), true)], 1000);
        // d0 is depth 1 below root; d{MAX_FOLDER_DEPTH} would be depth MAX+1.
        assert_eq!(walk.dirs.len(), MAX_FOLDER_DEPTH, "{:?}", walk.notes);
        assert_eq!(walk.font_files, MAX_FOLDER_DEPTH);
        assert!(walk.notes.iter().any(|n| n.contains("folder levels deep")));

        let capped = walk_font_folders(&[(root.clone(), true)], 3);
        assert_eq!(capped.font_files, 3);
        assert!(capped.notes.iter().any(|n| n.contains("limit of 3")));

        let missing = walk_font_folders(&[(root.join("absent"), true), (root.join("x"), false)], 9);
        assert_eq!(missing.notes.len(), 1, "{:?}", missing.notes);
        let _ = std::fs::remove_dir_all(&root);
    }
}
