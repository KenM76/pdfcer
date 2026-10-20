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

/// How the subset's glyphs are reached from characters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProgramAddressing<'a> {
    /// Through the program's own Unicode `cmap` (a simple TrueType font).
    #[default]
    Cmap,
    /// Through the PDF, not the program: a `CIDFontType2` descendant, whose
    /// program "cmap table is not needed and shall not be present"
    /// (ISO 32000-2 §9.9, Table 126). Each pair is a glyph id and the
    /// character `/ToUnicode` gives the CID that `/CIDToGIDMap` sends to it
    /// (§9.7.4.2). The identity check compares these glyphs, any `cmap` the
    /// program carries is copied unchanged, and the appended glyph ids come
    /// back in [`AugmentedProgram::glyph_ids`] for the caller to give CIDs.
    CidKeyed(&'a [(u32, char)]),
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
    /// How the program's glyphs are reached.
    pub addressing: ProgramAddressing<'a>,
}

impl<'a> AugmentRequest<'a> {
    /// A request to append `chars` to `program`, a simple font's subset named
    /// `base_font`, with no shown characters and the default policies. Core
    /// builds its own requests; this is for a shell testing its
    /// [`SubsetAugmenter`] without an edit session.
    ///
    /// ```
    /// use pdfcer_core::text_edit::{AugmentRequest, OutlineCheck, ProgramAddressing};
    ///
    /// let known = [(1, 'A'), (2, 'B')];
    /// let request = AugmentRequest::new(b"sfnt", "ABCDEF+Face", &['C'])
    ///     .with_shown(&['A'])
    ///     .with_outline_check(OutlineCheck::ShownOnly)
    ///     .with_addressing(ProgramAddressing::CidKeyed(&known));
    /// assert_eq!(request.chars, ['C']);
    /// ```
    #[must_use]
    pub fn new(program: &'a [u8], base_font: &'a str, chars: &'a [char]) -> Self {
        Self {
            program,
            base_font,
            chars,
            shown: &[],
            outline_check: OutlineCheck::default(),
            hinting_mismatch: HintingMismatch::default(),
            addressing: ProgramAddressing::default(),
        }
    }

    /// Set the characters the document shows in this font, returning `self`.
    #[must_use]
    pub fn with_shown(mut self, shown: &'a [char]) -> Self {
        self.shown = shown;
        self
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

    /// Set how the program's glyphs are reached, returning `self`.
    #[must_use]
    pub fn with_addressing(mut self, addressing: ProgramAddressing<'a>) -> Self {
        self.addressing = addressing;
        self
    }
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
    /// The glyph id each appended character received, request order.
    pub glyph_ids: Vec<(char, u32)>,
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
            glyph_ids: Vec::new(),
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

    /// Set the appended characters' glyph ids, returning `self`.
    #[must_use]
    pub fn with_glyph_ids(mut self, glyph_ids: Vec<(char, u32)>) -> Self {
        self.glyph_ids = glyph_ids;
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

    /// Every character a face that could extend this subset maps, for a
    /// typing repertoire over a font whose own data names no character
    /// list to ask about (a composite font). [`Self::addable`] still
    /// decides which of them are offered.
    ///
    /// Required, with no default: an empty answer silently removes every
    /// composite font from the typing repertoire while simple fonts keep
    /// working, so a wrapper that forwards only [`Self::augment`] and
    /// [`Self::addable`] must fail to compile rather than lose it (G210).
    /// Core cannot supply a better default; it has no font parser.
    fn candidates(&self, request: &AugmentRequest<'_>) -> Vec<char>;
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
