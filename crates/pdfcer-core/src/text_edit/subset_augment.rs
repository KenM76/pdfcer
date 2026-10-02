//! The seam through which an edit asks for an embedded TrueType subset to be
//! extended from its installed face (decision 173).
//!
//! `pdfcer-core` has no font parser (`R21`), so the shell installs a
//! [`SubsetAugmenter`] — `pdfcer-render`'s, over the faces the operator
//! supplied — and the embedded-subset floor asks it for a new program when
//! route A (decision 172) finds no outline for a character. Core never sees
//! a font path.

/// Which shared glyphs the identity check compares (decision 173 §3, I5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum OutlineCheck {
    /// Every outlined glyph the subset maps whose character the face also
    /// maps.
    #[default]
    AllShared,
    /// Only glyphs for characters the document shows in this font, which
    /// tolerates a face revision that changed an unrelated glyph.
    ShownOnly,
}

/// What to do when the face's hinting programs differ from the subset's
/// (decision 173 §5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum HintingMismatch {
    /// Append the glyphs without their instructions, and disclose it.
    #[default]
    Strip,
    /// Refuse the augmentation.
    Refuse,
}

/// One request to extend an embedded subset.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct AugmentRequest<'a> {
    /// The decoded `/FontFile2` program.
    pub program: &'a [u8],
    /// The font's `/BaseFont`, subset tag included.
    pub base_font: &'a str,
    /// The characters to append, in request order.
    pub chars: &'a [char],
    /// Characters the document shows in this font, for [`OutlineCheck::ShownOnly`].
    pub shown: &'a [char],
    /// The identity check's scope.
    pub outline_check: OutlineCheck,
    /// The hinting policy.
    pub hinting_mismatch: HintingMismatch,
}

/// A new program: the old one with glyphs appended (rule `R259` as amended).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AugmentedProgram {
    /// The complete new sfnt.
    pub program: Vec<u8>,
    /// Which supplied face the glyphs came from, for the disclosure.
    pub face_label: String,
    /// Whether the appended glyphs lost their instructions.
    pub instructions_stripped: bool,
    /// `head.unitsPerEm`.
    pub units_per_em: u16,
    /// `head`'s bounding box after the append, font units.
    pub bbox: [i16; 4],
    /// The identity evidence, one clause for the disclosure: what was
    /// compared and how much of it agreed.
    pub evidence: String,
}

impl AugmentedProgram {
    /// A program read from `face_label`.
    #[must_use]
    pub fn new(program: Vec<u8>, face_label: String, units_per_em: u16, bbox: [i16; 4]) -> Self {
        Self {
            program,
            face_label,
            instructions_stripped: false,
            units_per_em,
            bbox,
            evidence: String::new(),
        }
    }

    /// Record that the appended glyphs lost their instructions, returning
    /// `self`.
    #[must_use]
    pub fn with_instructions_stripped(mut self, stripped: bool) -> Self {
        self.instructions_stripped = stripped;
        self
    }

    /// Set the identity evidence clause, returning `self`.
    #[must_use]
    pub fn with_evidence(mut self, evidence: String) -> Self {
        self.evidence = evidence;
        self
    }
}

/// Why no supplied face could extend the subset.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{reason}")]
pub struct AugmentRefusal {
    /// A sentence naming the failed check, for the floor's refusal message.
    pub reason: String,
}

/// Extends embedded subsets on `pdfcer-core`'s behalf.
///
/// Implementations must be pure functions of their arguments and their
/// installed faces, so a preview and the commit of the same edit agree, and
/// must verify the result (rule `R260`) before returning it.
pub trait SubsetAugmenter: Send + Sync + std::fmt::Debug {
    /// The program extended with a glyph for every character in
    /// `request.chars`, from a face that passes the identity check.
    ///
    /// # Errors
    ///
    /// [`AugmentRefusal`] naming why no face qualified or the surgery
    /// refused.
    fn augment(&self, request: &AugmentRequest<'_>) -> Result<AugmentedProgram, AugmentRefusal>;

    /// The characters of `request.chars` this augmenter could append, for a
    /// typing repertoire. The edit itself still calls [`Self::augment`], so
    /// an over-answer here is refused at commit, never written.
    ///
    /// The default runs [`Self::augment`] and answers all or nothing; an
    /// implementation that can tell per character should override it.
    fn addable(&self, request: &AugmentRequest<'_>) -> Vec<char> {
        self.augment(request)
            .map(|_| request.chars.to_vec())
            .unwrap_or_default()
    }
}

/// The decision 173 settings an edit carries.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct SubsetAugment {
    /// The augmenter.
    pub source: &'static dyn SubsetAugmenter,
    /// The identity check's scope.
    pub outline_check: OutlineCheck,
    /// The hinting policy.
    pub hinting_mismatch: HintingMismatch,
}

impl SubsetAugment {
    /// `source` with the safe defaults: [`OutlineCheck::AllShared`] and
    /// [`HintingMismatch::Strip`].
    #[must_use]
    pub fn new(source: &'static dyn SubsetAugmenter) -> Self {
        Self {
            source,
            outline_check: OutlineCheck::default(),
            hinting_mismatch: HintingMismatch::default(),
        }
    }

    /// Set the identity check's scope, returning `self`.
    #[must_use]
    pub fn with_outline_check(mut self, check: OutlineCheck) -> Self {
        self.outline_check = check;
        self
    }

    /// Set the hinting policy, returning `self`.
    #[must_use]
    pub fn with_hinting_mismatch(mut self, policy: HintingMismatch) -> Self {
        self.hinting_mismatch = policy;
        self
    }
}
