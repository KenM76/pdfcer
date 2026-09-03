//! # Synthetic bold and italic — the ONE policy both authoring paths use
//!
//! Pass 19.2 / decision 019 §3.6, standing rule **R90**.
//!
//! ## What "synthetic" means here, and what it is not
//!
//! A **real** style change re-points a run at a font resource that actually
//! contains a Bold or Italic face — `Times-Roman` → `Times-Bold`. That is
//! Pass 14.2's family change (`format::FontSelector`), it is always
//! preferable, and it is not this module.
//!
//! **Synthesis** is what happens when no such face resolves: pdfcer fakes the
//! weight or the slant out of operators the PDF imaging model already has.
//! It is a *fallback*, it is *always disclosed*, and the operator can always
//! decline it. This module owns the numbers, the mechanism, the wording, and
//! the re-detection — one type used by both `format.rs` (in-place edit) and
//! `addtext.rs` (new content), so the two paths cannot drift into two
//! different behaviours the operator has to learn separately.
//!
//! ## The mechanism, and why these two operators
//!
//! | Style | Emission | Spec |
//! |---|---|---|
//! | Bold | text rendering mode **2** (fill-then-stroke) + a user-space stroke width + the stroking colour matched to the fill | §9.3.6 Table 106 |
//! | Italic | a horizontal **shear premultiplied into `Tm`** | §9.4.2 Table 108 |
//!
//! **Double-strike was rejected** (decision 019 §3.6). The industry
//! technique — show the run twice, one slightly larger — doubles the glyph
//! count in the content stream. pdfcer's provenance model maps *bytes to
//! glyphs*: `GlyphProvenance::operator_span` locates a run by the byte span
//! of the operator that showed it, and hit-testing, in-place editing and
//! text extraction all rest on that correspondence being one-to-one. Showing
//! the run twice breaks it — a later edit would find two candidate anchors
//! for one visible word, and extraction would emit the text twice. Mode 2 is
//! one operator and leaves the correspondence intact. It is also *weight*
//! inflation rather than *size* inflation, which is what bold actually is.
//!
//! **Synthesizing sheared outlines was rejected** for a simpler reason: it
//! requires the font-writing subsystem (FF-C) that does not exist yet, and
//! it would make a fallback more expensive than the real thing it stands in
//! for.
//!
//! ## The three spec traps, each of which is a bug if missed
//!
//! 1. **§9.3.6 — stroked text uses the STROKING colour.** The stroke does
//!    *not* inherit the fill colour; it takes whatever `G`/`RG`/`K`/`SCN`
//!    last set, whose Table 52 initial value is **black**. Emitting `2 Tr`
//!    on red text without matching the stroking colour therefore gives red
//!    text a black outline. The emitter in
//!    [`format`](crate::text_edit::format) always emits the match, and
//!    always restores the previous stroking colour.
//! 2. **§9.3.6 — the line width is in USER space.** "The line width shall be
//!    interpreted in user space rather than in text space" — so it is *not*
//!    scaled by `Tfs`, and a constant width that looks right at 10 pt is
//!    invisible at 72 pt. [`bold_stroke_width`] derives it from the
//!    rendered size instead.
//! 3. **A `Tm` shear is not text state.** It is not covered by R88's restore
//!    ladder, it is not saved by `q`/`Q`, and — the trap —
//!    `Td`/`TD`/`T*` derive the next line by *translating the line matrix*,
//!    so a shear left in `Tm` propagates into every later line of the text
//!    object. See [`shear_into`] and the gate in
//!    [`format`](crate::text_edit::format).
//!
//! ## Persistence: self-evident bytes, never a private marker
//!
//! Decision 019 §3.6 chose **P-selfevident**. pdfcer writes **nothing** into
//! the PDF to record that a run was synthesized. Instead the emission
//! mechanisms are chosen so that the result is *re-detectable by inspection*:
//! a run painted in mode 2 with a hairline stroke, in a font whose
//! `/BaseFont` does not say Bold, was faux-bolded; a `Tm` with a non-zero
//! `c` term under a font whose name does not say Italic was faux-obliqued.
//! [`detect`] is that inference, and it deliberately does not look for
//! anything pdfcer-specific — it recognizes **other producers'** faux styles
//! on the same evidence, which is a capability Acrobat's own output does not
//! self-disclose at all (third-party preflight has to infer it after the
//! fact).
//!
//! The rejected alternative was a private `/PieceInfo`-style key. It would
//! add bytes to a file for pdfcer's sole benefit, be invisible to every other
//! consumer, and sit badly beside `ARCHITECTURE.md` §5.6's "never
//! normalize". A session-only marker was rejected too, for the opposite
//! reason: it does not survive save-and-reopen, which is the whole point.
//!
//! ## Spec citations
//!
//! - §9.3.6, Table 106 — text rendering modes; mode 2 is "Fill, then stroke
//!   text".
//! - §9.3.6 — stroking colour and user-space line width for stroked text.
//! - §9.4.2, Table 108 — `Tm` sets **both** the text matrix and the text
//!   line matrix; `Td`/`TD`/`T*` translate the line matrix.
//! - §8.3.3 — matrix composition order.
//! - §9.6.2.2, §9.6.4 — `/BaseFont` naming and subset tags, the evidence
//!   [`detect`] reads.
//! - §8.2 Table 51 / Figure 9 — `w` (general graphics state) and
//!   `RG`/`G`/`K` (colour) are admitted inside a text object; `q`/`Q` are
//!   not, which is why every restore here is by value.

use std::fmt;

/// The tangent of pdfcer's oblique angle, **12°**.
///
/// ## Why 12°, and why a tangent rather than an angle
///
/// Decision 019 §3.6 declared 12°. It sits in the ordinary band for
/// synthesized obliques (true italic faces are typically drawn between 8°
/// and 15°, and the faux obliques other producers emit cluster near 12°), so
/// the result reads as ordinary rather than idiosyncratic in another viewer.
///
/// It is stored as the tangent because the tangent is what actually goes
/// into the matrix — the `c` operand of `Tm` *is* `tan θ` for a unit text
/// matrix. Storing the angle would mean a `tan` call at every emission site
/// and an invitation to disagree about degrees versus radians. `tan 12°` =
/// 0.212556…, rounded here to the six decimal places
/// `format::derived_operand` would round it to anyway.
///
/// This is pdfcer's own documented choice, **not** a parity claim: Acrobat's
/// internal oblique angle is undocumented and unsourced.
pub const OBLIQUE_TAN: f64 = 0.212_557;

/// The synthetic-bold stroke width as a fraction of the **rendered** size.
///
/// Decision 019 §3.6 declared ≈ 0.022. At 10 pt that is a 0.22 pt outline on
/// a glyph whose stems are perhaps 1 pt — a visible thickening that does not
/// close up the counters of an `e` or an `a`, which is the failure mode of an
/// over-heavy faux bold. Because a stroke straddles the outline, half of it
/// falls outside the glyph, so the apparent stem growth is about
/// 0.022 × size, not twice that.
///
/// Like [`OBLIQUE_TAN`] this is pdfcer's own choice. It is exposed as a public
/// constant rather than hidden in the emitter precisely because rule 4
/// forbids a magic number the operator cannot see: the save report quotes the
/// width it used, by value.
pub const BOLD_STROKE_RATIO: f64 = 0.022;

/// Which styles a synthesis applies — the *request*, and after the fact the
/// *provenance*.
///
/// One type serves both roles on purpose. A request that was honoured is
/// exactly the provenance of the run it produced, and keeping them the same
/// type is what makes "what pdfcer did" and "what pdfcer found on reload"
/// directly comparable (see [`detect`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum StyleSynthesis {
    /// No synthesis: the run's weight and slant are the font's own.
    #[default]
    None,
    /// Faux bold only — mode 2 plus a stroke.
    Bold,
    /// Faux italic only — a `Tm` shear.
    Italic,
    /// Both.
    BoldItalic,
}

impl StyleSynthesis {
    /// Build from two independent flags.
    #[must_use]
    pub const fn new(bold: bool, italic: bool) -> Self {
        match (bold, italic) {
            (false, false) => Self::None,
            (true, false) => Self::Bold,
            (false, true) => Self::Italic,
            (true, true) => Self::BoldItalic,
        }
    }

    /// Whether faux bold is part of this synthesis.
    #[must_use]
    pub const fn bold(self) -> bool {
        matches!(self, Self::Bold | Self::BoldItalic)
    }

    /// Whether faux italic is part of this synthesis.
    #[must_use]
    pub const fn italic(self) -> bool {
        matches!(self, Self::Italic | Self::BoldItalic)
    }

    /// Whether anything at all is synthesized.
    #[must_use]
    pub const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    /// An operator-facing name, for a disclosure or a properties badge.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Bold => "synthetic bold",
            Self::Italic => "synthetic italic",
            Self::BoldItalic => "synthetic bold italic",
        }
    }
}

impl fmt::Display for StyleSynthesis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which authoring path is offering synthesis — the **only** thing that
/// differs between them (decision 019 §3.6).
///
/// The gate, the wording, the declinability, the mechanism and the
/// provenance are identical on both paths. What differs is the **order the
/// two remedies are offered in**, and that difference is not a special case:
/// it falls out of what each path owes the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SynthesisPath {
    /// New content (`addtext.rs`). It owes nothing to any existing
    /// typography, and per R79 its default face is a bundled Standard-14
    /// whose family almost always *has* a real Bold — so the better first
    /// offer is "use a face with a real Bold", and the gate will rarely even
    /// open here.
    AddText,
    /// An existing run (`format.rs`). The run belongs to the document's own
    /// typography; changing its family to obtain a real Bold is the *more*
    /// visually disruptive action and may not be wanted at all. So synthesis
    /// — the least disruptive remedy — is offered first.
    InPlaceEdit,
}

impl SynthesisPath {
    /// The two remedies in this path's offering order, most-preferred first.
    ///
    /// Both remedies are offered on both paths and both are declinable; only
    /// the order differs, and the order is disclosed (that is what
    /// [`SynthesisOffer::disclosure`] prints).
    #[must_use]
    pub const fn remedy_order(self) -> [&'static str; 2] {
        match self {
            Self::AddText => [
                "use a font family that has a real Bold/Italic face",
                "synthesize the style",
            ],
            Self::InPlaceEdit => [
                "synthesize the style",
                "change this run's family to one with a real Bold/Italic face",
            ],
        }
    }
}

/// A **named, declinable, per-use** offer of synthesis — R90's disclosure
/// object.
///
/// This exists as a value rather than as a formatted string so that the CLI,
/// the save report and (in 19.3) the GUI all quote the *same* facts, and so
/// that a caller cannot accidentally apply synthesis without having
/// constructed the thing that explains it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SynthesisOffer {
    /// What would be synthesized.
    pub synthesis: StyleSynthesis,
    /// The `/BaseFont` of the run's current face, named in the disclosure.
    pub base_font: String,
    /// Which path is offering, and hence the remedy order.
    pub path: SynthesisPath,
    /// The real face this synthesis PASSED OVER, if the ladder found one and
    /// the posture proceeded anyway (`Pass 179.x`, decision 106).
    ///
    /// `None` means nothing was passed over — which, before this field
    /// existed, [`Self::disclosure`] asserted unconditionally.
    pub passed_over: Option<String>,
}

impl SynthesisOffer {
    /// The offer text.
    ///
    /// # ★★ THIS SENTENCE USED TO CONTRADICT ITS OWN REPORT
    ///
    /// It asserted, unconditionally, *"no real Bold face resolves for X on
    /// this page, so pdfcer cannot make this change with a genuine typeface"*
    /// — and after decision 106 it was emitted in reports that ALSO carried
    /// [`crate::text_edit::FormatReport::real_face_passed_over`] naming the
    /// exact face it claimed did not exist. Two disclosures, one report,
    /// flatly disagreeing.
    ///
    /// It also claimed synthesis *"is never applied silently, never as a
    /// global preference"*. Both halves became false the moment
    /// [`crate::settings::StylePolicy::Auto`] shipped as the default: it is a
    /// preference, it is global, and proceeding without asking is the point.
    ///
    /// ★ Note the DIRECTION of the error, because it is the one nobody
    /// watches for: this disclosure **understated** pdfcer's reach. Rule 4 is
    /// usually invoked against a claim that flatters the software; a claim
    /// that a capability is absent when it is present sends an operator to a
    /// worse remedy, and no test fails.
    ///
    /// So the sentence now branches on the fact rather than assuming it.
    #[must_use]
    pub fn disclosure(&self) -> String {
        let [first, second] = self.path.remedy_order();
        if let Some(face) = &self.passed_over {
            // A real face WAS there and the posture proceeded. Say that, name
            // it, and say why this is not a refusal -- the operator asked for
            // synthesis explicitly, or the posture is one that does not stop.
            return format!(
                "SYNTHETIC STYLE: pdfcer applied {} to '{}' even though a real face WAS available \
                 on this page. {face} This is not a mistake and not a refusal: synthesis was \
                 asked for explicitly, and the fallback posture in force does not stop for it \
                 (set it to `refuse` to be stopped instead). A synthesised weight is the regular \
                 letterforms thickened, not a genuine typeface.",
                self.synthesis.label(),
                self.base_font,
            );
        }
        format!(
            "SYNTHETIC STYLE: no real {} face resolves for '{}' on this page, so pdfcer cannot make \
             this change with a genuine typeface HERE. Remedies, in pdfcer's recommended order \
             here: (1) {first}; (2) {second}. A synthesised weight is a FALLBACK — the regular \
             letterforms thickened — not an alternative to a real face. pdfcer applied {}.",
            self.wanted_face_words(),
            self.base_font,
            self.synthesis.label(),
        )
    }

    /// "Bold" / "Italic" / "Bold and Italic", for the disclosure's first
    /// clause.
    fn wanted_face_words(&self) -> &'static str {
        match self.synthesis {
            StyleSynthesis::Bold => "Bold",
            StyleSynthesis::Italic => "Italic",
            StyleSynthesis::BoldItalic => "Bold and Italic",
            StyleSynthesis::None => "styled",
        }
    }
}

/// The synthetic-bold stroke width, in **user space** (§9.3.6).
///
/// `rendered_size` is the size the glyphs are actually painted at — the `Tf`
/// operand after any super/subscript reduction — multiplied by the scale the
/// text matrix and CTM impose. Deriving from it rather than using a constant
/// is trap 2 in the module docs: §9.3.6 puts the width in user space, so a
/// fixed `0.3 w` is a heavy outline on 8 pt text and an invisible one on
/// 72 pt text.
///
/// The `tm_scale` and `ctm_scale` arguments are the *linear* scale factors of
/// those matrices (see [`matrix_scale`]). They are separate arguments rather
/// than a single pre-multiplied number so each call site has to state which
/// matrices it accounted for — a synthetic bold that forgot the CTM is a bug
/// that only shows up on a scaled page.
///
/// # Examples
///
/// ```
/// use pdfcer_core::text_edit::synth::bold_stroke_width;
///
/// // 12 pt text, unscaled matrices: a 0.264 pt outline.
/// let w = bold_stroke_width(12.0, 1.0, 1.0);
/// assert!((w - 0.264).abs() < 1e-9);
///
/// // The same text inside a half-scale form: half the width, so the
/// // apparent weight on the page is unchanged.
/// let w = bold_stroke_width(12.0, 1.0, 0.5);
/// assert!((w - 0.132).abs() < 1e-9);
/// ```
#[must_use]
pub fn bold_stroke_width(rendered_size: f64, tm_scale: f64, ctm_scale: f64) -> f64 {
    (rendered_size * tm_scale * ctm_scale * BOLD_STROKE_RATIO).abs()
}

/// The linear scale factor of a PDF matrix `[a b c d e f]`, as the square
/// root of the absolute determinant of its 2×2 part.
///
/// Used to convert a text-space size into the user-space distance a stroke
/// width must be expressed in. The determinant form is chosen over "the
/// length of the `a`,`b` row" because it is the one that stays correct under
/// a shear: a shear has determinant 1 and does not change area, and it must
/// therefore not change the derived stroke width — which matters exactly
/// here, where a synthetic bold and a synthetic italic can be applied to the
/// same run.
#[must_use]
pub fn matrix_scale(m: [f64; 6]) -> f64 {
    let det = m[0] * m[3] - m[1] * m[2];
    det.abs().sqrt()
}

/// Premultiply pdfcer's oblique shear into a text matrix (§8.3.3).
///
/// The shear is `S = [1 0 tan θ 1 0 0]` and the result is `S × Tm`, which
/// for `Tm = [a b c d e f]` is
///
/// ```text
/// [ a,  b,  tanθ·a + c,  tanθ·b + d,  e,  f ]
/// ```
///
/// Premultiplication (shear **then** the text matrix) is what makes the lean
/// happen in text space, so a rotated run leans along its own baseline
/// rather than along the page's. Post-multiplying would shear the run in
/// *page* space, which looks correct only for an axis-aligned run and is
/// wrong the moment the producer rotated one.
///
/// The translation `e`,`f` is untouched: a shear fixes the origin, which is
/// exactly why the lean is anchored at the baseline and why a **raised** run
/// (non-zero `Ts`) is displaced horizontally by `Trise · tan θ` — the
/// interaction decision 019 §3.6 names, and which is a property of the
/// glyph-space rise being applied *before* this matrix, not something this
/// function can or should compensate for.
///
/// # Examples
///
/// ```
/// use pdfcer_core::text_edit::synth::{OBLIQUE_TAN, shear_into};
///
/// // An upright, unit-scale run at (72, 700).
/// let sheared = shear_into([1.0, 0.0, 0.0, 1.0, 72.0, 700.0]);
/// assert_eq!(sheared[2], OBLIQUE_TAN);
/// // The origin does not move — the lean is anchored at the baseline.
/// assert_eq!(sheared[4], 72.0);
/// assert_eq!(sheared[5], 700.0);
/// ```
#[must_use]
pub fn shear_into(tm: [f64; 6]) -> [f64; 6] {
    [
        tm[0],
        tm[1],
        OBLIQUE_TAN.mul_add(tm[0], tm[2]),
        OBLIQUE_TAN.mul_add(tm[1], tm[3]),
        tm[4],
        tm[5],
    ]
}

/// Whether a `/BaseFont` name claims a Bold face.
///
/// Name-based, because that is the only evidence available without parsing
/// the embedded font program: §9.6.2.2 gives `/BaseFont` as the PostScript
/// name of the font, and the conventional spellings are `-Bold`, `,Bold`,
/// `Black`, `Heavy` and `Semibold`. The §9.6.4 subset tag (`ABCDEF+`) is
/// irrelevant to the question and is tolerated by searching the whole string.
///
/// # It is a heuristic, and it IS used to refuse an edit — read this before
/// changing it
///
/// This doc comment used to say the opposite: *"it is used only in the
/// direction where being wrong is safe: [`detect`] uses it to say 'this looks
/// synthesized', never to refuse an edit."* **That was true when written and
/// was falsified by a later caller** — `text_edit::format`'s synthesis gate,
/// which asks this function which faces claim the style. Nothing reported the
/// drift, because `cargo doc` cannot check a claim about callers. The wording
/// is kept here, struck, rather than quietly replaced: the failure mode is
/// worth more than the correction.
///
/// ★★ **AND IT HAS NOW DRIFTED A SECOND TIME, BY THE SAME MECHANISM.** The
/// correction above said the gate *"refuses `set_synthetic` when a real styled
/// face is available"*. Decision 106 made that posture-dependent: the gate
/// **answers**, and only [`crate::settings::StylePolicy::Refuse`] turns the
/// answer into a refusal. Falsified by a later caller again, reported by
/// nothing again.
///
/// Two drifts, one cause, in a comment whose own body already named the
/// cause. A claim about callers is a measurement and goes stale silently;
/// writing that down did not stop it happening.
///
/// The two call sites today, and what being wrong costs at each:
///
/// - [`detect`] — "this run looks synthesized". A wrong answer mislabels a
///   report line. Safe, as the old wording said.
/// - **`format::survey_page_fonts`**, feeding `format::gate_synthesis` — "is
///   there a real bold face here to use instead?". A wrong answer here used
///   to make bold **unreachable**: the gate named a face on the strength of
///   its name alone, and `set_font` then refused it for encoding coverage.
///   `Pass 144.0`.
///
/// What makes that second use safe now is **not** this function getting more
/// accurate. It is that its answer is `AND`-ed with `set_font`'s own
/// acceptance test before anything is refused or recommended (`R221`): a
/// false positive from the name is filtered out by a face that cannot show
/// the run, and a false negative costs a synthesis where a real face existed
/// — the conservative direction, exactly as the old wording described.
///
/// **So: this may be made more or less eager without breaking the gate, but
/// it must never become the SOLE evidence for a refusal again.**
#[must_use]
pub fn name_claims_bold(base_font: &str) -> bool {
    let n = base_font.to_ascii_lowercase();
    n.contains("bold") || n.contains("black") || n.contains("heavy") || n.contains("semib")
}

/// Whether a `/BaseFont` name claims an Italic or Oblique face. Same
/// evidence and same caveats as [`name_claims_bold`] — **including its
/// second call site, which refuses an edit.** Read that function's note
/// before changing this one.
#[must_use]
pub fn name_claims_italic(base_font: &str) -> bool {
    let n = base_font.to_ascii_lowercase();
    n.contains("italic") || n.contains("oblique") || n.ends_with("-it") || n.contains(",italic")
}

/// **Re-detect** a synthesized style from the bytes alone — pdfcer's own and
/// other producers' (decision 019 §3.6, P-selfevident).
///
/// This is the whole of the persistence story. No marker was written into the
/// PDF, so on reload pdfcer infers the synthesis from the two pieces of
/// evidence its chosen mechanisms leave behind:
///
/// - **Faux bold** — the run is painted in a *stroking* rendering mode
///   (Table 106 modes 1, 2, 5, 6) with a **thin** stroke relative to the
///   text size, while the font's own name does **not** claim Bold. The
///   thinness test is what separates a synthesized weight from a deliberate
///   outlined-display-type effect: an outline heading is stroked at a width
///   the designer chose to be visible as an outline, typically a good deal
///   more than 2.2% of the size. [`MAX_DETECT_STROKE_RATIO`] is the cut.
/// - **Faux italic** — the text matrix has a non-zero shear term `c`,
///   normalized against `a` so the test is scale-independent, while the
///   font's own name does **not** claim Italic. A font that already says
///   Italic and *also* carries a shear is a deliberate extra lean, not a
///   synthesis, and is reported as not-synthesized.
///
/// Both halves are inferences and are labelled as such wherever they surface;
/// under rule 4 a detected synthesis is a *hint the operator can see and
/// override*, never a fact pdfcer acts on silently.
///
/// `render_mode` is `Tr`, `stroke_width` is the user-space `w`,
/// `rendered_size` is the effective painted size, and `tm` is the run's text
/// matrix.
///
/// # Examples
///
/// ```
/// use pdfcer_core::text_edit::synth::{StyleSynthesis, detect};
///
/// // A run pdfcer faux-bolded: mode 2, a 0.264 pt stroke on 12 pt Helvetica.
/// assert_eq!(
///     detect("Helvetica", 2, 0.264, 12.0, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
///     StyleSynthesis::Bold
/// );
///
/// // The same emission on a face that IS Bold is not a synthesis — it is
/// // an outlined bold, which is a design choice, not a fallback.
/// assert_eq!(
///     detect("Helvetica-Bold", 2, 0.264, 12.0, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
///     StyleSynthesis::None
/// );
///
/// // A sheared matrix on a non-italic face is a faux oblique.
/// assert_eq!(
///     detect("Helvetica", 0, 0.0, 12.0, [1.0, 0.0, 0.2126, 1.0, 0.0, 0.0]),
///     StyleSynthesis::Italic
/// );
/// ```
#[must_use]
pub fn detect(
    base_font: &str,
    render_mode: i64,
    stroke_width: f64,
    rendered_size: f64,
    tm: [f64; 6],
) -> StyleSynthesis {
    // Table 106: modes 1, 2, 5 and 6 stroke. A synthesized bold is
    // specifically mode 2 (fill AND stroke) — a pure stroke (mode 1) is an
    // outline effect, not a weight.
    let strokes_and_fills = render_mode == 2 || render_mode == 6;
    let thin = rendered_size.abs() > f64::EPSILON
        && (stroke_width / rendered_size).abs() <= MAX_DETECT_STROKE_RATIO;
    let bold = strokes_and_fills && thin && !name_claims_bold(base_font);

    // Normalize the shear against the matrix's own horizontal scale so the
    // test does not depend on the run's size: `c/a` IS tan θ for an
    // unrotated run, whatever `a` is.
    let sheared = if tm[0].abs() > f64::EPSILON {
        (tm[2] / tm[0]).abs() >= MIN_DETECT_SHEAR
    } else {
        false
    };
    let italic = sheared && !name_claims_italic(base_font);

    StyleSynthesis::new(bold, italic)
}

/// The largest stroke-to-size ratio [`detect`] will read as a synthesized
/// weight rather than as a deliberate outline effect.
///
/// Set at twice [`BOLD_STROKE_RATIO`], which gives pdfcer's own output ample
/// headroom (it emits exactly the ratio) while still excluding the outlined
/// display type that motivates the distinction — a visible outline is
/// typically 5–10% of the size, not 4.4%.
pub const MAX_DETECT_STROKE_RATIO: f64 = BOLD_STROKE_RATIO * 2.0;

/// The smallest normalized `Tm` shear [`detect`] will read as an oblique.
///
/// Well below pdfcer's own [`OBLIQUE_TAN`] so its output is always detected,
/// and well above the rounding noise a producer's matrix might carry (a
/// `c` of 1e-6 from an accumulated rotation is not an italic).
pub const MIN_DETECT_SHEAR: f64 = 0.02;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)]
mod tests {
    use super::*;

    /// The shear must be a genuine premultiplication, not a field poke: for
    /// a non-identity `Tm` the `d` term changes too.
    #[test]
    fn shear_premultiplies_rather_than_assigning() {
        // A run at 2× horizontal scale, rotated 90° (a = 0, b = 1, c = -1,
        // d = 0), which is the case a naive `tm[2] = TAN` gets wrong.
        let rotated = [0.0, 1.0, -1.0, 0.0, 10.0, 20.0];
        let s = shear_into(rotated);
        assert_eq!(s[0], 0.0);
        assert_eq!(s[1], 1.0);
        // c' = tanθ·a + c = tanθ·0 + (−1) = −1  (unchanged here)
        assert_eq!(s[2], -1.0);
        // d' = tanθ·b + d = tanθ·1 + 0 = tanθ   (this is the term a naive
        // implementation would have left at 0, losing the lean entirely)
        assert_eq!(s[3], OBLIQUE_TAN);
        assert_eq!((s[4], s[5]), (10.0, 20.0), "a shear fixes the origin");
    }

    /// The shear must not change the derived stroke width: a synthetic bold
    /// italic must weigh the same as a synthetic bold.
    #[test]
    fn a_shear_does_not_change_the_matrix_scale() {
        let upright = matrix_scale([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let sheared = matrix_scale(shear_into([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]));
        assert!((upright - sheared).abs() < 1e-12);
        assert_eq!(upright, 1.0);
    }

    #[test]
    fn stroke_width_tracks_the_rendered_size_not_a_constant() {
        let small = bold_stroke_width(10.0, 1.0, 1.0);
        let large = bold_stroke_width(72.0, 1.0, 1.0);
        assert!((large / small - 7.2).abs() < 1e-9, "linear in the size");
        // And it is in USER space: a 2× CTM doubles it.
        assert!((bold_stroke_width(10.0, 1.0, 2.0) - 2.0 * small).abs() < 1e-12);
    }

    /// The whole persistence story in one test: what pdfcer emits is what
    /// pdfcer detects, with no marker in between.
    #[test]
    fn pdfcer_own_emission_round_trips_through_detection() {
        let size = 14.0;
        let w = bold_stroke_width(size, 1.0, 1.0);
        let tm = shear_into([1.0, 0.0, 0.0, 1.0, 72.0, 700.0]);
        assert_eq!(
            detect("Helvetica", 2, w, size, tm),
            StyleSynthesis::BoldItalic
        );
    }

    /// The detector must recognize ANOTHER producer's faux bold — the
    /// capability decision 019 §3.6 calls "a genuine, cheap lead".
    #[test]
    fn another_producers_faux_bold_is_detected_too() {
        // A stroke ratio of 1.5% — not pdfcer's 2.2%, but the same technique.
        assert_eq!(
            detect(
                "ABCDEF+Arial",
                2,
                0.15,
                10.0,
                [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
            ),
            StyleSynthesis::Bold,
            "a subset-tagged non-bold face stroked in mode 2 is faux bold"
        );
    }

    /// The two false-positive guards, which are what make the detector worth
    /// surfacing at all.
    #[test]
    fn deliberate_outline_type_and_real_faces_are_not_reported_as_synthetic() {
        // 1. An outlined display heading: mode 2, but a FAT stroke.
        assert_eq!(
            detect("Helvetica", 2, 3.0, 24.0, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            StyleSynthesis::None,
            "a 12.5%-of-size outline is a design choice, not a faux weight"
        );
        // 2. A real italic face that also carries a lean.
        assert_eq!(
            detect(
                "Times-Italic",
                0,
                0.0,
                12.0,
                [1.0, 0.0, 0.25, 1.0, 0.0, 0.0]
            ),
            StyleSynthesis::None,
            "a face that says Italic is not being faked"
        );
        // 3. Matrix noise is not an oblique.
        assert_eq!(
            detect("Helvetica", 0, 0.0, 12.0, [1.0, 0.0, 1e-6, 1.0, 0.0, 0.0]),
            StyleSynthesis::None
        );
    }

    /// The one asymmetry decision 019 §3.6 permits, asserted to be an
    /// ordering difference and nothing more.
    #[test]
    fn the_two_paths_differ_only_in_remedy_order() {
        let add = SynthesisPath::AddText.remedy_order();
        let edit = SynthesisPath::InPlaceEdit.remedy_order();
        assert_ne!(add[0], edit[0], "the FIRST offer differs");
        // Both paths offer both remedies — neither withholds one.
        assert_eq!(add.len(), edit.len());
        assert!(add.iter().all(|r| !r.is_empty()));
        assert!(edit.iter().all(|r| !r.is_empty()));
    }

    /// ★★ **The disclosure must NOT claim a real face is absent when one was
    /// passed over.**
    ///
    /// Before decision 106 this sentence was unconditional: *"no real Bold
    /// face resolves for X on this page, so pdfcer cannot make this change with
    /// a genuine typeface."* Under [`crate::settings::StylePolicy::Auto`] it
    /// began shipping in reports that ALSO named the exact face it claimed did
    /// not exist — two disclosures, one report, flatly contradicting.
    ///
    /// Note the direction: it **understated** what pdfcer could do. Rule 4 is
    /// usually invoked against a claim that flatters the software, and a claim
    /// that a capability is missing sends the operator to a worse remedy with
    /// nothing failing.
    #[test]
    fn a_passed_over_face_is_named_rather_than_denied() {
        let offer = SynthesisOffer {
            synthesis: StyleSynthesis::Bold,
            base_font: "Times-Roman".to_owned(),
            path: SynthesisPath::InPlaceEdit,
            passed_over: Some(
                "a REAL bold face is available on this page as 'Times-Bold'.".to_owned(),
            ),
        };
        let d = offer.disclosure();
        assert!(
            d.contains("Times-Bold"),
            "the face that was passed over must be NAMED: {d}"
        );
        assert!(
            !d.contains("no real"),
            "★ and the sentence must not also assert that none resolves: {d}"
        );
        assert!(
            !d.contains("cannot make this change with a genuine typeface"),
            "★★ nor that pdfcer is incapable of it -- it demonstrably is not: {d}"
        );
        assert!(
            d.contains("refuse"),
            "and it should say how to be stopped instead: {d}"
        );
    }

    /// The disclosure must name the font, name what was applied, and say it
    /// is a fallback.
    #[test]
    fn the_disclosure_names_the_font_the_style_and_the_fallback_posture() {
        let offer = SynthesisOffer {
            synthesis: StyleSynthesis::Bold,
            base_font: "Calibri".to_owned(),
            path: SynthesisPath::InPlaceEdit,
            // Nothing was passed over: this is the arm that says a real face
            // does not resolve, which is TRUE only in this case.
            passed_over: None,
        };
        let d = offer.disclosure();
        assert!(d.contains("Calibri"), "{d}");
        assert!(d.contains("synthetic bold"), "{d}");
        assert!(d.contains("FALLBACK"), "{d}");
        // ★ The old assertion here required the sentence to contain
        // "never applied silently". That was true of pdfcer and is not any
        // more: decision 106 made `StylePolicy::Auto` the default, and it IS
        // silent and IS a global preference. The claim was removed from the
        // disclosure, so requiring it would pin a sentence pdfcer is no longer
        // entitled to say.
        //
        // Replaced with the half that survived and is what the operator
        // actually needs from this arm: the letterforms are the regular
        // face's, thickened.
        assert!(
            d.contains("letterforms thickened"),
            "the disclosure must still say what a synthesised weight IS: {d}"
        );
        assert!(
            !d.contains("never applied silently"),
            "★ and must NOT still claim it -- Auto is silent by default: {d}"
        );
    }

    #[test]
    fn style_flags_compose() {
        assert_eq!(StyleSynthesis::new(false, false), StyleSynthesis::None);
        assert!(StyleSynthesis::BoldItalic.bold());
        assert!(StyleSynthesis::BoldItalic.italic());
        assert!(!StyleSynthesis::Bold.italic());
        assert!(StyleSynthesis::None.is_none());
    }
}
