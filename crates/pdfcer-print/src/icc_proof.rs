//! Proof printing: make the desk printer show how a page will look in
//! another output condition — a CMYK press and paper, most often.
//!
//! The raster's sRGB is converted into the simulated condition's profile
//! (the press leg, at the chosen intent), then from there to the printer's
//! profile, or to sRGB when none is given (the printer leg). Values pass
//! between the legs unquantised. Every conversion is icccer's (decision 064).
//!
//! The printer leg is media-relative with black point compensation, so the
//! press's paper prints as the printer's paper white and its black as the
//! printer's black. With [`ProofColour::simulate_paper`](crate::icc_proof::ProofColour::simulate_paper) it is
//! ICC-absolute instead: the press paper's tint is printed, and so is its
//! weaker black (ICC.1:2022 §6.2.2).

use std::path::Path;

use crate::icc_output::{
    HeaderFacts, PrinterProfile, PrinterTransform, ProfileError, ProfileSpace, RenderingIntent,
    build_chain, description, read_capped, sig_text, srgb_profile,
};

/// An output condition to simulate: an RGB, grey or CMYK output or display
/// profile that converts both into and out of the PCS.
#[derive(Debug, Clone)]
pub struct ProofProfile {
    profile: icccer_profile::Profile,
    name: String,
    space: ProfileSpace,
}

impl ProofProfile {
    /// Parse and check a profile. Refuses n-colour and Lab spaces, input,
    /// device-link and abstract classes, and a profile icccer cannot convert
    /// both into and out of, with a named [`ProfileError`]; never panics.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProfileError> {
        let profile = icccer_profile::Profile::parse(bytes)
            .map_err(|e| ProfileError::NotAProfile(e.to_string()))?;
        let facts = HeaderFacts::of(&profile);
        if let ProfileSpace::Other(sig) = facts.space {
            return Err(ProfileError::UnsupportedSpace(sig_text(sig)));
        }
        if !facts.is_proof_source() {
            return Err(ProfileError::UnsupportedClass(sig_text(facts.class)));
        }
        let checked = Self {
            name: description(&profile),
            space: facts.space,
            profile,
        };
        // Both legs, built now, so an unusable profile is refused when it is
        // chosen rather than part-way through a job.
        checked.compile_with(
            &PrinterProfile::srgb()?,
            RenderingIntent::default(),
            true,
            false,
        )?;
        Ok(checked)
    }

    /// Read and check a profile file, refusing one over
    /// [`crate::icc_output::MAX_PROFILE_BYTES`] before reading it.
    pub fn read(path: &Path) -> Result<Self, ProfileError> {
        Self::from_bytes(&read_capped(path)?)
    }

    /// The profile's description; empty if it carries none.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// RGB, grey or CMYK.
    #[must_use]
    pub fn space(&self) -> ProfileSpace {
        self.space
    }

    fn compile_with(
        &self,
        printer: &PrinterProfile,
        intent: RenderingIntent,
        bpc: bool,
        simulate_paper: bool,
    ) -> Result<PrinterTransform, ProfileError> {
        let (press, press_refused) = build_chain(&srgb_profile()?, &self.profile, intent, bpc)?;
        let leg2_intent = if simulate_paper {
            RenderingIntent::AbsoluteColorimetric
        } else {
            RenderingIntent::RelativeColorimetric
        };
        let (print, print_refused) =
            build_chain(&self.profile, printer.profile(), leg2_intent, true)?;
        Ok(PrinterTransform {
            legs: vec![press, print],
            grey: printer.space() == ProfileSpace::Gray,
            bpc_refused: press_refused || print_refused,
        })
    }
}

/// A proof print: what to simulate, and on what.
#[derive(Debug, Clone)]
pub struct ProofColour {
    /// The condition simulated.
    pub proof: ProofProfile,
    /// The press leg's intent; relative colorimetric by default.
    pub intent: RenderingIntent,
    /// Black point compensation on the press leg; `true` in
    /// [`ProofColour::new`]. Ignored under absolute colorimetric.
    pub black_point_compensation: bool,
    /// Print the simulated paper's tint and black (ICC-absolute printer
    /// leg) rather than mapping them to the printer's own.
    pub simulate_paper: bool,
    /// The printer's profile; `None` prints the proof as sRGB.
    pub printer: Option<PrinterProfile>,
}

impl ProofColour {
    /// Relative colorimetric with black point compensation, paper not
    /// simulated, printed as sRGB.
    #[must_use]
    pub fn new(proof: ProofProfile) -> Self {
        Self {
            proof,
            intent: RenderingIntent::default(),
            black_point_compensation: true,
            simulate_paper: false,
            printer: None,
        }
    }

    /// Build the two-leg transform. Do this once per job; apply it per page
    /// with [`PrinterTransform::apply`].
    pub fn compile(&self) -> Result<PrinterTransform, ProfileError> {
        let srgb;
        let printer = match &self.printer {
            Some(p) => p,
            None => {
                srgb = PrinterProfile::srgb()?;
                &srgb
            }
        };
        self.proof.compile_with(
            printer,
            self.intent,
            self.black_point_compensation,
            self.simulate_paper,
        )
    }
}

#[cfg(test)]
mod tests;
