//! # Content-stream serializer (ISO 32000-1 §7.8.2, §8.2, §8.4.3, §8.5, §8.6)
//!
//! The project's **first content-stream writer**. Where
//! [`super::serialize`] turns a COS value tree into object bytes, this
//! module turns *graphics operations* into the postfix operator syntax of
//! a content stream — the bytes that live between `stream` and
//! `endstream` in a page's `/Contents` or, for Pass 6.1, inside an
//! annotation's `/AP` `/N` appearance form XObject.
//!
//! Two independent capabilities live here, sharing one set of token
//! emitters:
//!
//! 1. [`ContentBuilder`] — **authoring**. A stateful appender that emits
//!    path-construction, path-painting, graphics-state and colour
//!    operators for a *new* content stream (the appearance generator in
//!    [`crate::annot_author`] is its first and only Pass-6.1 consumer). It
//!    is deliberately a low-level primitive: it emits exactly the tokens
//!    asked for, in the order asked for, and it is the caller's job to
//!    respect the W-E ordering constraint (colour + graphics-state before
//!    the path, never inside it — see below). The one structural guard it
//!    *does* enforce is that operands cannot fuse (a SPACE is emitted
//!    between adjacent numeric operands so `1 0` never becomes `10`).
//!
//! 2. [`reemit_canonical`] — the **R46 identity gate's** re-emission.
//!    Parse → re-emit → byte-compare every content stream in the loadable
//!    corpus. This is Pass 3.0's object-level identity move applied one
//!    level down, and it is nearly free because [`crate::content`] already
//!    carries a per-token byte span. See that function's docs for exactly
//!    what it re-emits from value (numbers, the X6 target) versus verbatim
//!    (strings, arrays, operators, whitespace — everything a geometric
//!    serializer must never touch), and why that split is the mechanically
//!    self-enforcing guard against silent normalization (X6).
//!
//! ## Spec sources (PDF-spec RAG, ISO 32000-1:2008)
//!
//! - `iso32000__s__8.10.md` `## WRITE DIRECTION` (W-D operator emission,
//!   W-E ordering, WF1–WF6) — the authoring audit this module enacts.
//! - `iso32000__ref__writer_emission.md` (A1–A3 token/number emission).
//! - `iso32000__s__8.5.md` (path construction/painting operators),
//!   `iso32000__s__8.4.3.md` (`w`/`J`/`j`/`M`/`d`), `iso32000__s__8.6.md`
//!   (device colour operators — `CS`/`cs` unneeded, WF5),
//!   `iso32000__s__8.2.md` (Figure 9 state machine — the W-E ordering
//!   rule, and "no semantic significance to gstate arrangement", WF6).
//!
//! ## The W-E ordering constraint (Figure 9), stated once
//!
//! Graphics-state operators (`w`/`J`/`j`/`M`/`d`) and colour operators
//! (`RG`/`rg`/`G`/`g`/`K`/`k`/`CS`/`cs`/`gs`) are **illegal inside a path
//! object**: once `m`/`re` begins a path, only further construction
//! operators (and `W`/`W*`) are legal until the paint operator returns to
//! page-description level. A generator must therefore emit every colour
//! and line parameter it needs **before** the `m`/`re` that begins the
//! path. [`ContentBuilder`]'s method set is grouped to make the correct
//! call order the obvious one, and its doc comments name the constraint at
//! each path-construction entry point.
//!
//! ## Number emission (WF / A1–A3)
//!
//! Coordinates and parameters are `f64`. [`emit_number`] emits an integral
//! value in integer form (`10`, not `10.0`) and a non-integral value
//! through [`super::serialize::write_real`] — which guarantees no
//! exponential notation (§7.3.3 has no exponent) and never emits NaN/Inf.
//! Integer form for integral values keeps authored geometry compact and
//! matches the spelling the spec's own W-F worked examples use
//! (`10 10 m`, not `10.0 10.0 m`).

use crate::content::{ContentStream, ContentToken, ContentTokenKind};
use crate::object::Object;

use super::serialize::{write_name, write_real};

/// A line-cap style (`J` operator, §8.4.3.3, Table 54). The three legal
/// integer values, named so a caller cannot pass an out-of-range code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCap {
    /// 0 — butt cap: the stroke is squared off at the endpoint.
    Butt = 0,
    /// 1 — round cap: a semicircular arc caps the endpoint.
    Round = 1,
    /// 2 — projecting square cap: the stroke extends half a line width
    /// past the endpoint and is squared off.
    ProjectingSquare = 2,
}

/// A line-join style (`j` operator, §8.4.3.4, Table 55).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineJoin {
    /// 0 — miter join: the outer edges extend to meet at a point (subject
    /// to the miter limit `M`).
    Miter = 0,
    /// 1 — round join: an arc of a circle joins the segments.
    Round = 1,
    /// 2 — bevel join: the two segments are finished with butt caps and
    /// the notch is filled with a triangle.
    Bevel = 2,
}

/// The path-painting operator that ends one path object (§8.5.3,
/// Table 60). Exactly one paint operator ends each path object (§8.2
/// Figure 9), so this is a closed enum rather than free-form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    /// `S` — stroke the path (open geometry: Line, Ink, PolyLine).
    Stroke,
    /// `s` — close then stroke (≡ `h S`).
    CloseStroke,
    /// `f` — fill, nonzero winding. Emitted as `f`, never the
    /// compatibility-only `F` (§8.5: a writer `should` use `f`).
    Fill,
    /// `f*` — fill, even-odd winding.
    FillEvenOdd,
    /// `B` — fill then stroke. Consults the non-stroking colour for the
    /// fill and the stroking colour for the stroke, so **both** must be
    /// set before the path (§8.5 `B` NOTE).
    FillStroke,
    /// `b` — close, fill, then stroke (≡ `h B`).
    CloseFillStroke,
    /// `n` — end the path with no painting (used only to establish a clip
    /// with a preceding `W`/`W*`).
    NoPaint,
}

impl Paint {
    /// The operator keyword bytes.
    const fn keyword(self) -> &'static [u8] {
        match self {
            Self::Stroke => b"S",
            Self::CloseStroke => b"s",
            Self::Fill => b"f",
            Self::FillEvenOdd => b"f*",
            Self::FillStroke => b"B",
            Self::CloseFillStroke => b"b",
            Self::NoPaint => b"n",
        }
    }
}

/// A stateful builder that emits the bytes of a new content stream
/// (ISO 32000-1 §8.2 postfix operator syntax).
///
/// See the module docs for the W-E ordering contract the caller honours
/// and the one fusion guard the builder enforces. Construct with
/// [`ContentBuilder::new`], drive it with the operator methods, and take
/// the finished bytes with [`ContentBuilder::into_bytes`].
///
/// The finished bytes are a **raw** (unfiltered) content stream. Per WF2
/// the spec places no filter requirement on an appearance stream, and for
/// the small geometric appearances Pass 6.1 authors, raw is simpler and
/// more minimal-diff-friendly than Flate (nothing to reproduce
/// byte-identically on re-save) — a recorded convention, not a `shall`.
///
/// # Examples
///
/// The red 2-unit `Line` appearance from §8.10 W-F (W-E ordering: colour
/// and width precede `m`):
///
/// ```
/// use pdfcer_core::writer::content::{ContentBuilder, Paint};
///
/// let mut b = ContentBuilder::new();
/// b.set_stroke_rgb(1.0, 0.0, 0.0);
/// b.set_line_width(2.0);
/// b.move_to(10.0, 10.0);
/// b.line_to(90.0, 40.0);
/// b.paint(Paint::Stroke);
/// assert_eq!(b.into_bytes(), b"1 0 0 RG\n2 w\n10 10 m\n90 40 l\nS\n");
/// ```
#[derive(Debug, Default, Clone)]
pub struct ContentBuilder {
    out: Vec<u8>,
    /// Whether a path object is currently open (an `m`/`re` has been
    /// emitted and no paint operator has closed it yet). Used only for a
    /// `debug_assert` that catches a W-E ordering mistake in tests; it is
    /// never a runtime refusal (a content builder has no untrusted input).
    in_path: bool,
}

impl ContentBuilder {
    /// A fresh, empty builder. An empty content stream is itself a valid
    /// appearance (§8.10 W-C — the common `/Off` checkbox appearance), so
    /// a builder that is never driven produces conforming zero-byte output.
    #[must_use]
    pub fn new() -> Self {
        Self {
            out: Vec::new(),
            in_path: false,
        }
    }

    /// The finished content-stream bytes.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.out
    }

    /// The bytes emitted so far (for length checks before finishing).
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.out
    }

    /// Append already-formed content-stream bytes verbatim.
    ///
    /// The composition escape hatch, for splicing in a fragment some other
    /// generator produced — the case it exists for is baking a
    /// [`crate::vartext::build_variable_text`] block (a form field's value,
    /// a redaction's `/OverlayText`) into a larger stream, where
    /// re-emitting the text through this builder would mean a SECOND text
    /// layout implementation in the binary.
    ///
    /// # Contract
    ///
    /// The caller guarantees `bytes` is **self-contained and balanced**:
    /// every `q` matched by a `Q`, every `BT` by an `ET`, every `BMC`/`BDC`
    /// by an `EMC`, and no path left open. Nothing is parsed or validated
    /// here — this is a byte append. Passing an unbalanced fragment
    /// corrupts every operator that follows it in the stream, which is why
    /// the intended source is a generator with that guarantee in its own
    /// contract rather than hand-written bytes.
    ///
    /// Wrap the call in [`save_state`](Self::save_state) /
    /// [`restore_state`](Self::restore_state) if the fragment sets graphics
    /// state the surrounding content must not inherit.
    pub fn append_raw(&mut self, bytes: &[u8]) {
        debug_assert!(
            !self.in_path,
            "append_raw with an open path: the fragment would be spliced between a path \
             construction operator and its paint operator"
        );
        self.out.extend_from_slice(bytes);
    }

    // -- graphics state (§8.4.3) — must precede any path (W-E) ----------

    /// `w` — set the line width in form-space units (§8.4.3.2). A width of
    /// `0` denotes the thinnest line the device can render. Emit before
    /// beginning a path.
    pub fn set_line_width(&mut self, width: f64) {
        self.debug_assert_not_in_path("w");
        self.op1(width, b"w");
    }

    /// `J` — set the line-cap style (§8.4.3.3). Emit before beginning a
    /// path.
    pub fn set_line_cap(&mut self, cap: LineCap) {
        self.debug_assert_not_in_path("J");
        self.emit_int(cap as i64);
        self.finish_op(b"J");
    }

    /// `j` — set the line-join style (§8.4.3.4). Emit before beginning a
    /// path.
    pub fn set_line_join(&mut self, join: LineJoin) {
        self.debug_assert_not_in_path("j");
        self.emit_int(join as i64);
        self.finish_op(b"j");
    }

    /// `M` — set the miter limit (§8.4.3.5). Relevant only when the join
    /// style is miter.
    pub fn set_miter_limit(&mut self, limit: f64) {
        self.debug_assert_not_in_path("M");
        self.op1(limit, b"M");
    }

    /// `d` — set the dash pattern (§8.4.3.6): an array of on/off run
    /// lengths and a phase. `set_dash(&[], 0.0)` emits `[] 0 d`, the solid
    /// line. Dash elements must be non-negative and not all zero (§8.4.3.6)
    /// — a caller invariant, not enforced here.
    pub fn set_dash(&mut self, pattern: &[f64], phase: f64) {
        self.debug_assert_not_in_path("d");
        self.out.push(b'[');
        for (i, v) in pattern.iter().enumerate() {
            if i > 0 {
                self.out.push(b' ');
            }
            emit_number(&mut self.out, *v);
        }
        self.out.push(b']');
        self.out.push(b' ');
        self.operand(phase);
        self.finish_op(b"d");
    }

    /// `/name gs` — apply the named `ExtGState` resource (§8.4.5). Used by
    /// the Highlight appearance to select `/BM /Multiply`. The name must be
    /// present in the appearance stream's own `/Resources` `/ExtGState`
    /// (X8: an appearance stream is a closed resource world). Emit before
    /// beginning a path.
    pub fn set_ext_gstate(&mut self, name: &[u8]) {
        self.debug_assert_not_in_path("gs");
        write_name(&mut self.out, &crate::object::Name(name.to_vec()));
        self.out.push(b' ');
        self.finish_op(b"gs");
    }

    // -- colour (§8.6) — must precede any path (W-E) --------------------

    /// `RG` — set the stroking colour to a DeviceRGB triple (0.0–1.0). The
    /// operator sets both colour space and value; no `CS` and no
    /// `ColorSpace` resource is needed for a device colour (WF5).
    pub fn set_stroke_rgb(&mut self, r: f64, g: f64, b: f64) {
        self.debug_assert_not_in_path("RG");
        self.op3(r, g, b, b"RG");
    }

    /// `rg` — set the non-stroking (fill) colour to a DeviceRGB triple.
    pub fn set_fill_rgb(&mut self, r: f64, g: f64, b: f64) {
        self.debug_assert_not_in_path("rg");
        self.op3(r, g, b, b"rg");
    }

    /// `G` — set the stroking colour to a DeviceGray value (0=black,
    /// 1=white).
    pub fn set_stroke_gray(&mut self, gray: f64) {
        self.debug_assert_not_in_path("G");
        self.op1(gray, b"G");
    }

    /// `g` — set the non-stroking (fill) colour to a DeviceGray value.
    pub fn set_fill_gray(&mut self, gray: f64) {
        self.debug_assert_not_in_path("g");
        self.op1(gray, b"g");
    }

    /// `K` — set the stroking colour to a DeviceCMYK quadruple
    /// (`0 0 0 0`=white, `0 0 0 1`=black).
    pub fn set_stroke_cmyk(&mut self, c: f64, m: f64, y: f64, k: f64) {
        self.debug_assert_not_in_path("K");
        self.op4(c, m, y, k, b"K");
    }

    /// `k` — set the non-stroking (fill) colour to a DeviceCMYK quadruple.
    pub fn set_fill_cmyk(&mut self, c: f64, m: f64, y: f64, k: f64) {
        self.debug_assert_not_in_path("k");
        self.op4(c, m, y, k, b"k");
    }

    // -- path construction (§8.5.2) — the W-E boundary ------------------

    /// `m` — begin a new subpath at `(x, y)` (§8.5.2.1). This is the
    /// operator that enters path-object state: **every** colour and
    /// graphics-state parameter the path needs must already have been
    /// emitted (W-E). Consecutive `m` collapse (last wins).
    pub fn move_to(&mut self, x: f64, y: f64) {
        self.in_path = true;
        self.op2(x, y, b"m");
    }

    /// `l` — add a line segment from the current point to `(x, y)`.
    pub fn line_to(&mut self, x: f64, y: f64) {
        self.op2(x, y, b"l");
    }

    /// `c` — add a cubic Bézier with control points `(x1,y1)`, `(x2,y2)`
    /// and endpoint `(x3,y3)` (§8.5.2.2). A generator emits full `c`
    /// rather than the abbreviated `v`/`y` to avoid their operand-ordering
    /// confusion.
    #[allow(clippy::too_many_arguments)]
    pub fn curve_to(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) {
        self.operand(x1);
        self.operand(y1);
        self.operand(x2);
        self.operand(y2);
        self.operand(x3);
        self.operand(y3);
        self.finish_op(b"c");
    }

    /// `re` — append a complete rectangular subpath with lower-left corner
    /// `(x, y)` and the given width and height (§8.5.2.1). Like `m`, this
    /// enters path-object state, so all state must precede it (W-E). It is
    /// its own complete subpath (ends with an implicit `h`).
    pub fn rect(&mut self, x: f64, y: f64, width: f64, height: f64) {
        self.in_path = true;
        self.operand(x);
        self.operand(y);
        self.operand(width);
        self.operand(height);
        self.finish_op(b"re");
    }

    /// `h` — close the current subpath (§8.5.2.1) with a straight segment
    /// back to its start. Use to close a Polygon.
    pub fn close_subpath(&mut self) {
        self.finish_op(b"h");
    }

    /// `W` — intersect the current clipping path with the current path,
    /// nonzero winding (§8.5.4). Legal only inside a path object, and it
    /// does **not** end the path: a paint operator (usually
    /// [`Paint::NoPaint`], `n`) must follow. Used by the variable-text
    /// generator to clip the shown text to the appearance `/BBox`
    /// (§12.7.3.3 `/Tx BMC` body).
    pub fn clip_nonzero(&mut self) {
        self.finish_op(b"W");
    }

    // -- path painting (§8.5.3) — returns to page-description level ------

    /// Emit the single paint operator that ends the current path object
    /// (§8.2 Figure 9). Returns the builder to page-description level, so
    /// the next colour/graphics-state operator is legal again.
    pub fn paint(&mut self, op: Paint) {
        self.finish_op(op.keyword());
        self.in_path = false;
    }

    // -- graphics-state stack (§8.4.2) ----------------------------------

    /// `q` — save the current graphics state onto the stack (§8.4.2). The
    /// variable-text `/Tx BMC` body wraps its clip + text in `q … Q` so
    /// the clip does not leak past the appearance (§12.7.3.3 template).
    pub fn save_state(&mut self) {
        self.finish_op(b"q");
    }

    /// `Q` — restore the graphics state from the stack (§8.4.2).
    pub fn restore_state(&mut self) {
        self.finish_op(b"Q");
    }

    /// `a b c d e f cm` — concatenate a matrix onto the current
    /// transformation matrix (§8.3.4). Emitted at page-description level
    /// (outside a path object); used to translate a composed sub-appearance
    /// (e.g. centring a stamp's label band within its frame).
    #[allow(clippy::too_many_arguments)]
    pub fn concat_matrix(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) {
        self.debug_assert_not_in_path("cm");
        self.operand(a);
        self.operand(b);
        self.operand(c);
        self.operand(d);
        self.operand(e);
        self.operand(f);
        self.finish_op(b"cm");
    }

    /// `/name Do` — paint the named external object (§8.10.1). For Pass 7.1
    /// flatten, `name` is a form-XObject resource (an existing widget `/AP`
    /// `/N` appearance) present in the page's `/Resources` `/XObject`
    /// sub-dictionary; the `Do` procedure itself concatenates that form's
    /// `/Matrix` and clips to its `/BBox`, so the caller emits only the
    /// §12.5.5 placement `cm` beforehand (never the `/Matrix` again — the
    /// double-apply trap). Emitted at page-description level.
    pub fn invoke_xobject(&mut self, name: &[u8]) {
        self.debug_assert_not_in_path("Do");
        write_name(&mut self.out, &crate::object::Name(name.to_vec()));
        self.out.push(b' ');
        self.finish_op(b"Do");
    }

    // -- marked content (§14.6) -----------------------------------------

    /// `/tag BMC` — begin a marked-content sequence (§14.6.1). The
    /// variable-text generator tags its body `/Tx` so a future
    /// value-update can find and replace it (§12.7.3.3 update algorithm
    /// keys on the `/Tx BMC … EMC` span).
    pub fn begin_marked_content(&mut self, tag: &[u8]) {
        write_name(&mut self.out, &crate::object::Name(tag.to_vec()));
        self.out.push(b' ');
        self.finish_op(b"BMC");
    }

    /// `EMC` — end the innermost marked-content sequence (§14.6.1).
    pub fn end_marked_content(&mut self) {
        self.finish_op(b"EMC");
    }

    // -- text objects (§9.4) --------------------------------------------

    /// `BT` — begin a text object (§9.4.1). Resets the text matrix and
    /// text line matrix to identity. Text-showing and text-state
    /// operators are legal only between `BT` and `ET`.
    pub fn begin_text(&mut self) {
        self.finish_op(b"BT");
    }

    /// `ET` — end the current text object (§9.4.1).
    pub fn end_text(&mut self) {
        self.finish_op(b"ET");
    }

    /// `/name size Tf` — set the text font and size (§9.3.1). `name` is a
    /// resource name present in the stream's `/Resources` `/Font`
    /// sub-dictionary (X8: an appearance is a closed resource world).
    /// This is the one operator §12.7.3.3 mandates a `/DA` string carry.
    pub fn set_font(&mut self, name: &[u8], size: f64) {
        write_name(&mut self.out, &crate::object::Name(name.to_vec()));
        self.out.push(b' ');
        self.op1(size, b"Tf");
    }

    /// `mode Tr` — set the text rendering mode (§9.3.6, Table 106).
    ///
    /// The value pdfcer cares about most is **3, "neither fill nor stroke text
    /// (invisible)"** — the mechanism an OCR text layer uses to sit over a
    /// scanned image without painting anything. The page keeps the appearance
    /// of the original scan while the text becomes selectable, searchable and
    /// extractable.
    ///
    /// Two Table 106 rules a caller has to respect, neither of which this
    /// method can enforce:
    ///
    /// - Modes 4–7 accumulate a CLIPPING path, and §9.3.6 requires that the
    ///   mode "shall not be changed back to a nonclipping mode" before the
    ///   `ET` that ends the text object.
    /// - Text state is NOT reset by `BT` (§9.4.1 — only `Tm`/`Tlm` are), so a
    ///   mode set in one text object persists into the next one in the same
    ///   content stream. A caller that sets 3 and does not set it back has
    ///   made everything after it invisible too.
    ///
    /// Takes `u8` rather than an enum deliberately: Table 106 defines exactly
    /// eight values, but the operand is an integer and a malformed document
    /// may carry anything. This is the WRITER, so callers pass a literal from
    /// the table; the reader is where an out-of-range value gets a policy.
    pub fn set_render_mode(&mut self, mode: u8) {
        self.op1(f64::from(mode), b"Tr");
    }

    /// `l TL` — set the text leading (line spacing) in unscaled text
    /// units (§9.3.5). Consumed by `T*` and `'`; the variable-text
    /// generator uses explicit `Td` moves instead, but sets `TL` for
    /// readers that reflow.
    pub fn set_text_leading(&mut self, leading: f64) {
        self.op1(leading, b"TL");
    }

    /// `tx ty Td` — move to the start of the next line, offset `(tx, ty)`
    /// from the start of the current line, in unscaled text space
    /// (§9.4.2). Used for per-line positioning after the single `Tm`.
    pub fn text_move(&mut self, tx: f64, ty: f64) {
        self.op2(tx, ty, b"Td");
    }

    /// `tx ty TD` — like [`ContentBuilder::text_move`] but also sets the
    /// leading to `-ty` (§9.4.2).
    pub fn text_move_set_leading(&mut self, tx: f64, ty: f64) {
        self.op2(tx, ty, b"TD");
    }

    /// `a b c d e f Tm` — set the text matrix and text line matrix
    /// (§9.4.2). §12.7.3.3 allows **at most one** `Tm` in a `/DA` string;
    /// pdfcer authors zero in the `/DA` and emits exactly one here to set
    /// the first line's origin, then positions subsequent lines with
    /// `Td` (which is not a `Tm`).
    #[allow(clippy::too_many_arguments)]
    pub fn set_text_matrix(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) {
        self.operand(a);
        self.operand(b);
        self.operand(c);
        self.operand(d);
        self.operand(e);
        self.operand(f);
        self.finish_op(b"Tm");
    }

    /// `tc Tc` — set the character spacing (§9.3.2), added to each glyph's
    /// advance.
    pub fn set_char_spacing(&mut self, tc: f64) {
        self.op1(tc, b"Tc");
    }

    /// `tw Tw` — set the word spacing (§9.3.3), added to the advance of
    /// each single-byte code 32 (space).
    pub fn set_word_spacing(&mut self, tw: f64) {
        self.op1(tw, b"Tw");
    }

    /// `scale Tz` — set the horizontal scaling as a percentage (§9.3.4;
    /// `100` = normal).
    pub fn set_horizontal_scaling(&mut self, percent: f64) {
        self.op1(percent, b"Tz");
    }

    /// `(string) Tj` — show a text string (§9.4.3). `bytes` are the raw
    /// character codes (for the variable-text generator, `WinAnsi` bytes);
    /// they are emitted as a §7.3.4.2 literal string with `(`, `)` and `\`
    /// escaped and every non-printable or high byte written as a
    /// three-digit octal escape, so the content stream stays ASCII and
    /// paren-balanced regardless of the text.
    pub fn show_text(&mut self, bytes: &[u8]) {
        emit_literal_string(&mut self.out, bytes);
        self.out.push(b' ');
        self.finish_op(b"Tj");
    }

    // -- internal token emitters ---------------------------------------

    /// Emit one integer operand followed by the SPACE that separates it
    /// from what comes next (§7.3.3: no sign, no padding). The SPACE is
    /// mandatory before an operator or a following operand so tokens never
    /// fuse (`1 0` must not become `10`).
    fn emit_int(&mut self, v: i64) {
        self.out.extend_from_slice(v.to_string().as_bytes());
        self.out.push(b' ');
    }

    /// Emit one numeric operand followed by its separating SPACE.
    fn operand(&mut self, v: f64) {
        emit_number(&mut self.out, v);
        self.out.push(b' ');
    }

    /// Terminate an operation with the operator keyword and a LF. Any
    /// operands were each emitted with a trailing SPACE, so the keyword is
    /// already separated; a zero-operand operator (`S`, `h`, `n`) sits on
    /// its own line. One operation per line keeps authored streams
    /// `diff`-friendly and is the form the §8.10 W-F worked examples use.
    fn finish_op(&mut self, keyword: &[u8]) {
        self.out.extend_from_slice(keyword);
        self.out.push(b'\n');
    }

    /// One numeric operand + operator (`v op`).
    fn op1(&mut self, v: f64, keyword: &[u8]) {
        self.operand(v);
        self.finish_op(keyword);
    }

    /// Two numeric operands + operator (`a b op`).
    fn op2(&mut self, a: f64, b: f64, keyword: &[u8]) {
        self.operand(a);
        self.operand(b);
        self.finish_op(keyword);
    }

    /// Three numeric operands + operator (`a b c op`).
    fn op3(&mut self, a: f64, b: f64, c: f64, keyword: &[u8]) {
        self.operand(a);
        self.operand(b);
        self.operand(c);
        self.finish_op(keyword);
    }

    /// Four numeric operands + operator (`a b c d op`).
    fn op4(&mut self, a: f64, b: f64, c: f64, d: f64, keyword: &[u8]) {
        self.operand(a);
        self.operand(b);
        self.operand(c);
        self.operand(d);
        self.finish_op(keyword);
    }

    /// Debug-only W-E ordering guard: colour/graphics-state operators are
    /// illegal inside a path object (§8.2 Figure 9). A content builder has
    /// no untrusted input, so this is an implementation-bug tripwire in
    /// tests, never a runtime refusal.
    fn debug_assert_not_in_path(&self, op: &str) {
        debug_assert!(
            !self.in_path,
            "W-E ordering: {op} is illegal inside a path object; emit it before move_to/rect"
        );
    }
}

/// Emit a number operand in canonical content-stream form (§7.3.3,
/// A1–A3): integer form for an integral value, real form otherwise.
///
/// Integral values within the `i64` range emit without a decimal point
/// (`10`, not `10.0`) — compact, and the spelling the §8.10 W-F examples
/// use. Everything else routes through [`write_real`], which guarantees
/// fixed-point (never exponential) output and degrades a non-finite value
/// to `0.0` rather than emitting a token no reader can parse.
pub(crate) fn emit_number(out: &mut Vec<u8>, v: f64) {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 9.007_199_254_740_992e15 {
        // Exactly representable as an integer; emit integer form.
        out.extend_from_slice((v as i64).to_string().as_bytes());
    } else {
        write_real(out, v);
    }
}

/// Emit `bytes` as a §7.3.4.2 literal string operand `(…)` for a `Tj`
/// showing operator. `(`, `)` and `\` are backslash-escaped; every byte
/// outside the printable ASCII range `0x20..=0x7E` is written as a
/// three-digit octal escape (`\ooo`). This keeps an authored content
/// stream ASCII and paren-balanced for arbitrary (e.g. `WinAnsi`
/// high-byte) text, at the cost of never using the raw-byte form a
/// hand-written stream might — a deterministic, reader-safe choice for a
/// generator (the raw high bytes and the octal escape denote the same
/// string per §7.3.4.2).
pub(crate) fn emit_literal_string(out: &mut Vec<u8>, bytes: &[u8]) {
    out.push(b'(');
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(b);
            }
            0x20..=0x7E => out.push(b),
            other => {
                // Three-digit octal escape, always three digits so a
                // following digit cannot extend it (§7.3.4.2).
                out.push(b'\\');
                out.push(b'0' + (other >> 6));
                out.push(b'0' + ((other >> 3) & 0o7));
                out.push(b'0' + (other & 0o7));
            }
        }
    }
    out.push(b')');
}

/// The R46 identity gate's re-emission: reconstruct a parsed content
/// stream's bytes, re-emitting **numeric operands from their parsed value**
/// and copying **everything else verbatim** from the source span.
///
/// ## Why this exact split is the mechanically self-enforcing X6 guard
///
/// X6 is silent content-stream normalization: a serializer that emits
/// `1.0` as `1`, drops a leading `+`, collapses whitespace, or reorders
/// operands produces a plausible, working, **wrong** stream that passes
/// every structural check. The geometric serializer this Pass ships emits
/// exactly one class of thing that carries a normalization hazard —
/// **numbers** (coordinates, colours, widths). So the gate re-emits every
/// numeric operand in the corpus through the same [`emit_number`] /
/// [`write_real`] path the authoring builder uses, and copies strings,
/// names, arrays, dictionaries, inline images, operators, whitespace and
/// comments **verbatim** (a geometric serializer must never touch those,
/// and re-emitting a `TJ` array's internal spacing from value would drown
/// the signal in legal-whitespace noise).
///
/// The result: a stream whose numbers are already in canonical form
/// re-emits **byte-identically**, and one that is not is enumerated by the
/// caller with the first divergence as its reason (`1.` → `1.0`, `+5` →
/// `5`, `.5` → `0.5`). If a future edit to [`write_real`] or
/// [`emit_number`] ever normalizes a value it should not, the byte-compare
/// flips red across hundreds of corpus files at once — the same mechanism
/// that made the object-writer's R33 self-enforcing, one level down (R46).
///
/// The returned bytes are compared by the caller against `stream.buf` (the
/// decoded content buffer every token span indexes).
#[must_use]
pub fn reemit_canonical(stream: &ContentStream) -> Vec<u8> {
    let buf = &stream.buf;
    let mut out = Vec::with_capacity(buf.len());
    let mut cursor = 0usize;
    for token in &stream.tokens {
        // Inter-token bytes (whitespace, comments) are not part of any
        // token span; copy them verbatim so the gate is never confounded
        // by legal formatting a serializer must preserve.
        if let Some(gap) = buf.get(cursor..token.span.start) {
            out.extend_from_slice(gap);
        }
        emit_token_canonical(&mut out, buf, token);
        cursor = token.span.end();
    }
    // Trailing bytes after the last token (final whitespace/comment).
    if let Some(tail) = buf.get(cursor..) {
        out.extend_from_slice(tail);
    }
    out
}

/// Re-emit one token: a numeric operand from its parsed value (the X6
/// target), everything else verbatim from its source span.
fn emit_token_canonical(out: &mut Vec<u8>, buf: &[u8], token: &ContentToken) {
    let verbatim = |out: &mut Vec<u8>| {
        if let Some(bytes) = token.span.slice(buf) {
            out.extend_from_slice(bytes);
        }
    };
    match &token.kind {
        ContentTokenKind::Operand(Object::Integer(v)) => {
            out.extend_from_slice(v.to_string().as_bytes());
        }
        ContentTokenKind::Operand(Object::Real(v)) => {
            write_real(out, *v);
        }
        // Strings, names, booleans, null, arrays and dictionaries re-emit
        // verbatim: a geometric serializer authors none of them, and
        // re-emitting a string's escaping or an array's internal spacing
        // from value would be normalization the gate must not itself
        // introduce. Names in particular are non-canonical on read
        // (`/A#42` ≡ `/AB`, §7.3.5 NOTE 1) and must survive byte-exact.
        ContentTokenKind::Operand(_) | ContentTokenKind::Operator => verbatim(out),
        // An inline image is one indivisible token including its raw data
        // bytes; always verbatim.
        ContentTokenKind::InlineImage { .. } => verbatim(out),
    }
}

/// Classify why a re-emitted token diverged from its source bytes, for the
/// R46 gate's by-file/by-reason enumeration (R20). Returns `None` when the
/// token re-emits identically. Only numeric operands can diverge (the only
/// tokens [`reemit_canonical`] re-emits from value), so the reason is
/// always a number-spelling normalization.
#[must_use]
pub fn number_divergence_reason(buf: &[u8], token: &ContentToken) -> Option<String> {
    let source = token.span.slice(buf)?;
    let mut canonical = Vec::new();
    match &token.kind {
        ContentTokenKind::Operand(Object::Integer(v)) => {
            canonical.extend_from_slice(v.to_string().as_bytes());
        }
        ContentTokenKind::Operand(Object::Real(v)) => write_real(&mut canonical, *v),
        _ => return None,
    }
    if canonical == source {
        return None;
    }
    Some(format!(
        "{} -> {}",
        String::from_utf8_lossy(source),
        String::from_utf8_lossy(&canonical)
    ))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests {
    use super::*;

    fn s(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[test]
    fn empty_builder_is_a_valid_empty_appearance() {
        // §8.10 W-C: the empty content stream is a conforming appearance.
        assert!(ContentBuilder::new().into_bytes().is_empty());
    }

    #[test]
    fn line_worked_example_matches_the_spec() {
        // §8.10 W-F red 2-unit Line: W-E ordering, colour + width before m.
        let mut b = ContentBuilder::new();
        b.set_stroke_rgb(1.0, 0.0, 0.0);
        b.set_line_width(2.0);
        b.move_to(10.0, 10.0);
        b.line_to(90.0, 40.0);
        b.paint(Paint::Stroke);
        assert_eq!(s(&b.into_bytes()), "1 0 0 RG\n2 w\n10 10 m\n90 40 l\nS\n");
    }

    #[test]
    fn filled_stroked_square_sets_both_colours_before_the_path() {
        // §8.10 W-F: B fills (non-stroking colour) then strokes (stroking).
        let mut b = ContentBuilder::new();
        b.set_fill_rgb(1.0, 1.0, 0.0);
        b.set_stroke_rgb(0.0, 0.0, 1.0);
        b.set_line_width(2.0);
        b.rect(1.0, 1.0, 98.0, 58.0);
        b.paint(Paint::FillStroke);
        assert_eq!(
            s(&b.into_bytes()),
            "1 1 0 rg\n0 0 1 RG\n2 w\n1 1 98 58 re\nB\n"
        );
    }

    #[test]
    fn integral_values_emit_integer_form_reals_keep_the_point() {
        let mut out = Vec::new();
        emit_number(&mut out, 10.0);
        emit_number(&mut out, 0.5);
        emit_number(&mut out, -3.0);
        assert_eq!(s(&out), "100.5-3");
    }

    #[test]
    fn number_emission_never_uses_exponential_notation() {
        // §7.3.3 has no exponent; the write_real path must expand it.
        let mut out = Vec::new();
        emit_number(&mut out, 0.000_002);
        assert!(!s(&out).contains('e') && !s(&out).contains('E'));
    }

    #[test]
    fn dash_solid_and_patterned() {
        let mut b = ContentBuilder::new();
        b.set_dash(&[], 0.0);
        b.set_dash(&[3.0, 2.0], 1.0);
        assert_eq!(s(&b.into_bytes()), "[] 0 d\n[3 2] 1 d\n");
    }

    #[test]
    fn caps_joins_and_gs_emit_their_codes_and_names() {
        let mut b = ContentBuilder::new();
        b.set_line_cap(LineCap::Round);
        b.set_line_join(LineJoin::Bevel);
        b.set_ext_gstate(b"GS0");
        assert_eq!(s(&b.into_bytes()), "1 J\n2 j\n/GS0 gs\n");
    }

    #[test]
    fn curve_emits_six_operands() {
        let mut b = ContentBuilder::new();
        b.move_to(0.0, 0.0);
        b.curve_to(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
        b.paint(Paint::Fill);
        assert_eq!(s(&b.into_bytes()), "0 0 m\n1 2 3 4 5 6 c\nf\n");
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "W-E ordering")]
    fn colour_inside_a_path_object_trips_the_debug_guard() {
        // The non-conforming counter-example from §8.10 W-E:
        // `10 10 m  ... RG` — colour after m is illegal.
        let mut b = ContentBuilder::new();
        b.move_to(10.0, 10.0);
        b.set_stroke_rgb(1.0, 0.0, 0.0);
    }

    // -- R46 identity-gate re-emission ---------------------------------

    fn reemit(input: &[u8]) -> Vec<u8> {
        let cs = ContentStream::parse(input.to_vec()).unwrap();
        reemit_canonical(&cs)
    }

    #[test]
    fn canonical_stream_reemits_byte_identically() {
        // Integers, canonical reals, names, strings, arrays, operators and
        // varied whitespace all survive verbatim.
        for input in [
            &b"q 1 0 0 1 72 712 cm BT /F1 12 Tf (Hello) Tj ET Q"[..],
            b"0.5 0 0 0.5 0 0 cm /Im1 Do",
            b"[(He)-20(llo)] TJ",
            b"1 0 0 RG\n2 w\n10 10 m\n90 40 l\nS\n",
            b"% a comment\n1 1 1 rg\n0 0 10 10 re f\n",
        ] {
            assert_eq!(reemit(input), input, "{}", s(input));
        }
    }

    #[test]
    fn non_canonical_numbers_are_normalized_and_flagged() {
        // `1.` -> `1.0`, `+5` -> `5`, `.5` -> `0.5`: the X6 targets. Each
        // is a byte change, and number_divergence_reason names it.
        let cs = ContentStream::parse(b"1. +5 .5 re".to_vec()).unwrap();
        assert_ne!(reemit_canonical(&cs), cs.buf);
        let reasons: Vec<String> = cs
            .tokens
            .iter()
            .filter_map(|t| number_divergence_reason(&cs.buf, t))
            .collect();
        assert_eq!(reasons, vec!["1. -> 1.0", "+5 -> 5", ".5 -> 0.5"]);
    }

    #[test]
    fn inline_image_reemits_verbatim_including_its_data() {
        // The image data bytes must not be re-interpreted as tokens.
        let input: &[u8] = b"BI /W 2 /H 2 /CS /G /BPC 8 ID \x45\x49\x00\xFF EI Q";
        assert_eq!(reemit(input), input);
    }

    #[test]
    fn a_string_with_non_canonical_number_neighbours_survives() {
        // Names are non-canonical on read (§7.3.5) — must be verbatim.
        let input: &[u8] = b"/A#42 12 Tf";
        assert_eq!(reemit(input), input);
    }
}
