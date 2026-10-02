//! `pdfcer-core`'s [`SubsetAugmenter`] over the faces a shell supplied
//! (decision 173): pick the face whose PostScript name is the subset's, prove
//! it is the same font, append the glyphs.

use pdfcer_core::text_edit::{
    AugmentRefusal, AugmentRequest, AugmentedProgram, HintingMismatch, OutlineCheck,
    SubsetAugmenter,
};

use super::identity;
use super::{Hinting, append_glyphs};
use crate::font::{FontData, FontEnvironment};

/// Extends embedded TrueType subsets from operator-supplied faces.
///
/// A face qualifies only when its name ID 6 equals the subset's `/BaseFont`
/// without its tag and every decision 173 §3 identity check passes; the
/// first face that qualifies supplies the glyphs.
///
/// # Examples
///
/// ```
/// use pdfcer_core::text_edit::SubsetAugment;
/// use pdfcer_render::FontEnvironment;
/// use pdfcer_render::font::InstalledFaceAugmenter;
///
/// let faces = InstalledFaceAugmenter::from_environment(&FontEnvironment::bundled());
/// let settings = SubsetAugment::new(Box::leak(Box::new(faces)));
/// # let _ = settings;
/// ```
#[derive(Debug, Clone, Default)]
pub struct InstalledFaceAugmenter {
    faces: Vec<(String, FontData)>,
}

impl InstalledFaceAugmenter {
    /// No faces: every request refuses.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every face `env` has under a name ([`FontEnvironment::named_faces`]),
    /// in name order; the bundled fallbacks are not offered.
    #[must_use]
    pub fn from_environment(env: &FontEnvironment) -> Self {
        let faces = env
            .named_faces()
            .into_iter()
            .filter_map(|n| Some((n.to_owned(), env.named(n)?.clone())))
            .collect();
        Self { faces }
    }

    /// Offer `data` under `label`, the name the disclosure gives it.
    pub fn insert(&mut self, label: &str, data: FontData) {
        self.faces.push((label.to_owned(), data));
    }

    /// How many faces are offered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.faces.len()
    }

    /// Whether no face is offered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }
}

impl SubsetAugmenter for InstalledFaceAugmenter {
    fn augment(&self, request: &AugmentRequest<'_>) -> Result<AugmentedProgram, AugmentRefusal> {
        let scope = match request.outline_check {
            OutlineCheck::ShownOnly => identity::OutlineCheck::ShownOnly(request.shown),
            _ => identity::OutlineCheck::AllShared,
        };
        let hinting = match request.hinting_mismatch {
            HintingMismatch::Refuse => Hinting::Refuse,
            _ => Hinting::Strip,
        };
        let mut first_failure = None;
        for (label, data) in &self.faces {
            let face = data.bytes();
            let Some(index) = identity::candidate_index(face, request.base_font) else {
                continue;
            };
            let attempt = identity::check(request.program, face, index, request.chars, scope)
                .and_then(|n| {
                    Ok((
                        n,
                        append_glyphs(request.program, face, index, request.chars, hinting)?,
                    ))
                });
            match attempt {
                Ok((compared, a)) => {
                    let evidence = evidence(compared, request.outline_check);
                    return Ok(AugmentedProgram::new(
                        a.program,
                        label.clone(),
                        a.units_per_em,
                        a.bbox,
                    )
                    .with_instructions_stripped(a.instructions_stripped)
                    .with_evidence(evidence));
                }
                Err(e) => {
                    first_failure.get_or_insert_with(|| format!("'{label}': {e}"));
                }
            }
        }
        Err(AugmentRefusal {
            reason: first_failure.unwrap_or_else(|| {
                format!(
                    "no supplied font is named '{}'",
                    FontEnvironment::subset_stem(request.base_font)
                )
            }),
        })
    }

    /// The characters the first face that passes the identity check maps.
    fn addable(&self, request: &AugmentRequest<'_>) -> Vec<char> {
        let scope = match request.outline_check {
            OutlineCheck::ShownOnly => identity::OutlineCheck::ShownOnly(request.shown),
            _ => identity::OutlineCheck::AllShared,
        };
        for (_, data) in &self.faces {
            let face = data.bytes();
            let Some(index) = identity::candidate_index(face, request.base_font) else {
                continue;
            };
            let held = identity::face_chars(face, index, request.chars);
            if !held.is_empty()
                && identity::check(request.program, face, index, &held, scope).is_ok()
            {
                return held;
            }
        }
        Vec::new()
    }
}

fn evidence(compared: usize, scope: OutlineCheck) -> String {
    let which = match scope {
        OutlineCheck::ShownOnly => "the glyphs the document shows",
        _ => "every glyph both fonts map",
    };
    format!(
        "same PostScript name and design grid, and {compared} shared glyph outline(s) and \
         advance(s) identical, comparing {which}"
    )
}
