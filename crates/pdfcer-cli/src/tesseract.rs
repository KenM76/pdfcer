//! `ocr --ocr-engine tesseract`: runs a Tesseract executable as a subprocess.
//!
//! Tesseract is C++, so it is never linked: the shell spawns it, feeds it the
//! page as a PGM on stdin and reads TSV from stdout, which
//! `pdfcer_core::ocr::tesseract_tsv` parses. Spawning lives here rather than
//! in `pdfcer-core` so the engine crate stays free of processes and keeps
//! compiling for `wasm32`.
//!
//! The folder is `models/tesseract` beside `pdfcer.exe` (the portable package
//! ships a curl-free, archive-free static build there, see
//! `tools/tesseract/README.md`), or whatever `--model-dir` names. That folder
//! holds the executable and a `tessdata/` directory of `.traineddata` files,
//! which is also the layout of a stock Tesseract install, so
//! `--model-dir "C:\Program Files\Tesseract-OCR"` works as-is.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use pdfcer_core::ocr::RecognizedWord;
use pdfcer_core::ocr::tesseract_tsv;

/// Subdirectory of `models/` holding the bundled Tesseract.
pub const MODEL_DIR: &str = "tesseract";

/// The executable's file name inside the model folder.
#[cfg(windows)]
pub const EXE_FILE: &str = "tesseract.exe";
/// The executable's file name inside the model folder.
#[cfg(not(windows))]
pub const EXE_FILE: &str = "tesseract";

/// The language-data directory inside the model folder.
pub const TESSDATA_DIR: &str = "tessdata";

/// Largest stdout accepted from one page, in bytes. A page of TSV is a few
/// hundred kilobytes; anything past this is not a page of words.
const MAX_STDOUT: usize = 64 * 1024 * 1024;

/// A Tesseract executable plus the settings every page is read with.
#[derive(Debug, Clone)]
pub struct TesseractEngine {
    exe: PathBuf,
    tessdata: PathBuf,
    langs: String,
    dpi: u32,
}

impl TesseractEngine {
    /// Bind to the Tesseract in `dir`, checking that every language in
    /// `langs` (Tesseract's `eng+deu` syntax) has a `.traineddata` file.
    ///
    /// # Errors
    ///
    /// A message naming the missing executable or language files.
    pub fn from_dir(dir: &Path, langs: &str, dpi: f32) -> Result<Self, String> {
        let exe = dir.join(EXE_FILE);
        if !exe.is_file() {
            return Err(format!("{}: no Tesseract executable here", exe.display()));
        }
        let tessdata = dir.join(TESSDATA_DIR);
        let langs = langs.trim();
        if langs.is_empty()
            || langs.split('+').any(|l| {
                l.is_empty()
                    || !l
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            })
        {
            return Err(format!(
                "--ocr-lang {langs:?}: expected language codes joined by '+', e.g. eng or eng+deu"
            ));
        }
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
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        // --dpi is validated positive by the caller; Tesseract takes whole DPI
        let dpi = dpi.round().clamp(1.0, 2400.0) as u32;
        Ok(Self {
            exe,
            tessdata,
            langs: langs.to_owned(),
            dpi,
        })
    }

    /// The executable that will run.
    pub fn exe(&self) -> &Path {
        &self.exe
    }

    /// Recognise an 8-bit greyscale image (row-major, top-down).
    ///
    /// # Errors
    ///
    /// A message for a buffer of the wrong size, a failed spawn, a non-zero
    /// exit (with Tesseract's stderr), or output that is not TSV.
    pub fn recognize(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Vec<RecognizedWord>, String> {
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

        let mut cmd = Command::new(&self.exe);
        cmd.arg("stdin")
            .arg("stdout")
            .arg("--tessdata-dir")
            .arg(&self.tessdata)
            .arg("-l")
            .arg(&self.langs)
            .arg("--dpi")
            .arg(self.dpi.to_string())
            // Set directly rather than via the `tsv` config name, which needs
            // a `tessdata/configs` file that neither the bundle nor every
            // stock install carries.
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
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("{}: could not start: {e}", self.exe.display()))?;

        // Feed stdin on its own thread so a child that writes before it has
        // read everything cannot deadlock against a full pipe.
        let mut stdin = child.stdin.take().ok_or("no stdin pipe")?;
        let writer = std::thread::spawn(move || stdin.write_all(&pgm));
        let output = child
            .wait_with_output()
            .map_err(|e| format!("{}: {e}", self.exe.display()))?;
        let fed = writer
            .join()
            .map_err(|_| "stdin writer panicked".to_owned())?;

        if !output.status.success() {
            return Err(format!(
                "{} exited with {}: {}",
                self.exe.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        fed.map_err(|e| format!("{}: writing the page image: {e}", self.exe.display()))?;
        if output.stdout.len() > MAX_STDOUT {
            return Err(format!(
                "{}: output exceeds {MAX_STDOUT} bytes",
                self.exe.display()
            ));
        }
        let text = String::from_utf8(output.stdout)
            .map_err(|_| format!("{}: output is not UTF-8", self.exe.display()))?;
        tesseract_tsv::parse_tsv(&text).map_err(|e| e.to_string())
    }
}
