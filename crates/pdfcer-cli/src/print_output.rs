//! `print`'s options that change only the printed copy (colour, a header
//! or footer line) and its several-files loop. The file on disk is never
//! written: stamps go on the in-memory session, colour on the raster.

use std::path::{Path, PathBuf};

use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::running_text::{RunningText, RunningTextAlign, RunningTextKind};
use pdfcer_print::output_color::{OutputColor, apply_output_color};

use crate::exit;

/// clap's usage-error code.
const USAGE: u8 = 2;

/// `--color` on `print`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ColorArg {
    /// As the document is coloured.
    #[default]
    Color,
    /// Composite grey: each colour printed as its brightness.
    Grayscale,
    /// Every mark that is not white prints solid black.
    BlackOnly,
}

/// `--header-align` / `--footer-align` on `print`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum AlignArg {
    /// Left end of the edge.
    Left,
    /// Centred.
    #[default]
    Center,
    /// Right end of the edge.
    Right,
}

impl AlignArg {
    const fn core(self) -> RunningTextAlign {
        match self {
            Self::Left => RunningTextAlign::Left,
            Self::Center => RunningTextAlign::Center,
            Self::Right => RunningTextAlign::Right,
        }
    }
}

/// The printed-copy-only options of one `print` run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PrintOutput {
    pub color: ColorArg,
    pub header: Option<(String, AlignArg)>,
    pub footer: Option<(String, AlignArg)>,
}

// Consumed only by the `#[cfg(windows)]` `cmd_print`; pure, so compiled and
// tested everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
impl PrintOutput {
    /// Stamp the header and footer onto `session` (never saved). Returns the
    /// lines saying what the printed copy carries.
    pub(crate) fn stamp(&self, session: &mut EditSession) -> Result<Vec<String>, EditError> {
        let mut notes = Vec::new();
        for (kind, line) in [
            (RunningTextKind::Header, &self.header),
            (RunningTextKind::Footer, &self.footer),
        ] {
            let Some((text, align)) = line else {
                continue;
            };
            let mut stamp = RunningText::new(kind, text.clone());
            stamp.align = align.core();
            let out = session.stamp_running_text(&stamp)?;
            notes.push(format!(
                "printed copy only: {} \"{}\" on {} page(s); the file is unchanged.",
                kind.subtype().to_ascii_lowercase(),
                out.first_label,
                out.pages.len()
            ));
        }
        Ok(notes)
    }

    /// Recolour the rendered sheets. Returns the line saying so, if any.
    pub(crate) fn recolor(&self, sheets: &mut [pdfcer_print::PageBitmap]) -> Option<String> {
        let mode = match self.color {
            ColorArg::Color => return None,
            ColorArg::Grayscale => OutputColor::Grayscale,
            ColorArg::BlackOnly => OutputColor::BlackOnly,
        };
        for sheet in sheets {
            apply_output_color(&mut sheet.rgba, mode);
        }
        Some(match mode {
            OutputColor::BlackOnly => "printed copy only: black-only, every mark that is not \
                                       white prints solid black (tints and light colours too)."
                .to_owned(),
            _ => "printed copy only: grayscale, each colour as its brightness.".to_owned(),
        })
    }
}

/// Print each of `inputs` in order with `print_one`. A failing file is
/// named and the rest still print. Exit code: 0 when every file printed,
/// else the code of the first file that failed. `--to-file` names one
/// output, so it takes one input.
pub(crate) fn print_each(
    inputs: &[PathBuf],
    to_file: Option<&Path>,
    mut print_one: impl FnMut(&Path) -> u8,
) -> u8 {
    if inputs.len() > 1 && to_file.is_some() {
        eprintln!(
            "pdfcer: --to-file writes one file, so print one PDF with it, not {}.",
            inputs.len()
        );
        return USAGE;
    }
    let mut first_failure = exit::SUCCESS;
    let mut failed = Vec::new();
    for (i, input) in inputs.iter().enumerate() {
        if inputs.len() > 1 {
            eprintln!(
                "pdfcer: [{} of {}] {}",
                i + 1,
                inputs.len(),
                input.display()
            );
        }
        let code = print_one(input);
        if code != exit::SUCCESS {
            failed.push(input.display().to_string());
            if first_failure == exit::SUCCESS {
                first_failure = code;
            }
        }
    }
    if inputs.len() > 1 {
        if failed.is_empty() {
            eprintln!("pdfcer: all {} files printed.", inputs.len());
        } else {
            eprintln!(
                "pdfcer: {} of {} files did not print: {}",
                failed.len(),
                inputs.len(),
                failed.join(", ")
            );
        }
    }
    first_failure
}

#[cfg(test)]
mod tests;
