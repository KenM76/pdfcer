//! # The extraction walk over a page's content stream
//!
//! A single pass over [`crate::content::ContentStream`]'s lossless token
//! stream that maintains exactly the state extraction needs and nothing
//! else:
//!
//! | State | Operators | Clause |
//! |---|---|---|
//! | CTM | `q` `Q` `cm` | §8.4.4 |
//! | text object matrices `Tm`/`Tlm` | `BT` `ET` `Td` `TD` `Tm` `T*` | §9.4.2 |
//! | text state | `Tf` `Tc` `Tw` `Tz` `TL` `Ts` `Tr` | §9.3 |
//! | marked-content stack | `BMC` `BDC` `EMC` | §14.6 |
//! | form nesting | `Do` | §8.10.1 |
//!
//! Everything else — paths, colour, shading, images, `gs`, clipping — is
//! ignored by construction. This is *not* a stripped-down renderer: a
//! renderer needs colour to decide what a glyph looks like, and
//! extraction explicitly does not care whether a glyph is visible
//! (§14.8.2.2.3 item 3, a `shall`: "page content shall be considered to
//! include all text and illustrations in their entirety, regardless of
//! whether they are visible").
//!
//! ## The marked-content stack is the load-bearing part
//!
//! Four §14.6 tags change what comes out, and all four are attached by
//! nesting rather than by adjacency, so a stack is not an optimization:
//!
//! - **`/Artifact`** (§14.8.2.2.2) — classify the enclosed content;
//!   never drop it silently (A3).
//! - **`/Span` with `/ActualText`** (§14.9.4) — *replace* the enclosed
//!   content's characters.
//! - **`/ReversedChars`** (§14.8.2.3.3) — the enclosed show strings hold
//!   their characters in reverse of page content order.
//! - **`/TagSuspect`** (§14.8.2.3.1) — the producer disclaims its own
//!   ordering for the enclosed region.
//!
//! §14.6.2's rule for the `BDC` property-list operand is about
//! *indirectness*, not size: an all-direct dictionary may be inline, but
//! if any value is an indirect reference the list "shall be defined as a
//! named resource in the `Properties` subdictionary of the **current**
//! resource dictionary". Current, not the page's — which is why the
//! resource dictionary travels with the walk into form XObjects.
//!
//! ## Unbalanced marked content
//!
//! §14.6 N1: the standard states the nesting rules as *writer*
//! constraints and gives no reader-side recovery for an unbalanced `EMC`
//! or a `BMC` left open at end of stream. pdfcer ignores an `EMC` with an
//! empty stack and lets an unclosed sequence expire at end of stream —
//! the two choices that cannot lose content. Both are pdfcer policy.
//!
//! ## The axis-aligned assumption in the geometry
//!
//! Glyph origins are computed exactly, through the full §9.4.4 text
//! rendering matrix, and are therefore correct under any transform. The
//! *derived* line/word segmentation in [`super::layout`] then compares
//! those origins on the x and y axes, which assumes text runs left to
//! right along user-space x. Rotated text extracts with correct
//! characters and correct positions but over-produces derived line
//! breaks. This is a limitation of the derived layer only; it cannot
//! affect a sourced character, and
//! [`super::ExtractedText::sourced_text`] is unaffected by it.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::content::{ContentError, ContentStream, ContentTokenKind, Operation};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::page_tree::{Page, Rect};
use crate::settings::ActualTextPrecedence;
use crate::span::ByteSpan;
use crate::text_state::{AmbientTextState, TextStateParam};
use crate::textstring::decode_text_string;
use crate::view::DocumentView;

use super::font::{ExtractFont, FontNote, LadderRung, Rung3Gap};
use super::{
    ArtifactKind, ContentStreamRef, ExtractOptions, GlyphProvenance, TextColor, TextDiagnostics,
};

/// One thing the walk produced, before derived whitespace is inserted.
#[derive(Debug, Clone)]
pub(super) enum Item {
    /// One shown glyph and the characters it decoded to.
    Glyph(GlyphItem),
    /// An `/ActualText` replacement covering a marked-content sequence.
    Replacement(ReplacementItem),
}

/// One shown glyph.
#[derive(Debug, Clone)]
pub(super) struct GlyphItem {
    /// The characters this code produced (possibly several, possibly
    /// U+FFFD if the ladder failed).
    pub chars: String,
    /// The character code, as segmented from the show string.
    pub code: u32,
    /// Which ladder rung produced `chars`.
    pub rung: LadderRung,
    /// Origin x in default user space.
    pub x: f32,
    /// Origin y in default user space.
    pub y: f32,
    /// Advance in default user space, as a **length** along
    /// [`Self::direction`] — not necessarily along the page's x axis.
    pub advance: f32,
    /// Effective font size in default user space.
    pub size: f32,
    /// Unit vector of the writing direction in default user space (the
    /// normalised x basis of the text rendering matrix). `(1.0, 0.0)`
    /// for ordinary horizontal text and for a degenerate matrix.
    pub direction: (f32, f32),
    /// Text rendering mode 3 or 7.
    pub invisible: bool,
    /// Enclosing `/Artifact` classification, if any.
    pub artifact: Option<ArtifactKind>,
    /// Enclosing `/MCID`, if any.
    pub mcid: Option<u32>,
    /// Source-operator identity + text state, captured only when
    /// [`ExtractOptions::capture_provenance`] is set (otherwise `None`).
    pub provenance: Option<GlyphProvenance>,
}

/// An `/ActualText` replacement.
#[derive(Debug, Clone)]
pub(super) struct ReplacementItem {
    /// The decoded replacement text (a §7.9.2.2 text string).
    pub text: String,
    /// Enclosing `/Artifact` classification, if any.
    pub artifact: Option<ArtifactKind>,
    /// Enclosing `/MCID`, if any.
    pub mcid: Option<u32>,
    /// Bounding box of the glyphs the replacement covered, if it covered
    /// any. This is the *only* positional information an `/ActualText`
    /// run can carry — §14.9.4 N4 makes per-character correspondence
    /// impossible.
    pub bbox: Option<Rect>,
}

/// A 2-D affine transform in PDF's row-vector convention:
/// `[a b 0 / c d 0 / e f 1]` (§8.3.3).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Matrix {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Matrix {
    /// The identity transform.
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// `self × other`, in PDF's order — `self` applies first.
    fn mul(self, other: Self) -> Self {
        Self {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }

    /// A pure translation.
    const fn translate(tx: f32, ty: f32) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: tx,
            f: ty,
        }
    }

    /// The magnitude of the transformed unit x vector — how much one
    /// unit of horizontal text space measures in user space.
    fn x_scale(self) -> f32 {
        self.a.hypot(self.b)
    }

    /// The magnitude of the transformed unit y vector.
    fn y_scale(self) -> f32 {
        self.c.hypot(self.d)
    }

    /// The **direction** of the transformed unit x vector, as a unit
    /// vector — the half of the x basis that [`Self::x_scale`] throws
    /// away.
    ///
    /// # Why this exists (`Pass 139.0`)
    ///
    /// [`Self::x_scale`] and [`Self::y_scale`] reduce the two basis
    /// vectors of the text rendering matrix (§9.4.4) to their
    /// **magnitudes**, and every consumer downstream then has no choice
    /// but to assume the direction was `(1, 0)`. That assumption is true
    /// of virtually every word-processor page and **false of every CAD
    /// title block**, which stamps its source path with
    /// `Tm = [0 1 -1 0 e f]` — ordinary horizontal-mode text placed by a
    /// rotated matrix, not §9.7.4.3 vertical writing mode.
    ///
    /// Publishing the direction beside the magnitude is what lets
    /// [`super::layout`] measure its gap thresholds along the line's own
    /// axis instead of the page's. See
    /// [`ExtractedGlyph::direction`](super::ExtractedGlyph::direction).
    ///
    /// # The degenerate case
    ///
    /// A zero-length x basis (`Tf 0`, or a singular CTM) has no
    /// direction. `(1.0, 0.0)` is returned so that every ratio
    /// comparison downstream stays finite and reduces to the historical
    /// page-axis behaviour, rather than propagating a `NaN` into a
    /// caller's hit test.
    fn x_unit(self) -> (f32, f32) {
        let len = self.a.hypot(self.b);
        if len.is_finite() && len > 1e-9 {
            (self.a / len, self.b / len)
        } else {
            (1.0, 0.0)
        }
    }

    /// This matrix as PDF's 6-element `[a b c d e f]` row-vector array
    /// (§8.3.3) — the form [`GlyphProvenance`] carries for the surgery.
    const fn to_array(self) -> [f32; 6] {
        [self.a, self.b, self.c, self.d, self.e, self.f]
    }
}

/// §9.3's text state parameters, plus the current font.
///
/// # Pass 19.0: the six numeric parameters moved to the shared model
///
/// `Tc`/`Tw`/`Tz`/`TL`/`Ts`/`Tr` used to be six private `f32`/`i64` fields
/// here, duplicated by two more private trackers elsewhere in the crate
/// (see [`crate::text_state`]'s module docs for the table). They are now
/// one [`AmbientTextState`], which additionally records **where each value
/// came from** so a later authoring pass can restore it byte-faithfully.
///
/// The `Arc` is not premature: with
/// [`ExtractOptions::capture_provenance`](super::ExtractOptions::capture_provenance)
/// on, this state is published onto **every glyph**, and a `q` clones the
/// whole `TextState` onto the save stack. Sharing makes both a refcount
/// bump instead of six copies plus however many raw-operand allocations.
///
/// `font` and `size` deliberately stay here rather than joining the shared
/// type — see [`crate::text_state`]'s "which parameters, and why these
/// six": `Tfs` is narrowed to `f32` on this path because provenance
/// publishes `f32`, and moving it to the shared `f64` model would change
/// the precision of the §9.4.4 advance and therefore move published glyph
/// positions.
#[derive(Clone)]
struct TextState {
    font: Option<Rc<ExtractFont>>,
    /// `Tfs` — font size (§9.3.1). No default: showing text with no
    /// `Tf` is malformed, and pdfcer treats the size as 0 rather than
    /// inventing one.
    size: f32,
    /// The six shared §9.3 parameters with their restore provenance.
    /// Graphics state, so saved/restored by `q`/`Q` via this struct's
    /// `Clone` (§8.4.2).
    ambient: Arc<AmbientTextState>,
    /// The current *fill* colour (§8.6.8), captured for provenance only.
    /// Part of the graphics state, so it is saved/restored by `q`/`Q` via
    /// this struct's `Clone`. `None` = unset, i.e. the §8.6.8 default black.
    /// Set only by the device operators `g`/`rg`/`k`; a colour set in a
    /// named space is recorded as [`TextColor::Other`] (see [`TextColor`]).
    fill_color: Option<TextColor>,
}

impl TextState {
    /// `Tc` in the `f32` domain this walk has always computed in.
    fn char_spacing(&self) -> f32 {
        self.ambient.char_spacing.value as f32
    }

    /// `Tw` in the `f32` domain this walk has always computed in.
    fn word_spacing(&self) -> f32 {
        self.ambient.word_spacing.value as f32
    }

    /// `Th` = `Tz` ÷ 100 (§9.3.4).
    ///
    /// The narrowing happens **before** the division, exactly as it did
    /// when `h_scale` was an `f32` field assigned `v / 100.0` from an
    /// `f32` operand. `(v as f32) / 100.0f32` and `(v / 100.0f64) as f32`
    /// are not bit-identical for every operand, and this walk's outputs
    /// (glyph `x`/`y`/`advance`) are published `f32` — so the order is
    /// preserved deliberately, not incidentally.
    fn h_scale(&self) -> f32 {
        (self.ambient.h_scale.value as f32) / 100.0
    }

    /// `TL` in the `f32` domain this walk has always computed in.
    fn leading(&self) -> f32 {
        self.ambient.leading.value as f32
    }

    /// `Ts` in the `f32` domain this walk has always computed in.
    fn rise(&self) -> f32 {
        self.ambient.rise.value as f32
    }

    /// `Tmode` (§9.3.6, Table 106).
    fn render_mode(&self) -> i64 {
        self.ambient.render_mode.value as i64
    }

    /// Mutable access to the shared ambient state, cloning it out of the
    /// `Arc` only when it is actually shared (copy-on-write).
    fn ambient_mut(&mut self) -> &mut AmbientTextState {
        Arc::make_mut(&mut self.ambient)
    }
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            font: None,
            size: 0.0,
            ambient: Arc::new(AmbientTextState::initial()),
            fill_color: None,
        }
    }
}

/// One level of the §14.6 marked-content stack.
#[derive(Debug, Clone)]
struct MarkedLevel {
    artifact: Option<ArtifactKind>,
    mcid: Option<u32>,
    reversed_chars: bool,
    /// Present when this level is a `/Span` carrying `/ActualText`, and
    /// this level is the OUTERMOST such level (see the nesting policy in
    /// [`Walk::begin_marked`]).
    actual_text: Option<String>,
    /// Bounding box accumulated over the glyphs this level suppressed.
    covered: Option<Rect>,
}

/// The whole walk state.
struct Walk<'a> {
    doc: &'a DocumentView<'a>,
    options: &'a ExtractOptions,
    items: Vec<Item>,
    diagnostics: TextDiagnostics,

    ctm: Matrix,
    ctm_stack: Vec<Matrix>,
    ts: TextState,
    ts_stack: Vec<TextState>,
    tm: Matrix,
    tlm: Matrix,

    marked: Vec<MarkedLevel>,
    /// Font cache keyed by the resource name **plus** the resource
    /// dictionary's identity, because `/F1` inside a form XObject is a
    /// different font from `/F1` on the page (§8.10.1's resource
    /// switching — a correctness requirement, not an optimization).
    fonts: HashMap<(usize, Vec<u8>), Rc<ExtractFont>>,
    /// Fonts already reported, so a font used on 400 pages produces one
    /// diagnostic and one increment.
    fonts_seen: Vec<Vec<u8>>,
    depth: usize,
    /// XObject object numbers currently executing, for the §8.10.1 cycle
    /// guard. Keyed on object number, not resource name: the same stream
    /// can be reached under different names.
    active_xobjects: Vec<u32>,

    // --- provenance capture (only meaningful when the option is set) ---
    /// Which decoded buffer the current operator spans index: the page's
    /// own concatenated content, or the form XObject currently executing
    /// (§8.10.1). Saved/restored around every `Do`.
    stream_ref: ContentStreamRef,
    /// Byte span of the show operator currently being interpreted, in the
    /// buffer named by [`Self::stream_ref`]. Set for `Tj`/`'`/`"`/`TJ`
    /// before decoding, so [`Walk::show_code`] can attribute each glyph to
    /// its operator.
    cur_op_span: ByteSpan,
    /// The `/F1`-style resource name of the font selected by the most
    /// recent `Tf` (§9.3.1), as raw name bytes — for provenance only.
    cur_font_resource: Option<Vec<u8>>,
}

/// Walk one page and return its raw items plus diagnostics.
///
/// # Pass 17.1: the caller now chooses the revision
///
/// This used to take `&Document` and open with a comment reading *"BASE
/// READ … making it session-aware is a separate, deliberate change … NOT
/// part of Pass 17.0."* That change is this Pass. The walk now takes a
/// [`DocumentView`], so **the caller** decides whether the text being
/// extracted is the file as loaded (`document.view()` — the CLI, search,
/// the redaction census) or the file as the operator currently has it
/// (`session.view()` — the in-place text-edit tool and Copy Text).
///
/// Nothing about the walk itself changed: every `doc.resolve(…)` here is
/// the identical [`ObjectGraph`](crate::graph::ObjectGraph) method it
/// already called, and the one place that needed stream BYTES (the form
/// XObject payload) now asks the view for them so an R45-staged span
/// resolves instead of falling off the end of the base buffer.
pub(super) fn walk_page(
    doc: &DocumentView<'_>,
    page: &Page,
    options: &ExtractOptions,
) -> Result<(Vec<Item>, TextDiagnostics), ContentError> {
    let stream = ContentStream::from_page(doc, page)?;
    let mut walk = Walk {
        doc,
        options,
        items: Vec::new(),
        diagnostics: TextDiagnostics::default(),
        ctm: Matrix::IDENTITY,
        ctm_stack: Vec::new(),
        ts: TextState::default(),
        ts_stack: Vec::new(),
        tm: Matrix::IDENTITY,
        tlm: Matrix::IDENTITY,
        marked: Vec::new(),
        fonts: HashMap::new(),
        fonts_seen: Vec::new(),
        depth: 0,
        active_xobjects: Vec::new(),
        stream_ref: ContentStreamRef::Page,
        cur_op_span: ByteSpan::new(0, 0),
        cur_font_resource: None,
    };
    walk.run(&stream, &page.resources);
    Ok((walk.items, walk.diagnostics))
}

impl Walk<'_> {
    /// Execute one content stream against one resource dictionary.
    fn run(&mut self, stream: &ContentStream, resources: &Dict) {
        for op in stream.operations() {
            let Some(name) = op.operator_name(&stream.buf) else {
                // An inline image: a graphics object with no text.
                continue;
            };
            self.operator(name, &op, &stream.buf, resources);
        }
        // §14.6 N1: sequences left open at end of stream simply expire.
        // Any /ActualText they carried still has to be emitted, or the
        // replacement text would be lost along with the glyphs it
        // suppressed.
        while !self.marked.is_empty() {
            self.end_marked();
        }
    }

    /// Dispatch one operator.
    fn operator(&mut self, name: &[u8], op: &Operation<'_>, buf: &[u8], resources: &Dict) {
        let nums = |count: usize| -> Vec<f32> { operand_numbers(op, count) };
        match name {
            // --- graphics state (§8.4.4) ---
            b"q" => {
                self.ctm_stack.push(self.ctm);
                self.ts_stack.push(self.ts.clone());
                // A hostile stream of `q`s must not grow the stacks
                // without bound; 256 is far past any real nesting and
                // matches the posture of the other structural guards.
                if self.ctm_stack.len() > 256 {
                    self.ctm_stack.remove(0);
                    self.ts_stack.remove(0);
                }
            }
            b"Q" => {
                if let Some(m) = self.ctm_stack.pop() {
                    self.ctm = m;
                }
                if let Some(ts) = self.ts_stack.pop() {
                    self.ts = ts;
                }
            }
            b"cm" => {
                let v = nums(6);
                if let [a, b, c, d, e, f] = v[..] {
                    self.ctm = Matrix { a, b, c, d, e, f }.mul(self.ctm);
                }
            }

            // --- text object (§9.4.1) ---
            b"BT" => {
                self.tm = Matrix::IDENTITY;
                self.tlm = Matrix::IDENTITY;
            }
            b"ET" => {}

            // --- text state (§9.3) ---
            //
            // Pass 19.0: the six single-operand parameters (`Tc`/`Tw`/`Tz`/
            // `TL`/`Ts`/`Tr`) are no longer six hand-written arms here.
            // They go through the ONE shared update rule
            // (`AmbientTextState::apply_operator`), which additionally
            // captures each operator's raw bytes so a later authoring pass
            // can restore the ambient value byte-faithfully (R88 tier 2).
            // `Tf` keeps its own arm: it takes a NAME plus a number and
            // resolves a font resource, which is not a numeric parameter.
            b"Tf" => self.select_font(op, resources),
            b"Tc" | b"Tw" | b"Tz" | b"TL" | b"Ts" | b"Tr" => {
                let raw = op_bytes(op, buf).to_vec();
                let vals = operand_numbers_f64(op, 1);
                self.ts.ambient_mut().apply_operator(name, &vals, &raw);
            }

            // --- text positioning (§9.4.2) ---
            b"Td" => {
                if let [tx, ty] = nums(2)[..] {
                    self.next_line(tx, ty);
                }
            }
            b"TD" => {
                if let [tx, ty] = nums(2)[..] {
                    // "sets the leading parameter to -ty" (Table 108).
                    //
                    // Recorded as ObservedIndirect, NOT as Observed: `TD`
                    // genuinely sets `TL`, so claiming the leading is still
                    // at its Table 105 default would be the guess R88
                    // forbids — but this operator's bytes also move to the
                    // next line, so re-emitting them as a "restore" would
                    // displace every following glyph. The value is kept and
                    // restores by re-spelling as `-ty TL`.
                    self.ts.ambient_mut().set_indirect(
                        TextStateParam::Leading,
                        f64::from(-ty),
                        "TD",
                    );
                    self.next_line(tx, ty);
                }
            }
            b"Tm" => {
                let v = nums(6);
                if let [a, b, c, d, e, f] = v[..] {
                    self.tlm = Matrix { a, b, c, d, e, f };
                    self.tm = self.tlm;
                }
            }
            b"T*" => {
                let leading = self.ts.leading();
                self.next_line(0.0, -leading);
            }

            // --- text showing (§9.4.3, Table 109) ---
            // Each show operator records its own byte span (in the current
            // stream buffer) so every glyph it produces can be attributed
            // back to it for provenance. Inert when provenance capture is
            // off — the field is simply never read.
            //
            // CORRECTION (Pass 19.3, found by observing the running GUI):
            // this used to claim "Pass 14.1's surgery locates the operator by
            // EXACTLY this span". It does not, and could not: the span
            // recorded here is `op.operator.span`, the operator TOKEN alone
            // (`Tj`), while the authoring walk's `op_span` records the
            // operand-inclusive extent (`(hello) Tj`). The surgery's pinned
            // path compared for equality against the latter, so every request
            // pinned from this field failed to locate anything at all. The
            // comparison now accepts either convention — see
            // `text_edit::edit::pin_names_operator`. Do NOT "fix" this by
            // widening the span published here: it is a consumer-facing
            // field, it correctly names the operator, and the CLI prints it.
            b"Tj" => {
                self.cur_op_span = op.operator.span;
                if let Some(s) = last_string(op) {
                    self.show(&s);
                }
            }
            b"'" => {
                self.cur_op_span = op.operator.span;
                let leading = self.ts.leading();
                self.next_line(0.0, -leading);
                if let Some(s) = last_string(op) {
                    self.show(&s);
                }
            }
            b"\"" => {
                // `aw ac string "` — sets word and character spacing,
                // then behaves like `'`.
                self.cur_op_span = op.operator.span;
                // Table 109: `"` sets BOTH `Tw` and `Tc` before showing.
                // Routed through the shared update rule so the two
                // parameters are recorded as observed with the same
                // provenance discipline as a standalone `Tw`/`Tc` — see
                // `AmbientTextState::apply_operator`'s doc comment for why
                // the raw bytes of a `"` are not a usable restore.
                let raw = op_bytes(op, buf).to_vec();
                let v = operand_numbers_f64(op, 3);
                self.ts.ambient_mut().apply_operator(name, &v, &raw);
                let leading = self.ts.leading();
                self.next_line(0.0, -leading);
                if let Some(s) = last_string(op) {
                    self.show(&s);
                }
            }
            b"TJ" => {
                self.cur_op_span = op.operator.span;
                self.show_array(op);
            }

            // --- fill colour (§8.6.8), provenance only ---
            // Only the lowercase (fill) device operators are read; the
            // uppercase G/RG/K set the STROKE colour, which does not paint
            // text under the default rendering modes. A colour set through
            // sc/scn in a named space is left as the prior value or, once
            // any such operator is seen, marked Other — never decoded here.
            b"g" => {
                if let [gray] = nums(1)[..] {
                    self.ts.fill_color = Some(TextColor::Gray(gray));
                }
            }
            b"rg" => {
                if let [r, g, b] = nums(3)[..] {
                    self.ts.fill_color = Some(TextColor::Rgb(r, g, b));
                }
            }
            b"k" => {
                if let [c, m, y, kk] = nums(4)[..] {
                    self.ts.fill_color = Some(TextColor::Cmyk(c, m, y, kk));
                }
            }
            b"sc" | b"scn" => {
                // A fill colour in the current (possibly named) space.
                // pdfcer's read-only walk does not track the fill colour
                // space, so the value is recorded as present-but-unmodelled
                // rather than guessed (§8.6.8; see TextColor::Other).
                self.ts.fill_color = Some(TextColor::Other);
            }

            // --- marked content (§14.6) ---
            b"BMC" => {
                let tag = operand_name(op, 0).unwrap_or_default();
                self.begin_marked(&tag, None);
            }
            b"BDC" => {
                let tag = operand_name(op, 0).unwrap_or_default();
                let props = self.resolve_properties(op, resources);
                self.begin_marked(&tag, props.as_ref());
            }
            b"EMC" => self.end_marked(),

            // --- XObjects (§8.10) ---
            b"Do" => self.do_xobject(op, buf, resources),

            _ => {}
        }
    }

    /// `Td`: "move to the start of the next line, offset from the start
    /// of the current line by (tx, ty)" — `Tlm = translate × Tlm`, then
    /// `Tm = Tlm` (Table 108).
    fn next_line(&mut self, tx: f32, ty: f32) {
        self.tlm = Matrix::translate(tx, ty).mul(self.tlm);
        self.tm = self.tlm;
    }

    /// `Tf`: resolve the named font resource, with caching.
    fn select_font(&mut self, op: &Operation<'_>, resources: &Dict) {
        // `Tf` is `font size Tf` — a NAME then a number. `operand_numbers`
        // filters non-numeric operands out, so asking it for "the second
        // of two" would silently return the font size at index 0 on a
        // well-formed operator and nothing at all once the name is
        // dropped. Read the size from the last operand directly.
        if let Some(size) = op
            .operands
            .last()
            .and_then(operand_object)
            .and_then(Object::as_number)
        {
            self.ts.size = size as f32;
        }
        let Some(name) = operand_name(op, 0) else {
            return;
        };
        // The cache key must include the resource dictionary's identity:
        // `/F1` in a form's own /Resources is a different font from the
        // page's `/F1`, and conflating them paints — or here, extracts —
        // the wrong characters entirely.
        let key = (std::ptr::from_ref(resources) as usize, name.clone());
        // Record the resource name alongside the font, so provenance can
        // report which /Resources /Font key painted a glyph. Set only on a
        // successful selection: the not-found path below keeps the previous
        // font, so it must keep the previous resource name too.
        if let Some(font) = self.fonts.get(&key) {
            self.ts.font = Some(Rc::clone(font));
            self.cur_font_resource = Some(name);
            return;
        }
        let font_dict = self
            .doc
            .resolve(resources.get(b"Font").unwrap_or(&Object::Null))
            .as_dict()
            .and_then(|fonts| fonts.get(&name))
            .map(|o| self.doc.resolve(o))
            .and_then(Object::as_dict);
        let Some(font_dict) = font_dict else {
            // A `Tf` naming a resource that is not there: §7.8.3 makes
            // this malformed with no recovery. Keep the previous font
            // rather than silently dropping the text that follows.
            self.diagnostics.note(format!(
                "text: font resource /{} not found in the current /Resources — \
                 following text uses the previously selected font",
                String::from_utf8_lossy(&name)
            ));
            return;
        };
        let font = Rc::new(ExtractFont::resolve(self.doc, font_dict));
        self.report_font(&font, &key);
        self.fonts.insert(key, Rc::clone(&font));
        self.ts.font = Some(font);
        self.cur_font_resource = Some(name);
    }

    /// Turn a newly resolved font's [`FontNote`]s into counted, named
    /// diagnostics — once per distinct font, not once per `Tf`.
    ///
    /// # Why the de-duplication key is not simply `/BaseFont`
    ///
    /// It was, until `Pass 127.0`, and that silently **suppressed every
    /// diagnostic for every font the standard does not give a name to**.
    /// ISO 32000-1 Table 112 — the Type 3 font dictionary — has **no
    /// `/BaseFont` entry at all**; a conformant Type 3 font therefore
    /// resolves to an empty `base_font`, and so does any font whose
    /// `/BaseFont` is missing or malformed. Keyed on that empty string,
    /// every unnamed font on a page shared one slot.
    ///
    /// ★ **A/B'd rather than reasoned about, and the measurement was worse
    /// than the prediction.** The expectation written first was that N
    /// unnamed dead ends would report as `1`. Measured against
    /// `tounicode_gate.pdf` on the pre-fix code, the counter reported
    /// **`0`** — because the first unnamed font to be resolved on that page
    /// is `/TA`, which has a `/ToUnicode` and therefore no note to emit. It
    /// claimed the empty key, and `/TB` and `/TC` were then skipped before
    /// their notes were ever read. So the old key did not merely
    /// under-count coincident fonts; **one clean unnamed font silenced
    /// every unnamed font behind it**, and the document with two dead ends
    /// reported none at all.
    ///
    /// So a named font still de-duplicates by name — that is the property
    /// worth having, and it is why a font used by both the page and a form
    /// XObject reports once. An **unnamed** font falls back to the
    /// resource identity it was selected through, which is exactly the
    /// cache key `select_font` already computes and is distinct per
    /// `(resource dictionary, resource name)` pair.
    fn report_font(&mut self, font: &ExtractFont, resource_key: &(usize, Vec<u8>)) {
        let key = if font.base_font.is_empty() {
            let mut k = b"r:".to_vec();
            k.extend_from_slice(&resource_key.0.to_le_bytes());
            k.push(b'/');
            k.extend_from_slice(&resource_key.1);
            k
        } else {
            let mut k = b"n:".to_vec();
            k.extend_from_slice(font.base_font.as_bytes());
            k
        };
        if self.fonts_seen.contains(&key) {
            return;
        }
        self.fonts_seen.push(key);
        // A `/BaseFont`-less font — every conformant Type 3 font, per
        // Table 112 — is named by the RESOURCE KEY the content stream
        // selected it with (`/T3`), because that is the only handle the
        // operator has: it is what `Tf` says, what `list-fonts` shows, and
        // what a hex editor finds. `<unnamed>` was accurate and useless,
        // and became actively misleading once more than one such font
        // could be reported per page.
        let resource_name = String::from_utf8_lossy(&resource_key.1);
        let name = if font.base_font.is_empty() {
            format!("/{resource_name}")
        } else {
            font.base_font.clone()
        };
        let name = name.as_str();
        for note in &font.notes {
            match note {
                FontNote::Rung3(Rung3Gap::IdentityNoToUnicode) => {
                    self.diagnostics.identity_fonts_without_to_unicode += 1;
                    self.diagnostics.note(format!(
                        "text: font {name} is Identity-H/Adobe-Identity-0 with NO /ToUnicode — \
                         ISO 32000-1 §9.10.2 excludes it from every ladder rung, so no Unicode is \
                         recoverable for it"
                    ));
                }
                FontNote::Rung3(Rung3Gap::Ucs2NotBundled { cmap_name }) => {
                    self.diagnostics.ucs2_cmaps_unavailable += 1;
                    self.diagnostics.note(format!(
                        "text: font {name} uses a known Adobe character collection, but the \
                         {cmap_name} CID-to-Unicode CMap (§9.10.2 rung 3 step d) is an Adobe \
                         resource file pdfcer does not bundle"
                    ));
                }
                FontNote::Rung3(Rung3Gap::PredefinedCmapNotBundled { cmap_name }) => {
                    self.diagnostics.predefined_cmaps_unavailable += 1;
                    self.diagnostics.note(format!(
                        "text: font {name} uses predefined CMap {cmap_name} — pdfcer bundles \
                         neither its codespace nor a CID-to-Unicode mapping; 2-byte code \
                         segmentation assumed"
                    ));
                }
                FontNote::BuiltinEncodingUnreadable => {
                    self.diagnostics.note(format!(
                        "text: font {name} relies on its embedded program's built-in encoding, \
                         which pdfcer-core cannot read; StandardEncoding assumed and any recovered \
                         characters are counted as the glyph-name extension, not as §9.10.2 rung 2"
                    ));
                }
                FontNote::Type3NoToUnicode => {
                    self.diagnostics.type3_fonts_without_to_unicode += 1;
                    self.diagnostics.note(format!(
                        "text: font {name} is a Type 3 font with NO /ToUnicode — its glyphs are \
                         content streams named by arbitrary /CharProcs keys (ISO 32000-1 §9.6.5), \
                         so §9.10.2 leaves no sourced route to Unicode: text set in it RENDERS \
                         correctly but cannot be searched, copied or extracted. Acrobat is gated \
                         on the same entry"
                    ));
                }
                FontNote::UnknownSubtype => {
                    self.diagnostics.note(format!(
                        "text: font {name} has an absent or unrecognized /Subtype (Table 110); \
                         treated as a simple font"
                    ));
                }
                FontNote::ToUnicodeUnusable => {
                    self.diagnostics.note(format!(
                        "text: font {name} has a /ToUnicode entry that could not be decoded or \
                         yielded no mappings"
                    ));
                }
                FontNote::CodespaceWidthConflict {
                    font: f,
                    to_unicode,
                } => {
                    self.diagnostics.note(format!(
                        "text: font {name} declares a {to_unicode}-byte /ToUnicode codespace but \
                         its encoding implies {f}-byte codes (§9.10.3 requires consistency and \
                         states no recovery); segmented by the font's encoding"
                    ));
                }
                FontNote::WidthsEstimated => {
                    self.diagnostics.fonts_with_estimated_widths += 1;
                    self.diagnostics.note(format!(
                        "text: font {name} has no /Widths and is not a standard-14 face — advance \
                         widths are ESTIMATED, so derived word and line breaks near it are less \
                         reliable (characters are unaffected)"
                    ));
                }
            }
        }
    }

    /// `Tj` / `'` / `"`: show one string.
    fn show(&mut self, string: &[u8]) {
        let Some(font) = self.ts.font.clone() else {
            // Showing text with no font selected is malformed (§9.4.1's
            // "Tf shall precede"). There is nothing to decode with, so
            // the codes are counted as failures rather than dropped —
            // "we saw N characters we could not read" is the honest
            // report, and silence would hide the text entirely.
            self.diagnostics.codes_total += string.len() as u64;
            self.diagnostics.ladder_failures += string.len() as u64;
            self.diagnostics.note(
                "text: a show operator appeared with no font selected (§9.4.1 requires Tf first); \
                 those character codes are counted as unresolvable"
                    .to_string(),
            );
            return;
        };
        let start = self.items.len();
        for code in font.codes(string) {
            self.show_code(&font, code.value, code.word_spacing_applies);
        }
        // §14.8.2.3.3: "only the individual characters within each string
        // shall be reversed; the strings themselves shall be in natural
        // reading order." Per-string, not per-sequence — reversing the
        // sequence instead would reverse the word order of the whole run.
        if self.reversed_chars()
            && let Some(slice) = self.items.get_mut(start..)
        {
            slice.reverse();
        }
    }

    /// `TJ`: an array of strings and number adjustments (Table 109).
    ///
    /// The adjustment "shall be subtracted from the current horizontal
    /// coordinate", expressed in **thousandths of a unit of text space**
    /// — and it is applied *before* the next glyph, which is why it is
    /// carried into [`Walk::show_code`] rather than applied as a
    /// standalone translation.
    fn show_array(&mut self, op: &Operation<'_>) {
        let Some(Object::Array(items)) = op.operands.last().and_then(operand_object) else {
            return;
        };
        let items = items.clone();
        let Some(font) = self.ts.font.clone() else {
            for item in &items {
                if let Object::String(s) = item {
                    self.diagnostics.codes_total += s.len() as u64;
                    self.diagnostics.ladder_failures += s.len() as u64;
                }
            }
            return;
        };
        let start = self.items.len();
        for item in &items {
            match item {
                Object::String(s) => {
                    for code in font.codes(s) {
                        self.show_code(&font, code.value, code.word_spacing_applies);
                    }
                }
                other => {
                    if let Some(v) = other.as_number() {
                        // Table 109: "the amount shall be subtracted from
                        // the current horizontal coordinate", scaled by
                        // the font size and Tz. Applied NOW, so the next
                        // glyph is placed at the shifted origin.
                        let tx = -(v as f32) / 1000.0 * self.ts.size * self.ts.h_scale();
                        self.tm = Matrix::translate(tx, 0.0).mul(self.tm);
                    }
                }
            }
        }
        if self.reversed_chars()
            && let Some(slice) = self.items.get_mut(start..)
        {
            slice.reverse();
        }
    }

    /// Decode and place one character code, then advance the text matrix
    /// per §9.4.4.
    ///
    /// `TJ` adjustments are **not** a parameter here. §9.4.4 folds them
    /// into the displacement formula as `(w0 − Tj/1000)`, which reads as
    /// though the adjustment belonged to the glyph being shown — but
    /// Table 109 is explicit that the number "shall be subtracted from
    /// the current horizontal coordinate", i.e. it moves the position
    /// and *then* the next glyph is shown there. Folding it into the
    /// current glyph's advance instead places that glyph at the
    /// pre-shift origin, which leaves the whole shift invisible to a
    /// gap-based word-space heuristic reading origins. [`Walk::show_array`]
    /// therefore applies each adjustment to the text matrix as it meets
    /// it.
    fn show_code(&mut self, font: &ExtractFont, code: u32, word_spacing: bool) {
        // `TX-A1` (R169): what an unmappable code looks like is the
        // operator's setting, threaded from `ExtractOptions` rather than
        // read from anywhere global — two walks in one session may
        // legitimately use different sentinels.
        let (chars, rung) = font.to_unicode(code, self.options.unmappable_code);
        self.diagnostics.codes_total += 1;
        match rung {
            LadderRung::ToUnicode => self.diagnostics.via_to_unicode += 1,
            LadderRung::EncodingAgl => self.diagnostics.via_encoding_agl += 1,
            LadderRung::CidCollection => self.diagnostics.via_cid_collection += 1,
            LadderRung::GlyphNameExtension => self.diagnostics.via_glyph_name_extension += 1,
            LadderRung::Failed => self.diagnostics.ladder_failures += 1,
        }

        // §9.4.4: Trm = [Tfs·Th 0 0 Tfs 0 Ts] × Tm × CTM.
        let params = Matrix {
            a: self.ts.size * self.ts.h_scale(),
            b: 0.0,
            c: 0.0,
            d: self.ts.size,
            e: 0.0,
            f: self.ts.rise(),
        };
        let tm_ctm = self.tm.mul(self.ctm);
        let trm = params.mul(tm_ctm);

        // §9.4.4's displacement:
        //   tx = ((w0 − Tj/1000)·Tfs + Tc + Tw) · Th
        // Tw participates ONLY for a single-byte code 32 (§9.3.3) —
        // which is why it is inert under Identity-H and useless as a
        // word-break signal in modern documents (S6).
        let w0 = font.width(code);
        let tw = if word_spacing {
            self.ts.word_spacing()
        } else {
            0.0
        };
        // The ONE copy of §9.4.4's displacement (`font::advance_tx`), not a
        // local restatement of it — see that function's "why it is a
        // function at all".
        let tx = super::font::advance_tx(
            f64::from(w0),
            f64::from(self.ts.size),
            f64::from(self.ts.char_spacing()),
            f64::from(tw),
            f64::from(self.ts.h_scale()),
        ) as f32;

        let invisible = matches!(self.ts.render_mode(), 3 | 7);
        if invisible {
            self.diagnostics.invisible_glyphs += 1;
        }

        let x = trm.e;
        let y = trm.f;
        let size = trm.y_scale();
        let advance = tx * tm_ctm.x_scale() * if tx < 0.0 { -1.0 } else { 1.0 };
        // `Pass 139.0`: the DIRECTION the two lines above discard.
        //
        // `size` and `advance` are the magnitudes of the two transformed
        // basis vectors; `x`/`y` are exact and orientation-independent.
        // Without the direction, nothing downstream can tell a title
        // block's 90°-stamped file path from ordinary horizontal text,
        // and the derived-layout thresholds — which were stated in page
        // axes — then fire between every letter (see `layout::classify`).
        //
        // Taken from `trm` rather than `tm_ctm` because §9.4.4 makes
        // `Trm` the matrix the glyph is actually placed by. The two
        // agree in direction whenever `Tfs · Th > 0`, which is every
        // real file; where they disagree (a negative `Tz`, a negative
        // `Tf`) the glyph is genuinely drawn the other way round and
        // `Trm` is the one that says so.
        let direction = trm.x_unit();

        // `Tm` at the instant this glyph is shown — captured BEFORE the
        // advance below, because that is the matrix the glyph was placed
        // by (and the one the surgery must reproduce). Any TJ pre-shift is
        // already folded in: `show_array` applied it to `self.tm` before
        // calling here.
        let text_matrix_at_show = self.tm;

        // Advance first, so a suppressed glyph still moves the matrix.
        self.tm = Matrix::translate(tx, 0.0).mul(self.tm);

        // Inside an /ActualText sequence the glyphs are REPLACED, not
        // shown: accumulate their extent for the replacement run's bbox
        // and emit nothing (§14.9.4).
        if self.actual_text_active() {
            self.extend_covered(x, y, advance, size, direction);
            return;
        }

        let artifact = self.artifact();
        if artifact.is_some() {
            self.diagnostics.artifact_chars += chars.chars().count() as u64;
        }
        // Provenance is built only on demand (default off), so the Pass 4
        // output is byte-for-byte unchanged for callers that do not ask for
        // it. When asked, it is a snapshot of SOURCED state — operator
        // span, governing font/size, fill colour, matrices — never derived.
        let provenance = if self.options.capture_provenance {
            Some(GlyphProvenance {
                content_stream: self.stream_ref,
                operator_span: self.cur_op_span,
                font_resource: self.cur_font_resource.clone(),
                tf_size: self.ts.size,
                fill_color: self.ts.fill_color,
                text_matrix: text_matrix_at_show.to_array(),
                ctm: self.ctm.to_array(),
                // Pass 19.0: the ambient §9.3 state used to be tracked
                // here and then DROPPED at exactly this line, which is why
                // pdfcer could not restore an ambient rise it never
                // published. It now rides along — with each parameter's
                // restore provenance, so an authoring pass can put the
                // stream back byte-faithfully or refuse honestly.
                // `Arc::clone` so publishing onto every glyph of a page is
                // a refcount bump rather than six copies.
                text_state: Arc::clone(&self.ts.ambient),
                // §9.3.3: `Tw` is void for multi-byte codes. Published
                // here rather than re-derived per caller, because the
                // caller would need the resolved font to derive it and a
                // GUI asking "is this run composite?" would otherwise have
                // to provoke an error return to find out (R83 cannot be
                // honoured against a capability nothing exposes).
                composite: !font.is_simple(),
            })
        } else {
            None
        };
        self.items.push(Item::Glyph(GlyphItem {
            chars,
            code,
            rung,
            x,
            y,
            advance,
            size,
            direction,
            invisible,
            artifact,
            mcid: self.mcid(),
            provenance,
        }));
    }

    // -----------------------------------------------------------------
    // Marked content
    // -----------------------------------------------------------------

    /// `BDC`'s `properties` operand: an inline dictionary, or a name to
    /// resolve against the **current** resource dictionary's
    /// `/Properties` (§14.6.2).
    ///
    /// §14.6 N2: a name absent from `/Properties` is legal-by-silence;
    /// pdfcer treats it as an empty property list.
    fn resolve_properties(&self, op: &Operation<'_>, resources: &Dict) -> Option<Dict> {
        match op.operands.last().and_then(operand_object)? {
            Object::Dict(d) => Some(d.clone()),
            Object::Name(n) => self
                .doc
                .resolve(resources.get(b"Properties")?)
                .as_dict()?
                .get(n.as_bytes())
                .map(|o| self.doc.resolve(o))
                .and_then(Object::as_dict)
                .cloned(),
            _ => None,
        }
    }

    /// Push a marked-content level, reading the four tags that matter.
    ///
    /// **`/ActualText` nesting policy:** an `/ActualText` inside a
    /// sequence that already has one is IGNORED. §14.9.4 N2 records that
    /// no clause says which applies when an ancestor and a descendant
    /// both carry one; "innermost wins" is the obvious reading but,
    /// combined with Table 323 scoping the entry to "the structure
    /// element **and its children**", it would make the ancestor's value
    /// cover the descendant's region *as well*, emitting both and
    /// duplicating text. Outermost-wins cannot duplicate, so that is
    /// pdfcer's rule, and the ignored inner values are counted.
    fn begin_marked(&mut self, tag: &[u8], props: Option<&Dict>) {
        // A hostile stream of BMCs must not grow the stack without
        // bound. 256 matches the graphics-state guard above.
        if self.marked.len() >= 256 {
            return;
        }
        let mut level = MarkedLevel {
            artifact: self.artifact(),
            mcid: self.mcid(),
            reversed_chars: self.reversed_chars(),
            actual_text: None,
            covered: None,
        };

        match tag {
            b"Artifact" => {
                self.diagnostics.artifact_sequences += 1;
                level.artifact = Some(artifact_kind(self.doc, props));
            }
            b"ReversedChars" => {
                self.diagnostics.reversed_chars_sequences += 1;
                level.reversed_chars = true;
            }
            b"TagSuspect" => {
                self.diagnostics.tag_suspect_sequences += 1;
                self.diagnostics.note(
                    "text: /TagSuspect /Ordering region — the producer declares the enclosed \
                     content's order does not meet Tagged PDF specifications (§14.8.2.3.1)"
                        .to_string(),
                );
            }
            _ => {}
        }

        if let Some(props) = props {
            // /MCID (§14.7.4.2) — the join key to the structure tree.
            if let Some(mcid) = props
                .get(b"MCID")
                .map(|o| self.doc.resolve(o))
                .and_then(Object::as_int)
                .and_then(|v| u32::try_from(v).ok())
            {
                level.mcid = Some(mcid);
            }
            // /Alt and /E are counted, never substituted — see the
            // module docs on `super`.
            if props.get(b"Alt").is_some() {
                self.diagnostics.alt_entries += 1;
            }
            if props.get(b"E").is_some() {
                self.diagnostics.expansion_entries += 1;
            }
            if let Some(Object::String(bytes)) =
                props.get(b"ActualText").map(|o| self.doc.resolve(o))
            {
                // §14.9.4 names `/Span` normatively for the
                // marked-content form. Real producers attach
                // /ActualText to other tags (N5); pdfcer honours it
                // wherever it appears — dropping recoverable text over a
                // tag name would be the worse error — and says so once.
                if tag != b"Span" {
                    self.diagnostics.note(format!(
                        "text: /ActualText on a /{} marked-content sequence — §14.9.4 names /Span \
                         normatively; honoured anyway",
                        String::from_utf8_lossy(tag)
                    ));
                }
                if self.actual_text_active() {
                    self.diagnostics.note(
                        "text: nested /ActualText — the outermost value covers the region \
                         (§14.9.4 N2 states no nesting rule); the inner value was not applied"
                            .to_string(),
                    );
                } else {
                    let decoded = decode_text_string(bytes);
                    if decoded.text.is_empty() {
                        self.diagnostics.actual_text_suppressions += 1;
                    } else {
                        self.diagnostics.actual_text_applied += 1;
                    }
                    // `AT-A1` (R169). §14.9.4 says the value "shall be
                    // used as a replacement"; §14.8.2.4.2 NOTE 2 says a
                    // reader "may choose to use" it and that only "some
                    // conforming readers" do; §9.10.1 says "may be used".
                    // The only sentence about PRECEDENCE is a `may` inside
                    // an informative NOTE, so neither reading is
                    // dislodgeable and the direction is the operator's.
                    //
                    // The counters above run BEFORE this test on purpose:
                    // "how many /ActualText entries does this page carry"
                    // is a property of the document, not of pdfcer's
                    // settings, and an operator who turned substitution off
                    // still needs to see that the entries are there.
                    let substitute = match self.options.actual_text {
                        ActualTextPrecedence::Always => true,
                        // "Inside tagged content" is tested as an /MCID in
                        // scope — on this sequence or an enclosing one.
                        // That is the only test a content stream can
                        // answer: §14.7.4.2 makes /MCID precisely the join
                        // key between a marked-content sequence and a
                        // structure element, so a sequence with none in
                        // scope is not part of the structure tree in any
                        // sense the page itself expresses. `level.mcid`
                        // already carries the inherited value (set from
                        // `self.mcid()` when the level was built) and has
                        // been overwritten by this sequence's own /MCID
                        // above if it had one.
                        ActualTextPrecedence::TaggedOnly => level.mcid.is_some(),
                        ActualTextPrecedence::Glyphs => false,
                    };
                    if substitute {
                        level.actual_text = Some(decoded.text);
                    }
                }
            }
        }

        self.marked.push(level);
    }

    /// `EMC`: pop a level and, if it carried `/ActualText`, emit the
    /// replacement run now — at the end of the region it covered, which
    /// is where its characters belong in page content order.
    fn end_marked(&mut self) {
        // §14.6 N1: an EMC with an empty stack is unbalanced and the
        // standard states no recovery. Ignoring it cannot lose content.
        let Some(level) = self.marked.pop() else {
            return;
        };
        let Some(text) = level.actual_text else {
            return;
        };
        if text.is_empty() {
            // An empty /ActualText suppressed its content deliberately
            // (N7); emitting an empty run would be noise.
            return;
        }
        self.items.push(Item::Replacement(ReplacementItem {
            text,
            artifact: level.artifact,
            mcid: level.mcid,
            bbox: level.covered,
        }));
    }

    /// The innermost enclosing artifact classification.
    fn artifact(&self) -> Option<ArtifactKind> {
        self.marked.last().and_then(|l| l.artifact.clone())
    }

    /// The innermost enclosing `/MCID`.
    fn mcid(&self) -> Option<u32> {
        self.marked.last().and_then(|l| l.mcid)
    }

    /// Whether any enclosing sequence is `/ReversedChars`.
    fn reversed_chars(&self) -> bool {
        self.marked.last().is_some_and(|l| l.reversed_chars)
    }

    /// Whether any enclosing sequence is replacing its content.
    fn actual_text_active(&self) -> bool {
        self.marked.iter().any(|l| l.actual_text.is_some())
    }

    /// Grow the bounding box of the outermost active `/ActualText`
    /// level to include one suppressed glyph.
    fn extend_covered(&mut self, x: f32, y: f32, advance: f32, size: f32, direction: (f32, f32)) {
        let Some(level) = self.marked.iter_mut().find(|l| l.actual_text.is_some()) else {
            return;
        };
        // The glyph box is approximated as one em tall from the
        // baseline, with a quarter-em descender — enough to locate a
        // replacement run on the page, which is all §14.9.4 N4 permits
        // anyway. `Pass 139.0`: the corners are taken in the glyph's own
        // frame by the ONE copy of that arithmetic
        // ([`super::glyph_cell`]) rather than restated here in page
        // axes, which is how this box came to be hung off the wrong
        // corner for rotated text.
        let cell = super::glyph_cell(x, y, advance, size, direction);
        level.covered = Some(match level.covered {
            None => cell,
            Some(r) => Rect {
                llx: r.llx.min(cell.llx),
                lly: r.lly.min(cell.lly),
                urx: r.urx.max(cell.urx),
                ury: r.ury.max(cell.ury),
            },
        });
    }

    // -----------------------------------------------------------------
    // Form XObjects (§8.10.1)
    // -----------------------------------------------------------------

    /// `Do`: execute a form XObject's content with its own `/Resources`
    /// and `/Matrix`.
    ///
    /// Image XObjects are skipped (they hold no text). The recursion
    /// follows §8.10.1's five-step procedure in the parts that matter
    /// here: save state, concatenate `/Matrix`, execute with the form's
    /// resource dictionary, restore state.
    fn do_xobject(&mut self, op: &Operation<'_>, _buf: &[u8], resources: &Dict) {
        let Some(name) = operand_name(op, 0) else {
            return;
        };
        // Copy the document reference out of `self` first: everything
        // below reads through it while later lines take `&mut self`, and
        // the copy makes those two borrows provably independent.
        let doc = self.doc;
        let Some(entry) = doc
            .resolve(resources.get(b"XObject").unwrap_or(&Object::Null))
            .as_dict()
            .and_then(|d| d.get(&name))
        else {
            return;
        };
        // The object number, if this was an indirect reference — the
        // cycle guard's key.
        let obj_num = entry.as_reference().map(|id| id.num);
        let Object::Stream(stream) = doc.resolve(entry) else {
            return;
        };
        if doc
            .resolve(stream.dict.get(b"Subtype").unwrap_or(&Object::Null))
            .as_name()
            .is_none_or(|n| n.as_bytes() != b"Form")
        {
            return;
        }
        if self.depth >= self.options.max_form_depth {
            self.diagnostics.form_depth_overflows += 1;
            self.diagnostics.note(format!(
                "text: form XObject nesting exceeded {} levels — the deeper content was not \
                 extracted",
                self.options.max_form_depth
            ));
            return;
        }
        // §8.10.1 cycle guard, keyed on object number rather than
        // resource name: the same stream can be reached under different
        // names, and a name-keyed guard would miss the cycle.
        if let Some(num) = obj_num {
            if self.active_xobjects.contains(&num) {
                return;
            }
            self.active_xobjects.push(num);
        }

        let inner_resources = doc
            .resolve(stream.dict.get(b"Resources").unwrap_or(&Object::Null))
            .as_dict()
            .cloned()
            .unwrap_or_else(|| resources.clone());

        // `view.slice(span)`, not `span.slice(view.bytes)` (Pass 17.1): a
        // form XObject the SESSION authored carries an R45 span starting
        // past the end of the base buffer, and only the view's
        // `StreamSource` knows which of its two halves such a span indexes.
        // The `None` arm is unchanged in meaning — an unresolvable or
        // undecodable form is skipped, not fatal.
        let content = doc
            .slice(stream.data_span)
            .and_then(|raw| crate::filters::decode_stream(&stream.dict, raw).ok())
            .and_then(|decoded| ContentStream::parse(decoded).ok());

        if let Some(content) = content {
            self.diagnostics.forms_executed += 1;
            let saved_ctm = self.ctm;
            let saved_tm = self.tm;
            let saved_tlm = self.tlm;
            let saved_ts = self.ts.clone();
            let saved_marked_depth = self.marked.len();
            // Provenance spans inside the form index the FORM's own decoded
            // buffer (§8.10.1 — a separate content stream), so the walk
            // switches its stream reference for the duration and restores
            // it on return. The font-resource mirror is restored too, since
            // a `Tf` inside the form selected from the form's /Resources.
            let saved_stream_ref = self.stream_ref;
            let saved_op_span = self.cur_op_span;
            let saved_font_resource = self.cur_font_resource.clone();
            if let Some(num) = obj_num {
                self.stream_ref = ContentStreamRef::Form { object: num };
            }
            // R88 tier 3 (decision 019 §3.4). The form INHERITS the
            // invoking context's text state (§8.10.1), so every value
            // stays in force and the advance arithmetic below is
            // unaffected — but the operators that set those values live in
            // the PAGE's buffer, not the form's. A later authoring pass
            // editing a run inside this form therefore has nothing it
            // could re-emit as a restore, and must refuse and disclose
            // rather than guess the Table 105 default. Marking it here, at
            // the exact moment the buffer changes, is what makes that
            // refusal structural instead of a rule someone has to
            // remember. Values a `Tc`/`Ts`/… INSIDE the form sets are
            // observable in the form's own buffer and overwrite the mark.
            self.ts.ambient_mut().enter_form(obj_num);

            if let Some(m) = matrix_of(doc, &stream.dict) {
                self.ctm = m.mul(self.ctm);
            }
            self.depth += 1;
            self.run(&content, &inner_resources);
            self.depth -= 1;

            // §8.10.1 steps (a)/(e): the form's state changes cannot
            // escape. Restoring explicitly rather than relying on the
            // form's own q/Q balance makes that structural — an
            // unbalanced `Q` inside a form provably cannot pop the
            // caller's state.
            self.ctm = saved_ctm;
            self.tm = saved_tm;
            self.tlm = saved_tlm;
            self.ts = saved_ts;
            self.marked.truncate(saved_marked_depth);
            self.stream_ref = saved_stream_ref;
            self.cur_op_span = saved_op_span;
            self.cur_font_resource = saved_font_resource;
        }

        if obj_num.is_some() {
            self.active_xobjects.pop();
        }
    }
}

// ---------------------------------------------------------------------------
// Operand helpers
// ---------------------------------------------------------------------------

/// The object carried by a content token, if it is an operand.
fn operand_object(token: &crate::content::ContentToken) -> Option<&Object> {
    match &token.kind {
        ContentTokenKind::Operand(o) => Some(o),
        _ => None,
    }
}

/// The last `count` numeric operands, in order. Returns fewer than
/// `count` (and the caller's slice pattern then fails to match) when the
/// operator is malformed.
/// The operator's bytes **as written**, operands included — the raw
/// sequence a tier-2 R88 restore re-emits (see [`crate::text_state`]).
///
/// Spans from the first operand's start (or the operator keyword's, for a
/// no-operand operator) to the operator keyword's end. Whitespace and
/// comments *between* the operands are inside the span and therefore
/// preserved, which is the point: the restore must be the bytes the
/// producer wrote, not a re-serialization of the parsed numbers.
fn op_bytes<'b>(op: &Operation<'_>, buf: &'b [u8]) -> &'b [u8] {
    let start = op
        .operands
        .first()
        .map_or(op.operator.span.start, |t| t.span.start);
    buf.get(start..op.operator.span.end()).unwrap_or_default()
}

/// The last `count` operands as `f64`, non-numeric operands filtered out.
///
/// The `f64` sibling of [`operand_numbers`]: the shared text-state model
/// stores `f64` (the width the tokenizer parses at), and every consumer
/// that needs this walk's historical `f32` precision narrows explicitly at
/// its own point of use — see [`TextState`]'s accessors and the note there
/// about why the narrowing order is preserved deliberately.
fn operand_numbers_f64(op: &Operation<'_>, count: usize) -> Vec<f64> {
    let start = op.operands.len().saturating_sub(count);
    op.operands
        .get(start..)
        .unwrap_or(&[])
        .iter()
        .filter_map(operand_object)
        .filter_map(Object::as_number)
        .collect()
}

fn operand_numbers(op: &Operation<'_>, count: usize) -> Vec<f32> {
    let start = op.operands.len().saturating_sub(count);
    op.operands
        .get(start..)
        .unwrap_or(&[])
        .iter()
        .filter_map(operand_object)
        .filter_map(|o| o.as_number())
        .map(|v| v as f32)
        .collect()
}

/// The operand at `index` as a name's bytes.
fn operand_name(op: &Operation<'_>, index: usize) -> Option<Vec<u8>> {
    op.operands
        .get(index)
        .and_then(operand_object)
        .and_then(Object::as_name)
        .map(|n| n.as_bytes().to_vec())
}

/// The last string operand.
fn last_string(op: &Operation<'_>) -> Option<Vec<u8>> {
    match op.operands.last().and_then(operand_object)? {
        Object::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// Table 330's `/Type`, or `Unspecified` for the generic form.
fn artifact_kind(doc: &DocumentView<'_>, props: Option<&Dict>) -> ArtifactKind {
    let Some(props) = props else {
        return ArtifactKind::Unspecified;
    };
    match props.get(b"Type").map(|o| doc.resolve(o)) {
        Some(Object::Name(n)) => match n.as_bytes() {
            b"Pagination" => ArtifactKind::Pagination,
            b"Layout" => ArtifactKind::Layout,
            b"Page" => ArtifactKind::Page,
            b"Background" => ArtifactKind::Background,
            other => ArtifactKind::Other(String::from_utf8_lossy(other).into_owned()),
        },
        _ => ArtifactKind::Unspecified,
    }
}

/// A form XObject's `/Matrix` (Table 95; default identity).
fn matrix_of(doc: &DocumentView<'_>, dict: &Dict) -> Option<Matrix> {
    let items = doc.resolve(dict.get(b"Matrix")?).as_array()?;
    let v: Vec<f32> = items
        .iter()
        .filter_map(|o| doc.resolve(o).as_number())
        .map(|n| n as f32)
        .collect();
    match v[..] {
        [a, b, c, d, e, f] => Some(Matrix { a, b, c, d, e, f }),
        _ => None,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn matrix_multiplication_follows_pdf_row_vector_convention() {
        // Translate then scale: the translation is scaled too.
        let t = Matrix::translate(10.0, 0.0);
        let s = Matrix {
            a: 2.0,
            b: 0.0,
            c: 0.0,
            d: 2.0,
            e: 0.0,
            f: 0.0,
        };
        let m = t.mul(s);
        assert!((m.e - 20.0).abs() < 1e-6);
        assert!((m.a - 2.0).abs() < 1e-6);
    }

    #[test]
    fn scales_measure_transformed_unit_vectors() {
        let m = Matrix {
            a: 3.0,
            b: 4.0,
            c: 0.0,
            d: 5.0,
            e: 0.0,
            f: 0.0,
        };
        assert!((m.x_scale() - 5.0).abs() < 1e-6);
        assert!((m.y_scale() - 5.0).abs() < 1e-6);
    }

    // -- Pass 19.0: ambient text state published on provenance ----------

    use crate::document::Document;
    use crate::text_state::{AmbientOrigin, TextStateParam, UnobservableAmbient};

    /// A one-page PDF whose page content is `page_content` and which
    /// carries one form XObject `/X1` (object 6) with `form_content`.
    ///
    /// Deliberately hand-assembled rather than pulled from
    /// `fixtures/synthetic/`: the assertions below are about the exact
    /// **spelling** of operands (`0.5000`, not `0.5`), and a fixture whose
    /// bytes live in another file is a fixture whose bytes can be
    /// reformatted by an unrelated regeneration.
    fn pdf_with_form(page_content: &str, form_content: &str) -> Vec<u8> {
        let font = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
                     /Encoding /WinAnsiEncoding >>"
            .to_vec();
        let mut form = format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 200 200] \
             /Resources << /Font << /F1 5 0 R >> >> /Length {} >>\nstream\n",
            form_content.len()
        )
        .into_bytes();
        form.extend_from_slice(form_content.as_bytes());
        form.extend_from_slice(b"\nendstream");

        let mut content = format!("<< /Length {} >>\nstream\n", page_content.len()).into_bytes();
        content.extend_from_slice(page_content.as_bytes());
        content.extend_from_slice(b"\nendstream");

        let objects: Vec<(u32, Vec<u8>)> = vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] \
                  /Resources << /Font << /F1 5 0 R >> /XObject << /X1 6 0 R >> >> >>"
                    .to_vec(),
            ),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
            ),
            (4, content),
            (5, font),
            (6, form),
        ];

        let highest = 6u32;
        let mut out = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offsets = std::collections::BTreeMap::new();
        for (num, obj) in &objects {
            offsets.insert(*num, out.len());
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            out.extend_from_slice(obj);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", highest + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for num in 1..=highest {
            match offsets.get(&num) {
                Some(off) => out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
                None => out.extend_from_slice(b"0000000000 65535 f \n"),
            }
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
                highest + 1
            )
            .as_bytes(),
        );
        out
    }

    /// Every glyph of the page, with provenance capture on, in order.
    fn glyphs_with_provenance(bytes: &[u8]) -> Vec<crate::text_extract::ExtractedGlyph> {
        let doc = Document::from_bytes(bytes.to_vec()).unwrap();
        let pages = crate::page_tree::pages(&doc).unwrap();
        let opts = ExtractOptions::default().with_provenance(true);
        let page = super::super::extract_page(&doc, &pages[0], 0, &opts).unwrap();
        page.runs
            .into_iter()
            .flat_map(|r| r.glyphs.into_iter())
            .collect()
    }

    /// R88 tier 1 + tier 2, end to end through the public extraction API.
    /// The ambient state used to be tracked here and then dropped at
    /// provenance-construction time; the operand spelling is preserved so a
    /// restore is byte-faithful rather than renormalized.
    #[test]
    fn provenance_publishes_the_ambient_state_with_raw_operand_bytes() {
        let bytes = pdf_with_form(
            "0.5000 Tc 3 Ts 90 Tz 2 Tr BT /F1 12 Tf 72 700 Td (hi) Tj ET\n",
            "BT /F1 12 Tf 10 10 Td (x) Tj ET\n",
        );
        let glyphs = glyphs_with_provenance(&bytes);
        let prov = glyphs[0].provenance.as_ref().expect("provenance captured");
        let ts = &prov.text_state;

        // Values.
        let p = ts.params();
        assert_eq!(p.char_spacing, 0.5);
        assert_eq!(p.rise, 3.0);
        assert_eq!(p.h_scale, 0.9, "Tz 90 ⇒ Th 0.9");
        assert_eq!(p.render_mode, 2);

        // Tier 2: restore the bytes AS WRITTEN.
        assert_eq!(
            ts.restore_bytes(TextStateParam::CharSpacing).unwrap(),
            b"0.5000 Tc",
            "a trailing-zero operand must survive verbatim"
        );
        assert_eq!(ts.restore_bytes(TextStateParam::Rise).unwrap(), b"3 Ts");
        assert_eq!(
            ts.restore_bytes(TextStateParam::HorizScale).unwrap(),
            b"90 Tz"
        );

        // Tier 1: `TL` was never set, so it restores to the spec default.
        assert!(matches!(ts.leading.origin, AmbientOrigin::Initial));
        assert_eq!(ts.restore_bytes(TextStateParam::Leading).unwrap(), b"0 TL");

        // This run is a simple font, so it is not composite.
        assert!(!prov.composite);
    }

    /// R88 tier 3 — **the refuse tier**. A run inside a form XObject
    /// inherits its text state from the invoking context (§8.10.1), so the
    /// operator that set it is in the page's buffer, not the form's. A
    /// restore emitted into the form would be a guess; pdfcer refuses and
    /// names the form.
    #[test]
    fn a_run_inside_a_form_xobject_refuses_the_restore_by_name() {
        let bytes = pdf_with_form(
            "0.5 Tc 3 Ts BT /F1 12 Tf 72 700 Td (hi) Tj ET\n/X1 Do\n",
            "BT /F1 12 Tf 10 10 Td (x) Tj ET\n",
        );
        let glyphs = glyphs_with_provenance(&bytes);
        // Page run: "hi" (2 glyphs). Form run: "x" (1 glyph), last.
        let inside = glyphs
            .last()
            .and_then(|g| g.provenance.as_ref())
            .expect("the form's glyph carries provenance");
        assert_eq!(
            inside.content_stream,
            ContentStreamRef::Form { object: 6 },
            "the last glyph must be the form's"
        );

        for param in [TextStateParam::CharSpacing, TextStateParam::Rise] {
            let err = inside
                .text_state
                .restore_bytes(param)
                .expect_err("an inherited value must NOT be restorable");
            assert!(
                matches!(
                    err,
                    crate::text_state::AmbientRestoreError::Unobservable {
                        reason: UnobservableAmbient::FormXObject { object: Some(6) },
                        ..
                    }
                ),
                "{err:?}"
            );
            // The disclosure names the parameter and the form, and says
            // pdfcer refused rather than guessed.
            let msg = err.to_string();
            assert!(msg.contains("form XObject 6"), "{msg}");
            assert!(msg.contains("refuses"), "{msg}");
        }

        // Unobservable is about RESTORABILITY, not knowledge: the inherited
        // values are still in force and still drive the advance arithmetic.
        assert_eq!(inside.text_state.params().char_spacing, 0.5);
        assert_eq!(inside.text_state.params().rise, 3.0);

        // A parameter nothing ever set stays restorable even inside the
        // form — the Table 105 default holds everywhere, so emitting it is
        // provably correct rather than a guess.
        assert_eq!(
            inside
                .text_state
                .restore_bytes(TextStateParam::HorizScale)
                .unwrap(),
            b"100 Tz"
        );
    }

    /// A value set INSIDE the form is observable in the form's own buffer,
    /// so it is restorable there — the inheritance mark is not sticky.
    #[test]
    fn a_value_set_inside_the_form_is_restorable_again() {
        let bytes = pdf_with_form(
            "0.5 Tc BT /F1 12 Tf 72 700 Td (hi) Tj ET\n/X1 Do\n",
            "1.25 Tc BT /F1 12 Tf 10 10 Td (x) Tj ET\n",
        );
        let glyphs = glyphs_with_provenance(&bytes);
        let inside = glyphs.last().and_then(|g| g.provenance.as_ref()).unwrap();
        assert_eq!(
            inside
                .text_state
                .restore_bytes(TextStateParam::CharSpacing)
                .unwrap(),
            b"1.25 Tc"
        );
    }

    /// §8.10.1: a form's state changes cannot escape it. The page run that
    /// follows the `Do` must see the page's own ambient, not the form's.
    #[test]
    fn form_state_does_not_escape_back_to_the_page() {
        let bytes = pdf_with_form(
            "0.5 Tc BT /F1 12 Tf 72 700 Td (a) Tj ET\n/X1 Do\n\
             BT /F1 12 Tf 72 680 Td (b) Tj ET\n",
            "9 Tc 9 Ts BT /F1 12 Tf 10 10 Td (x) Tj ET\n",
        );
        let glyphs = glyphs_with_provenance(&bytes);
        let after = glyphs.last().and_then(|g| g.provenance.as_ref()).unwrap();
        assert_eq!(after.content_stream, ContentStreamRef::Page);
        assert_eq!(after.text_state.params().char_spacing, 0.5);
        assert_eq!(after.text_state.params().rise, 0.0);
        assert_eq!(
            after
                .text_state
                .restore_bytes(TextStateParam::CharSpacing)
                .unwrap(),
            b"0.5 Tc",
            "back on the page, the page's own operator is observable again"
        );
    }

    /// §8.4.2: `q`/`Q` save and restore text state on the read path too.
    #[test]
    fn q_and_q_bracket_the_published_ambient_state() {
        let bytes = pdf_with_form(
            "q 0.5 Tc 3 Ts BT /F1 12 Tf 72 700 Td (a) Tj ET Q \
             BT /F1 12 Tf 72 680 Td (b) Tj ET\n",
            "BT /F1 12 Tf 10 10 Td (x) Tj ET\n",
        );
        let glyphs = glyphs_with_provenance(&bytes);
        let inside = glyphs[0].provenance.as_ref().unwrap();
        assert_eq!(inside.text_state.params().rise, 3.0);
        let outside = glyphs
            .last()
            .and_then(|g| g.provenance.as_ref())
            .expect("the post-Q glyph");
        assert_eq!(outside.text_state.params().rise, 0.0);
        assert_eq!(outside.text_state.params().char_spacing, 0.0);
        assert_eq!(
            outside
                .text_state
                .restore_bytes(TextStateParam::Rise)
                .unwrap(),
            b"0 Ts"
        );
    }
}
