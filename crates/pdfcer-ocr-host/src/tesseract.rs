//! The Tesseract program protocol: the page goes in on stdin as a PGM, TSV
//! comes back on stdout and `pdfcer_core::ocr::tesseract_tsv` parses it.
//!
//! The folder layout is the executable plus a `tessdata/` directory of
//! `.traineddata` files, which is also a stock Tesseract install's layout.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use pdfcer_core::ocr::OcrPage;
use pdfcer_core::ocr::tesseract_tsv;

/// The engine token, and the `models/` folder name of a stock layout.
pub const ENGINE: &str = "tesseract";

/// The executable's file name in a stock layout.
#[cfg(windows)]
pub const EXE_FILE: &str = "tesseract.exe";
/// The executable's file name in a stock layout.
#[cfg(not(windows))]
pub const EXE_FILE: &str = "tesseract";

/// The language-data directory, when a manifest names no `data`.
pub const TESSDATA_DIR: &str = "tessdata";

/// Largest stdout accepted from one page, in bytes. A page of TSV is a few
/// hundred kilobytes; anything past this is not a page of words.
const MAX_STDOUT: usize = 64 * 1024 * 1024;

/// Validate Tesseract's `eng+deu` language syntax.
///
/// # Errors
///
/// A message when `langs` is empty or a code holds anything but ASCII
/// letters, digits, `_` and `-`.
pub fn check_languages(langs: &str) -> Result<&str, String> {
    let langs = langs.trim();
    let bad_code = |l: &str| {
        l.is_empty()
            || !l
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    if langs.is_empty() || langs.split('+').any(bad_code) {
        return Err(format!(
            "languages {langs:?}: expected language codes joined by '+', e.g. eng or eng+deu"
        ));
    }
    Ok(langs)
}

/// The settings every page is read with.
#[derive(Debug, Clone)]
pub(crate) struct Invocation {
    tessdata: PathBuf,
    langs: String,
    dpi: u32,
}

impl Invocation {
    /// Check the languages and that each has a `.traineddata` in `tessdata`.
    pub(crate) fn new(tessdata: PathBuf, langs: &str, dpi: f32) -> Result<Self, String> {
        let langs = check_languages(langs)?;
        let missing: Vec<String> = langs
            .split('+')
            .map(|l| tessdata.join(format!("{l}.traineddata")))
            .filter(|p| !p.is_file())
            .map(|p| p.display().to_string())
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "language data not found: {}. Copy the .traineddata files into {} \
                 (from github.com/tesseract-ocr/tessdata_fast, tessdata or tessdata_best).",
                missing.join(", "),
                tessdata.display()
            ));
        }
        Ok(Self {
            tessdata,
            langs: langs.to_owned(),
            dpi: whole_dpi(dpi),
        })
    }

    /// The checked `-l` value, for the run disclosure.
    pub(crate) fn langs(&self) -> &str {
        &self.langs
    }

    /// Run `program` on an 8-bit greyscale image (row-major, top-down),
    /// passing `dpi` when given and the load-time resolution otherwise.
    pub(crate) fn run(
        &self,
        program: &Path,
        width: u32,
        height: u32,
        pixels: &[u8],
        dpi: Option<f32>,
    ) -> Result<OcrPage, String> {
        let dpi = dpi.map_or(self.dpi, whole_dpi);
        let pgm = pgm(width, height, pixels)?;
        let mut cmd = Command::new(program);
        cmd.arg("stdin")
            .arg("stdout")
            .arg("--tessdata-dir")
            .arg(&self.tessdata)
            .arg("-l")
            .arg(&self.langs)
            .arg("--dpi")
            .arg(dpi.to_string())
            // Set directly rather than via the `tsv` config name, which needs
            // a `tessdata/configs` file not every install carries.
            .args(["-c", "tessedit_create_tsv=1", "-c", "tessedit_create_txt=0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let stdout = feed_and_collect(cmd, program, pgm)?;
        let text = String::from_utf8(stdout)
            .map_err(|_| format!("{}: output is not UTF-8", program.display()))?;
        tesseract_tsv::parse_tsv_page(&text).map_err(|e| e.to_string())
    }
}

/// `dpi` clamped to Tesseract's accepted whole-DPI range.
fn whole_dpi(dpi: f32) -> u32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // clamped to 1..=2400 first; NaN saturates to 0 and is clamped below
    let d = dpi.round().clamp(1.0, 2400.0) as u32;
    d.max(1)
}

fn pgm(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, String> {
    let expected = usize::try_from(u64::from(width) * u64::from(height))
        .map_err(|_| "image too large".to_owned())?;
    if pixels.len() != expected {
        return Err(format!(
            "greyscale buffer is {} bytes, expected {width}x{height} = {expected}",
            pixels.len()
        ));
    }
    let mut pgm = format!("P5\n{width} {height}\n255\n").into_bytes();
    pgm.extend_from_slice(pixels);
    Ok(pgm)
}

/// Start `cmd`, write `input` to its stdin on a thread (so a child writing
/// before it has read everything cannot deadlock against a full pipe), and
/// return its stdout once it exits successfully.
fn feed_and_collect(mut cmd: Command, program: &Path, input: Vec<u8>) -> Result<Vec<u8>, String> {
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{}: could not start: {e}", program.display()))?;
    let mut stdin = child.stdin.take().ok_or("no stdin pipe")?;
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child
        .wait_with_output()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    let fed = writer
        .join()
        .map_err(|_| "stdin writer panicked".to_owned())?;
    if !output.status.success() {
        return Err(format!(
            "{} exited with {}: {}",
            program.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    fed.map_err(|e| format!("{}: writing the page image: {e}", program.display()))?;
    if output.stdout.len() > MAX_STDOUT {
        return Err(format!(
            "{}: output exceeds {MAX_STDOUT} bytes",
            program.display()
        ));
    }
    Ok(output.stdout)
}
