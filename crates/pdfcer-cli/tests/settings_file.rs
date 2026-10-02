//! The portable settings file (`--settings`, `--no-settings`,
//! `pdfcer-settings.txt` beside the program) over the real binary.
//!
//! The beside-the-program cases run a COPY of the binary in a per-process
//! temp folder: a settings file written beside the shared test binary would
//! change every other test running at the same time.
//!
//! The vehicle is `embed-font`'s dry run on `embed-attach.pdf`, whose one
//! non-embedded font, `pdfceMissing`, is found only when a font folder holds
//! `pdfceMissing.ttf`: `match=exact` with it, `reason=no-source-font`
//! without.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EDIT_REFUSED: i32 = 9;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

/// A fresh per-process, per-test folder.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-settings-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A copy of the binary in `dir`, so a settings file can sit beside it.
fn binary_in(dir: &Path) -> PathBuf {
    let name = Path::new(BIN).file_name().unwrap();
    let copy = dir.join(name);
    if std::fs::hard_link(BIN, &copy).is_err() {
        std::fs::copy(BIN, &copy).unwrap();
    }
    copy
}

/// `pdfceMissing.ttf` placed `levels` folders below `root`.
fn nest_font(root: &Path, levels: usize) {
    let mut dir = root.to_path_buf();
    for level in 0..levels {
        dir = dir.join(format!("level{level}"));
    }
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(
        fixture("embed/fonts/pdfceMissing.ttf"),
        dir.join("pdfceMissing.ttf"),
    )
    .unwrap();
}

fn embed_dry_run(bin: &Path, args: &[&str]) -> Output {
    Command::new(bin)
        .args(args)
        .arg("embed-font")
        .arg(fixture("embed/embed-attach.pdf"))
        .arg("--all-missing")
        .output()
        .unwrap()
}

fn text(o: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

/// With no settings file beside the program, output is what `--no-settings`
/// produces, byte for byte, written file included.
#[test]
fn no_settings_file_changes_nothing() {
    let dir = scratch("none");
    let bin = binary_in(&dir);
    let fonts = fixture("embed/fonts");
    let run = |bin: &Path, out: &Path, extra: &[&str]| {
        Command::new(bin)
            .args(extra)
            .arg("embed-font")
            .arg(fixture("embed/embed-attach.pdf"))
            .args(["--all-missing", "--apply", "--font-dir"])
            .arg(&fonts)
            .arg("-o")
            .arg(out)
            .output()
            .unwrap()
    };
    let plain_out = dir.join("plain.pdf");
    let pinned_out = dir.join("pinned.pdf");
    let plain = run(&bin, &plain_out, &[]);
    let pinned = run(Path::new(BIN), &pinned_out, &["--no-settings"]);
    assert_eq!(plain.status.code(), Some(0), "{:?}", text(&plain));
    // stdout names the output file; everything else must match.
    let unpath = |o: &Output, out: &Path| text(o).0.replace(&out.display().to_string(), "OUT");
    assert_eq!(unpath(&plain, &plain_out), unpath(&pinned, &pinned_out));
    assert!(!text(&plain).1.contains("settings:"), "{}", text(&plain).1);
    assert_eq!(
        std::fs::read(&plain_out).unwrap(),
        std::fs::read(&pinned_out).unwrap()
    );
}

/// A settings file beside the program is read, named once on stderr, and
/// its font folder is searched recursively; `--no-settings` ignores it.
#[test]
fn a_file_beside_the_program_is_named_and_no_settings_ignores_it() {
    let dir = scratch("beside");
    let bin = binary_in(&dir);
    nest_font(&dir.join("fonts"), 2);
    std::fs::write(
        dir.join("pdfcer-settings.txt"),
        "# portable fonts\nworkarounds = always\nfont_folder = fonts\n",
    )
    .unwrap();

    let used = embed_dry_run(&bin, &[]);
    let (stdout, stderr) = text(&used);
    assert_eq!(used.status.code(), Some(0), "{stdout}\n{stderr}");
    assert!(stdout.contains("match=exact"), "{stdout}");
    assert_eq!(stderr.matches("settings: using").count(), 1, "{stderr}");
    assert!(stderr.contains("pdfcer-settings.txt"), "{stderr}");
    assert!(
        stderr.contains("workarounds=always system_fonts=off font_folders=1"),
        "{stderr}"
    );

    let ignored = embed_dry_run(&bin, &["--no-settings"]);
    let (stdout, stderr) = text(&ignored);
    assert_eq!(ignored.status.code(), Some(EDIT_REFUSED), "{stdout}");
    assert!(stdout.contains("reason=no-source-font"), "{stdout}");
    assert!(!stderr.contains("settings:"), "{stderr}");
}

/// A font deeper than the depth guard is not found, and the skip is named.
#[test]
fn the_depth_guard_stops_the_walk_and_says_so() {
    let dir = scratch("depth");
    nest_font(&dir.join("fonts"), 9);
    let settings = dir.join("s.txt");
    std::fs::write(&settings, "font_folder = fonts\n").unwrap();
    let o = embed_dry_run(Path::new(BIN), &["--settings", settings.to_str().unwrap()]);
    let (stdout, stderr) = text(&o);
    assert_eq!(o.status.code(), Some(EDIT_REFUSED), "{stdout}\n{stderr}");
    assert!(
        stderr.contains("more than 8 folder levels deep"),
        "{stderr}"
    );

    std::fs::remove_dir_all(dir.join("fonts")).unwrap();
    nest_font(&dir.join("fonts"), 8);
    let o = embed_dry_run(Path::new(BIN), &["--settings", settings.to_str().unwrap()]);
    assert!(text(&o).0.contains("match=exact"), "{:?}", text(&o));
}

/// `font_file_limit` stops the walk before the folder that would pass it.
#[test]
fn the_font_file_limit_stops_the_walk_and_says_so() {
    let dir = scratch("count");
    let fonts = dir.join("fonts");
    std::fs::create_dir_all(fonts.join("a")).unwrap();
    for n in 0..3 {
        std::fs::write(fonts.join("a").join(format!("junk{n}.ttf")), b"not a font").unwrap();
    }
    nest_font(&fonts.join("b"), 0);
    let settings = dir.join("s.txt");
    let run = |limit: usize| {
        std::fs::write(
            &settings,
            format!("font_folder = fonts\nfont_file_limit = {limit}\n"),
        )
        .unwrap();
        text(&embed_dry_run(
            Path::new(BIN),
            &["--settings", settings.to_str().unwrap()],
        ))
    };
    let (stdout, stderr) = run(3);
    assert!(stdout.contains("reason=no-source-font"), "{stdout}");
    assert!(stderr.contains("would pass the limit of 3"), "{stderr}");

    let (stdout, stderr) = run(4);
    assert!(stdout.contains("match=exact"), "{stdout}\n{stderr}");
    assert!(stderr.contains("3 skipped"), "{stderr}");
}

/// A typo stops pdfcer with the line number; a missing `--settings` file is
/// an I/O error; `--settings` with `--no-settings` is a usage error.
#[test]
fn a_bad_or_missing_settings_file_stops_pdfcer() {
    let dir = scratch("bad");
    let settings = dir.join("s.txt");
    std::fs::write(&settings, "system_fonts = on\nworkaround = always\n").unwrap();
    let o = embed_dry_run(Path::new(BIN), &["--settings", settings.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{:?}", text(&o));
    assert!(text(&o).1.contains("line 2: unknown key"), "{:?}", text(&o));
    assert!(o.stdout.is_empty());

    let missing = dir.join("absent.txt");
    let o = embed_dry_run(Path::new(BIN), &["--settings", missing.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(3), "{:?}", text(&o));

    let o = embed_dry_run(
        Path::new(BIN),
        &["--settings", settings.to_str().unwrap(), "--no-settings"],
    );
    assert_eq!(o.status.code(), Some(2), "{:?}", text(&o));
}
