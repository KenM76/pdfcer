//! `print --printer-profile` and `list-icc-profiles`: converting the printed
//! raster into a printer's ICC profile, and naming the profiles installed.
//! The conversion itself is `pdfcer_print::icc_output` (icccer, decision 064).

use std::path::Path;

use pdfcer_print::icc_output::{PrinterColour, PrinterProfile, ProfileSpace, RenderingIntent};
use pdfcer_print::icc_profiles::{InstalledProfile, find_installed, installed_profiles};

use crate::exit;

/// `--printer-intent` on `print`.
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

/// The conversion `--printer-profile SPEC` asks for: `spec` is a profile
/// file, else an installed print profile's name, file name or stem.
pub(crate) fn resolve(
    spec: &str,
    intent: PrinterIntentArg,
    no_bpc: bool,
) -> Result<PrinterColour, String> {
    let path = Path::new(spec);
    let profile = if path.is_file() {
        PrinterProfile::read(path).map_err(|e| format!("--printer-profile {spec}: {e}"))?
    } else {
        let installed = installed_profiles();
        let found = find_installed(&installed, spec).ok_or_else(|| {
            format!(
                "--printer-profile {spec}: no such file and no installed profile by that \
                 name; `pdfcer list-icc-profiles` lists them"
            )
        })?;
        PrinterProfile::read(&found.path)
            .map_err(|e| format!("--printer-profile {spec} ({}): {e}", found.path.display()))?
    };
    let mut colour = PrinterColour::new(profile);
    colour.intent = intent.intent();
    colour.black_point_compensation = !no_bpc;
    Ok(colour)
}

/// The stderr line naming the conversion a job applies.
pub(crate) fn disclosure(colour: &PrinterColour, intent: PrinterIntentArg) -> String {
    let bpc = if intent == PrinterIntentArg::Absolute {
        "black point compensation does not apply"
    } else if colour.black_point_compensation {
        "black point compensation on"
    } else {
        "black point compensation off"
    };
    format!(
        "printed copy only: converted from sRGB into printer profile {:?}, {} intent, {bpc}.",
        colour.profile.name(),
        intent.label()
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
        "icc-profile name={:?} space={} class={} printable={} path={:?}",
        p.name,
        space_name(p.color_space),
        String::from_utf8_lossy(&p.class).trim(),
        u32::from(p.is_print_destination()),
        p.path.display().to_string(),
    )
}

/// `list-icc-profiles`: the installed profiles `--printer-profile` can
/// name, or every installed profile with `all`.
pub(crate) fn cmd_list_icc_profiles(all: bool) -> u8 {
    let profiles = installed_profiles();
    let mut count = 0_usize;
    for p in profiles.iter().filter(|p| all || p.is_print_destination()) {
        println!("{}", profile_line(p));
        count += 1;
    }
    println!("list-icc-profiles count={count}");
    exit::SUCCESS
}

#[cfg(test)]
mod tests;
