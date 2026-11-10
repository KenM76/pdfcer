//! `print --printer-profile` / `--proof-profile` and `list-icc-profiles`:
//! converting the printed raster into a printer's ICC profile, simulating
//! another output condition (a CMYK press) on it, and naming the profiles
//! installed. The conversion itself is `pdfcer_print::icc_output` and
//! `icc_proof` (icccer, decision 064).

use std::path::Path;

use pdfcer_print::icc_output::{
    PrinterColour, PrinterProfile, PrinterTransform, ProfileError, ProfileSpace, RenderingIntent,
};
use pdfcer_print::icc_profiles::{InstalledProfile, find_installed, installed_profiles};
use pdfcer_print::icc_proof::{ProofColour, ProofProfile};

use crate::exit;

/// `--printer-intent` / `--proof-intent` on `print`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum PrinterIntentArg {
    /// Compress the whole gamut to fit, keeping relationships between
    /// colours. For photographs.
    Perceptual,
    /// Match in-gamut colours exactly, relative to the paper white; clip
    /// the rest.
    #[default]
    Relative,
    /// Keep colours vivid rather than accurate. For charts.
    Saturation,
    /// Match colours exactly, including the paper tint. For proofing.
    Absolute,
}

impl PrinterIntentArg {
    const fn intent(self) -> RenderingIntent {
        match self {
            Self::Perceptual => RenderingIntent::Perceptual,
            Self::Relative => RenderingIntent::RelativeColorimetric,
            Self::Saturation => RenderingIntent::Saturation,
            Self::Absolute => RenderingIntent::AbsoluteColorimetric,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Perceptual => "perceptual",
            Self::Relative => "relative colorimetric",
            Self::Saturation => "saturation",
            Self::Absolute => "absolute colorimetric",
        }
    }
}

/// The ICC options of one `print` run, as parsed.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct IccArgs<'a> {
    pub printer_profile: Option<&'a str>,
    pub printer_intent: PrinterIntentArg,
    pub no_printer_bpc: bool,
    pub proof_profile: Option<&'a str>,
    pub proof_intent: PrinterIntentArg,
    pub no_proof_bpc: bool,
    pub simulate_paper: bool,
}

/// What a `print` run converts its raster into.
#[derive(Clone, Debug)]
pub(crate) enum IccPrint {
    /// `--printer-profile` alone.
    Printer(Box<PrinterColour>, PrinterIntentArg),
    /// `--proof-profile`, with or without `--printer-profile`.
    Proof(Box<ProofColour>, PrinterIntentArg),
}

impl IccPrint {
    /// Resolve the flags; `None` when neither profile was given.
    pub(crate) fn from_args(a: &IccArgs<'_>) -> Result<Option<Self>, String> {
        let Some(proof) = a.proof_profile else {
            return a
                .printer_profile
                .map(|spec| resolve(spec, a.printer_intent, a.no_printer_bpc))
                .transpose()
                .map(|c| c.map(|c| Self::Printer(Box::new(c), a.printer_intent)));
        };
        let mut colour = ProofColour::new(load("--proof-profile", proof, ProofProfile::read)?);
        colour.intent = a.proof_intent.intent();
        colour.black_point_compensation = !a.no_proof_bpc;
        colour.simulate_paper = a.simulate_paper;
        colour.printer = a
            .printer_profile
            .map(|spec| load("--printer-profile", spec, PrinterProfile::read))
            .transpose()?;
        Ok(Some(Self::Proof(Box::new(colour), a.proof_intent)))
    }

    /// Build the transform once for the job.
    pub(crate) fn compile(&self) -> Result<PrinterTransform, ProfileError> {
        match self {
            Self::Printer(c, _) => c.compile(),
            Self::Proof(c, _) => c.compile(),
        }
    }

    /// The stderr line naming the conversion `transform` applies.
    pub(crate) fn disclosure(&self, transform: &PrinterTransform) -> String {
        let line = match self {
            Self::Printer(c, intent) => disclosure(c, *intent),
            Self::Proof(c, intent) => proof_disclosure(c, *intent),
        };
        if transform.black_point_compensation_refused() {
            format!(
                "{line} Black point compensation was asked for but a profile's black point \
                 cannot be estimated, so it is off for that step."
            )
        } else {
            line
        }
    }
}

/// `spec` as a profile file, else an installed profile's name, file name or
/// stem, read with `read`. Errors name `flag`.
fn load<T>(
    flag: &str,
    spec: &str,
    read: impl Fn(&Path) -> Result<T, ProfileError>,
) -> Result<T, String> {
    let hint = |e: &ProfileError| match e {
        ProfileError::Cmyk => " with --proof-profile",
        _ => "",
    };
    let path = Path::new(spec);
    if path.is_file() {
        return read(path).map_err(|e| format!("{flag} {spec}: {e}{}", hint(&e)));
    }
    let installed = installed_profiles();
    let found = find_installed(&installed, spec).ok_or_else(|| {
        format!(
            "{flag} {spec}: no such file and no installed profile by that name; \
             `pdfcer list-icc-profiles` lists them"
        )
    })?;
    read(&found.path)
        .map_err(|e| format!("{flag} {spec} ({}): {e}{}", found.path.display(), hint(&e)))
}

/// The conversion `--printer-profile SPEC` asks for on its own.
pub(crate) fn resolve(
    spec: &str,
    intent: PrinterIntentArg,
    no_bpc: bool,
) -> Result<PrinterColour, String> {
    let mut colour = PrinterColour::new(load("--printer-profile", spec, PrinterProfile::read)?);
    colour.intent = intent.intent();
    colour.black_point_compensation = !no_bpc;
    Ok(colour)
}

fn bpc_words(intent: PrinterIntentArg, on: bool) -> &'static str {
    if intent == PrinterIntentArg::Absolute {
        "black point compensation does not apply"
    } else if on {
        "black point compensation on"
    } else {
        "black point compensation off"
    }
}

/// The stderr line for `--printer-profile` alone.
pub(crate) fn disclosure(colour: &PrinterColour, intent: PrinterIntentArg) -> String {
    format!(
        "printed copy only: converted from sRGB into printer profile {:?}, {} intent, {}.",
        colour.profile.name(),
        intent.label(),
        bpc_words(intent, colour.black_point_compensation)
    )
}

/// The stderr line for `--proof-profile`.
pub(crate) fn proof_disclosure(colour: &ProofColour, intent: PrinterIntentArg) -> String {
    let target = colour.printer.as_ref().map_or_else(
        || "printed as sRGB".to_owned(),
        |p| format!("printed through printer profile {:?}", p.name()),
    );
    let paper = if colour.simulate_paper {
        "the proof's paper tint and black are simulated"
    } else {
        "the proof's paper and black map to this printer's"
    };
    format!(
        "printed copy only: proof of {:?} ({}), {} intent, {}; {target}; {paper}.",
        colour.proof.name(),
        space_name(colour.proof.space()),
        intent.label(),
        bpc_words(intent, colour.black_point_compensation)
    )
}

fn space_name(space: ProfileSpace) -> String {
    match space {
        ProfileSpace::Gray => "gray".to_owned(),
        ProfileSpace::Rgb => "rgb".to_owned(),
        ProfileSpace::Cmyk => "cmyk".to_owned(),
        ProfileSpace::Other(sig) => String::from_utf8_lossy(&sig).trim().to_lowercase(),
        _ => "unknown".to_owned(),
    }
}

/// One `list-icc-profiles` line.
pub(crate) fn profile_line(p: &InstalledProfile) -> String {
    format!(
        "icc-profile name={:?} space={} class={} printable={} proof={} path={:?}",
        p.name,
        space_name(p.color_space),
        String::from_utf8_lossy(&p.class).trim(),
        u32::from(p.is_print_destination()),
        u32::from(p.is_proof_source()),
        p.path.display().to_string(),
    )
}

/// `list-icc-profiles`: the installed profiles `--printer-profile` or
/// `--proof-profile` can name, or every installed profile with `all`.
pub(crate) fn cmd_list_icc_profiles(all: bool) -> u8 {
    let profiles = installed_profiles();
    let mut count = 0_usize;
    for p in profiles
        .iter()
        .filter(|p| all || p.is_print_destination() || p.is_proof_source())
    {
        println!("{}", profile_line(p));
        count += 1;
    }
    println!("list-icc-profiles count={count}");
    exit::SUCCESS
}

#[cfg(test)]
mod tests;
