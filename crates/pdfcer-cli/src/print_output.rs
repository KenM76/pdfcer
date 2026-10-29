//! `print`'s options that change only the printed copy (colour, a header
//! or footer line) and its several-files loop. The file on disk is never
//! written: stamps go on the in-memory session, colour on the raster.

use std::path::{Path, PathBuf};

use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::printer_marks::PrinterMarks;
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

/// One `--marks` entry on `print`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum MarkArg {
    /// Crop marks at the trim box corners.
    Trim,
    /// Bleed marks at the bleed box corners.
    Bleed,
    /// Registration targets.
    Registration,
    /// Colour bars.
    ColorBars,
    /// File name and page number.
    Info,
    /// Every mark.
    All,
}

/// The printed-copy-only options of one `print` run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PrintOutput {
    pub color: ColorArg,
    pub header: Option<(String, AlignArg)>,
    pub footer: Option<(String, AlignArg)>,
    pub marks: Vec<MarkArg>,
    pub separations: bool,
    pub print_as_image: bool,
}

// Consumed only by the `#[cfg(windows)]` `cmd_print`; pure, so compiled and
// tested everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
impl PrintOutput {
    /// The core marks request, or `None` when no `--marks` were given.
    /// `label` starts the information line.
    pub(crate) fn printer_marks(&self, label: &str) -> Option<PrinterMarks> {
        if self.marks.is_empty() {
            return None;
        }
        let mut m = PrinterMarks::none(label);
        for mark in &self.marks {
            match mark {
                MarkArg::Trim => m.trim = true,
                MarkArg::Bleed => m.bleed = true,
                MarkArg::Registration => m.registration = true,
                MarkArg::ColorBars => m.color_bars = true,
                MarkArg::Info => m.page_info = true,
                MarkArg::All => m = PrinterMarks::all(label),
            }
        }
        Some(m)
    }

    /// Stamp the header, footer and printer's marks onto `session` (never
    /// saved). `label` names the file in the marks' information line.
    /// Returns the lines saying what the printed copy carries.
    pub(crate) fn stamp(
        &self,
        session: &mut EditSession,
        label: &str,
    ) -> Result<Vec<String>, EditError> {
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
        if let Some(marks) = self.printer_marks(label) {
            let out = session.add_printer_marks(&marks)?;
            notes.push(format!(
                "printed copy only: printer's marks on {} page(s), each page grown by up to {} pt per side and its content cut to its bleed box.",
                out.pages.len(),
                out.grown_by
            ));
            if !out.trim_assumed.is_empty() {
                notes.push(format!(
                    "{} page(s) have no trim box; their crop box was taken as the trim.",
                    out.trim_assumed.len()
                ));
            }
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
