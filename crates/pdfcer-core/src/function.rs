//! # `function` — PDF function objects (ISO 32000-1 §7.10)
//!
//! A PDF *function* is a static, self-contained numerical transformation:
//! *m* input numbers in, *n* output numbers out, no side effects, no state.
//! §7.10's opening paragraph is emphatic that this is **not** a programming
//! facility — *"PDF is not a programming language, and a PDF file is not a
//! program"* — and that framing is load-bearing for this module. Everything
//! here is a pure function of its inputs; nothing reads the document, the
//! graphics state, or the clock.
//!
//! ## Why this module exists at all
//!
//! Nothing in pdfcer could evaluate a function before this module, and that
//! single gap blocked four unrelated features at once. A function is the
//! representation the standard reaches for whenever a value has to be
//! *computed* rather than looked up:
//!
//! | Consumer | Clause | What the function does there |
//! |---|---|---|
//! | `Separation` colour space | §8.6.6.4 | tint → alternate-space components |
//! | `DeviceN` colour space | §8.6.6.5 | *k* tints → alternate-space components |
//! | Shading dictionaries | §8.7.4 | parametric coordinate → colour |
//! | Soft-mask transfer function (`/TR`) | §11.6.5.2 | mask value → mask value |
//! | ExtGState `/TR`, `/TR2`, `/BG`, `/UCR` | §8.4.5 | tone/black-generation curves |
//! | Halftone spot functions, `/DotGain` | §10.5.3, §8.6.6.5 | screening |
//!
//! One evaluator serves all of them, which is the reason this lives in
//! `pdfcer-core` rather than beside any one consumer. A second, specialised
//! copy would drift, and a tint transform that disagrees with a shading's
//! transform over the same function object is a defect nobody would think to
//! look for.
//!
//! ## The four types, and the missing one
//!
//! | `/FunctionType` | Name | PDF | Carried as | `/Range` |
//! |---|---|---|---|---|
//! | **0** | Sampled | 1.2 | **stream** | **Required** |
//! | **2** | Exponential interpolation | 1.3 | dictionary | Optional |
//! | **3** | Stitching | 1.3 | dictionary | Optional |
//! | **4** | PostScript calculator | 1.3 | **stream** | **Required** |
//!
//! There is **no type 1**. The gap is real, not a transcription slip, and
//! [`FunctionError::UnknownFunctionType`] reports `1` exactly like it reports
//! `7` — this module never invents a meaning for it.
//!
//! `/Range` is required for types 0 and 4 because *n* cannot be recovered any
//! other way (§7.10.1: *"The number of output values can usually be inferred
//! from other attributes of the function; if not (as is always the case for
//! type 0 and type 4 functions), the `Range` entry is required"*). For types 2
//! and 3 it is optional, and **its absence means no output clipping at all** —
//! see the clamping section below, because getting that backwards is the
//! single most damaging mistake available in this file.
//!
//! ## Clamping is specified, not optional
//!
//! Table 38, on `/Domain`: *"Input values outside the declared domain **shall**
//! be clipped to the nearest boundary value."*
//!
//! Table 38, on `/Range`: *"Output values outside the declared range **shall**
//! be clipped to the nearest boundary value. **If this entry is absent, no
//! clipping shall be done.**"*
//!
//! Both are `shall`s, and both are implemented once, centrally, in
//! [`PdfFunction::eval_into`] — not per type — so no type can be written that
//! forgets one. The asymmetry in the second rule is why [`PdfFunction::range`]
//! returns `Option` rather than a defaulted `[0, 1]` per output: a shading
//! function whose outputs were silently squeezed into the unit interval
//! produces colour that is wrong in a way that still *looks* like colour.
//!
//! A tint of `1.4` is therefore evaluated as `1.0`, and a CMYK component of
//! `-0.2` coming out of a transform with a `/Range` of `[0 1 0 1 0 1 0 1]` is
//! clamped to `0.0`. Neither is a tolerance; both are the standard.
//!
//! ## Refusal posture: a wrong colour is worse than no colour
//!
//! §7.10.5.2 is the only place in §7.10 that addresses failure, it covers
//! **type 4 only**, and it explicitly declines to say what a reader should do:
//! *"This specification does not define a representation for the errors; those
//! details shall be provided by the conforming reader."* For types 0, 2 and 3
//! the standard says nothing at all about a truncated sample stream, a
//! `/Bounds` array out of order, or a negative `/N` with `x = 0`.
//!
//! So the failure behaviour here is **pdfcer policy, chosen deliberately**, and
//! recorded as such (the spec RAG files this as negative result `F-N1` in
//! `iso32000__s__7.10.md`): every malformed function is **refused by name**.
//! [`FunctionError`] never degrades to a plausible substitute — no all-zero
//! output vector, no identity transform, no black. The reason is specific to
//! colour: an empty output vector read as "all components zero" is *white* in
//! `DeviceCMYK` and *black* in `DeviceGray`, so any silent substitution is
//! guaranteed to be catastrophically wrong for half of its callers while
//! looking entirely reasonable. A named error lets the caller disclose the
//! failure (project rule 4, fuzzy-never-sneaky); a fabricated colour cannot be
//! distinguished from a real one downstream.
//!
//! ## Resource bounds
//!
//! Functions are attacker-controlled input (`ARCHITECTURE.md` §10). Every loop
//! in this module is bounded:
//!
//! | Bound | Value | Source |
//! |---|---|---|
//! | [`PS_STACK_LIMIT`] | 100 | **spec** — §7.10.5.1 `shall`-minimum |
//! | [`MAX_PS_STEPS`] | 1,000,000 | **pdfcer policy** — no spec equivalent |
//! | [`MAX_PS_NESTING`] | 32 | **pdfcer policy** — no spec equivalent |
//! | [`MAX_SAMPLED_INPUTS`] | 8 | **pdfcer policy**, spec-sanctioned (§7.10.2) |
//! | [`MAX_FUNCTION_DEPTH`] | 8 | **pdfcer policy** — no spec equivalent |
//!
//! The distinction between the first row and the rest is not pedantry. The
//! 100-entry stack is a conformance fact: §7.10.5.1 says an implementation
//! *"shall provide a stack with room for at least 100 entries"*, that *"no
//! implementation shall be required to provide a larger stack"*, and that it
//! *"shall be an error to overflow the stack"*. A fixed 100-entry stack that
//! errors on overflow is therefore **exactly conformant**, and growing it
//! dynamically would not be robustness — it would be inventing a dialect that
//! only pdfcer can run.
//!
//! The step and nesting caps have no such backing. ISO 32000-1 imposes no
//! limit on how many operators a type 4 program may execute, so a program that
//! never terminates is *legal* and a conforming reader must still not hang.
//! [`MAX_PS_STEPS`] is pdfcer's answer to that, and it is a policy number that
//! may be tuned; it is not a claim about the standard.
//!
//! [`MAX_SAMPLED_INPUTS`] sits between the two. §7.10.2 says there is *"no
//! dimensionality limit of a sampled function **except for possible
//! implementation limits**"* — an explicit licence to impose one. Eight is
//! chosen because multilinear interpolation visits 2^m corners per evaluation
//! (256 at m = 8) and a `DeviceN` image runs the transform once per pixel; the
//! spec's own worked example of a hard case is a six-component hexachrome.
//!
//! ## Spec sources
//!
//! - `iso32000__s__7.10.md` — §7.10 in full: Tables 38–42, the type 0
//!   evaluation algorithm, the type 3 partition rule, §7.10.5.2's error list,
//!   and the recorded ambiguities `F-A1` (cubic spline unspecified), `F-A2`
//!   (multilinear named only in an informative NOTE) and `F-N1`/`F-N2` (no
//!   fallback for an unevaluatable function or an arity mismatch).
//! - `iso32000__annex__b.md` — **ISO 32000-1 Annex B, `(normative)`,
//!   "Operators in Type 4 Functions"**: the stack effect, arity and
//!   operand/result typing of all 42 Table 42 entries. §7.10.5 never
//!   cross-references it, so it is easy to conclude from the clause alone that
//!   the operator semantics are unsourceable inside ISO 32000-1. They are not.
//! - PLRM3 §8.2 (`_sources/Adobe_PLRM3_1999.pdf`) — what Annex B leaves out:
//!   rounding directions, sign conventions, integer preservation, `atan`'s
//!   `[0, 360)` normalisation, the zero-fill of a right shift. §7.10.5.1 points
//!   at PLRM3's *"Appendix B"* for this, which is a **misdirected
//!   cross-reference** (erratum `F-E3`) — that appendix is *Implementation
//!   Limits*; the operators are Chapter 8. PLRM3 is a Bibliography reference
//!   and therefore *informative*, so it is cited alongside Annex B, never
//!   instead of it.
//! - `iso32000__s__7.3.8.md` — stream extent, for the type 0 "long enough"
//!   rule.
//! - `iso32000__annex__c.md` — implementation limits, which type 4
//!   *intermediates* are explicitly exempt from.

use crate::filters::{self, FilterError};
use crate::graph::ObjectGraph as _;
use crate::lexer::{LexError, Lexer, TokenKind};
use crate::object::{Dict, Object, Stream};
use crate::view::DocumentView;

// ---------------------------------------------------------------------------
// Resource bounds
// ---------------------------------------------------------------------------

/// Operand-stack capacity for a type 4 program — **a spec value, not policy**.
///
/// §7.10.5.1: *"Implementations of type 4 functions shall provide a stack with
/// room for at least 100 entries. No implementation shall be required to
/// provide a larger stack, and it shall be an error to overflow the stack."*
///
/// Both halves matter. 100 is a floor pdfcer must meet, and it is simultaneously
/// a ceiling pdfcer is entitled to stop at — so overflowing at exactly 100 with
/// [`FunctionError::StackOverflow`] is conformant behaviour, not a limitation.
/// A program that needs 101 entries is malformed by the standard's own
/// definition; accommodating it would mean silently accepting files that no
/// other conforming reader accepts.
pub const PS_STACK_LIMIT: usize = 100;

/// Maximum operators a single type 4 evaluation may execute — **pdfcer policy**.
///
/// **ISO 32000-1 imposes no execution-step limit of any kind.** Annex C's
/// implementation limits cover object sizes and nesting in the file grammar,
/// and §7.10.5.1 explicitly grants type 4 *intermediate results* an exemption
/// from even those: *"the intermediate results in type 4 function computations
/// shall not [fall under those limits]. An implementation may use a
/// representation that exceeds those limits."* There is no clause to cite for
/// a step cap, which is exactly why this one is labelled policy.
///
/// **What it actually defends against, stated honestly.** Table 42 has no jump,
/// loop or repeat operator, and a `{ … }` block runs at most once per
/// `if`/`ifelse` it is attached to — so an evaluation's step count is bounded
/// above by the number of nodes in the parsed program. A type 4 function
/// therefore cannot hang. The hazard is *size*, not looping: a function stream
/// may decode to as much as [`crate::filters::MAX_DECODED_LEN`] (256 MiB), which
/// is room for tens of millions of operators, and a `DeviceN` image runs the
/// transform **once per pixel**. This cap keeps that product bounded.
///
/// One million steps is several orders of magnitude beyond any legitimate tint
/// transform (real ones run in tens of operators — the spec's own DoubleDot
/// example is nine) while keeping a hostile function's per-evaluation cost to
/// milliseconds. Tunable; not a conformance claim.
pub const MAX_PS_STEPS: usize = 1_000_000;

/// Maximum `{ }` nesting depth inside a type 4 program — **pdfcer policy**.
///
/// §7.10.5.1 describes the brace syntax for `if`/`ifelse` and states no depth
/// limit; Annex C's nesting limits are about the *file* grammar (arrays and
/// dictionaries), not this sub-language. The cap exists because the parser
/// recurses one frame per `{`, and `pdfcer-core`'s panic-free policy treats a
/// stack overflow from untrusted input exactly as seriously as an `unwrap` —
/// an abort is not a graceful refusal.
///
/// 32 is far past any hand-written or machine-generated transform; the deepest
/// realistic construct is a handful of nested `ifelse` branches.
pub const MAX_PS_NESTING: usize = 32;

/// Maximum input dimensionality of a type 0 sampled function — **pdfcer policy,
/// explicitly permitted by the spec**.
///
/// §7.10.2: *"There shall be no dimensionality limit of a sampled function
/// except for possible implementation limits."* That clause grants the limit;
/// it does not set it.
///
/// Eight is chosen from the cost of the evaluation itself. Multilinear
/// interpolation reads 2^m table corners per evaluation — 256 at m = 8, 65,536
/// at m = 16 — and a `DeviceN` image runs the transform once per pixel, so the
/// exponent is multiplied by megapixels. The spec's own illustration of an
/// expensive case is a six-component hexachrome `DeviceN`, which fits.
pub const MAX_SAMPLED_INPUTS: usize = 8;

/// Maximum nesting depth of type 3 stitching sub-functions — **pdfcer policy**.
///
/// A type 3 function's `/Functions` array may itself contain type 3 functions,
/// and nothing in §7.10.4 forbids a cycle through indirect references
/// (`5 0 obj << /FunctionType 3 /Functions [5 0 R] … >>` is syntactically
/// legal). Without a depth guard, loading that recurses forever.
///
/// This is the same class of guard `ARCHITECTURE.md` §10 requires on every
/// recursive structure walker. Eight levels is far beyond the one or two
/// levels real stitching functions use.
pub const MAX_FUNCTION_DEPTH: usize = 8;

/// Sample widths `/BitsPerSample` may take (§7.10.2 Table 39).
///
/// *"Valid values shall be 1, 2, 4, 8, 12, 16, 24, and 32."* Note what is
/// **not** in the list: 3, 5, 6, 10, 64. The set is closed, so
/// [`FunctionError::BadBitsPerSample`] is a refusal with spec backing rather
/// than an implementation gap.
pub const VALID_BITS_PER_SAMPLE: [u32; 8] = [1, 2, 4, 8, 12, 16, 24, 32];

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Everything that can stop a function from loading or evaluating.
///
/// Follows the Rust API Guidelines' `C-GOOD-ERR`: implements
/// [`std::error::Error`] through `thiserror` and is `Send + Sync + 'static`.
/// `#[non_exhaustive]` so later work (a cubic-spline evaluator, a type 5) can
/// add variants without a breaking change.
///
/// Every variant names *what* was wrong specifically enough that a caller can
/// put it in front of the operator. That is the whole point — see the module
/// docs' refusal-posture section for why there is no `Unknown` catch-all and no
/// silent degradation.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum FunctionError {
    /// The object is not a dictionary or a stream, so it cannot be a function
    /// (§7.10.1: every type is one or the other).
    #[error("function object is neither a dictionary nor a stream")]
    NotAFunction,

    /// `/FunctionType` is absent or is not an integer.
    #[error("/FunctionType is missing or not an integer")]
    MissingFunctionType,

    /// `/FunctionType` held a value §7.10 does not define. **`1` reaches here
    /// too** — there is no type 1 (module docs).
    #[error("unknown /FunctionType {0} (ISO 32000-1 §7.10 defines 0, 2, 3 and 4 only)")]
    UnknownFunctionType(i64),

    /// The type requires a stream (types 0 and 4 carry their payload as stream
    /// data) but the object was a bare dictionary.
    #[error("/FunctionType {function_type} must be a stream, not a dictionary")]
    NotAStream {
        /// The declared `/FunctionType`.
        function_type: i64,
    },

    /// The stream's byte span could not be served by the view — a truncated
    /// file or a span from a different document (see
    /// [`crate::view::StreamSource::slice`]).
    #[error("the function stream's data span could not be read from this document")]
    StreamUnreadable,

    /// The stream's `/Filter` chain failed to decode.
    #[error("function stream decode failed: {0}")]
    Filter(#[from] FilterError),

    /// A required entry is absent.
    #[error("/{key} is required for /FunctionType {function_type} and is missing")]
    MissingEntry {
        /// The absent key, without its leading solidus.
        key: &'static str,
        /// The declared `/FunctionType`.
        function_type: i64,
    },

    /// An entry is present but structurally wrong (not an array, holds a
    /// non-number, an odd element count where pairs are required, …).
    #[error("/{key} is malformed: {detail}")]
    BadEntry {
        /// The offending key, without its leading solidus.
        key: &'static str,
        /// What specifically was wrong.
        detail: String,
    },

    /// An array's length disagrees with the dimensionality every other entry
    /// implies. Table 38: the dimensionality *"shall be consistent"*.
    #[error("/{key} has {got} elements; {expected} required for this function's dimensionality")]
    BadArrayLength {
        /// The offending key, without its leading solidus.
        key: &'static str,
        /// The length the rest of the function implies.
        expected: usize,
        /// The length actually found.
        got: usize,
    },

    /// A `/Domain` or `/Range` pair had its bounds inverted. Table 38 requires
    /// `Domain_2i <= Domain_2i+1`.
    #[error("/{key} pair {index} is inverted: [{low}, {high}]")]
    InvertedInterval {
        /// `Domain` or `Range`.
        key: &'static str,
        /// Which pair (0-based).
        index: usize,
        /// The declared lower bound.
        low: f64,
        /// The declared upper bound.
        high: f64,
    },

    /// Input dimensionality exceeded [`MAX_SAMPLED_INPUTS`].
    #[error(
        "sampled function has {got} inputs; pdfcer's limit is {limit} (§7.10.2 permits an implementation limit)"
    )]
    TooManyInputs {
        /// The declared *m*.
        got: usize,
        /// [`MAX_SAMPLED_INPUTS`].
        limit: usize,
    },

    /// `/BitsPerSample` was not one of [`VALID_BITS_PER_SAMPLE`].
    #[error("/BitsPerSample {0} is not one of 1, 2, 4, 8, 12, 16, 24, 32 (§7.10.2 Table 39)")]
    BadBitsPerSample(i64),

    /// The sample table implied by `/Size`, `/Range` and `/BitsPerSample`
    /// needs more bytes than the decoded stream holds (§7.10.2: *"The stream
    /// data shall be long enough to contain the entire sample array"*).
    #[error(
        "sample stream holds {have} bytes; {need} required by /Size, /Range and /BitsPerSample"
    )]
    SampleDataTooShort {
        /// Bytes the table requires.
        need: usize,
        /// Bytes actually decoded.
        have: usize,
    },

    /// `/Size`'s product overflowed `usize`, or the resulting bit count did.
    /// The table cannot exist, so the arithmetic is refused rather than
    /// wrapped.
    #[error("the sample table implied by /Size and /BitsPerSample overflows address arithmetic")]
    SampleTableTooLarge,

    /// A sample index fell outside the validated table. Structurally
    /// unreachable — the load-time length check makes every index computable
    /// from a clamped input in range — and reported rather than papered over
    /// so that a future refactor which breaks the invariant is visible instead
    /// of silently reading zeros.
    #[error("internal: sample index {index} is outside the validated sample table")]
    SampleIndexOutOfRange {
        /// The offending flat sample index.
        index: usize,
    },

    /// Type 2's `/N` is non-integral while `/Domain` admits a negative `x`, or
    /// `/N` is negative while `/Domain` admits `x = 0`. §7.10.3: *"Values of
    /// `Domain` shall constrain x in such a way that if N is not an integer,
    /// all values of x shall be non-negative, and if N is negative, no value
    /// of x shall be zero."* Both cases would evaluate to `NaN` or infinity.
    #[error("/Domain [{low}, {high}] does not constrain x for /N {n} (§7.10.3)")]
    DomainIncompatibleWithExponent {
        /// The exponent.
        n: f64,
        /// `Domain_0`.
        low: f64,
        /// `Domain_1`.
        high: f64,
    },

    /// A type 2 or type 3 function declared more than one input. §7.10.3 and
    /// §7.10.4 both restrict these to *m* = 1.
    #[error("/FunctionType {function_type} takes exactly one input; /Domain declares {got}")]
    NotOneInput {
        /// The declared `/FunctionType`.
        function_type: i64,
        /// The *m* implied by `/Domain`.
        got: usize,
    },

    /// A type 3's `/Functions` array was empty. §7.10.4 allows *k* = 1 but not
    /// *k* = 0 — with no sub-function there is nothing to evaluate and *n* is
    /// undefined.
    #[error("/Functions is empty; a stitching function needs at least one sub-function")]
    NoSubFunctions,

    /// Two sub-functions of a type 3 disagree on output dimensionality.
    /// §7.10.4: *"The output dimensionality of all functions shall be the
    /// same."*
    #[error(
        "/Functions sub-function {index} produces {got} outputs; sub-function 0 produces {expected}"
    )]
    SubFunctionArity {
        /// Which sub-function disagreed.
        index: usize,
        /// The count set by sub-function 0.
        expected: usize,
        /// The count this one declared.
        got: usize,
    },

    /// `/Bounds` was not in non-decreasing order, or a bound fell outside
    /// `/Domain`. §7.10.4: *"`Bounds` elements shall be in order of increasing
    /// value, and each value shall be within the domain defined by `Domain`."*
    #[error("/Bounds is not a valid partition of /Domain: {detail}")]
    BadBounds {
        /// What specifically was wrong.
        detail: String,
    },

    /// Function nesting (type 3 sub-functions) exceeded [`MAX_FUNCTION_DEPTH`],
    /// which also catches a reference cycle through `/Functions`.
    #[error(
        "function nesting exceeded pdfcer's depth limit of {limit} (cycle, or pathologically nested /Functions)"
    )]
    NestingTooDeep {
        /// [`MAX_FUNCTION_DEPTH`].
        limit: usize,
    },

    /// The type 4 program stream did not lex.
    #[error("type 4 program lex error: {0}")]
    PostScriptLex(#[from] LexError),

    /// The type 4 program lexed but did not parse (missing outer braces, a
    /// brace block not consumed by `if`/`ifelse`, an unterminated block, …).
    /// §7.10.5.2 makes reader-side syntax detection a `shall`.
    #[error("type 4 program syntax error: {detail}")]
    PostScriptSyntax {
        /// What specifically was wrong.
        detail: String,
    },

    /// A token in the type 4 program is not one of Table 42's 42 operators.
    ///
    /// The set is closed: §7.10.5.1 introduces it as *"Table 42 lists the
    /// operators that can be used in this type of function"*, and the same
    /// paragraph bans everything a name could otherwise refer to — *"no
    /// composite data structures such as strings or arrays, no procedures, and
    /// no variables or names"*. So an unrecognised token is a defect in the
    /// file, not a gap in pdfcer.
    #[error("unknown type 4 operator {0:?}; §7.10.5.1 Table 42 is the complete operator set")]
    UnknownOperator(String),

    /// `{ }` nesting exceeded [`MAX_PS_NESTING`].
    #[error("type 4 program brace nesting exceeded pdfcer's limit of {limit}")]
    PostScriptNestingTooDeep {
        /// [`MAX_PS_NESTING`].
        limit: usize,
    },

    /// The operand stack grew past [`PS_STACK_LIMIT`]. §7.10.5.2 lists stack
    /// overflow first among the errors a reader *"shall detect and report"*.
    #[error("type 4 operand stack overflowed the {limit}-entry limit (§7.10.5.1)")]
    StackOverflow {
        /// [`PS_STACK_LIMIT`].
        limit: usize,
    },

    /// An operator needed more operands than the stack held (§7.10.5.2, stack
    /// underflow).
    #[error("type 4 stack underflow: {op} needs {needed} operand(s), {had} available")]
    StackUnderflow {
        /// The operator that underflowed.
        op: &'static str,
        /// Operands required.
        needed: usize,
        /// Operands present.
        had: usize,
    },

    /// An operand had the wrong type (§7.10.5.2's *"type error (for example,
    /// applying `not` to a real number)"*).
    #[error("type 4 type error at {op}: {detail}")]
    PostScriptType {
        /// The operator that rejected its operand.
        op: &'static str,
        /// What was expected and what arrived.
        detail: &'static str,
    },

    /// An operand was outside an operator's mathematical domain
    /// (§7.10.5.2's *"range error (for example, applying `sqrt` to a negative
    /// number)"*).
    #[error("type 4 range error at {op}: {detail}")]
    PostScriptRange {
        /// The operator that rejected its operand.
        op: &'static str,
        /// What was out of range.
        detail: &'static str,
    },

    /// The computation had no defined result (§7.10.5.2's *"undefined result
    /// (for example, dividing by 0)"*).
    #[error("type 4 undefined result at {op}: {detail}")]
    UndefinedResult {
        /// The operator with no defined result.
        op: &'static str,
        /// Why it is undefined.
        detail: &'static str,
    },

    /// Execution passed [`MAX_PS_STEPS`] (pdfcer policy — see that constant).
    #[error("type 4 program exceeded pdfcer's {limit}-step execution cap")]
    StepLimit {
        /// [`MAX_PS_STEPS`].
        limit: usize,
    },

    /// The program finished with an operand count that does not match
    /// `/Range`. §7.10.5.1: *"It shall be an error for the number of remaining
    /// operands to differ from the number of output variables specified by
    /// `Range`."*
    #[error("type 4 program left {got} value(s) on the stack; /Range requires {expected}")]
    OutputArity {
        /// *n*, from `/Range`.
        expected: usize,
        /// What the program actually left.
        got: usize,
    },

    /// The program left a boolean where `/Range` requires a number.
    /// §7.10.5.1 makes this an error in the same sentence as the count
    /// mismatch: *"…or for any of them to be objects other than numbers."*
    #[error("type 4 program left a boolean in output position {index}; outputs must be numbers")]
    NonNumericOutput {
        /// Which output position held the boolean.
        index: usize,
    },

    /// [`PdfFunction::eval`] was handed the wrong number of inputs. Not a file
    /// defect — a caller defect — but reported the same way rather than padded
    /// or truncated, because a `DeviceN` transform fed the wrong tint count
    /// would otherwise return a confidently wrong colour.
    #[error("this function takes {expected} input(s); {got} supplied")]
    InputArity {
        /// *m*.
        expected: usize,
        /// What the caller passed.
        got: usize,
    },

    /// An input was `NaN` or infinite.
    ///
    /// Clamping cannot rescue these: Rust's `f64::max` returns the *non*-`NaN`
    /// operand, so a `NaN` tint clamped to `/Domain` would silently become the
    /// domain's lower bound — a fabricated value wearing the shape of a real
    /// one. Refused instead.
    #[error("input {index} is not finite ({value})")]
    NonFiniteInput {
        /// Which input.
        index: usize,
        /// The offending value.
        value: f64,
    },

    /// An output was `NaN` or infinite *after* `/Range` clamping (so it can
    /// only happen when `/Range` is absent, since clamping maps infinities
    /// onto finite bounds).
    #[error("output {index} is not finite ({value}) and /Range is absent, so it cannot be clamped")]
    NonFiniteOutput {
        /// Which output.
        index: usize,
        /// The offending value.
        value: f64,
    },
}

// ---------------------------------------------------------------------------
// Public type
// ---------------------------------------------------------------------------

/// Which of §7.10's four types a [`PdfFunction`] is.
///
/// Exposed so a caller can report or branch on the type without matching on
/// private internals — the renderer wants to say "type 4 tint transform failed"
/// in a diagnostic, and a `DeviceN` optimiser may want to pre-bake a type 4 into
/// a lookup table while leaving a type 0 alone (which already *is* one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FunctionType {
    /// Type 0 — sampled (§7.10.2).
    Sampled,
    /// Type 2 — exponential interpolation (§7.10.3).
    Exponential,
    /// Type 3 — stitching (§7.10.4).
    Stitching,
    /// Type 4 — PostScript calculator (§7.10.5).
    PostScript,
}

impl FunctionType {
    /// The `/FunctionType` integer this variant is written as.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::function::FunctionType;
    /// assert_eq!(FunctionType::Stitching.as_i64(), 3);
    /// ```
    #[must_use]
    pub const fn as_i64(self) -> i64 {
        match self {
            Self::Sampled => 0,
            Self::Exponential => 2,
            Self::Stitching => 3,
            Self::PostScript => 4,
        }
    }
}

/// A loaded, evaluatable PDF function (§7.10).
///
/// Construct with [`PdfFunction::load`], evaluate with [`PdfFunction::eval`] or
/// [`PdfFunction::eval_into`]. The value is self-contained once loaded: it holds
/// its own sample bytes or program, borrows nothing from the document, and is
/// therefore cheap to keep alongside a colour space and reuse per pixel.
///
/// # Examples
///
/// The commonest tint transform in the wild — a type 2 with `/N 1`, which is a
/// straight interpolation from `C0` (paper) to `C1` (the colorant at full
/// strength):
///
/// ```
/// use pdfcer_core::PdfVersion;
/// use pdfcer_core::function::PdfFunction;
/// use pdfcer_core::graph::ObjectGraph;
/// use pdfcer_core::object::{Dict, Name, ObjId, Object};
/// use pdfcer_core::view::DocumentView;
///
/// // A function whose entries are all direct needs no real document behind it.
/// struct NoObjects;
/// impl ObjectGraph for NoObjects {
///     fn value(&self, _: ObjId) -> Option<&Object> { None }
///     fn trailer_entry(&self, _: &[u8]) -> Option<&Object> { None }
/// }
///
/// fn num(v: f64) -> Object { Object::Real(v) }
///
/// let mut d = Dict::new();
/// d.insert(Name::from(&b"FunctionType"[..]), Object::Integer(2));
/// d.insert(Name::from(&b"Domain"[..]), Object::Array(vec![num(0.0), num(1.0)]));
/// d.insert(Name::from(&b"N"[..]), Object::Integer(1));
/// // Full-strength ink is 100% magenta.
/// d.insert(Name::from(&b"C0"[..]), Object::Array(vec![num(0.0), num(0.0), num(0.0), num(0.0)]));
/// d.insert(Name::from(&b"C1"[..]), Object::Array(vec![num(0.0), num(1.0), num(0.0), num(0.0)]));
///
/// let graph = NoObjects;
/// let view = DocumentView::new(&graph, b"", PdfVersion { major: 1, minor: 7 });
/// let f = PdfFunction::load(&view, &Object::Dict(d)).unwrap();
///
/// assert_eq!(f.inputs(), 1);
/// assert_eq!(f.outputs(), 4);
/// assert_eq!(f.eval(&[0.5]).unwrap(), vec![0.0, 0.5, 0.0, 0.0]);
///
/// // Table 38: an input outside /Domain is clipped, not extrapolated.
/// assert_eq!(f.eval(&[1.4]).unwrap(), vec![0.0, 1.0, 0.0, 0.0]);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct PdfFunction {
    /// `/Domain`, reshaped from its flat 2 × m form into one `[min, max]` pair
    /// per input. Validated non-empty with `min <= max` at load.
    domain: Vec<[f64; 2]>,
    /// `/Range`, reshaped the same way. `None` means the entry was absent,
    /// which Table 38 defines as *no output clipping* — see the module docs.
    range: Option<Vec<[f64; 2]>>,
    /// The type-specific payload.
    kind: Kind,
}

/// Type-specific payload. Private: the four shapes have nothing useful in
/// common for a caller, and exposing them would freeze internal
/// representations (the sample table's `Vec<u8>`, the program tree) as public
/// API for no benefit.
#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Sampled(Sampled),
    Exponential(Exponential),
    Stitching(Stitching),
    PostScript(PostScript),
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

impl PdfFunction {
    /// Load a function from an object in `view`'s graph.
    ///
    /// `obj` may be the function dictionary/stream itself or an indirect
    /// reference to it; references are followed with §7.3.10's rules, and so
    /// are the individual entries (a `/Domain` written as `12 0 R` is legal and
    /// is resolved here).
    ///
    /// Loading is where **all structural validation happens**. By the time this
    /// returns `Ok`, the function's dimensionality is internally consistent, its
    /// intervals are ordered, a type 0's sample stream is known to be long
    /// enough, a type 2's `/Domain` is known to be compatible with its `/N`, a
    /// type 3's `/Bounds` is known to partition its `/Domain`, and a type 4's
    /// program is known to parse. [`PdfFunction::eval`] can then fail only for
    /// reasons that genuinely depend on the input values (a type 4 dividing by
    /// zero, an arity mismatch from the caller).
    ///
    /// That split is deliberate: a tint transform is loaded once and evaluated
    /// once per pixel, so per-evaluation validation would be both slow and, far
    /// worse, would surface a file defect one pixel at a time.
    ///
    /// # Errors
    ///
    /// [`FunctionError`] — every variant except the evaluation-time ones
    /// ([`FunctionError::InputArity`], [`FunctionError::NonFiniteInput`], the
    /// `PostScript*` runtime errors, [`FunctionError::StepLimit`],
    /// [`FunctionError::OutputArity`]). See the module docs on refusal posture:
    /// a malformed function is never repaired into a plausible one.
    pub fn load(view: &DocumentView<'_>, obj: &Object) -> Result<Self, FunctionError> {
        Self::load_at_depth(view, obj, 0)
    }

    /// [`PdfFunction::load`] with the type 3 recursion counter threaded
    /// through, so `/Functions` cycles terminate ([`MAX_FUNCTION_DEPTH`]).
    fn load_at_depth(
        view: &DocumentView<'_>,
        obj: &Object,
        depth: usize,
    ) -> Result<Self, FunctionError> {
        if depth > MAX_FUNCTION_DEPTH {
            return Err(FunctionError::NestingTooDeep {
                limit: MAX_FUNCTION_DEPTH,
            });
        }

        let resolved = view.resolve(obj);
        // `Object::as_dict` deliberately answers for a stream too — a stream is
        // usable wherever its dictionary's content is what matters, which is
        // exactly the case for the Table 38 entries common to all four types.
        let dict = resolved.as_dict().ok_or(FunctionError::NotAFunction)?;

        let function_type = entry(view, dict, b"FunctionType")
            .and_then(Object::as_int)
            .ok_or(FunctionError::MissingFunctionType)?;

        let domain = pairs(view, dict, b"Domain")?.ok_or(FunctionError::MissingEntry {
            key: "Domain",
            function_type,
        })?;
        if domain.is_empty() {
            return Err(FunctionError::BadEntry {
                key: "Domain",
                detail: "empty; a function needs at least one input".to_owned(),
            });
        }
        check_ordered(&domain, "Domain")?;

        let range = pairs(view, dict, b"Range")?;
        if let Some(r) = range.as_deref() {
            if r.is_empty() {
                return Err(FunctionError::BadEntry {
                    key: "Range",
                    detail: "empty; /Range must declare at least one output".to_owned(),
                });
            }
            check_ordered(r, "Range")?;
        }

        let kind = match function_type {
            0 => Kind::Sampled(load_sampled(
                view,
                resolved,
                dict,
                &domain,
                range.as_deref(),
            )?),
            2 => Kind::Exponential(load_exponential(view, dict, &domain)?),
            3 => Kind::Stitching(load_stitching(
                view,
                dict,
                &domain,
                range.as_deref(),
                depth,
            )?),
            4 => Kind::PostScript(load_postscript(view, resolved, &domain, range.as_deref())?),
            other => return Err(FunctionError::UnknownFunctionType(other)),
        };

        // Table 38: "The dimensionality of the function implied by the Domain
        // and Range entries shall be consistent with that implied by other
        // attributes of the function." For types 0 and 4, /Range IS the only
        // source of n, so there is nothing to cross-check; for 2 and 3, n comes
        // from /C0//C1 or from the sub-functions and /Range must agree with it.
        if let Some(r) = range.as_deref() {
            let implied = kind.outputs();
            if r.len() != implied {
                return Err(FunctionError::BadArrayLength {
                    key: "Range",
                    expected: implied * 2,
                    got: r.len() * 2,
                });
            }
        }

        Ok(Self {
            domain,
            range,
            kind,
        })
    }
}

// ---------------------------------------------------------------------------
// Accessors
// ---------------------------------------------------------------------------

impl PdfFunction {
    /// Which of the four types this is.
    #[must_use]
    pub const fn function_type(&self) -> FunctionType {
        match self.kind {
            Kind::Sampled(_) => FunctionType::Sampled,
            Kind::Exponential(_) => FunctionType::Exponential,
            Kind::Stitching(_) => FunctionType::Stitching,
            Kind::PostScript(_) => FunctionType::PostScript,
        }
    }

    /// *m* — the number of input values this function takes.
    #[must_use]
    pub fn inputs(&self) -> usize {
        self.domain.len()
    }

    /// *n* — the number of output values this function produces.
    #[must_use]
    pub fn outputs(&self) -> usize {
        self.kind.outputs()
    }

    /// `/Domain` as one `[min, max]` pair per input.
    ///
    /// Reshaped from the flat 2 × m array the file carries, because every use
    /// of it is pairwise and a flat slice invites an off-by-one that produces a
    /// plausible-looking wrong answer.
    #[must_use]
    pub fn domain(&self) -> &[[f64; 2]] {
        &self.domain
    }

    /// `/Range` as one `[min, max]` pair per output, or `None` when the entry
    /// was absent.
    ///
    /// **`None` is meaningful and must not be defaulted.** Table 38: *"If this
    /// entry is absent, no clipping shall be done."* Types 0 and 4 always
    /// return `Some` (the entry is required for them).
    #[must_use]
    pub fn range(&self) -> Option<&[[f64; 2]]> {
        self.range.as_deref()
    }

    /// Whether evaluating this function silently uses linear interpolation
    /// where the file asked for cubic — i.e. a type 0 with `/Order 3` that the
    /// spec does *not* let pdfcer ignore.
    ///
    /// This is disclosure plumbing for project rule 4 (fuzzy, never sneaky).
    /// pdfcer evaluates every sampled function multilinearly. §7.10.2 permits
    /// exactly one case of ignoring `/Order 3` — *"If `Size` is less than 4,
    /// cubic spline interpolation is not possible and `Order` 3 shall be
    /// ignored if specified"* — and in that case this returns `false`, because
    /// nothing was downgraded. It returns `true` only when a cubic
    /// interpolation was genuinely requested, genuinely possible, and not
    /// performed, so a caller can say so rather than let the operator assume
    /// fidelity that isn't there.
    ///
    /// The reason pdfcer does not simply implement it: §7.10.2 names *"cubic
    /// spline interpolation"* and then specifies **no spline** — no basis, no
    /// end conditions, no continuity requirement. Two conforming readers
    /// legitimately produce different pixels (recorded as ambiguity `F-A1` in
    /// the spec RAG). Guessing a spline would be inventing the standard's
    /// missing half and calling the result fidelity.
    ///
    /// Recurses into a type 3's sub-functions, since that is where a sampled
    /// function usually hides.
    #[must_use]
    pub fn cubic_downgraded(&self) -> bool {
        match &self.kind {
            Kind::Sampled(s) => s.order3_downgraded,
            Kind::Stitching(s) => s.functions.iter().any(Self::cubic_downgraded),
            Kind::Exponential(_) | Kind::PostScript(_) => false,
        }
    }
}

impl Kind {
    /// *n* for this payload.
    fn outputs(&self) -> usize {
        match self {
            Self::Sampled(s) => s.decode.len(),
            Self::Exponential(e) => e.c0.len(),
            Self::Stitching(s) => s.outputs,
            Self::PostScript(p) => p.outputs,
        }
    }
}

// ---------------------------------------------------------------------------
// Evaluation — the shared clamping frame
// ---------------------------------------------------------------------------

impl PdfFunction {
    /// Evaluate the function, allocating a fresh output vector.
    ///
    /// Convenience over [`PdfFunction::eval_into`]; prefer that one in a
    /// per-pixel loop, where this allocation dominates the arithmetic.
    ///
    /// # Errors
    ///
    /// [`FunctionError::InputArity`] if `inputs.len()` is not
    /// [`PdfFunction::inputs`]; [`FunctionError::NonFiniteInput`] for a `NaN` or
    /// infinite input; plus any type 4 runtime error. See
    /// [`PdfFunction::eval_into`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use pdfcer_core::PdfVersion;
    /// # use pdfcer_core::function::PdfFunction;
    /// # use pdfcer_core::graph::ObjectGraph;
    /// # use pdfcer_core::object::{Dict, Name, ObjId, Object};
    /// # use pdfcer_core::view::DocumentView;
    /// # struct G;
    /// # impl ObjectGraph for G {
    /// #     fn value(&self, _: ObjId) -> Option<&Object> { None }
    /// #     fn trailer_entry(&self, _: &[u8]) -> Option<&Object> { None }
    /// # }
    /// # let mut d = Dict::new();
    /// # d.insert(Name::from(&b"FunctionType"[..]), Object::Integer(2));
    /// # d.insert(Name::from(&b"Domain"[..]), Object::Array(vec![Object::Real(0.0), Object::Real(1.0)]));
    /// # d.insert(Name::from(&b"N"[..]), Object::Integer(2));
    /// # let g = G;
    /// # let view = DocumentView::new(&g, b"", PdfVersion { major: 1, minor: 7 });
    /// // /C0 and /C1 default to [0.0] and [1.0], so this is y = x^2.
    /// let f = PdfFunction::load(&view, &Object::Dict(d)).unwrap();
    /// assert_eq!(f.eval(&[0.5]).unwrap(), vec![0.25]);
    /// ```
    pub fn eval(&self, inputs: &[f64]) -> Result<Vec<f64>, FunctionError> {
        let mut out = Vec::with_capacity(self.outputs());
        self.eval_into(inputs, &mut out)?;
        Ok(out)
    }

    /// Evaluate the function into `out`, which is cleared first.
    ///
    /// This is the real entry point; [`PdfFunction::eval`] wraps it. Reusing one
    /// buffer across a scanline is the difference between one allocation and one
    /// per pixel for a `DeviceN` image.
    ///
    /// ## What this method owns, and why it is not per-type
    ///
    /// The two Table 38 clipping `shall`s are applied **here**, around whatever
    /// the type-specific evaluator does:
    ///
    /// 1. every input is clipped to its `/Domain` pair before dispatch;
    /// 2. every output is clipped to its `/Range` pair afterwards — and, when
    ///    `/Range` is absent, deliberately **not** clipped.
    ///
    /// Putting them in one place means no type can be added later that forgets
    /// one. Type 0's own algorithm (§7.10.2) restates both clips internally;
    /// applying them again here is idempotent, so the duplication costs
    /// nothing and the guarantee holds even if that evaluator changes.
    ///
    /// ## Non-finite values are refused, not clamped
    ///
    /// A `NaN` input cannot be clipped meaningfully — Rust's `f64::max` returns
    /// the non-`NaN` operand, so clamping would quietly turn `NaN` into
    /// `Domain_0`. That is a fabricated value that looks exactly like a real
    /// one downstream, so [`FunctionError::NonFiniteInput`] is returned instead.
    /// Symmetrically, an output that is still non-finite after clipping (only
    /// possible when `/Range` is absent) yields
    /// [`FunctionError::NonFiniteOutput`].
    ///
    /// # Errors
    ///
    /// - [`FunctionError::InputArity`] — wrong number of inputs.
    /// - [`FunctionError::NonFiniteInput`] / [`FunctionError::NonFiniteOutput`].
    /// - [`FunctionError::SampleIndexOutOfRange`] — type 0 internal invariant.
    /// - Type 4 runtime errors: [`FunctionError::StackOverflow`],
    ///   [`FunctionError::StackUnderflow`], [`FunctionError::PostScriptType`],
    ///   [`FunctionError::PostScriptRange`], [`FunctionError::UndefinedResult`],
    ///   [`FunctionError::StepLimit`], [`FunctionError::OutputArity`],
    ///   [`FunctionError::NonNumericOutput`].
    pub fn eval_into(&self, inputs: &[f64], out: &mut Vec<f64>) -> Result<(), FunctionError> {
        if inputs.len() != self.domain.len() {
            return Err(FunctionError::InputArity {
                expected: self.domain.len(),
                got: inputs.len(),
            });
        }

        // Clip inputs to /Domain (Table 38, `shall`). Collected into a small
        // stack-friendly vector rather than mutated in place because `inputs`
        // is the caller's slice.
        let mut clipped: Vec<f64> = Vec::with_capacity(inputs.len());
        for (index, (&x, pair)) in inputs.iter().zip(self.domain.iter()).enumerate() {
            if !x.is_finite() {
                return Err(FunctionError::NonFiniteInput { index, value: x });
            }
            clipped.push(clip(x, pair[0], pair[1]));
        }

        out.clear();
        match &self.kind {
            Kind::Sampled(s) => s.eval(&self.domain, &clipped, out)?,
            Kind::Exponential(e) => e.eval(&clipped, out),
            Kind::Stitching(s) => s.eval(&self.domain, &clipped, out)?,
            Kind::PostScript(p) => p.eval(&clipped, out)?,
        }

        // Clip outputs to /Range (Table 38, `shall`) — or, when /Range is
        // absent, do not (Table 38, equally a `shall`).
        match self.range.as_deref() {
            Some(range) => {
                for (value, pair) in out.iter_mut().zip(range.iter()) {
                    *value = clip(*value, pair[0], pair[1]);
                }
            }
            None => {
                for (index, &value) in out.iter().enumerate() {
                    if !value.is_finite() {
                        return Err(FunctionError::NonFiniteOutput { index, value });
                    }
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Shared numeric helpers
// ---------------------------------------------------------------------------

/// §7.10.1's `Interpolate(x, xmin, xmax, ymin, ymax)`:
/// `ymin + ((x − xmin) × (ymax − ymin) / (xmax − xmin))`.
///
/// The whole of §7.10 is built from this one primitive — type 0 uses it twice
/// (encode and decode), type 3 uses it to map a subdomain onto a sub-function's
/// domain.
///
/// ## The degenerate case, and a place where the spec meets us halfway
///
/// `xmax == xmin` makes the formula a division by zero, and §7.10.1 does not
/// address it even though Table 38 permits it (`Domain_2i <= Domain_2i+1`,
/// with equality allowed — a `/Domain [0.5 0.5]` is legal). pdfcer returns
/// `ymin`, which is the limit of the expression as the interval collapses from
/// the left and the only value the interval can produce.
///
/// That choice is not free-floating: §7.10.4 states the same answer for the one
/// instance of this it *does* consider — *"If the last bound, `Bounds_k−2`, is
/// equal to `Domain_1`, then x′ shall be defined to be `Encode_2i`"* — which is
/// exactly `ymin` for that collapsed interval. Generalising it is pdfcer policy,
/// but policy converging on the standard's own answer rather than away from it.
fn interpolate(x: f64, xmin: f64, xmax: f64, ymin: f64, ymax: f64) -> f64 {
    let span = xmax - xmin;
    if span == 0.0 {
        return ymin;
    }
    ymin + (x - xmin) * (ymax - ymin) / span
}

/// Clip `v` into `[lo, hi]` — Table 38's *"clipped to the nearest boundary
/// value"*, for both the `/Domain` and `/Range` cases.
///
/// Written as explicit comparisons rather than `f64::clamp` because `clamp`
/// panics when `lo > hi`, and `pdfcer-core`'s panic-free policy does not accept
/// "the loader validated it" as a reason to leave a reachable panic in place.
/// (The loader *does* validate it — [`check_ordered`] — so the comparisons
/// below never see an inverted pair; this is belt and braces on untrusted
/// input.) `NaN` propagates unchanged, and callers check for it explicitly.
fn clip(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// Resolve a dictionary entry through the graph (an entry may be an indirect
/// reference — §7.3.10's substitutability applies to function entries like any
/// other).
fn entry<'v>(view: &'v DocumentView<'_>, dict: &'v Dict, key: &[u8]) -> Option<&'v Object> {
    dict.get(key).map(|obj| view.resolve(obj))
}

/// Read an array-valued entry as a flat `Vec<f64>`, resolving both the array
/// itself and each element.
///
/// `Ok(None)` means the entry was absent — which is meaningfully different from
/// "present but empty" and from "present but wrong", both of which are errors.
///
/// # Errors
///
/// [`FunctionError::BadEntry`] if the entry is not an array, or holds an element
/// that is not a number (§7.3.3's integer-where-real rule is honoured via
/// [`Object::as_number`]).
fn numbers(
    view: &DocumentView<'_>,
    dict: &Dict,
    key: &'static str,
) -> Result<Option<Vec<f64>>, FunctionError> {
    let Some(obj) = entry(view, dict, key.as_bytes()) else {
        return Ok(None);
    };
    let Some(items) = obj.as_array() else {
        return Err(FunctionError::BadEntry {
            key,
            detail: "not an array".to_owned(),
        });
    };
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let Some(v) = view.resolve(item).as_number() else {
            return Err(FunctionError::BadEntry {
                key,
                detail: format!("element {i} is not a number"),
            });
        };
        if !v.is_finite() {
            return Err(FunctionError::BadEntry {
                key,
                detail: format!("element {i} is not finite"),
            });
        }
        out.push(v);
    }
    Ok(Some(out))
}

/// [`numbers`], reshaped into `[min, max]` pairs — the form every 2 × k entry in
/// §7.10 actually has (`/Domain`, `/Range`, `/Encode`, `/Decode`).
///
/// # Errors
///
/// [`FunctionError::BadEntry`] for a non-array, a non-numeric element, or an odd
/// element count (which would leave a pair half-specified — refused rather than
/// truncated, since silently dropping the last value would shift every
/// subsequent pair).
fn pairs(
    view: &DocumentView<'_>,
    dict: &Dict,
    key: &'static [u8],
) -> Result<Option<Vec<[f64; 2]>>, FunctionError> {
    // The key is ASCII by construction (all §7.10 keys are), so the lossy
    // conversion below cannot lose anything; it exists only to satisfy the
    // `&'static str` in the error payload.
    let key_str: &'static str = match key {
        b"Domain" => "Domain",
        b"Range" => "Range",
        b"Encode" => "Encode",
        b"Decode" => "Decode",
        _ => "unknown",
    };
    let Some(flat) = numbers(view, dict, key_str)? else {
        return Ok(None);
    };
    if flat.len() % 2 != 0 {
        return Err(FunctionError::BadEntry {
            key: key_str,
            detail: format!("has {} elements; a 2 × n array is required", flat.len()),
        });
    }
    Ok(Some(
        flat.chunks_exact(2)
            .map(|c| {
                [
                    c.first().copied().unwrap_or(0.0),
                    c.get(1).copied().unwrap_or(0.0),
                ]
            })
            .collect(),
    ))
}

/// Enforce Table 38's *"`Domain_2i` shall be less than or equal to
/// `Domain_2i+1`"* (and the identical rule for `/Range`).
///
/// Equality is allowed by the spec and accepted here — a collapsed interval is
/// a legal, if odd, way to pin an input to one value, and [`interpolate`]
/// handles it. Only an *inverted* pair is refused, because there is no reading
/// of `[1.0, 0.0]` that the standard sanctions and clipping into it would
/// produce whichever bound the comparison happened to hit first.
///
/// # Errors
///
/// [`FunctionError::InvertedInterval`].
fn check_ordered(intervals: &[[f64; 2]], key: &'static str) -> Result<(), FunctionError> {
    for (index, pair) in intervals.iter().enumerate() {
        if pair[0] > pair[1] {
            return Err(FunctionError::InvertedInterval {
                key,
                index,
                low: pair[0],
                high: pair[1],
            });
        }
    }
    Ok(())
}

/// Decode a function stream's data through its `/Filter` chain.
///
/// # Errors
///
/// [`FunctionError::StreamUnreadable`] if the view cannot serve the span,
/// [`FunctionError::Filter`] if the chain fails.
fn stream_bytes(view: &DocumentView<'_>, stream: &Stream) -> Result<Vec<u8>, FunctionError> {
    let raw = view
        .slice(stream.data_span)
        .ok_or(FunctionError::StreamUnreadable)?;
    Ok(filters::decode_stream(&stream.dict, raw)?)
}

// ---------------------------------------------------------------------------
// Type 0 — sampled (§7.10.2)
// ---------------------------------------------------------------------------

/// A type 0 sampled function: a lookup table plus the two linear maps that get
/// values into and out of it.
///
/// ## The shape of the data
///
/// The table has `Size_0 × Size_1 × … × Size_(m−1)` entries, each entry holding
/// *n* samples of `bits_per_sample` bits. §7.10.2 fixes the layout exactly:
///
/// > *"Sample data shall be represented as a stream of bytes. The bytes shall
/// > constitute a continuous bit stream, with the high-order bit of each byte
/// > first. Each sample value shall be represented as a sequence of
/// > `BitsPerSample` bits. Successive values shall be adjacent in the bit
/// > stream; there shall be no padding at byte boundaries."*
///
/// Two consequences that are easy to get wrong and produce garbage rather than
/// an error:
///
/// - **No byte alignment.** At `/BitsPerSample 4` two samples share a byte, high
///   nibble first; at 12, sample 0 occupies bits 0–11 and sample 1 bits 12–23,
///   so sample 1 straddles bytes 1 and 2. This is why [`read_bits`] works in
///   bits rather than bytes, with a byte-wise fast path only for the widths
///   where alignment is guaranteed.
/// - **First dimension varies fastest.** §7.10.2: *"the sample values in the
///   first dimension vary fastest, and the values in the last dimension vary
///   slowest"* — `f(0,0)`, `f(1,0)`, … `f(Size_0−1, 0)`, `f(0,1)`, …. That is
///   column-major, the *opposite* of C row-major order, and getting it backwards
///   transposes a 2-input transform without any symptom the code can detect.
///   [`Sampled::strides`] encodes it.
///
/// ## Interpolation
///
/// pdfcer interpolates multilinearly (2^m corners, weights the product of each
/// axis's fraction). The normative text says only *"Interpolation shall be used
/// to determine output values from the nearest surrounding values in the sample
/// table"*; the word "multilinear" appears only in an **informative NOTE**
/// (recorded as ambiguity `F-A2` in the spec RAG), so multilinear is the
/// universal reading rather than a quoted requirement. `/Order 3` is discussed
/// on [`PdfFunction::cubic_downgraded`].
#[derive(Debug, Clone, PartialEq)]
struct Sampled {
    /// `/Size` — samples per input dimension, each ≥ 1. Length *m*.
    size: Vec<usize>,
    /// Flat-index multipliers, `strides[0] = 1`,
    /// `strides[i] = strides[i−1] × size[i−1]`. Precomputed because it encodes
    /// the first-dimension-fastest rule in exactly one place.
    strides: Vec<usize>,
    /// `/BitsPerSample`, one of [`VALID_BITS_PER_SAMPLE`].
    bits_per_sample: u32,
    /// `2^bits_per_sample − 1`, the `xmax` of the decode interpolation.
    /// Precomputed as `f64` because it is otherwise recomputed per output per
    /// evaluation, and at 32 bits it does not fit a `u32`.
    max_sample: f64,
    /// `/Encode` — one pair per input. Defaults to `[0, Size_i − 1]`.
    encode: Vec<[f64; 2]>,
    /// `/Decode` — one pair per output. Defaults to `/Range`. Its length is *n*.
    decode: Vec<[f64; 2]>,
    /// The decoded sample stream.
    samples: Vec<u8>,
    /// `/Order 3` was requested, cubic was possible, and pdfcer did it linearly
    /// anyway. See [`PdfFunction::cubic_downgraded`].
    order3_downgraded: bool,
}

/// Build a [`Sampled`] from its stream and dictionary, validating everything
/// Table 39 constrains.
///
/// # Errors
///
/// [`FunctionError`] — see [`PdfFunction::load`].
fn load_sampled(
    view: &DocumentView<'_>,
    resolved: &Object,
    dict: &Dict,
    domain: &[[f64; 2]],
    range: Option<&[[f64; 2]]>,
) -> Result<Sampled, FunctionError> {
    let Object::Stream(stream) = resolved else {
        return Err(FunctionError::NotAStream { function_type: 0 });
    };

    let m = domain.len();
    if m > MAX_SAMPLED_INPUTS {
        return Err(FunctionError::TooManyInputs {
            got: m,
            limit: MAX_SAMPLED_INPUTS,
        });
    }

    // /Range is required for type 0 — it is the only source of n (§7.10.1).
    let range = range.ok_or(FunctionError::MissingEntry {
        key: "Range",
        function_type: 0,
    })?;
    let n = range.len();

    // --- /Size ---------------------------------------------------------
    let size_raw = numbers(view, dict, "Size")?.ok_or(FunctionError::MissingEntry {
        key: "Size",
        function_type: 0,
    })?;
    if size_raw.len() != m {
        return Err(FunctionError::BadArrayLength {
            key: "Size",
            expected: m,
            got: size_raw.len(),
        });
    }
    let mut size = Vec::with_capacity(m);
    for (i, &s) in size_raw.iter().enumerate() {
        // Table 39: "m positive integers". A zero or fractional Size makes the
        // table's extent undefined, so it is refused rather than rounded.
        if s < 1.0 || s.fract() != 0.0 {
            return Err(FunctionError::BadEntry {
                key: "Size",
                detail: format!("element {i} is {s}; each must be a positive integer"),
            });
        }
        // `s` is finite (checked in `numbers`), ≥ 1 and integral; the cast is
        // exact for any value the length check below will accept, and a value
        // beyond `usize` range saturates into `SampleTableTooLarge` there.
        size.push(s as usize);
    }

    // Strides: first dimension fastest (see `Sampled`'s docs).
    let mut strides = Vec::with_capacity(m);
    let mut acc: usize = 1;
    for &s in &size {
        strides.push(acc);
        acc = acc
            .checked_mul(s)
            .ok_or(FunctionError::SampleTableTooLarge)?;
    }
    let total_entries = acc;

    // --- /BitsPerSample ------------------------------------------------
    let bits_raw = entry(view, dict, b"BitsPerSample")
        .and_then(Object::as_int)
        .ok_or(FunctionError::MissingEntry {
            key: "BitsPerSample",
            function_type: 0,
        })?;
    let bits_per_sample = u32::try_from(bits_raw)
        .ok()
        .filter(|b| VALID_BITS_PER_SAMPLE.contains(b))
        .ok_or(FunctionError::BadBitsPerSample(bits_raw))?;

    // --- the "long enough" check (§7.10.2, §7.3.8.2) --------------------
    //
    // Every multiplication is checked: `Size` is attacker-controlled, and a
    // wrapped product would compute a *small* required length, pass this check,
    // and then read arbitrary in-bounds bytes as colour.
    let total_samples = total_entries
        .checked_mul(n)
        .ok_or(FunctionError::SampleTableTooLarge)?;
    let total_bits = total_samples
        .checked_mul(bits_per_sample as usize)
        .ok_or(FunctionError::SampleTableTooLarge)?;
    let need = total_bits
        .checked_add(7)
        .ok_or(FunctionError::SampleTableTooLarge)?
        / 8;

    let samples = stream_bytes(view, stream)?;
    if samples.len() < need {
        return Err(FunctionError::SampleDataTooShort {
            need,
            have: samples.len(),
        });
    }

    // --- /Encode (default [0, Size_i − 1]) ------------------------------
    let encode = match pairs(view, dict, b"Encode")? {
        Some(e) if e.len() != m => {
            return Err(FunctionError::BadArrayLength {
                key: "Encode",
                expected: m * 2,
                got: e.len() * 2,
            });
        }
        Some(e) => e,
        None => size
            .iter()
            .map(|&s| [0.0, (s.saturating_sub(1)) as f64])
            .collect(),
    };

    // --- /Decode (default: same as /Range) ------------------------------
    let decode = match pairs(view, dict, b"Decode")? {
        Some(d) if d.len() != n => {
            return Err(FunctionError::BadArrayLength {
                key: "Decode",
                expected: n * 2,
                got: d.len() * 2,
            });
        }
        Some(d) => d,
        None => range.to_vec(),
    };

    // --- /Order ----------------------------------------------------------
    let order = entry(view, dict, b"Order")
        .and_then(Object::as_int)
        .unwrap_or(1);
    if order != 1 && order != 3 {
        return Err(FunctionError::BadEntry {
            key: "Order",
            detail: format!("{order} is not 1 (linear) or 3 (cubic spline)"),
        });
    }
    // §7.10.2: "If Size is less than 4, cubic spline interpolation is not
    // possible and Order 3 shall be ignored if specified." So an /Order 3 on a
    // table with no axis of 4+ samples is not a downgrade at all — ignoring it
    // is what the spec requires. Only the other case is a disclosure.
    let order3_downgraded = order == 3 && size.iter().any(|&s| s >= 4);

    Ok(Sampled {
        size,
        strides,
        bits_per_sample,
        max_sample: (2f64).powi(bits_per_sample as i32) - 1.0,
        encode,
        decode,
        samples,
        order3_downgraded,
    })
}

/// Per-axis interpolation state for one evaluation: which two table indices
/// bracket the encoded input, how far between them it fell, and the flat-index
/// multiplier for the axis.
///
/// Materialised as a small struct rather than three parallel vectors so the
/// corner loop can iterate without indexing (crate policy denies
/// `clippy::indexing_slicing`) and so `upper` is computed once per axis rather
/// than once per corner.
struct Axis {
    /// Lower bracketing table index.
    lower: usize,
    /// Upper bracketing index — `min(lower + 1, size − 1)`, so the top edge of
    /// the table brackets against itself with weight zero.
    upper: usize,
    /// Fractional position between `lower` and `upper`, in `[0, 1)`.
    frac: f64,
    /// Flat-index multiplier for this axis.
    stride: usize,
}

impl Sampled {
    /// Evaluate at `inputs` (already clipped to `/Domain` by
    /// [`PdfFunction::eval_into`]).
    ///
    /// Implements §7.10.2's algorithm verbatim:
    ///
    /// ```text
    /// e_i  = Interpolate(x_i, Domain_2i, Domain_2i+1, Encode_2i, Encode_2i+1)
    /// e'_i = min(max(e_i, 0), Size_i − 1)
    /// r_j  = <multilinear blend of the surrounding table samples>
    /// r'_j = Interpolate(r_j, 0, 2^BitsPerSample − 1, Decode_2j, Decode_2j+1)
    /// y_j  = min(max(r'_j, Range_2j), Range_2j+1)      <- applied by the caller
    /// ```
    ///
    /// The blend is done on **raw** sample values and decoded afterwards, which
    /// is what the algorithm above says and is also equivalent to decoding
    /// first: both `Interpolate` steps are affine, and an affine map commutes
    /// with a convex combination. Doing it in the spec's order keeps one decode
    /// per output instead of one per corner per output.
    ///
    /// # Errors
    ///
    /// [`FunctionError::SampleIndexOutOfRange`] — structurally unreachable given
    /// the load-time length check; see that variant.
    fn eval(
        &self,
        domain: &[[f64; 2]],
        inputs: &[f64],
        out: &mut Vec<f64>,
    ) -> Result<(), FunctionError> {
        let n = self.decode.len();

        // --- encode each input into table coordinates --------------------
        let mut axes: Vec<Axis> = Vec::with_capacity(self.size.len());
        for (((&x, dom), enc), (&size, &stride)) in inputs
            .iter()
            .zip(domain.iter())
            .zip(self.encode.iter())
            .zip(self.size.iter().zip(self.strides.iter()))
        {
            let top = size.saturating_sub(1);
            // `top` is a table index, always small enough to be exact in f64.
            let top_f = top as f64;
            let e = clip(interpolate(x, dom[0], dom[1], enc[0], enc[1]), 0.0, top_f);
            // `e` is finite and in [0, top], so `floor` is in range and the cast
            // is exact. `min(top)` guards the `e == top` case where floor lands
            // exactly on the last index.
            let floor = e.floor();
            let lower = (floor as usize).min(top);
            axes.push(Axis {
                lower,
                upper: (lower + 1).min(top),
                frac: e - floor,
                stride,
            });
        }

        // --- multilinear blend over 2^m corners --------------------------
        //
        // Bounded by MAX_SAMPLED_INPUTS (load-time), so `1 << m` cannot
        // overflow and the loop cannot run away.
        let mut acc = vec![0.0f64; n];
        let corners = 1usize << axes.len();
        for corner in 0..corners {
            let mut weight = 1.0f64;
            let mut flat = 0usize;
            for (i, axis) in axes.iter().enumerate() {
                let take_upper = (corner >> i) & 1 == 1;
                let (index, w) = if take_upper {
                    (axis.upper, axis.frac)
                } else {
                    (axis.lower, 1.0 - axis.frac)
                };
                weight *= w;
                flat = flat.saturating_add(index.saturating_mul(axis.stride));
            }
            // A zero-weight corner contributes nothing; skipping it also avoids
            // reading the duplicate top-edge index for a collapsed axis.
            if weight == 0.0 {
                continue;
            }
            for (j, slot) in acc.iter_mut().enumerate() {
                *slot += weight * self.sample(flat, j, n)?;
            }
        }

        // --- decode ------------------------------------------------------
        for (raw, dec) in acc.into_iter().zip(self.decode.iter()) {
            out.push(interpolate(raw, 0.0, self.max_sample, dec[0], dec[1]));
        }
        Ok(())
    }

    /// Read output `j` of table entry `flat` as a raw sample value.
    ///
    /// The flat sample number is `flat × n + j` (§7.10.2: multidimensional
    /// output values *"shall be stored in the same order as `Range`"*, i.e. the
    /// *n* outputs of one table entry are adjacent), and its bit offset is that
    /// number times `/BitsPerSample`.
    ///
    /// # Errors
    ///
    /// [`FunctionError::SampleIndexOutOfRange`] if the computed bits fall
    /// outside the loaded stream — unreachable after the load-time length
    /// check, and reported rather than defaulted so a broken invariant is
    /// visible instead of rendering as a colour.
    fn sample(&self, flat: usize, j: usize, n: usize) -> Result<f64, FunctionError> {
        let index = flat
            .checked_mul(n)
            .and_then(|base| base.checked_add(j))
            .ok_or(FunctionError::SampleIndexOutOfRange { index: flat })?;
        let bit_offset = index
            .checked_mul(self.bits_per_sample as usize)
            .ok_or(FunctionError::SampleIndexOutOfRange { index })?;
        let raw = read_bits(&self.samples, bit_offset, self.bits_per_sample)
            .ok_or(FunctionError::SampleIndexOutOfRange { index })?;
        // u64 -> f64 is exact for every value 32 bits can hold.
        Ok(raw as f64)
    }
}

/// Read `bits` bits starting at bit `offset`, MSB-first within each byte.
///
/// §7.10.2's bit stream is big-endian at the bit level: *"the high-order bit of
/// each byte first"*, values *"adjacent in the bit stream"* with *"no padding at
/// byte boundaries"*. So at `/BitsPerSample 4`, byte `0xAB` holds sample `0xA`
/// then sample `0xB`; at 12, bytes `0xAB 0xCD` hold `0xABC` and the first nibble
/// of the next sample is `0xD`.
///
/// Returns `None` when the requested bits are not wholly inside `data`, which
/// callers turn into a named refusal rather than a zero sample.
///
/// ## Why two paths
///
/// For widths 8, 16, 24 and 32 the offset is necessarily a whole number of
/// bytes (every preceding sample was too), so those read byte-wise — the common
/// case, and the one that runs per pixel. Widths 1, 2, 4 and 12 take the bit
/// loop, which runs at most 12 iterations. The alternative, a single generic
/// shift-and-mask over a `u64` window, needs careful handling when a 32-bit
/// sample starts mid-byte (it cannot, but the code would have to prove it) and
/// is not obviously correct on inspection; this version is.
fn read_bits(data: &[u8], offset: usize, bits: u32) -> Option<u64> {
    let bits_usize = bits as usize;
    let end = offset.checked_add(bits_usize)?;
    if end > data.len().checked_mul(8)? {
        return None;
    }

    if bits.is_multiple_of(8) && offset.is_multiple_of(8) {
        let start = offset / 8;
        let stop = end / 8;
        let mut acc: u64 = 0;
        for &byte in data.get(start..stop)? {
            acc = (acc << 8) | u64::from(byte);
        }
        return Some(acc);
    }

    let mut acc: u64 = 0;
    for i in 0..bits_usize {
        let bit_index = offset.checked_add(i)?;
        let byte = *data.get(bit_index / 8)?;
        // 7 − (bit_index % 8): bit 0 of the stream is the *high* bit of byte 0.
        let shift = 7 - (bit_index % 8);
        acc = (acc << 1) | u64::from((byte >> shift) & 1);
    }
    Some(acc)
}

// ---------------------------------------------------------------------------
// Type 2 — exponential interpolation (§7.10.3)
// ---------------------------------------------------------------------------

/// A type 2 function: `y_j = C0_j + x^N × (C1_j − C0_j)`.
///
/// One input, *n* outputs, three numbers of configuration. §7.10.3 notes that
/// *"when N is 1, the function performs a linear interpolation between C0 and
/// C1"* — and that case, a straight ramp from paper to full-strength ink, is
/// what the overwhelming majority of `Separation` tint transforms actually are.
///
/// ## The default trap
///
/// `/C0` defaults to `[0.0]` and `/C1` to `[1.0]` — **scalars, not vectors**. A
/// type 2 with neither entry present is therefore a 1-output function, and a
/// 4-output tint transform *must* carry explicit four-element arrays. A loader
/// that defaulted them to "n zeros" using an *n* from somewhere else would
/// silently accept a malformed function; here *n* is *defined* by `/C0`/`/C1`
/// and any `/Range` present is checked against it
/// ([`FunctionError::BadArrayLength`]).
#[derive(Debug, Clone, PartialEq)]
struct Exponential {
    /// `/C0` — the result at `x = 0`. Length *n*.
    c0: Vec<f64>,
    /// `/C1` — the result at `x = 1`. Length *n*, equal to `c0`'s.
    c1: Vec<f64>,
    /// `/N` — the interpolation exponent.
    n: f64,
}

/// Build an [`Exponential`], validating Table 40 and §7.10.3's domain
/// constraint.
///
/// # Errors
///
/// [`FunctionError`] — see [`PdfFunction::load`].
fn load_exponential(
    view: &DocumentView<'_>,
    dict: &Dict,
    domain: &[[f64; 2]],
) -> Result<Exponential, FunctionError> {
    // §7.10.3: "exponential interpolation of one input value".
    if domain.len() != 1 {
        return Err(FunctionError::NotOneInput {
            function_type: 2,
            got: domain.len(),
        });
    }
    let bounds = domain.first().copied().unwrap_or([0.0, 1.0]);

    let exponent =
        entry(view, dict, b"N")
            .and_then(Object::as_number)
            .ok_or(FunctionError::MissingEntry {
                key: "N",
                function_type: 2,
            })?;
    if !exponent.is_finite() {
        return Err(FunctionError::BadEntry {
            key: "N",
            detail: "not finite".to_owned(),
        });
    }

    let c0 = numbers(view, dict, "C0")?.unwrap_or_else(|| vec![0.0]);
    let c1 = numbers(view, dict, "C1")?.unwrap_or_else(|| vec![1.0]);
    if c0.is_empty() {
        return Err(FunctionError::BadEntry {
            key: "C0",
            detail: "empty".to_owned(),
        });
    }
    if c0.len() != c1.len() {
        return Err(FunctionError::BadArrayLength {
            key: "C1",
            expected: c0.len(),
            got: c1.len(),
        });
    }

    // §7.10.3: "Values of Domain shall constrain x in such a way that if N is
    // not an integer, all values of x shall be non-negative, and if N is
    // negative, no value of x shall be zero."
    //
    // Both are checked at LOAD, from /Domain, rather than at evaluation from
    // x — which is what the sentence actually requires, and which is also the
    // only way to catch it once instead of once per pixel. The failure modes
    // they prevent are `NaN` (a negative base under a fractional exponent) and
    // an infinity (a zero base under a negative exponent); either would sail
    // through a /Range clamp as a boundary colour.
    if exponent.fract() != 0.0 && bounds[0] < 0.0 {
        return Err(FunctionError::DomainIncompatibleWithExponent {
            n: exponent,
            low: bounds[0],
            high: bounds[1],
        });
    }
    if exponent < 0.0 && bounds[0] <= 0.0 && bounds[1] >= 0.0 {
        return Err(FunctionError::DomainIncompatibleWithExponent {
            n: exponent,
            low: bounds[0],
            high: bounds[1],
        });
    }

    Ok(Exponential {
        c0,
        c1,
        n: exponent,
    })
}

impl Exponential {
    /// `y_j = C0_j + x^N × (C1_j − C0_j)` for each output.
    ///
    /// `x` arrives already clipped to `/Domain`, and the load-time checks above
    /// guarantee `x^N` is finite for every `x` the domain admits, so this
    /// cannot produce `NaN`.
    fn eval(&self, inputs: &[f64], out: &mut Vec<f64>) {
        let x = inputs.first().copied().unwrap_or(0.0);
        // `powi` for integral exponents: exact for the common N = 1 and N = 2,
        // and avoids `powf`'s log/exp round trip. `powf` otherwise.
        let xn = if self.n.fract() == 0.0 && self.n.abs() <= f64::from(i32::MAX) {
            x.powi(self.n as i32)
        } else {
            x.powf(self.n)
        };
        for (a, b) in self.c0.iter().zip(self.c1.iter()) {
            out.push(a + xn * (b - a));
        }
    }
}

// ---------------------------------------------------------------------------
// Type 3 — stitching (§7.10.4)
// ---------------------------------------------------------------------------

/// A type 3 function: *k* 1-input sub-functions laid end to end across
/// `/Domain`, each fed a rescaled slice of it.
///
/// ## The partition, and the half-open convention
///
/// §7.10.4 fixes the ordering
///
/// ```text
/// Domain_0 < Bounds_0 < Bounds_1 < … < Bounds_(k−2) < Domain_1
/// ```
///
/// and the intervals are *"half-open intervals, closed on the left and open on
/// the right (except the last, which is closed on the right as well)"*:
///
/// | Sub-function | Interval |
/// |---|---|
/// | 0 | `Domain_0 ≤ x < Bounds_0` |
/// | *i* | `Bounds_(i−1) ≤ x < Bounds_i` |
/// | *k−1* | `Bounds_(k−2) ≤ x ≤ Domain_1` |
///
/// **Exactly on a bound, the later sub-function wins.** [`Stitching::select`]
/// implements that by counting how many bounds `x` is greater than or equal to,
/// which gets the boundary case right by construction rather than by an
/// `if`-chain that has to remember which comparison is strict.
///
/// ## `k = 1` is the domain-reversal idiom
///
/// §7.10.4 explicitly allows a single sub-function with an empty `/Bounds`, and
/// notes that *"type 3 functions provide a general mechanism for inverting the
/// domains of 1-input functions"*: with `/Domain [0 1]` and `/Encode [1 0]`, a
/// one-element stitching function computes `g(x) = f(1 − x)`. It is not a
/// degenerate case to be special-cased away — it is a deliberate construction
/// that appears in real shadings.
#[derive(Debug, Clone, PartialEq)]
struct Stitching {
    /// `/Functions` — *k* sub-functions, each with one input.
    functions: Vec<PdfFunction>,
    /// `/Bounds` — *k* − 1 interior partition points.
    bounds: Vec<f64>,
    /// `/Encode` — one `[lo, hi]` pair per sub-function.
    encode: Vec<[f64; 2]>,
    /// *n*, taken from sub-function 0 and verified equal for all the others.
    outputs: usize,
}

/// Build a [`Stitching`], validating Table 41 and the partition rule.
///
/// # Errors
///
/// [`FunctionError`] — see [`PdfFunction::load`].
fn load_stitching(
    view: &DocumentView<'_>,
    dict: &Dict,
    domain: &[[f64; 2]],
    range: Option<&[[f64; 2]]>,
    depth: usize,
) -> Result<Stitching, FunctionError> {
    // §7.10.4: "Domain shall be of size 2 (that is, m = 1)".
    if domain.len() != 1 {
        return Err(FunctionError::NotOneInput {
            function_type: 3,
            got: domain.len(),
        });
    }
    let bounds_of_domain = domain.first().copied().unwrap_or([0.0, 1.0]);

    // --- /Functions ------------------------------------------------------
    let entries: Vec<Object> = entry(view, dict, b"Functions")
        .and_then(Object::as_array)
        .ok_or(FunctionError::MissingEntry {
            key: "Functions",
            function_type: 3,
        })?
        // Cloned because each sub-function load re-borrows the view, and a
        // borrow of the array would alias that. The array holds at most a
        // handful of references; this is not a hot path (load, not eval).
        .to_vec();
    if entries.is_empty() {
        return Err(FunctionError::NoSubFunctions);
    }

    let mut functions = Vec::with_capacity(entries.len());
    let mut outputs = 0usize;
    for (index, sub) in entries.iter().enumerate() {
        let f = PdfFunction::load_at_depth(view, sub, depth + 1)?;
        // §7.10.4: "an array of k 1-input functions".
        if f.inputs() != 1 {
            return Err(FunctionError::BadEntry {
                key: "Functions",
                detail: format!(
                    "sub-function {index} takes {} inputs; stitching sub-functions take one",
                    f.inputs()
                ),
            });
        }
        // §7.10.4: "The output dimensionality of all functions shall be the
        // same". Sub-function 0 sets it.
        if index == 0 {
            outputs = f.outputs();
        } else if f.outputs() != outputs {
            return Err(FunctionError::SubFunctionArity {
                index,
                expected: outputs,
                got: f.outputs(),
            });
        }
        functions.push(f);
    }
    let k = functions.len();

    // Table 38's consistency rule, applied to the one place a type 3 can
    // contradict itself: /Range says n, the sub-functions say something else.
    if let Some(r) = range
        && r.len() != outputs
    {
        return Err(FunctionError::BadArrayLength {
            key: "Range",
            expected: outputs * 2,
            got: r.len() * 2,
        });
    }

    // --- /Bounds ---------------------------------------------------------
    let bounds = numbers(view, dict, "Bounds")?.ok_or(FunctionError::MissingEntry {
        key: "Bounds",
        function_type: 3,
    })?;
    // §7.10.4: "an array of k − 1 numbers"; "The value of k may be 1, in which
    // case the Bounds array shall be empty".
    if bounds.len() + 1 != k {
        return Err(FunctionError::BadArrayLength {
            key: "Bounds",
            expected: k - 1,
            got: bounds.len(),
        });
    }
    // "Bounds elements shall be in order of increasing value, and each value
    // shall be within the domain defined by Domain."
    //
    // Two deliberate readings, both narrower than a maximally strict one:
    //
    //  * EQUAL adjacent bounds are ACCEPTED. The partition rule is written with
    //    strict `<`, but `[b, b)` is an unambiguously empty interval whose
    //    sub-function is simply never selected — there is nothing to guess.
    //    Refusing it would reject files over a defect with no observable
    //    consequence. Only a strictly DECREASING pair is refused, because that
    //    has no reading at all.
    //  * A bound EQUAL to Domain_0 or Domain_1 is accepted, for the same
    //    reason and because §7.10.4 explicitly contemplates the second case
    //    ("If the last bound … is equal to Domain_1 …").
    let mut previous = f64::NEG_INFINITY;
    for (i, &b) in bounds.iter().enumerate() {
        if b < previous {
            return Err(FunctionError::BadBounds {
                detail: format!(
                    "element {i} ({b}) is less than element {} ({previous})",
                    i - 1
                ),
            });
        }
        if b < bounds_of_domain[0] || b > bounds_of_domain[1] {
            return Err(FunctionError::BadBounds {
                detail: format!(
                    "element {i} ({b}) is outside /Domain [{}, {}]",
                    bounds_of_domain[0], bounds_of_domain[1]
                ),
            });
        }
        previous = b;
    }

    // --- /Encode ---------------------------------------------------------
    let encode = pairs(view, dict, b"Encode")?.ok_or(FunctionError::MissingEntry {
        key: "Encode",
        function_type: 3,
    })?;
    if encode.len() != k {
        return Err(FunctionError::BadArrayLength {
            key: "Encode",
            expected: k * 2,
            got: encode.len() * 2,
        });
    }
    // /Encode pairs are NOT checked with `check_ordered`: unlike /Domain and
    // /Range they are a mapping, not an interval, and `[1 0]` is the documented
    // domain-reversal idiom (see this type's docs).

    Ok(Stitching {
        functions,
        bounds,
        encode,
        outputs,
    })
}

impl Stitching {
    /// Which sub-function owns `x`.
    ///
    /// Counts the bounds `x` has reached or passed. With the half-open
    /// convention (closed left, open right) that count *is* the index: `x`
    /// below every bound gives 0; `x` exactly on `Bounds_0` gives 1, which is
    /// the "closed on the left" rule; `x` at `Domain_1` gives `k − 1`, the last
    /// sub-function, whose interval is closed on the right too.
    ///
    /// The `min` is belt and braces — `x` is pre-clipped to `/Domain` and every
    /// bound is inside it, so the count cannot exceed `k − 1` — but it keeps the
    /// function total for any input.
    fn select(&self, x: f64) -> usize {
        self.bounds
            .iter()
            .filter(|&&b| x >= b)
            .count()
            .min(self.functions.len().saturating_sub(1))
    }

    /// Evaluate: pick the sub-function, rescale `x` onto its domain, delegate.
    ///
    /// # Errors
    ///
    /// Whatever the selected sub-function returns.
    fn eval(
        &self,
        domain: &[[f64; 2]],
        inputs: &[f64],
        out: &mut Vec<f64>,
    ) -> Result<(), FunctionError> {
        let bounds_of_domain = domain.first().copied().unwrap_or([0.0, 1.0]);
        let x = inputs.first().copied().unwrap_or(0.0);
        let i = self.select(x);

        // §7.10.4's Bounds_(−1) = Domain_0 and Bounds_(k−1) = Domain_1
        // convention, made explicit.
        let low = if i == 0 {
            bounds_of_domain[0]
        } else {
            self.bounds
                .get(i - 1)
                .copied()
                .unwrap_or(bounds_of_domain[0])
        };
        let high = self.bounds.get(i).copied().unwrap_or(bounds_of_domain[1]);

        let enc = self.encode.get(i).copied().unwrap_or([0.0, 1.0]);
        // When low == high (a collapsed subinterval, including §7.10.4's
        // "last bound equal to Domain_1" case), `interpolate` returns
        // `enc[0]` — which is precisely what that clause specifies.
        let encoded = interpolate(x, low, high, enc[0], enc[1]);

        let Some(f) = self.functions.get(i) else {
            // Unreachable: `select` clamps to the function count, and
            // `load_stitching` refuses an empty array.
            return Err(FunctionError::NoSubFunctions);
        };
        // The sub-function clips `encoded` to its OWN /Domain and its result to
        // its own /Range; the outer /Range clip then applies on top, in
        // `PdfFunction::eval_into`. Both are required — they are different
        // functions' declarations.
        f.eval_into(&[encoded], out)
    }
}

// ---------------------------------------------------------------------------
// Type 4 — PostScript calculator (§7.10.5)
// ---------------------------------------------------------------------------

/// A parsed type 4 program plus its declared output count.
///
/// §7.10.5.1: the code is *"a small subset of the PostScript language"* with
/// *"integers, real numbers, and boolean values only"* and *"no composite data
/// structures such as strings or arrays, no procedures, and no variables or
/// names"*. So there is no environment to model — the whole machine is an
/// operand stack.
#[derive(Debug, Clone, PartialEq)]
struct PostScript {
    /// The top-level block, already stripped of its enclosing braces.
    program: Vec<PsOp>,
    /// *n*, from `/Range` (required for type 4).
    outputs: usize,
}

/// One instruction.
///
/// `if`/`ifelse` carry their branches inline rather than as jump targets. The
/// spec's own framing supports it: *"This construct is purely syntactic; unlike
/// in PostScript, no 'procedure objects' shall be involved"* — a brace block is
/// not a value that can be pushed, duplicated or stored, so it can only ever be
/// the immediate operand of the operator that follows it. A tree therefore
/// loses nothing and makes the "a block must be consumed by `if`/`ifelse`"
/// rule a parse-time property rather than a runtime one.
#[derive(Debug, Clone, PartialEq)]
enum PsOp {
    /// A numeric literal. Integers and reals are kept apart because Table 42
    /// distinguishes them (`idiv` and `bitshift` are integer-only).
    Push(PsValue),
    /// A Table 42 operator.
    Op(PsOperator),
    /// `bool { … } if`
    If(Vec<PsOp>),
    /// `bool { … } { … } ifelse`
    IfElse(Vec<PsOp>, Vec<PsOp>),
}

/// A value on the operand stack.
///
/// Three types, exactly as §7.10.5.1 allows. Integers are `i32`: PostScript
/// integers are 32-bit, Annex C's implementation limits describe the same
/// width for the file grammar, and `bitshift` is only well defined against a
/// fixed width.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PsValue {
    /// An integer.
    Int(i32),
    /// A real.
    Real(f64),
    /// A boolean.
    Bool(bool),
}

impl PsValue {
    /// The numeric value, or `None` for a boolean.
    fn as_number(self) -> Option<f64> {
        match self {
            Self::Int(i) => Some(f64::from(i)),
            Self::Real(r) => Some(r),
            Self::Bool(_) => None,
        }
    }

    /// The boolean value, or `None` for a number.
    fn as_bool(self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(b),
            Self::Int(_) | Self::Real(_) => None,
        }
    }

    /// The integer value, or `None` for a real or boolean.
    fn as_int(self) -> Option<i32> {
        match self {
            Self::Int(i) => Some(i),
            Self::Real(_) | Self::Bool(_) => None,
        }
    }
}

/// Table 42's operator set — **42 entries**, of which the 38 below are
/// dispatched at run time and the other four are handled structurally by the
/// parser (`true`/`false` become [`PsOp::Push`], `if`/`ifelse` become
/// [`PsOp::If`]/[`PsOp::IfElse`]).
///
/// The 42/38 split is worth stating because the table's own grouping invites a
/// miscount: its "Relational, boolean, and bitwise" row contains 13 names, two
/// of which (`true`, `false`) are literals rather than operators.
///
/// **The set is closed.** §7.10.5.1 presents Table 42 as the operators
/// available, and §7.10.5.2 makes syntax detection a reader `shall`, so a token
/// that is not in this list is a defect in the file
/// ([`FunctionError::UnknownOperator`]) rather than a pdfcer gap to be silently
/// skipped. Skipping an unknown token would leave the stack the wrong depth and
/// surface as a bewildering output-arity error at the end of an otherwise valid
/// program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PsOperator {
    // Arithmetic (21).
    Abs,
    Add,
    Atan,
    Ceiling,
    Cos,
    Cvi,
    Cvr,
    Div,
    Exp,
    Floor,
    Idiv,
    Ln,
    Log,
    Mod,
    Mul,
    Neg,
    Round,
    Sin,
    Sqrt,
    Sub,
    Truncate,
    // Relational, boolean, bitwise (11 — `true` and `false` are literals).
    And,
    Bitshift,
    Eq,
    Ge,
    Gt,
    Le,
    Lt,
    Ne,
    Not,
    Or,
    Xor,
    // Stack (6).
    Copy,
    Dup,
    Exch,
    Index,
    Pop,
    Roll,
}

impl PsOperator {
    /// Map a source token to an operator.
    ///
    /// Returns `None` for `true`, `false`, `if` and `ifelse`, which the parser
    /// handles structurally, and for anything not in Table 42.
    fn from_token(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"abs" => Self::Abs,
            b"add" => Self::Add,
            b"atan" => Self::Atan,
            b"ceiling" => Self::Ceiling,
            b"cos" => Self::Cos,
            b"cvi" => Self::Cvi,
            b"cvr" => Self::Cvr,
            b"div" => Self::Div,
            b"exp" => Self::Exp,
            b"floor" => Self::Floor,
            b"idiv" => Self::Idiv,
            b"ln" => Self::Ln,
            b"log" => Self::Log,
            b"mod" => Self::Mod,
            b"mul" => Self::Mul,
            b"neg" => Self::Neg,
            b"round" => Self::Round,
            b"sin" => Self::Sin,
            b"sqrt" => Self::Sqrt,
            b"sub" => Self::Sub,
            b"truncate" => Self::Truncate,
            b"and" => Self::And,
            b"bitshift" => Self::Bitshift,
            b"eq" => Self::Eq,
            b"ge" => Self::Ge,
            b"gt" => Self::Gt,
            b"le" => Self::Le,
            b"lt" => Self::Lt,
            b"ne" => Self::Ne,
            b"not" => Self::Not,
            b"or" => Self::Or,
            b"xor" => Self::Xor,
            b"copy" => Self::Copy,
            b"dup" => Self::Dup,
            b"exch" => Self::Exch,
            b"index" => Self::Index,
            b"pop" => Self::Pop,
            b"roll" => Self::Roll,
            _ => return None,
        })
    }

    /// The operator's source spelling, for error messages.
    const fn name(self) -> &'static str {
        match self {
            Self::Abs => "abs",
            Self::Add => "add",
            Self::Atan => "atan",
            Self::Ceiling => "ceiling",
            Self::Cos => "cos",
            Self::Cvi => "cvi",
            Self::Cvr => "cvr",
            Self::Div => "div",
            Self::Exp => "exp",
            Self::Floor => "floor",
            Self::Idiv => "idiv",
            Self::Ln => "ln",
            Self::Log => "log",
            Self::Mod => "mod",
            Self::Mul => "mul",
            Self::Neg => "neg",
            Self::Round => "round",
            Self::Sin => "sin",
            Self::Sqrt => "sqrt",
            Self::Sub => "sub",
            Self::Truncate => "truncate",
            Self::And => "and",
            Self::Bitshift => "bitshift",
            Self::Eq => "eq",
            Self::Ge => "ge",
            Self::Gt => "gt",
            Self::Le => "le",
            Self::Lt => "lt",
            Self::Ne => "ne",
            Self::Not => "not",
            Self::Or => "or",
            Self::Xor => "xor",
            Self::Copy => "copy",
            Self::Dup => "dup",
            Self::Exch => "exch",
            Self::Index => "index",
            Self::Pop => "pop",
            Self::Roll => "roll",
        }
    }
}

/// Build a [`PostScript`] from its stream: decode, lex, parse.
///
/// # Errors
///
/// [`FunctionError`] — see [`PdfFunction::load`].
fn load_postscript(
    view: &DocumentView<'_>,
    resolved: &Object,
    domain: &[[f64; 2]],
    range: Option<&[[f64; 2]]>,
) -> Result<PostScript, FunctionError> {
    let Object::Stream(stream) = resolved else {
        return Err(FunctionError::NotAStream { function_type: 4 });
    };
    // §7.10.5.1: "The Domain and Range entries shall both be required."
    // /Domain's presence is enforced by the common loader (which also rejects an
    // empty one); /Range is checked here because it is type-4-specific and is
    // the only source of n.
    let _ = domain;
    let range = range.ok_or(FunctionError::MissingEntry {
        key: "Range",
        function_type: 4,
    })?;

    let source = stream_bytes(view, stream)?;
    let program = parse_program(&source)?;
    Ok(PostScript {
        program,
        outputs: range.len(),
    })
}

/// Parse a whole type 4 stream into its top-level block.
///
/// §7.10.5.1: *"The entire code stream defining the function shall be enclosed
/// in braces `{ }`."* pdfcer enforces that literally — a stream whose first
/// token is not `{` is refused rather than treated as an implicit block.
///
/// That strictness is a choice, and it is the one this project's posture calls
/// for: the alternative is guessing that a producer meant an implicit outer
/// block, and a guess that turns out wrong yields a program that runs and
/// produces a colour. A refusal naming the missing brace can be acted on.
///
/// The lexer already classifies `{` and `}` as [`TokenKind::BraceOpen`] and
/// [`TokenKind::BraceClose`] (they are Table 2 delimiters) and skips comments
/// and white-space per §7.2 — so this parser needs no tokenizer of its own.
/// That reuse is deliberate: a second tokenizer would eventually disagree with
/// the first about something like `4.` or a `%` comment inside a program.
///
/// # Errors
///
/// [`FunctionError::PostScriptLex`], [`FunctionError::PostScriptSyntax`],
/// [`FunctionError::UnknownOperator`],
/// [`FunctionError::PostScriptNestingTooDeep`].
fn parse_program(source: &[u8]) -> Result<Vec<PsOp>, FunctionError> {
    let mut lexer = Lexer::new(source);
    let Some(first) = lexer.next_token()? else {
        return Err(FunctionError::PostScriptSyntax {
            detail: "the program stream is empty".to_owned(),
        });
    };
    if first.kind != TokenKind::BraceOpen {
        return Err(FunctionError::PostScriptSyntax {
            detail: "the program is not enclosed in braces (§7.10.5.1 requires `{ … }`)".to_owned(),
        });
    }
    let block = parse_block(&mut lexer, source, 1)?;
    if let Some(extra) = lexer.next_token()? {
        return Err(FunctionError::PostScriptSyntax {
            detail: format!(
                "trailing content after the closing brace, starting at byte {}",
                extra.span.start
            ),
        });
    }
    Ok(block)
}

/// Parse tokens up to the matching `}`.
///
/// The one non-obvious rule is how brace blocks attach to their operator. In
/// this sub-language a block is **not a value** — §7.10.5.1: *"This construct is
/// purely syntactic; unlike in PostScript, no 'procedure objects' shall be
/// involved"* — so it can only be the operand of the `if` or `ifelse` that
/// immediately follows. A parsed block is therefore held in `pending` until one
/// of those two claims it, and anything else appearing while `pending` is
/// non-empty is a syntax error. That check rejects `{ 1 } 2 add` and
/// `{ 1 } { 2 } { 3 } ifelse` at parse time instead of letting them become a
/// confusing runtime failure.
///
/// # Errors
///
/// As [`parse_program`].
fn parse_block(
    lexer: &mut Lexer<'_>,
    source: &[u8],
    depth: usize,
) -> Result<Vec<PsOp>, FunctionError> {
    if depth > MAX_PS_NESTING {
        return Err(FunctionError::PostScriptNestingTooDeep {
            limit: MAX_PS_NESTING,
        });
    }

    let mut ops: Vec<PsOp> = Vec::new();
    // At most two blocks can be pending (the `ifelse` case).
    let mut pending: Vec<Vec<PsOp>> = Vec::new();

    loop {
        let Some(token) = lexer.next_token()? else {
            return Err(FunctionError::PostScriptSyntax {
                detail: "end of stream inside a `{ … }` block".to_owned(),
            });
        };

        match token.kind {
            TokenKind::BraceClose => {
                reject_pending(&pending)?;
                return Ok(ops);
            }
            TokenKind::BraceOpen => {
                if pending.len() >= 2 {
                    return Err(FunctionError::PostScriptSyntax {
                        detail: "more than two consecutive `{ … }` blocks; `ifelse` takes two"
                            .to_owned(),
                    });
                }
                let nested = parse_block(lexer, source, depth + 1)?;
                pending.push(nested);
            }
            TokenKind::Integer(v) => {
                reject_pending(&pending)?;
                // Annex C's integer range is ±2^31; a literal outside i32 is
                // refused rather than saturated, because a saturated constant is
                // a wrong number that then computes silently.
                let Ok(i) = i32::try_from(v) else {
                    return Err(FunctionError::PostScriptSyntax {
                        detail: format!("integer literal {v} does not fit a 32-bit integer"),
                    });
                };
                ops.push(PsOp::Push(PsValue::Int(i)));
            }
            TokenKind::Real(v) => {
                reject_pending(&pending)?;
                if !v.is_finite() {
                    return Err(FunctionError::PostScriptSyntax {
                        detail: format!("real literal {v} is not finite"),
                    });
                }
                ops.push(PsOp::Push(PsValue::Real(v)));
            }
            TokenKind::Keyword => {
                let name = token.lexeme(source).unwrap_or(b"");
                match name {
                    b"true" | b"false" => {
                        reject_pending(&pending)?;
                        ops.push(PsOp::Push(PsValue::Bool(name == b"true")));
                    }
                    b"if" => {
                        if pending.len() != 1 {
                            return Err(FunctionError::PostScriptSyntax {
                                detail: format!(
                                    "`if` needs exactly one preceding `{{ … }}` block; found {}",
                                    pending.len()
                                ),
                            });
                        }
                        let then_block = pending.pop().unwrap_or_default();
                        ops.push(PsOp::If(then_block));
                    }
                    b"ifelse" => {
                        if pending.len() != 2 {
                            return Err(FunctionError::PostScriptSyntax {
                                detail: format!(
                                    "`ifelse` needs exactly two preceding `{{ … }}` blocks; found {}",
                                    pending.len()
                                ),
                            });
                        }
                        // `pending` is [then, else] in source order, so `pop`
                        // yields the else branch first.
                        let else_block = pending.pop().unwrap_or_default();
                        let then_block = pending.pop().unwrap_or_default();
                        ops.push(PsOp::IfElse(then_block, else_block));
                    }
                    other => {
                        reject_pending(&pending)?;
                        let op = PsOperator::from_token(other).ok_or_else(|| {
                            FunctionError::UnknownOperator(
                                String::from_utf8_lossy(other).into_owned(),
                            )
                        })?;
                        ops.push(PsOp::Op(op));
                    }
                }
            }
            other => {
                return Err(FunctionError::PostScriptSyntax {
                    detail: format!(
                        "{other:?} at byte {} has no meaning in a type 4 program \
                         (§7.10.5.1 allows numbers, booleans, operators and braces only)",
                        token.span.start
                    ),
                });
            }
        }
    }
}

/// Refuse a token that appears while a `{ … }` block is still waiting for its
/// `if`/`ifelse`.
///
/// # Errors
///
/// [`FunctionError::PostScriptSyntax`].
fn reject_pending(pending: &[Vec<PsOp>]) -> Result<(), FunctionError> {
    if pending.is_empty() {
        Ok(())
    } else {
        Err(FunctionError::PostScriptSyntax {
            detail: "a `{ … }` block must be followed immediately by `if` or `ifelse`".to_owned(),
        })
    }
}

/// The operand stack, with §7.10.5.1's capacity rule enforced on every push.
///
/// A newtype rather than a bare `Vec<PsValue>` for one reason: the overflow
/// check has to be on *every* growth path, and there are several (`push`, `dup`,
/// `copy`, the initial input load). Centralising it here makes
/// [`FunctionError::StackOverflow`] impossible to forget, which matters because
/// the spec makes overflow *an error* rather than a resize.
#[derive(Debug)]
struct PsStack {
    values: Vec<PsValue>,
}

impl PsStack {
    /// A stack pre-sized to the spec's 100-entry capacity, so no evaluation
    /// reallocates.
    fn new() -> Self {
        Self {
            values: Vec::with_capacity(PS_STACK_LIMIT),
        }
    }

    /// Push, refusing to exceed [`PS_STACK_LIMIT`].
    ///
    /// # Errors
    ///
    /// [`FunctionError::StackOverflow`].
    fn push(&mut self, value: PsValue) -> Result<(), FunctionError> {
        if self.values.len() >= PS_STACK_LIMIT {
            return Err(FunctionError::StackOverflow {
                limit: PS_STACK_LIMIT,
            });
        }
        self.values.push(value);
        Ok(())
    }

    /// Pop one value.
    ///
    /// # Errors
    ///
    /// [`FunctionError::StackUnderflow`].
    fn pop(&mut self, op: &'static str) -> Result<PsValue, FunctionError> {
        self.values.pop().ok_or(FunctionError::StackUnderflow {
            op,
            needed: 1,
            had: 0,
        })
    }

    /// Pop two values and return them in **source order** — `(first, second)`
    /// for a program that wrote `first second op`.
    ///
    /// Returning them pre-swapped is not a nicety. `sub`, `div`, `idiv`, `mod`,
    /// `exp`, `bitshift` and every relational operator are non-commutative, and
    /// the classic defect in a hand-written PostScript machine is
    /// `a b sub` computing `b − a`. Doing the swap in exactly one place removes
    /// the chance to get it wrong per operator.
    ///
    /// # Errors
    ///
    /// [`FunctionError::StackUnderflow`].
    fn pop2(&mut self, op: &'static str) -> Result<(PsValue, PsValue), FunctionError> {
        if self.values.len() < 2 {
            return Err(FunctionError::StackUnderflow {
                op,
                needed: 2,
                had: self.values.len(),
            });
        }
        let second = self.values.pop().ok_or(FunctionError::StackUnderflow {
            op,
            needed: 2,
            had: 0,
        })?;
        let first = self.values.pop().ok_or(FunctionError::StackUnderflow {
            op,
            needed: 2,
            had: 1,
        })?;
        Ok((first, second))
    }

    /// Pop one value as a number.
    ///
    /// # Errors
    ///
    /// [`FunctionError::StackUnderflow`], or [`FunctionError::PostScriptType`]
    /// if the operand is a boolean.
    fn pop_number(&mut self, op: &'static str) -> Result<f64, FunctionError> {
        self.pop(op)?
            .as_number()
            .ok_or(FunctionError::PostScriptType {
                op,
                detail: "expected a number, found a boolean",
            })
    }

    /// Pop two values as numbers, in source order.
    ///
    /// # Errors
    ///
    /// As [`PsStack::pop2`] and [`PsStack::pop_number`].
    fn pop2_numbers(&mut self, op: &'static str) -> Result<(f64, f64), FunctionError> {
        let (a, b) = self.pop2(op)?;
        let ty = FunctionError::PostScriptType {
            op,
            detail: "expected two numbers, found a boolean",
        };
        Ok((
            a.as_number().ok_or_else(|| ty.clone())?,
            b.as_number().ok_or(ty)?,
        ))
    }

    /// Pop one value as a 32-bit integer.
    ///
    /// # Errors
    ///
    /// [`FunctionError::StackUnderflow`], or [`FunctionError::PostScriptType`]
    /// for a real or a boolean. A real is rejected rather than truncated: `idiv`
    /// and `bitshift` are integer operators, and quietly truncating `2.7` to `2`
    /// would turn a file defect into a wrong number.
    fn pop_int(&mut self, op: &'static str) -> Result<i32, FunctionError> {
        self.pop(op)?.as_int().ok_or(FunctionError::PostScriptType {
            op,
            detail: "expected an integer; reals and booleans are not accepted here",
        })
    }

    /// Pop one value as a count operand for `copy`, `index` or `roll`.
    ///
    /// # Errors
    ///
    /// [`FunctionError::StackUnderflow`], [`FunctionError::PostScriptType`] for a
    /// non-integer, or [`FunctionError::PostScriptRange`] for a negative count.
    fn pop_count(&mut self, op: &'static str) -> Result<usize, FunctionError> {
        let n = self.pop_int(op)?;
        usize::try_from(n).map_err(|_| FunctionError::PostScriptRange {
            op,
            detail: "the count operand is negative",
        })
    }

    /// Current depth.
    fn len(&self) -> usize {
        self.values.len()
    }
}

impl PostScript {
    /// Run the program with `inputs` as the initial operand stack.
    ///
    /// §7.10.5.1: *"The input variables shall constitute the initial operand
    /// stack; the items remaining on the operand stack after execution … shall
    /// be the output variables. It shall be an error for the number of
    /// remaining operands to differ from the number of output variables
    /// specified by `Range` or for any of them to be objects other than
    /// numbers."*
    ///
    /// Both halves of that final sentence are enforced
    /// ([`FunctionError::OutputArity`], [`FunctionError::NonNumericOutput`]).
    /// Neither is negotiable: a program that leaves three values for a
    /// four-component alternate space has no defined meaning, and §7.10's
    /// negative result `F-N2` records that the standard offers no recovery for
    /// it either.
    ///
    /// Inputs are pushed **in order**, so input 0 is deepest — the natural
    /// reading of "the input variables constitute the initial stack", and what
    /// makes a 2-in program's `exch` behave as its author intended.
    ///
    /// # Errors
    ///
    /// Every `PostScript*` variant of [`FunctionError`], plus
    /// [`FunctionError::StepLimit`], [`FunctionError::OutputArity`] and
    /// [`FunctionError::NonNumericOutput`].
    fn eval(&self, inputs: &[f64], out: &mut Vec<f64>) -> Result<(), FunctionError> {
        let mut stack = PsStack::new();
        for &x in inputs {
            stack.push(PsValue::Real(x))?;
        }

        let mut steps = 0usize;
        exec_block(&self.program, &mut stack, &mut steps)?;

        if stack.len() != self.outputs {
            return Err(FunctionError::OutputArity {
                expected: self.outputs,
                got: stack.len(),
            });
        }
        for (index, value) in stack.values.iter().enumerate() {
            let Some(v) = value.as_number() else {
                return Err(FunctionError::NonNumericOutput { index });
            };
            out.push(v);
        }
        Ok(())
    }
}

/// Execute a block of operations against `stack`.
///
/// `steps` is threaded through rather than held per-block so the
/// [`MAX_PS_STEPS`] cap covers the whole evaluation, not each branch
/// separately. Recursion depth is bounded by [`MAX_PS_NESTING`] at parse time,
/// so this cannot overflow the native stack.
///
/// # Errors
///
/// As [`PostScript::eval`].
fn exec_block(ops: &[PsOp], stack: &mut PsStack, steps: &mut usize) -> Result<(), FunctionError> {
    for op in ops {
        *steps += 1;
        if *steps > MAX_PS_STEPS {
            return Err(FunctionError::StepLimit {
                limit: MAX_PS_STEPS,
            });
        }
        match op {
            PsOp::Push(v) => stack.push(*v)?,
            PsOp::Op(o) => apply(*o, stack)?,
            PsOp::If(then_block) => {
                let cond = stack.pop("if")?;
                let Some(b) = cond.as_bool() else {
                    return Err(FunctionError::PostScriptType {
                        op: "if",
                        detail: "the condition operand is not a boolean",
                    });
                };
                if b {
                    exec_block(then_block, stack, steps)?;
                }
            }
            PsOp::IfElse(then_block, else_block) => {
                let cond = stack.pop("ifelse")?;
                let Some(b) = cond.as_bool() else {
                    return Err(FunctionError::PostScriptType {
                        op: "ifelse",
                        detail: "the condition operand is not a boolean",
                    });
                };
                if b {
                    exec_block(then_block, stack, steps)?;
                } else {
                    exec_block(else_block, stack, steps)?;
                }
            }
        }
    }
    Ok(())
}

/// Integer width for type 4 arithmetic — **a resolved spec ambiguity, not a
/// sourced value** (recorded as `F-A3` in `iso32000__s__7.10.md`).
///
/// `bitshift`'s *direction* and zero-fill are sourced (Annex B and PLRM3 §8.2),
/// but the *width* it shifts within is disclaimed by both documents.
/// §7.10.5.1 exempts type 4 intermediates from Annex C's limits — *"an
/// implementation may use a representation that exceeds those limits"* — and
/// PLRM3 says the representation of integers *"may depend on the CPU
/// architecture"*. So there is no width to look up.
///
/// pdfcer fixes it at **32**, because Annex C Table C.1's `integer` row is the
/// only integer range ISO 32000-1 states anywhere, and because it is what every
/// PostScript interpreter of the era used. The choice is observable in three
/// places: the result of a right shift on a negative operand, the point at
/// which `add`/`sub`/`mul` spill from integer to real, and `cvi`'s
/// `rangecheck`.
///
/// This is exactly the shape of thing that belongs in
/// [`crate::settings::Settings`] alongside the other resolved ambiguities
/// rather than being frozen in a constant; it is a constant here only because
/// this module was added without touching the settings surface. Wiring it up is
/// a small, mechanical follow-up.
pub const PS_INTEGER_BITS: u32 = 32;

/// A `typecheck` refusal (§7.10.5.2's *"type error"* class).
const fn type_err(op: &'static str, detail: &'static str) -> FunctionError {
    FunctionError::PostScriptType { op, detail }
}

/// A `rangecheck` refusal (§7.10.5.2's *"range error"* class).
const fn range_err(op: &'static str, detail: &'static str) -> FunctionError {
    FunctionError::PostScriptRange { op, detail }
}

/// An `undefinedresult` refusal (§7.10.5.2's *"undefined result"* class).
const fn undefined(op: &'static str, detail: &'static str) -> FunctionError {
    FunctionError::UndefinedResult { op, detail }
}

/// Binary arithmetic with Annex B's integer-preservation rule.
///
/// Annex B types `add`, `sub` and `mul` as `num1 num2 → sum`, and PLRM3 §8.2
/// fixes what "num" means on the way out: the result is an **integer if both
/// operands were integers and the true result is representable as one**, and a
/// **real otherwise**. The overflow half of that is not a rounding detail — it
/// is why `2000000000 2000000000 add` yields `4.0e9` rather than wrapping to a
/// negative number, and a wrapped value here would come out of a tint transform
/// as a wildly wrong colour component.
///
/// `int_op` is the checked integer form (returning `None` on overflow) and
/// `real_op` the `f64` fallback, so the two paths cannot drift apart.
///
/// # Errors
///
/// [`FunctionError::PostScriptType`] if either operand is a boolean.
fn arith(
    op: &'static str,
    a: PsValue,
    b: PsValue,
    int_op: fn(i32, i32) -> Option<i32>,
    real_op: fn(f64, f64) -> f64,
) -> Result<PsValue, FunctionError> {
    if let (PsValue::Int(x), PsValue::Int(y)) = (a, b) {
        return Ok(match int_op(x, y) {
            Some(v) => PsValue::Int(v),
            None => PsValue::Real(real_op(f64::from(x), f64::from(y))),
        });
    }
    let detail = "expected two numbers, found a boolean";
    let x = a.as_number().ok_or(type_err(op, detail))?;
    let y = b.as_number().ok_or(type_err(op, detail))?;
    Ok(PsValue::Real(real_op(x, y)))
}

/// The four rounding operators, which are **type-preserving, not
/// integer-producing**.
///
/// This is the single most surprising rule in Table 42 and the one most likely
/// to be got wrong from intuition. PLRM3 §8.2: `3.2 floor` yields the **real**
/// `3.0`, not the integer `3`; `99 floor` yields the integer `99` unchanged.
/// Only `cvi` converts a real to an integer.
///
/// It matters because it is *observable*: a program that writes
/// `2 div floor 2 idiv` is malformed, because `floor` handed `idiv` a real and
/// `idiv` is integer-only. An implementation that helpfully returned an integer
/// from `floor` would accept that program and compute a value no conforming
/// reader computes.
///
/// # Errors
///
/// [`FunctionError::PostScriptType`] for a boolean operand.
fn round_like(
    op: &'static str,
    v: PsValue,
    real_op: fn(f64) -> f64,
) -> Result<PsValue, FunctionError> {
    match v {
        // Already integral, and its type is preserved.
        PsValue::Int(i) => Ok(PsValue::Int(i)),
        PsValue::Real(r) => Ok(PsValue::Real(real_op(r))),
        PsValue::Bool(_) => Err(type_err(op, "expected a number, found a boolean")),
    }
}

/// PostScript's `round`: to the nearest integral value, **ties toward the
/// greater** value.
///
/// PLRM3 §8.2 gives `6.5 round → 7.0` and `-6.5 round → -6.0`. Rust's
/// `f64::round` breaks ties *away from zero*, so it returns `-7.0` for the
/// second — the one case where the obvious call is wrong. `(x + 0.5).floor()`
/// reproduces the specified behaviour for both signs.
fn round_half_up(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// Apply one Table 42 operator to the operand stack.
///
/// ## Where these semantics come from
///
/// Table 42 itself (§7.10.5.1) lists **only operator names** — it has no
/// semantics column. The clause delegates: *"the semantics are those of the
/// corresponding PostScript operators"*, pointing at the *PostScript Language
/// Reference*, 3rd edition. Two things follow, and both are recorded in
/// `iso32000__s__7.10.md`:
///
/// 1. **ISO 32000-1 Annex B, `(normative)`, "Operators in Type 4 Functions"**
///    supplies the stack effect, arity and operand/result typing for all 42
///    entries. §7.10.5 never cross-references it, which is why it is easy to
///    miss from the clause — but it is normative, first-party, and it settles
///    degrees-versus-radians, `atan`'s two-operand form, `exp` as `pow`, `log`
///    as base 10, `bitshift`'s direction, and the `bool|int` polymorphism of
///    `and`/`or`/`xor`/`not`.
/// 2. **PLRM3 §8.2** supplies what Annex B does not: rounding directions, sign
///    conventions, integer preservation, `atan`'s `[0, 360)` normalisation, the
///    zero-fill of a right shift, and the error classes. Note that §7.10.5.1's
///    own pointer to *"Appendix B"* of PLRM3 is **misdirected** — PLRM3's
///    Appendix B is *Implementation Limits*; the operator definitions are in
///    Chapter 8. (Erratum `F-E3`.) PLRM3 is a Bibliography reference, i.e.
///    *informative*, which is why it is cited here alongside Annex B rather
///    than instead of it.
///
/// ## Errors, and only these errors
///
/// §7.10.5.2 lists five classes and they map one-to-one onto PostScript's:
/// `stackoverflow`, `stackunderflow`, `typecheck`, `rangecheck`,
/// `undefinedresult`. PostScript's `invalidaccess`, `syntaxerror` and
/// `limitcheck` are **unreachable** in this subset — they belong to the
/// string/array/dictionary/file forms of `copy`, `cvi`, `cvr`, `eq` and the
/// relationals, and the subset has no composite types at all. They are
/// deliberately not implemented.
///
/// # Errors
///
/// [`FunctionError::StackUnderflow`], [`FunctionError::StackOverflow`],
/// [`FunctionError::PostScriptType`], [`FunctionError::PostScriptRange`],
/// [`FunctionError::UndefinedResult`].
fn apply(op: PsOperator, stack: &mut PsStack) -> Result<(), FunctionError> {
    use PsOperator as O;
    let name = op.name();

    match op {
        // --- Arithmetic (Annex B; PLRM3 §8.2) ------------------------------
        O::Add => {
            let (a, b) = stack.pop2(name)?;
            let v = arith(name, a, b, i32::checked_add, |x, y| x + y)?;
            stack.push(v)?;
        }
        O::Sub => {
            let (a, b) = stack.pop2(name)?;
            let v = arith(name, a, b, i32::checked_sub, |x, y| x - y)?;
            stack.push(v)?;
        }
        O::Mul => {
            let (a, b) = stack.pop2(name)?;
            let v = arith(name, a, b, i32::checked_mul, |x, y| x * y)?;
            stack.push(v)?;
        }
        // Annex B types `div` as `num1 num2 div → quotient`, and PLRM3 makes
        // the result ALWAYS a real, even for `4 2 div` (which yields 2.0, not
        // the integer 2). Integer division is a separate operator.
        O::Div => {
            let (a, b) = stack.pop2_numbers(name)?;
            if b == 0.0 {
                return Err(undefined(name, "division by zero"));
            }
            stack.push(PsValue::Real(a / b))?;
        }
        // `idiv` and `mod` are integer-only, and their sign conventions ARE
        // stated (PLRM3 §8.2): `idiv` truncates toward zero (`-5 2 idiv → -2`)
        // and `mod` takes the sign of the DIVIDEND (`-5 3 mod → -2`, described
        // as "a remainder operation rather than a true modulo"). Rust's `/` and
        // `%` on `i32` have exactly those semantics, so no adjustment is needed
        // — but the checked forms are used because `i32::MIN / -1` overflows.
        O::Idiv => {
            let (a, b) = stack.pop2(name)?;
            let (x, y) = (int_operand(name, a)?, int_operand(name, b)?);
            let q = x
                .checked_div(y)
                .ok_or(undefined(name, "division by zero, or i32::MIN / -1"))?;
            stack.push(PsValue::Int(q))?;
        }
        O::Mod => {
            let (a, b) = stack.pop2(name)?;
            let (x, y) = (int_operand(name, a)?, int_operand(name, b)?);
            let r = x
                .checked_rem(y)
                .ok_or(undefined(name, "division by zero, or i32::MIN % -1"))?;
            stack.push(PsValue::Int(r))?;
        }
        // `neg` and `abs` preserve their operand's type, with one documented
        // escape: the most negative integer has no positive counterpart, so it
        // becomes a real rather than wrapping back to itself.
        O::Neg => {
            let v = stack.pop(name)?;
            let r = match v {
                PsValue::Int(i) => i
                    .checked_neg()
                    .map_or_else(|| PsValue::Real(-f64::from(i)), PsValue::Int),
                PsValue::Real(r) => PsValue::Real(-r),
                PsValue::Bool(_) => {
                    return Err(type_err(name, "expected a number, found a boolean"));
                }
            };
            stack.push(r)?;
        }
        O::Abs => {
            let v = stack.pop(name)?;
            let r = match v {
                PsValue::Int(i) => i
                    .checked_abs()
                    .map_or_else(|| PsValue::Real(f64::from(i).abs()), PsValue::Int),
                PsValue::Real(r) => PsValue::Real(r.abs()),
                PsValue::Bool(_) => {
                    return Err(type_err(name, "expected a number, found a boolean"));
                }
            };
            stack.push(r)?;
        }
        O::Ceiling => {
            let v = stack.pop(name)?;
            let r = round_like(name, v, f64::ceil)?;
            stack.push(r)?;
        }
        O::Floor => {
            let v = stack.pop(name)?;
            let r = round_like(name, v, f64::floor)?;
            stack.push(r)?;
        }
        O::Round => {
            let v = stack.pop(name)?;
            let r = round_like(name, v, round_half_up)?;
            stack.push(r)?;
        }
        O::Truncate => {
            let v = stack.pop(name)?;
            let r = round_like(name, v, f64::trunc)?;
            stack.push(r)?;
        }
        O::Sqrt => {
            let x = stack.pop_number(name)?;
            if x < 0.0 {
                return Err(range_err(name, "sqrt of a negative number"));
            }
            stack.push(PsValue::Real(x.sqrt()))?;
        }
        // Annex B types these `angle sin → real` with the operand "in degrees".
        // The clause's own EXAMPLE corroborates it: the DoubleDot spot function
        // is `{ 360 mul sin 2 div exch 360 mul sin 2 div add }` over a
        // /Domain of [-1 1 -1 1] — the `360 mul` before each `sin` only makes
        // sense if `sin` wants degrees.
        O::Sin => {
            let x = stack.pop_number(name)?;
            stack.push(PsValue::Real(x.to_radians().sin()))?;
        }
        O::Cos => {
            let x = stack.pop_number(name)?;
            stack.push(PsValue::Real(x.to_radians().cos()))?;
        }
        // `num den atan → angle`: a two-operand arctangent, in DEGREES,
        // normalised to [0, 360) and therefore never negative.
        //
        // It is `atan2` with three divergences, and each one is a defect if
        // missed: Rust's `f64::atan2` returns radians, returns (−180°, 180°]
        // once converted, and answers 0 for `atan2(0.0, 0.0)`. PLRM3 requires
        // 270.0 where `atan2` gives −90.0, and makes `0 0 atan` an error.
        O::Atan => {
            let (num, den) = stack.pop2_numbers(name)?;
            if num == 0.0 && den == 0.0 {
                return Err(undefined(name, "atan of 0 over 0"));
            }
            let degrees = num.atan2(den).to_degrees();
            // `+ 0.0` normalises a negative zero to positive zero, so the
            // result is genuinely in [0, 360) rather than [-0.0, 360).
            let normalised = if degrees < 0.0 {
                degrees + 360.0
            } else {
                degrees + 0.0
            };
            stack.push(PsValue::Real(normalised))?;
        }
        // Annex B: `base exponent exp → real`. This is POW, not e^x — `9 0.5
        // exp` is 3.0. (`e^x` would be written `2.718… exch exp`.) Reading it
        // as e^x is the single easiest way to make a shading look plausible and
        // be wrong.
        O::Exp => {
            let (base, exponent) = stack.pop2_numbers(name)?;
            let r = base.powf(exponent);
            if !r.is_finite() {
                // 0 raised to a negative power, or a negative base under a
                // fractional exponent. §7.10.5.2's "undefined result".
                return Err(undefined(name, "exp has no finite result"));
            }
            stack.push(PsValue::Real(r))?;
        }
        // `ln` is natural, `log` is base 10 (Annex B states both). Both require
        // a positive operand.
        O::Ln => {
            let x = stack.pop_number(name)?;
            if x <= 0.0 {
                return Err(range_err(name, "ln of a non-positive number"));
            }
            stack.push(PsValue::Real(x.ln()))?;
        }
        O::Log => {
            let x = stack.pop_number(name)?;
            if x <= 0.0 {
                return Err(range_err(name, "log of a non-positive number"));
            }
            stack.push(PsValue::Real(x.log10()))?;
        }
        // `cvi` is the ONLY real-to-integer conversion in the language, and it
        // truncates toward zero (`-47.8 cvi → -47`), not toward negative
        // infinity. A value outside the integer range is a `rangecheck`.
        O::Cvi => {
            let x = stack.pop_number(name)?;
            let truncated = x.trunc();
            if !truncated.is_finite()
                || truncated < f64::from(i32::MIN)
                || truncated > f64::from(i32::MAX)
            {
                return Err(range_err(name, "value is outside the integer range"));
            }
            // In range and integral by construction, so the cast is exact.
            stack.push(PsValue::Int(truncated as i32))?;
        }
        O::Cvr => {
            let x = stack.pop_number(name)?;
            stack.push(PsValue::Real(x))?;
        }

        // --- Relational, boolean, bitwise ----------------------------------
        //
        // Annex B's typing here is deliberately three-way, and the contrast is
        // the evidence that it is deliberate:
        //   `eq`/`ne`                 -> any, any
        //   `gt`/`ge`/`lt`/`le`       -> num, num   (a boolean is a typecheck)
        //   `and`/`or`/`xor`/`not`    -> bool|int   (POLYMORPHIC)
        O::Eq => {
            let (a, b) = stack.pop2(name)?;
            stack.push(PsValue::Bool(ps_equal(a, b)))?;
        }
        O::Ne => {
            let (a, b) = stack.pop2(name)?;
            stack.push(PsValue::Bool(!ps_equal(a, b)))?;
        }
        O::Gt => {
            let (a, b) = stack.pop2_numbers(name)?;
            stack.push(PsValue::Bool(a > b))?;
        }
        O::Ge => {
            let (a, b) = stack.pop2_numbers(name)?;
            stack.push(PsValue::Bool(a >= b))?;
        }
        O::Lt => {
            let (a, b) = stack.pop2_numbers(name)?;
            stack.push(PsValue::Bool(a < b))?;
        }
        O::Le => {
            let (a, b) = stack.pop2_numbers(name)?;
            stack.push(PsValue::Bool(a <= b))?;
        }
        O::And => {
            let (a, b) = stack.pop2(name)?;
            let r = logical(name, a, b, |x, y| x && y, |x, y| x & y)?;
            stack.push(r)?;
        }
        O::Or => {
            let (a, b) = stack.pop2(name)?;
            let r = logical(name, a, b, |x, y| x || y, |x, y| x | y)?;
            stack.push(r)?;
        }
        O::Xor => {
            let (a, b) = stack.pop2(name)?;
            let r = logical(name, a, b, |x, y| x != y, |x, y| x ^ y)?;
            stack.push(r)?;
        }
        // On an integer, `not` is the ONES COMPLEMENT, not a zero test:
        // PLRM3's own example is `52 not → -53`. A boolean-only `not` that
        // coerced an integer would silently produce `false` there.
        O::Not => {
            let v = stack.pop(name)?;
            let r = match v {
                PsValue::Bool(b) => PsValue::Bool(!b),
                PsValue::Int(i) => PsValue::Int(!i),
                PsValue::Real(_) => {
                    return Err(type_err(
                        name,
                        "expected a boolean or an integer, found a real",
                    ));
                }
            };
            stack.push(r)?;
        }
        // `int1 shift bitshift → int2`. Positive shifts left, negative shifts
        // right, and PLRM3 is explicit that "bits shifted out are lost; bits
        // shifted in are 0" — so the right shift is LOGICAL, and `-8 -1
        // bitshift` is a large positive number rather than `-4`. The width it
        // shifts within is [`PS_INTEGER_BITS`], which is a resolved ambiguity
        // rather than a sourced value; see that constant.
        O::Bitshift => {
            let (a, s) = stack.pop2(name)?;
            let value = int_operand(name, a)?.cast_unsigned();
            let shift = int_operand(name, s)?;
            let width = PS_INTEGER_BITS;
            let shifted = if shift >= 0 {
                // `shift` is non-negative, so the conversion cannot fail.
                let by = u32::try_from(shift).unwrap_or(width);
                if by >= width { 0 } else { value << by }
            } else {
                // `checked_neg` guards `i32::MIN`, whose magnitude is far past
                // the width anyway — either way the answer is 0.
                let by = shift
                    .checked_neg()
                    .and_then(|m| u32::try_from(m).ok())
                    .unwrap_or(width);
                if by >= width { 0 } else { value >> by }
            };
            stack.push(PsValue::Int(shifted.cast_signed()))?;
        }

        // --- Stack ----------------------------------------------------------
        O::Pop => {
            stack.pop(name)?;
        }
        O::Exch => {
            let (a, b) = stack.pop2(name)?;
            stack.push(b)?;
            stack.push(a)?;
        }
        // Read rather than pop-and-push-twice, so a `dup` at exactly the stack
        // limit reports the overflow the spec calls for instead of succeeding
        // by transiently freeing a slot.
        O::Dup => {
            let top = *stack.values.last().ok_or(FunctionError::StackUnderflow {
                op: name,
                needed: 1,
                had: 0,
            })?;
            stack.push(top)?;
        }
        // `any1 … anyn n copy → any1 … anyn any1 … anyn`. The count is popped
        // first and is NOT part of the copied region. `0 copy` is explicitly a
        // legal no-op in PLRM3, so it must not be special-cased into an error.
        O::Copy => {
            let n = stack.pop_count(name)?;
            if n > 0 {
                let start = stack
                    .len()
                    .checked_sub(n)
                    .ok_or(range_err(name, "count exceeds the stack depth"))?;
                let duplicated: Vec<PsValue> = stack
                    .values
                    .get(start..)
                    .ok_or(range_err(name, "count exceeds the stack depth"))?
                    .to_vec();
                for v in duplicated {
                    stack.push(v)?;
                }
            }
        }
        // `anyn … any0 n index → anyn … any0 anyn`. Zero-based **from the top**,
        // so `0 index` is exactly `dup` — an implementation that counted from
        // the bottom, or from 1, would return a plausible wrong value rather
        // than fail.
        O::Index => {
            let n = stack.pop_count(name)?;
            let position = stack
                .len()
                .checked_sub(1)
                .and_then(|top| top.checked_sub(n))
                .ok_or(range_err(
                    name,
                    "index reaches past the bottom of the stack",
                ))?;
            let v = *stack.values.get(position).ok_or(range_err(
                name,
                "index reaches past the bottom of the stack",
            ))?;
            stack.push(v)?;
        }
        // `anyn−1 … any0 n j roll`: rotate the top `n` elements by `j`, with a
        // **positive `j` rotating upward** (toward the top).
        //
        // Reading `(a)(b)(c)` bottom-to-top with `(c)` on top, PLRM3 gives
        // `3 1 roll → (c)(a)(b)` and `3 -1 roll → (b)(c)(a)`. In a `Vec` whose
        // last element is the top, "upward by j" is precisely `rotate_right(j)`.
        // An implementation that rotated the other way agrees at `j = 0` and
        // disagrees at every other `j`, which is why the tests pin `j = ±1`.
        //
        // `n = 0` leaves `j mod n` undefined (ambiguity `F-A4`); treated as the
        // identity, consistent with `0 copy` being a legal no-op.
        O::Roll => {
            let j = stack.pop_int(name)?;
            let n = stack.pop_count(name)?;
            if n > 0 {
                let start = stack
                    .len()
                    .checked_sub(n)
                    .ok_or(range_err(name, "count exceeds the stack depth"))?;
                let window = i64::try_from(n).unwrap_or(i64::MAX);
                // Euclidean remainder, so a negative `j` lands in [0, n).
                let by = i64::from(j).rem_euclid(window);
                let by = usize::try_from(by).unwrap_or(0);
                stack
                    .values
                    .get_mut(start..)
                    .ok_or(range_err(name, "count exceeds the stack depth"))?
                    .rotate_right(by);
            }
        }
    }
    Ok(())
}

/// Pop-side type check for the integer-only operators (`idiv`, `mod`,
/// `bitshift`).
///
/// A real is **refused, not truncated**. Annex B types these operands `int`,
/// and `3.0 2 idiv` is a malformed program: truncating it to `3 2 idiv` would
/// make pdfcer compute a value where a conforming reader raises `typecheck`.
///
/// # Errors
///
/// [`FunctionError::PostScriptType`].
fn int_operand(op: &'static str, v: PsValue) -> Result<i32, FunctionError> {
    v.as_int().ok_or(type_err(
        op,
        "expected an integer; reals and booleans are not accepted here",
    ))
}

/// `eq`/`ne` equality over Annex B's `any` operands.
///
/// Numbers compare across the integer/real divide — PLRM3's `4.0 4 eq → true`
/// — while a boolean is equal only to a boolean. A boolean compared with a
/// number is **`false`, not an error**: Annex B types these two operators
/// `any any`, unlike `gt`/`ge`/`lt`/`le` which it types `num num`. That
/// asymmetry is deliberate and is the reason these two do not share the
/// relational path.
fn ps_equal(a: PsValue, b: PsValue) -> bool {
    match (a, b) {
        (PsValue::Bool(x), PsValue::Bool(y)) => x == y,
        (PsValue::Bool(_), _) | (_, PsValue::Bool(_)) => false,
        _ => a.as_number() == b.as_number(),
    }
}

/// `and`/`or`/`xor`: polymorphic over two booleans **or** two integers.
///
/// Annex B writes the operands and the result as `bool|int` for all three, so
/// the polymorphism is normative rather than inferred: `true false and` is
/// `false`, and `52 7 and` is `4`. A **mixed** pair is a `typecheck` — there is
/// no coercion in either direction.
///
/// # Errors
///
/// [`FunctionError::PostScriptType`] for a real operand or a mixed pair.
fn logical(
    op: &'static str,
    a: PsValue,
    b: PsValue,
    bool_op: fn(bool, bool) -> bool,
    int_op: fn(i32, i32) -> i32,
) -> Result<PsValue, FunctionError> {
    match (a, b) {
        (PsValue::Bool(x), PsValue::Bool(y)) => Ok(PsValue::Bool(bool_op(x, y))),
        (PsValue::Int(x), PsValue::Int(y)) => Ok(PsValue::Int(int_op(x, y))),
        _ => Err(type_err(
            op,
            "expected two booleans or two integers; mixed and real operands are not accepted",
        )),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
// Tests are exempt from the crate's panic-free policy: a panicking assertion IS
// the test-failure mechanism (see lib.rs's crate-level lint rationale).
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::PdfVersion;
    use crate::graph::ObjectGraph;
    use crate::object::{Name, ObjId};
    use crate::span::ByteSpan;
    use std::collections::BTreeMap;

    const V17: PdfVersion = PdfVersion { major: 1, minor: 7 };

    /// A hand-built object graph, so a function can be exercised without
    /// dragging a parsed file in. Only type 3's `/Functions` and the
    /// indirect-entry tests need the object map at all.
    #[derive(Default)]
    struct TestGraph {
        objects: BTreeMap<ObjId, Object>,
        trailer: Dict,
    }

    impl ObjectGraph for TestGraph {
        fn value(&self, id: ObjId) -> Option<&Object> {
            self.objects.get(&id)
        }
        fn trailer_entry(&self, key: &[u8]) -> Option<&Object> {
            self.trailer.get(key)
        }
    }

    fn dict(entries: &[(&[u8], Object)]) -> Dict {
        let mut d = Dict::new();
        for (k, v) in entries {
            d.insert(Name::from(*k), v.clone());
        }
        d
    }

    /// `[a, b, c]` as a PDF array of reals.
    fn arr(values: &[f64]) -> Object {
        Object::Array(values.iter().map(|&v| Object::Real(v)).collect())
    }

    /// Load a dictionary-shaped function (types 2 and 3).
    fn load_dict(entries: &[(&[u8], Object)]) -> Result<PdfFunction, FunctionError> {
        let graph = TestGraph::default();
        let view = DocumentView::new(&graph, b"", V17);
        PdfFunction::load(&view, &Object::Dict(dict(entries)))
    }

    /// Load a stream-shaped function (types 0 and 4). `data` becomes both the
    /// view's byte source and the stream's span, unfiltered.
    fn load_stream(entries: &[(&[u8], Object)], data: &[u8]) -> Result<PdfFunction, FunctionError> {
        let graph = TestGraph::default();
        let view = DocumentView::new(&graph, data, V17);
        let stream = Stream {
            dict: dict(entries),
            data_span: ByteSpan::new(0, data.len()),
        };
        PdfFunction::load(&view, &Object::Stream(stream))
    }

    /// A one-input, one-output identity ramp as a type 2 — the building block
    /// for the stitching tests.
    fn ramp(domain: &[f64], c0: &[f64], c1: &[f64]) -> Object {
        Object::Dict(dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(domain)),
            (b"N", Object::Integer(1)),
            (b"C0", arr(c0)),
            (b"C1", arr(c1)),
        ]))
    }

    #[track_caller]
    fn close(actual: &[f64], expected: &[f64]) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "output arity: {actual:?} vs {expected:?}"
        );
        for (a, e) in actual.iter().zip(expected.iter()) {
            assert!(
                (a - e).abs() < 1e-9,
                "expected {expected:?}, got {actual:?}"
            );
        }
    }

    // -- Table 38: the entries common to every type -------------------------

    /// Catches a loader that accepts a non-function object (an array, a name)
    /// and then fails somewhere less legible — or, worse, treats a stream's
    /// absence of `/FunctionType` as type 0.
    #[test]
    fn non_dictionary_object_is_refused() {
        let graph = TestGraph::default();
        let view = DocumentView::new(&graph, b"", V17);
        let err = PdfFunction::load(&view, &Object::Integer(4)).unwrap_err();
        assert_eq!(err, FunctionError::NotAFunction);
    }

    /// Catches a loader that defaults a missing `/FunctionType`. There is no
    /// default; Table 38 marks it Required.
    #[test]
    fn missing_function_type_is_refused() {
        let err = load_dict(&[(b"Domain", arr(&[0.0, 1.0]))]).unwrap_err();
        assert_eq!(err, FunctionError::MissingFunctionType);
    }

    /// Catches an implementation that invents a type 1. §7.10 defines 0, 2, 3
    /// and 4 only, and the gap at 1 is real — see the module docs.
    #[test]
    fn function_type_one_is_unknown_like_any_other() {
        for t in [1i64, 5, -3] {
            let err = load_dict(&[
                (b"FunctionType", Object::Integer(t)),
                (b"Domain", arr(&[0.0, 1.0])),
            ])
            .unwrap_err();
            assert_eq!(err, FunctionError::UnknownFunctionType(t));
        }
    }

    /// Catches a loader that treats `/Domain` as optional. Every type requires
    /// it; without it *m* is unknown.
    #[test]
    fn missing_domain_is_refused() {
        let err = load_dict(&[(b"FunctionType", Object::Integer(2))]).unwrap_err();
        assert_eq!(
            err,
            FunctionError::MissingEntry {
                key: "Domain",
                function_type: 2
            }
        );
    }

    /// Catches a loader that silently drops a trailing odd element from a
    /// 2 × n array — which would shift every subsequent pair by one and produce
    /// a function that evaluates without complaint and is entirely wrong.
    #[test]
    fn odd_length_domain_is_refused() {
        let err = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[0.0, 1.0, 2.0])),
        ])
        .unwrap_err();
        assert!(matches!(err, FunctionError::BadEntry { key: "Domain", .. }));
    }

    /// Catches clipping into an inverted interval, where the result depends on
    /// which comparison runs first rather than on the file.
    #[test]
    fn inverted_domain_pair_is_refused() {
        let err = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[1.0, 0.0])),
            (b"N", Object::Integer(1)),
        ])
        .unwrap_err();
        assert!(matches!(
            err,
            FunctionError::InvertedInterval {
                key: "Domain",
                index: 0,
                ..
            }
        ));
    }

    /// Catches a `/Range` whose output count contradicts what `/C0` implies —
    /// Table 38's "shall be consistent" rule. A reader that let this through
    /// would hand a 3-vector to a 4-component alternate space.
    #[test]
    fn range_inconsistent_with_c0_is_refused() {
        let err = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[0.0, 1.0])),
            (b"N", Object::Integer(1)),
            (b"C0", arr(&[0.0, 0.0])),
            (b"C1", arr(&[1.0, 1.0])),
            (b"Range", arr(&[0.0, 1.0])),
        ])
        .unwrap_err();
        assert!(matches!(
            err,
            FunctionError::BadArrayLength { key: "Range", .. }
        ));
    }

    // -- Table 38: the two clipping `shall`s --------------------------------

    /// Catches a missing `/Domain` clip at BOTH ends in one test: a tint of
    /// −5 must evaluate as 0 and a tint of +5 as 1, not extrapolate.
    #[test]
    fn inputs_are_clipped_to_domain_at_both_ends() {
        let f = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[0.0, 1.0])),
            (b"N", Object::Integer(1)),
            (b"C0", arr(&[10.0])),
            (b"C1", arr(&[20.0])),
        ])
        .unwrap();
        close(&f.eval(&[-5.0]).unwrap(), &[10.0]);
        close(&f.eval(&[5.0]).unwrap(), &[20.0]);
        // And the interior is untouched.
        close(&f.eval(&[0.5]).unwrap(), &[15.0]);
    }

    /// Catches a missing `/Range` clip at both ends. With `/Range [0.25 0.75]`
    /// the unclipped results would be 0.0 and 1.0.
    #[test]
    fn outputs_are_clipped_to_range_at_both_ends() {
        let f = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[0.0, 1.0])),
            (b"N", Object::Integer(1)),
            (b"C0", arr(&[0.0])),
            (b"C1", arr(&[1.0])),
            (b"Range", arr(&[0.25, 0.75])),
        ])
        .unwrap();
        close(&f.eval(&[0.0]).unwrap(), &[0.25]);
        close(&f.eval(&[1.0]).unwrap(), &[0.75]);
        close(&f.eval(&[0.5]).unwrap(), &[0.5]);
    }

    /// Catches the inverse and more damaging error: clipping to a default
    /// `[0, 1]` when `/Range` is ABSENT. Table 38 — "If this entry is absent,
    /// no clipping shall be done." A shading function whose outputs were
    /// squeezed into the unit interval renders as plausible, wrong colour.
    #[test]
    fn absent_range_means_no_output_clipping() {
        let f = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[0.0, 1.0])),
            (b"N", Object::Integer(1)),
            (b"C0", arr(&[-3.0])),
            (b"C1", arr(&[7.0])),
        ])
        .unwrap();
        assert!(f.range().is_none());
        close(&f.eval(&[0.0]).unwrap(), &[-3.0]);
        close(&f.eval(&[1.0]).unwrap(), &[7.0]);
    }

    /// Catches a `NaN` input being silently clamped to `Domain_0`.
    /// `f64::max` returns the non-`NaN` operand, so a naive clamp fabricates a
    /// boundary value that is indistinguishable from a real one downstream.
    #[test]
    fn non_finite_input_is_refused_not_clamped() {
        let f = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[0.0, 1.0])),
            (b"N", Object::Integer(1)),
        ])
        .unwrap();
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let err = f.eval(&[bad]).unwrap_err();
            assert!(matches!(
                err,
                FunctionError::NonFiniteInput { index: 0, .. }
            ));
        }
    }

    /// Catches a caller passing the wrong tint count being papered over by
    /// padding or truncation instead of refused.
    #[test]
    fn input_arity_mismatch_is_refused() {
        let f = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[0.0, 1.0])),
            (b"N", Object::Integer(1)),
        ])
        .unwrap();
        assert_eq!(
            f.eval(&[]).unwrap_err(),
            FunctionError::InputArity {
                expected: 1,
                got: 0
            }
        );
        assert_eq!(
            f.eval(&[0.1, 0.2]).unwrap_err(),
            FunctionError::InputArity {
                expected: 1,
                got: 2
            }
        );
    }

    // -- Type 0 (sampled), §7.10.2 ------------------------------------------

    /// The 8-bit base case: linear interpolation between two table entries.
    /// Catches an evaluator that snaps to the nearest sample instead of
    /// interpolating ("Interpolation shall be used", §7.10.2).
    #[test]
    fn type0_interpolates_between_eight_bit_samples() {
        // Size 2, samples 0 and 255, Decode defaults to Range [0 1].
        let f = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&[0.0, 1.0])),
                (b"Range", arr(&[0.0, 1.0])),
                (b"Size", Object::Array(vec![Object::Integer(2)])),
                (b"BitsPerSample", Object::Integer(8)),
            ],
            &[0x00, 0xFF],
        )
        .unwrap();
        assert_eq!(f.function_type(), FunctionType::Sampled);
        assert_eq!(f.inputs(), 1);
        assert_eq!(f.outputs(), 1);
        close(&f.eval(&[0.0]).unwrap(), &[0.0]);
        close(&f.eval(&[0.25]).unwrap(), &[0.25]);
        close(&f.eval(&[1.0]).unwrap(), &[1.0]);
    }

    /// **4-bit samples.** Catches the commonest bit-unpacking defect: reading a
    /// whole byte per sample, or taking the LOW nibble first. §7.10.2 packs two
    /// 4-bit samples per byte, high-order bits first, so `0x05` is samples 0 and
    /// 5 — not 5 and 0, and not the single value 5.
    #[test]
    fn type0_four_bit_samples_pack_two_per_byte_high_nibble_first() {
        // Four samples 0, 5, 10, 15 -> bytes 0x05, 0xAF.
        let f = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&[0.0, 1.0])),
                (b"Range", arr(&[0.0, 15.0])),
                (b"Size", Object::Array(vec![Object::Integer(4)])),
                (b"BitsPerSample", Object::Integer(4)),
                // Decode maps the raw 0..15 straight through, so the assertions
                // below are on the RAW sample values and a mis-unpack is
                // unmistakable.
                (b"Decode", arr(&[0.0, 15.0])),
            ],
            &[0x05, 0xAF],
        )
        .unwrap();
        close(&f.eval(&[0.0]).unwrap(), &[0.0]); // sample 0, high nibble of 0x05
        close(&f.eval(&[1.0 / 3.0]).unwrap(), &[5.0]); // sample 1, low nibble
        close(&f.eval(&[2.0 / 3.0]).unwrap(), &[10.0]); // sample 2, high of 0xAF
        close(&f.eval(&[1.0]).unwrap(), &[15.0]); // sample 3, low of 0xAF
        // And halfway between samples 1 and 2 is 7.5, proving interpolation
        // happens on unpacked values rather than on bytes.
        close(&f.eval(&[0.5]).unwrap(), &[7.5]);
    }

    /// **12-bit samples.** The width where a byte-oriented reader breaks
    /// outright: sample 1 straddles bytes 1 and 2. Catches an unpacker that
    /// assumes byte alignment, and one that pads each sample to 16 bits.
    #[test]
    fn type0_twelve_bit_samples_straddle_byte_boundaries() {
        // Samples 0x000, 0x800, 0xFFF as a continuous bit stream:
        //   0000 0000 0000 | 1000 0000 0000 | 1111 1111 1111 | (4 pad bits)
        // = 0x00 0x08 0x00 0xFF 0xF0
        let f = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&[0.0, 1.0])),
                (b"Range", arr(&[0.0, 4095.0])),
                (b"Size", Object::Array(vec![Object::Integer(3)])),
                (b"BitsPerSample", Object::Integer(12)),
                (b"Decode", arr(&[0.0, 4095.0])),
            ],
            &[0x00, 0x08, 0x00, 0xFF, 0xF0],
        )
        .unwrap();
        close(&f.eval(&[0.0]).unwrap(), &[0.0]);
        close(&f.eval(&[0.5]).unwrap(), &[2048.0]);
        close(&f.eval(&[1.0]).unwrap(), &[4095.0]);
    }

    /// 16-bit samples, where the fast byte-aligned path runs. Catches a
    /// little-endian read (§7.10.2 is high-order-bit-first, so `0x12 0x34` is
    /// 0x1234, not 0x3412).
    #[test]
    fn type0_sixteen_bit_samples_are_big_endian() {
        let f = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&[0.0, 1.0])),
                (b"Range", arr(&[0.0, 65535.0])),
                (b"Size", Object::Array(vec![Object::Integer(2)])),
                (b"BitsPerSample", Object::Integer(16)),
                (b"Decode", arr(&[0.0, 65535.0])),
            ],
            &[0x12, 0x34, 0xFF, 0xFF],
        )
        .unwrap();
        close(&f.eval(&[0.0]).unwrap(), &[f64::from(0x1234u32)]);
    }

    /// **Multi-input interpolation.** Catches two distinct defects at once:
    /// nearest-neighbour instead of bilinear blending, and — via the asymmetric
    /// corner values — a transposed sample order. §7.10.2 stores the FIRST
    /// dimension fastest, which is column-major and the opposite of C's default.
    #[test]
    fn type0_two_input_bilinear_with_first_dimension_fastest() {
        // f(0,0)=0  f(1,0)=51  f(0,1)=102  f(1,1)=204, stored in the spec's
        // order: f(0,0), f(1,0), f(0,1), f(1,1).
        let f = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&[0.0, 1.0, 0.0, 1.0])),
                (b"Range", arr(&[0.0, 1.0])),
                (
                    b"Size",
                    Object::Array(vec![Object::Integer(2), Object::Integer(2)]),
                ),
                (b"BitsPerSample", Object::Integer(8)),
            ],
            &[0, 51, 102, 204],
        )
        .unwrap();
        assert_eq!(f.inputs(), 2);
        // The two off-diagonal corners are what a transposition swaps.
        close(&f.eval(&[1.0, 0.0]).unwrap(), &[51.0 / 255.0]);
        close(&f.eval(&[0.0, 1.0]).unwrap(), &[102.0 / 255.0]);
        close(&f.eval(&[1.0, 1.0]).unwrap(), &[204.0 / 255.0]);
        // The centre is the mean of all four corners only if all four are
        // blended — a nearest-neighbour evaluator returns one of them.
        close(
            &f.eval(&[0.5, 0.5]).unwrap(),
            &[(0.0 + 51.0 + 102.0 + 204.0) / 4.0 / 255.0],
        );
    }

    /// Multi-OUTPUT layout: §7.10.2 stores one table entry's *n* samples
    /// adjacently, "in the same order as Range". Catches an evaluator that
    /// strides by table entry per output plane instead.
    #[test]
    fn type0_multi_output_samples_are_adjacent_per_entry() {
        // Two entries, three outputs each: (10,20,30) then (40,50,60).
        let f = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&[0.0, 1.0])),
                (b"Range", arr(&[0.0, 255.0, 0.0, 255.0, 0.0, 255.0])),
                (b"Size", Object::Array(vec![Object::Integer(2)])),
                (b"BitsPerSample", Object::Integer(8)),
                (b"Decode", arr(&[0.0, 255.0, 0.0, 255.0, 0.0, 255.0])),
            ],
            &[10, 20, 30, 40, 50, 60],
        )
        .unwrap();
        assert_eq!(f.outputs(), 3);
        close(&f.eval(&[0.0]).unwrap(), &[10.0, 20.0, 30.0]);
        close(&f.eval(&[1.0]).unwrap(), &[40.0, 50.0, 60.0]);
    }

    /// Catches an `/Encode` default of `[0, 1]` instead of `[0, Size − 1]`,
    /// which would confine every lookup to the first two samples of the table.
    #[test]
    fn type0_encode_defaults_to_zero_through_size_minus_one() {
        let f = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&[0.0, 1.0])),
                (b"Range", arr(&[0.0, 3.0])),
                (b"Size", Object::Array(vec![Object::Integer(4)])),
                (b"BitsPerSample", Object::Integer(8)),
                (b"Decode", arr(&[0.0, 255.0])),
            ],
            &[0, 1, 2, 3],
        )
        .unwrap();
        // x = 1.0 must reach the LAST sample, not the second.
        close(&f.eval(&[1.0]).unwrap(), &[3.0]);
    }

    /// An explicit `/Encode` that reverses the table. Catches an evaluator that
    /// ignores `/Encode` entirely (which the default case above cannot detect,
    /// because the default is what an ignoring evaluator accidentally does for
    /// a `[0, 1]` domain).
    #[test]
    fn type0_explicit_encode_can_reverse_the_table() {
        let f = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&[0.0, 1.0])),
                (b"Range", arr(&[0.0, 255.0])),
                (b"Size", Object::Array(vec![Object::Integer(4)])),
                (b"BitsPerSample", Object::Integer(8)),
                (b"Encode", arr(&[3.0, 0.0])),
                (b"Decode", arr(&[0.0, 255.0])),
            ],
            &[0, 1, 2, 3],
        )
        .unwrap();
        close(&f.eval(&[0.0]).unwrap(), &[3.0]);
        close(&f.eval(&[1.0]).unwrap(), &[0.0]);
    }

    /// Catches a `/Decode` default of `[0, 1]` rather than "same as `/Range`"
    /// (Table 39). With `/Range [0 100]` and no `/Decode`, a full-scale sample
    /// must decode to 100.
    #[test]
    fn type0_decode_defaults_to_range() {
        let f = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&[0.0, 1.0])),
                (b"Range", arr(&[0.0, 100.0])),
                (b"Size", Object::Array(vec![Object::Integer(2)])),
                (b"BitsPerSample", Object::Integer(8)),
            ],
            &[0x00, 0xFF],
        )
        .unwrap();
        close(&f.eval(&[1.0]).unwrap(), &[100.0]);
    }

    /// §7.10.2: "The `Size` value for an input dimension can be 1, in which case
    /// all input values in that dimension shall be mapped to the single allowed
    /// value." Catches a division by `Size − 1 = 0` in the encode step, which
    /// yields `NaN` or an out-of-range index.
    #[test]
    fn type0_size_one_axis_maps_every_input_to_the_single_sample() {
        let f = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&[0.0, 1.0])),
                (b"Range", arr(&[0.0, 255.0])),
                (b"Size", Object::Array(vec![Object::Integer(1)])),
                (b"BitsPerSample", Object::Integer(8)),
                (b"Decode", arr(&[0.0, 255.0])),
            ],
            &[77],
        )
        .unwrap();
        for x in [0.0, 0.5, 1.0] {
            close(&f.eval(&[x]).unwrap(), &[77.0]);
        }
    }

    /// §7.10.2 / §7.3.8.2: "The stream data shall be long enough to contain the
    /// entire sample array." Catches an evaluator that reads past the end and
    /// treats missing bytes as zero — which paints black where the file is
    /// truncated.
    #[test]
    fn type0_short_sample_stream_is_refused() {
        let err = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&[0.0, 1.0])),
                (b"Range", arr(&[0.0, 1.0])),
                (b"Size", Object::Array(vec![Object::Integer(4)])),
                (b"BitsPerSample", Object::Integer(8)),
            ],
            &[0x00, 0xFF], // 2 bytes where 4 are required
        )
        .unwrap_err();
        assert_eq!(err, FunctionError::SampleDataTooShort { need: 4, have: 2 });
    }

    /// Table 39's `/BitsPerSample` set is closed. Catches an implementation that
    /// accepts any width its bit reader happens to handle.
    #[test]
    fn type0_invalid_bits_per_sample_is_refused() {
        for bad in [0i64, 3, 5, 6, 10, 64] {
            let err = load_stream(
                &[
                    (b"FunctionType", Object::Integer(0)),
                    (b"Domain", arr(&[0.0, 1.0])),
                    (b"Range", arr(&[0.0, 1.0])),
                    (b"Size", Object::Array(vec![Object::Integer(2)])),
                    (b"BitsPerSample", Object::Integer(bad)),
                ],
                &[0; 64],
            )
            .unwrap_err();
            assert_eq!(err, FunctionError::BadBitsPerSample(bad));
        }
        // And every legal width loads.
        for good in VALID_BITS_PER_SAMPLE {
            load_stream(
                &[
                    (b"FunctionType", Object::Integer(0)),
                    (b"Domain", arr(&[0.0, 1.0])),
                    (b"Range", arr(&[0.0, 1.0])),
                    (b"Size", Object::Array(vec![Object::Integer(2)])),
                    (b"BitsPerSample", Object::Integer(i64::from(good))),
                ],
                &[0; 64],
            )
            .unwrap();
        }
    }

    /// `/Range` is Required for type 0 — it is the only source of *n*. Catches a
    /// loader that infers *n* as 1 when it is absent.
    #[test]
    fn type0_missing_range_is_refused() {
        let err = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&[0.0, 1.0])),
                (b"Size", Object::Array(vec![Object::Integer(2)])),
                (b"BitsPerSample", Object::Integer(8)),
            ],
            &[0x00, 0xFF],
        )
        .unwrap_err();
        assert_eq!(
            err,
            FunctionError::MissingEntry {
                key: "Range",
                function_type: 0
            }
        );
    }

    /// A type 0 written as a bare dictionary has nowhere to keep its samples.
    /// Catches a loader that accepts it and then evaluates against an empty
    /// table.
    #[test]
    fn type0_as_a_bare_dictionary_is_refused() {
        let err = load_dict(&[
            (b"FunctionType", Object::Integer(0)),
            (b"Domain", arr(&[0.0, 1.0])),
            (b"Range", arr(&[0.0, 1.0])),
            (b"Size", Object::Array(vec![Object::Integer(2)])),
            (b"BitsPerSample", Object::Integer(8)),
        ])
        .unwrap_err();
        assert_eq!(err, FunctionError::NotAStream { function_type: 0 });
    }

    /// Table 39: "m positive integers". Catches a zero or fractional `/Size`
    /// being rounded into something workable.
    #[test]
    fn type0_non_positive_size_is_refused() {
        for bad in [0.0, -2.0, 2.5] {
            let err = load_stream(
                &[
                    (b"FunctionType", Object::Integer(0)),
                    (b"Domain", arr(&[0.0, 1.0])),
                    (b"Range", arr(&[0.0, 1.0])),
                    (b"Size", arr(&[bad])),
                    (b"BitsPerSample", Object::Integer(8)),
                ],
                &[0; 16],
            )
            .unwrap_err();
            assert!(matches!(err, FunctionError::BadEntry { key: "Size", .. }));
        }
    }

    /// Catches a `/Size` whose element count disagrees with `/Domain`'s *m* —
    /// Table 38's consistency rule, in the place it bites hardest (the stride
    /// computation would silently use the wrong dimensionality).
    #[test]
    fn type0_size_length_must_match_domain() {
        let err = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&[0.0, 1.0, 0.0, 1.0])),
                (b"Range", arr(&[0.0, 1.0])),
                (b"Size", Object::Array(vec![Object::Integer(2)])),
                (b"BitsPerSample", Object::Integer(8)),
            ],
            &[0; 16],
        )
        .unwrap_err();
        assert_eq!(
            err,
            FunctionError::BadArrayLength {
                key: "Size",
                expected: 2,
                got: 1
            }
        );
    }

    /// §7.10.2 permits an implementation limit on dimensionality; this pins
    /// pdfcer's. Catches the limit silently not being enforced, which would let a
    /// crafted `/Size` of 30 dimensions ask for 2^30 corner reads per
    /// evaluation.
    #[test]
    fn type0_excess_input_dimensions_are_refused() {
        let m = MAX_SAMPLED_INPUTS + 1;
        let domain: Vec<f64> = (0..m).flat_map(|_| [0.0, 1.0]).collect();
        let size = Object::Array((0..m).map(|_| Object::Integer(2)).collect());
        let err = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&domain)),
                (b"Range", arr(&[0.0, 1.0])),
                (b"Size", size),
                (b"BitsPerSample", Object::Integer(8)),
            ],
            &[0; 4096],
        )
        .unwrap_err();
        assert_eq!(
            err,
            FunctionError::TooManyInputs {
                got: m,
                limit: MAX_SAMPLED_INPUTS
            }
        );
    }

    /// `/Order 3` on a table big enough for a cubic spline is a real fidelity
    /// downgrade and must be disclosed (project rule 4). Catches the disclosure
    /// being dropped, which would let the operator assume a fidelity pdfcer does
    /// not provide.
    #[test]
    fn type0_order_three_on_a_large_table_is_disclosed_as_downgraded() {
        let f = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&[0.0, 1.0])),
                (b"Range", arr(&[0.0, 1.0])),
                (b"Size", Object::Array(vec![Object::Integer(4)])),
                (b"BitsPerSample", Object::Integer(8)),
                (b"Order", Object::Integer(3)),
            ],
            &[0, 1, 2, 3],
        )
        .unwrap();
        assert!(f.cubic_downgraded());
    }

    /// The converse: §7.10.2 says `/Order 3` "shall be ignored" when `Size` is
    /// below 4, so ignoring it there is conformance, not a downgrade. Catches a
    /// disclosure that cries wolf on every small table.
    #[test]
    fn type0_order_three_below_size_four_is_not_a_downgrade() {
        let f = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&[0.0, 1.0])),
                (b"Range", arr(&[0.0, 1.0])),
                (b"Size", Object::Array(vec![Object::Integer(3)])),
                (b"BitsPerSample", Object::Integer(8)),
                (b"Order", Object::Integer(3)),
            ],
            &[0, 1, 2],
        )
        .unwrap();
        assert!(!f.cubic_downgraded());
    }

    /// Table 39 allows `/Order` 1 and 3 only. Catches a loader that accepts an
    /// arbitrary integer and quietly treats it as linear.
    #[test]
    fn type0_unknown_order_is_refused() {
        let err = load_stream(
            &[
                (b"FunctionType", Object::Integer(0)),
                (b"Domain", arr(&[0.0, 1.0])),
                (b"Range", arr(&[0.0, 1.0])),
                (b"Size", Object::Array(vec![Object::Integer(4)])),
                (b"BitsPerSample", Object::Integer(8)),
                (b"Order", Object::Integer(2)),
            ],
            &[0, 1, 2, 3],
        )
        .unwrap_err();
        assert!(matches!(err, FunctionError::BadEntry { key: "Order", .. }));
    }

    /// Direct coverage of the bit reader at every legal width, independent of
    /// the surrounding function machinery. Catches an off-by-one in the shift
    /// arithmetic that a whole-function test could mask through interpolation.
    #[test]
    fn read_bits_is_msb_first_at_every_width() {
        let data = [0b1010_0110u8, 0b1100_0011, 0xFF, 0x01];
        assert_eq!(read_bits(&data, 0, 1), Some(1));
        assert_eq!(read_bits(&data, 1, 1), Some(0));
        assert_eq!(read_bits(&data, 0, 2), Some(0b10));
        assert_eq!(read_bits(&data, 2, 2), Some(0b10));
        assert_eq!(read_bits(&data, 0, 4), Some(0b1010));
        assert_eq!(read_bits(&data, 4, 4), Some(0b0110));
        assert_eq!(read_bits(&data, 0, 8), Some(0b1010_0110));
        // A 12-bit read spanning bytes 0 and 1.
        assert_eq!(read_bits(&data, 0, 12), Some(0b1010_0110_1100));
        assert_eq!(read_bits(&data, 12, 12), Some(0b0011_1111_1111));
        assert_eq!(read_bits(&data, 0, 16), Some(0b1010_0110_1100_0011));
        assert_eq!(read_bits(&data, 0, 24), Some(0x00A6_C3FF));
        assert_eq!(read_bits(&data, 0, 32), Some(0xA6C3_FF01));
        // Past the end is None, never a zero-filled value.
        assert_eq!(read_bits(&data, 32, 1), None);
        assert_eq!(read_bits(&data, 24, 16), None);
    }

    // -- Type 2 (exponential interpolation), §7.10.3 ------------------------

    /// The dominant real-world tint transform: `/N 1`, a straight ramp from
    /// paper to full-strength ink. Catches an evaluator that applies the
    /// exponent to the whole expression rather than to `x`.
    #[test]
    fn type2_with_n_one_is_a_linear_ramp() {
        let f = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[0.0, 1.0])),
            (b"N", Object::Integer(1)),
            (b"C0", arr(&[0.0, 0.0, 0.0, 0.0])),
            (b"C1", arr(&[0.0, 1.0, 0.6, 0.1])),
        ])
        .unwrap();
        assert_eq!(f.function_type(), FunctionType::Exponential);
        assert_eq!(f.outputs(), 4);
        close(&f.eval(&[0.0]).unwrap(), &[0.0, 0.0, 0.0, 0.0]);
        close(&f.eval(&[0.5]).unwrap(), &[0.0, 0.5, 0.3, 0.05]);
        close(&f.eval(&[1.0]).unwrap(), &[0.0, 1.0, 0.6, 0.1]);
    }

    /// A non-unit exponent. Catches `y = (C0 + x(C1 − C0))^N` — the plausible
    /// misreading of the formula, which agrees with the correct one at N = 1 and
    /// so slips past a linear-only test.
    #[test]
    fn type2_applies_the_exponent_to_x_not_to_the_result() {
        let f = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[0.0, 1.0])),
            (b"N", Object::Integer(2)),
            (b"C0", arr(&[1.0])),
            (b"C1", arr(&[5.0])),
        ])
        .unwrap();
        // 1 + 0.5^2 * (5 − 1) = 2.0. The misreading gives (1 + 0.5*4)^2 = 9.
        close(&f.eval(&[0.5]).unwrap(), &[2.0]);
    }

    /// Table 40's defaults are the SCALARS `[0.0]` and `[1.0]`, so a type 2 with
    /// neither entry is a 1-output identity-ish function. Catches a loader that
    /// invents an *n*-wide default from `/Range` or from a caller's expectation.
    #[test]
    fn type2_c0_and_c1_default_to_one_element_arrays() {
        let f = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[0.0, 1.0])),
            (b"N", Object::Integer(1)),
        ])
        .unwrap();
        assert_eq!(f.outputs(), 1);
        close(&f.eval(&[0.3]).unwrap(), &[0.3]);
    }

    /// §7.10.3 restricts type 2 to one input. Catches a loader that accepts a
    /// 2-input `/Domain` and then reads only the first value.
    #[test]
    fn type2_multi_input_domain_is_refused() {
        let err = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[0.0, 1.0, 0.0, 1.0])),
            (b"N", Object::Integer(1)),
        ])
        .unwrap_err();
        assert_eq!(
            err,
            FunctionError::NotOneInput {
                function_type: 2,
                got: 2
            }
        );
    }

    /// §7.10.3: "if N is negative, no value of x shall be zero". Catches the
    /// resulting infinity being produced and then clamped into a `/Range`
    /// boundary, which looks like a legitimate saturated colour.
    #[test]
    fn type2_negative_exponent_with_zero_in_domain_is_refused() {
        let err = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[0.0, 1.0])),
            (b"N", Object::Real(-1.0)),
        ])
        .unwrap_err();
        assert!(matches!(
            err,
            FunctionError::DomainIncompatibleWithExponent { .. }
        ));
        // The same exponent over a domain that excludes zero is fine.
        let f = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[1.0, 2.0])),
            (b"N", Object::Real(-1.0)),
            (b"C0", arr(&[0.0])),
            (b"C1", arr(&[1.0])),
        ])
        .unwrap();
        close(&f.eval(&[2.0]).unwrap(), &[0.5]);
    }

    /// §7.10.3: "if N is not an integer, all values of x shall be
    /// non-negative". Catches a `NaN` from a negative base under a fractional
    /// exponent.
    #[test]
    fn type2_fractional_exponent_with_negative_domain_is_refused() {
        let err = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[-1.0, 1.0])),
            (b"N", Object::Real(0.5)),
        ])
        .unwrap_err();
        assert!(matches!(
            err,
            FunctionError::DomainIncompatibleWithExponent { .. }
        ));
    }

    /// Catches `/C0` and `/C1` of different lengths being zipped to the shorter
    /// one, which silently drops output components.
    #[test]
    fn type2_mismatched_c0_c1_lengths_are_refused() {
        let err = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[0.0, 1.0])),
            (b"N", Object::Integer(1)),
            (b"C0", arr(&[0.0, 0.0, 0.0])),
            (b"C1", arr(&[1.0, 1.0])),
        ])
        .unwrap_err();
        assert_eq!(
            err,
            FunctionError::BadArrayLength {
                key: "C1",
                expected: 3,
                got: 2
            }
        );
    }

    /// `/N` is Required (Table 40). Catches a default of 1 being invented.
    #[test]
    fn type2_missing_exponent_is_refused() {
        let err = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[0.0, 1.0])),
        ])
        .unwrap_err();
        assert_eq!(
            err,
            FunctionError::MissingEntry {
                key: "N",
                function_type: 2
            }
        );
    }

    // -- Type 3 (stitching), §7.10.4 ----------------------------------------

    /// Catches a subdomain-selection off-by-one and a missing `/Encode`
    /// rescale in one pass: two ramps stitched at 0.5, each encoded onto
    /// `[0, 1]`, so the composite runs 0→10 then 100→200.
    #[test]
    fn type3_selects_and_rescales_each_subdomain() {
        let f = load_dict(&[
            (b"FunctionType", Object::Integer(3)),
            (b"Domain", arr(&[0.0, 1.0])),
            (
                b"Functions",
                Object::Array(vec![
                    ramp(&[0.0, 1.0], &[0.0], &[10.0]),
                    ramp(&[0.0, 1.0], &[100.0], &[200.0]),
                ]),
            ),
            (b"Bounds", arr(&[0.5])),
            (b"Encode", arr(&[0.0, 1.0, 0.0, 1.0])),
        ])
        .unwrap();
        assert_eq!(f.function_type(), FunctionType::Stitching);
        assert_eq!(f.outputs(), 1);
        close(&f.eval(&[0.0]).unwrap(), &[0.0]);
        close(&f.eval(&[0.25]).unwrap(), &[5.0]); // halfway through [0, 0.5)
        close(&f.eval(&[0.75]).unwrap(), &[150.0]); // halfway through [0.5, 1]
        close(&f.eval(&[1.0]).unwrap(), &[200.0]);
    }

    /// **Exactly on a bound.** §7.10.4's intervals are closed on the left and
    /// open on the right, so `x == Bounds_0` belongs to the LATER
    /// sub-function. Catches the off-by-one that puts it in the earlier one —
    /// invisible everywhere except at the seam, which is exactly where a
    /// gradient shows a visible step.
    #[test]
    fn type3_exactly_on_a_bound_selects_the_later_subfunction() {
        let f = load_dict(&[
            (b"FunctionType", Object::Integer(3)),
            (b"Domain", arr(&[0.0, 1.0])),
            (
                b"Functions",
                Object::Array(vec![
                    // Two constants, so only the SELECTION is under test.
                    ramp(&[0.0, 1.0], &[10.0], &[10.0]),
                    ramp(&[0.0, 1.0], &[20.0], &[20.0]),
                ]),
            ),
            (b"Bounds", arr(&[0.5])),
            (b"Encode", arr(&[0.0, 1.0, 0.0, 1.0])),
        ])
        .unwrap();
        close(&f.eval(&[0.499_999]).unwrap(), &[10.0]);
        close(&f.eval(&[0.5]).unwrap(), &[20.0]); // the bound itself
        close(&f.eval(&[0.500_001]).unwrap(), &[20.0]);
        // And the last interval is closed on the RIGHT as well.
        close(&f.eval(&[1.0]).unwrap(), &[20.0]);
    }

    /// The three-interval case, so the middle sub-function's
    /// `Bounds_(i−1) ≤ x < Bounds_i` lookup is exercised rather than only the
    /// first-and-last special cases.
    #[test]
    fn type3_middle_interval_uses_both_neighbouring_bounds() {
        let f = load_dict(&[
            (b"FunctionType", Object::Integer(3)),
            (b"Domain", arr(&[0.0, 3.0])),
            (
                b"Functions",
                Object::Array(vec![
                    ramp(&[0.0, 1.0], &[0.0], &[1.0]),
                    ramp(&[0.0, 1.0], &[10.0], &[11.0]),
                    ramp(&[0.0, 1.0], &[20.0], &[21.0]),
                ]),
            ),
            (b"Bounds", arr(&[1.0, 2.0])),
            (b"Encode", arr(&[0.0, 1.0, 0.0, 1.0, 0.0, 1.0])),
        ])
        .unwrap();
        close(&f.eval(&[0.5]).unwrap(), &[0.5]);
        close(&f.eval(&[1.5]).unwrap(), &[10.5]);
        close(&f.eval(&[2.5]).unwrap(), &[20.5]);
    }

    /// §7.10.4's `k = 1` domain-reversal idiom: one sub-function, an empty
    /// `/Bounds`, and `/Encode [1 0]` gives `g(x) = f(1 − x)`. Catches a loader
    /// that rejects an empty `/Bounds`, and an evaluator that ignores `/Encode`
    /// when there is only one interval.
    #[test]
    fn type3_single_subfunction_with_reversed_encode_inverts_the_domain() {
        let f = load_dict(&[
            (b"FunctionType", Object::Integer(3)),
            (b"Domain", arr(&[0.0, 1.0])),
            (
                b"Functions",
                Object::Array(vec![ramp(&[0.0, 1.0], &[0.0], &[1.0])]),
            ),
            (b"Bounds", Object::Array(vec![])),
            (b"Encode", arr(&[1.0, 0.0])),
        ])
        .unwrap();
        close(&f.eval(&[0.25]).unwrap(), &[0.75]);
        close(&f.eval(&[0.0]).unwrap(), &[1.0]);
        close(&f.eval(&[1.0]).unwrap(), &[0.0]);
    }

    /// §7.10.4: "If the last bound, `Bounds_k−2`, is equal to `Domain_1`, then
    /// x′ shall be defined to be `Encode_2i`." That collapses the last
    /// interval, and a naive `Interpolate` divides by zero there. Catches the
    /// resulting `NaN`.
    #[test]
    fn type3_last_bound_equal_to_domain_high_uses_encode_low() {
        let f = load_dict(&[
            (b"FunctionType", Object::Integer(3)),
            (b"Domain", arr(&[0.0, 1.0])),
            (
                b"Functions",
                Object::Array(vec![
                    ramp(&[0.0, 10.0], &[0.0], &[1.0]),
                    // y = C0 + x·(C1 − C0) = x over [0, 10], i.e. the identity,
                    // so the value asserted below IS the x' the spec's rule
                    // produces rather than something derived from it.
                    ramp(&[0.0, 10.0], &[0.0], &[1.0]),
                ]),
            ),
            (b"Bounds", arr(&[1.0])), // == Domain_1
            (b"Encode", arr(&[0.0, 1.0, 5.0, 9.0])),
        ])
        .unwrap();
        // x = 1.0 selects sub-function 1, whose interval is [1.0, 1.0]; the
        // spec pins x' at Encode_2 = 5.
        close(&f.eval(&[1.0]).unwrap(), &[5.0]);
    }

    /// §7.10.4: "`Bounds` elements shall be in order of increasing value."
    /// Catches a decreasing pair producing an interval that can never be
    /// selected, silently disabling a sub-function.
    #[test]
    fn type3_decreasing_bounds_are_refused() {
        let err = load_dict(&[
            (b"FunctionType", Object::Integer(3)),
            (b"Domain", arr(&[0.0, 1.0])),
            (
                b"Functions",
                Object::Array(vec![
                    ramp(&[0.0, 1.0], &[0.0], &[1.0]),
                    ramp(&[0.0, 1.0], &[0.0], &[1.0]),
                    ramp(&[0.0, 1.0], &[0.0], &[1.0]),
                ]),
            ),
            (b"Bounds", arr(&[0.7, 0.3])),
            (b"Encode", arr(&[0.0, 1.0, 0.0, 1.0, 0.0, 1.0])),
        ])
        .unwrap_err();
        assert!(matches!(err, FunctionError::BadBounds { .. }));
    }

    /// §7.10.4: "each value shall be within the domain defined by `Domain`".
    /// Catches a bound outside the domain, which makes one sub-function
    /// unreachable for every legal input.
    #[test]
    fn type3_bound_outside_domain_is_refused() {
        let err = load_dict(&[
            (b"FunctionType", Object::Integer(3)),
            (b"Domain", arr(&[0.0, 1.0])),
            (
                b"Functions",
                Object::Array(vec![
                    ramp(&[0.0, 1.0], &[0.0], &[1.0]),
                    ramp(&[0.0, 1.0], &[0.0], &[1.0]),
                ]),
            ),
            (b"Bounds", arr(&[1.5])),
            (b"Encode", arr(&[0.0, 1.0, 0.0, 1.0])),
        ])
        .unwrap_err();
        assert!(matches!(err, FunctionError::BadBounds { .. }));
    }

    /// §7.10.4: `/Bounds` holds exactly *k* − 1 numbers. Catches a length
    /// mismatch being absorbed by a `zip`, which would leave the last
    /// sub-function permanently selected or permanently dead.
    #[test]
    fn type3_bounds_length_must_be_k_minus_one() {
        let err = load_dict(&[
            (b"FunctionType", Object::Integer(3)),
            (b"Domain", arr(&[0.0, 1.0])),
            (
                b"Functions",
                Object::Array(vec![
                    ramp(&[0.0, 1.0], &[0.0], &[1.0]),
                    ramp(&[0.0, 1.0], &[0.0], &[1.0]),
                ]),
            ),
            (b"Bounds", arr(&[0.3, 0.6])), // 2 bounds for 2 functions
            (b"Encode", arr(&[0.0, 1.0, 0.0, 1.0])),
        ])
        .unwrap_err();
        assert_eq!(
            err,
            FunctionError::BadArrayLength {
                key: "Bounds",
                expected: 1,
                got: 2
            }
        );
    }

    /// §7.10.4: "The output dimensionality of all functions shall be the same."
    /// Catches a mismatch reaching evaluation, where the output vector's length
    /// would then depend on which subdomain the input fell in.
    #[test]
    fn type3_subfunctions_with_different_output_counts_are_refused() {
        let err = load_dict(&[
            (b"FunctionType", Object::Integer(3)),
            (b"Domain", arr(&[0.0, 1.0])),
            (
                b"Functions",
                Object::Array(vec![
                    ramp(&[0.0, 1.0], &[0.0, 0.0], &[1.0, 1.0]),
                    ramp(&[0.0, 1.0], &[0.0], &[1.0]),
                ]),
            ),
            (b"Bounds", arr(&[0.5])),
            (b"Encode", arr(&[0.0, 1.0, 0.0, 1.0])),
        ])
        .unwrap_err();
        assert_eq!(
            err,
            FunctionError::SubFunctionArity {
                index: 1,
                expected: 2,
                got: 1
            }
        );
    }

    /// Catches an empty `/Functions` loading successfully and then panicking or
    /// returning an empty output vector at evaluation.
    #[test]
    fn type3_empty_functions_array_is_refused() {
        let err = load_dict(&[
            (b"FunctionType", Object::Integer(3)),
            (b"Domain", arr(&[0.0, 1.0])),
            (b"Functions", Object::Array(vec![])),
            (b"Bounds", Object::Array(vec![])),
            (b"Encode", Object::Array(vec![])),
        ])
        .unwrap_err();
        assert_eq!(err, FunctionError::NoSubFunctions);
    }

    /// §7.10.4: the sub-functions are "k 1-input functions". Catches a 2-input
    /// sub-function being fed a 1-element slice at evaluation time.
    #[test]
    fn type3_multi_input_subfunction_is_refused() {
        let two_in = Object::Dict(dict(&[
            (b"FunctionType", Object::Integer(4)),
            (b"Domain", arr(&[0.0, 1.0, 0.0, 1.0])),
            (b"Range", arr(&[0.0, 1.0])),
        ]));
        let err = load_dict(&[
            (b"FunctionType", Object::Integer(3)),
            (b"Domain", arr(&[0.0, 1.0])),
            (b"Functions", Object::Array(vec![two_in])),
            (b"Bounds", Object::Array(vec![])),
            (b"Encode", arr(&[0.0, 1.0])),
        ])
        .unwrap_err();
        // A type 4 written as a dictionary is refused before its arity is
        // reached, which is itself the right answer here.
        assert_eq!(err, FunctionError::NotAStream { function_type: 4 });
    }

    /// A `/Functions` array referring back to its own object is legal syntax and
    /// would recurse forever. Catches a missing depth guard —
    /// `ARCHITECTURE.md` §10's rule for every recursive structure walker.
    #[test]
    fn type3_reference_cycle_through_functions_is_depth_limited() {
        let self_ref = ObjId::new(1, 0);
        let mut objects = BTreeMap::new();
        objects.insert(
            self_ref,
            Object::Dict(dict(&[
                (b"FunctionType", Object::Integer(3)),
                (b"Domain", arr(&[0.0, 1.0])),
                (
                    b"Functions",
                    Object::Array(vec![Object::Reference(self_ref)]),
                ),
                (b"Bounds", Object::Array(vec![])),
                (b"Encode", arr(&[0.0, 1.0])),
            ])),
        );
        let graph = TestGraph {
            objects,
            trailer: Dict::new(),
        };
        let view = DocumentView::new(&graph, b"", V17);
        let err = PdfFunction::load(&view, &Object::Reference(self_ref)).unwrap_err();
        assert_eq!(
            err,
            FunctionError::NestingTooDeep {
                limit: MAX_FUNCTION_DEPTH
            }
        );
    }

    /// A nested type 0 with `/Order 3` must surface through the enclosing type 3
    /// — the disclosure is about the document, not about one dictionary.
    /// Catches `cubic_downgraded` that only looks at the top level.
    #[test]
    fn type3_reports_a_nested_cubic_downgrade() {
        // The sampled sub-function needs a stream, so this one is assembled
        // through the object graph rather than with the dict helper.
        let sampled_id = ObjId::new(2, 0);
        let data: &[u8] = &[0, 1, 2, 3];
        let mut objects = BTreeMap::new();
        objects.insert(
            sampled_id,
            Object::Stream(Stream {
                dict: dict(&[
                    (b"FunctionType", Object::Integer(0)),
                    (b"Domain", arr(&[0.0, 1.0])),
                    (b"Range", arr(&[0.0, 1.0])),
                    (b"Size", Object::Array(vec![Object::Integer(4)])),
                    (b"BitsPerSample", Object::Integer(8)),
                    (b"Order", Object::Integer(3)),
                ]),
                data_span: ByteSpan::new(0, data.len()),
            }),
        );
        let graph = TestGraph {
            objects,
            trailer: Dict::new(),
        };
        let view = DocumentView::new(&graph, data, V17);
        let stitch = Object::Dict(dict(&[
            (b"FunctionType", Object::Integer(3)),
            (b"Domain", arr(&[0.0, 1.0])),
            (
                b"Functions",
                Object::Array(vec![Object::Reference(sampled_id)]),
            ),
            (b"Bounds", Object::Array(vec![])),
            (b"Encode", arr(&[0.0, 1.0])),
        ]));
        let f = PdfFunction::load(&view, &stitch).unwrap();
        assert!(f.cubic_downgraded());
    }

    /// Every §7.10 entry may be an indirect reference (§7.3.10). Catches a
    /// loader that reads `dict.get(...)` without resolving, which would report
    /// a perfectly valid function as malformed.
    #[test]
    fn indirect_entries_are_resolved() {
        let domain_id = ObjId::new(3, 0);
        let n_id = ObjId::new(4, 0);
        let mut objects = BTreeMap::new();
        objects.insert(domain_id, arr(&[0.0, 1.0]));
        objects.insert(n_id, Object::Integer(1));
        let graph = TestGraph {
            objects,
            trailer: Dict::new(),
        };
        let view = DocumentView::new(&graph, b"", V17);
        let f = PdfFunction::load(
            &view,
            &Object::Dict(dict(&[
                (b"FunctionType", Object::Integer(2)),
                (b"Domain", Object::Reference(domain_id)),
                (b"N", Object::Reference(n_id)),
            ])),
        )
        .unwrap();
        close(&f.eval(&[0.5]).unwrap(), &[0.5]);
    }

    /// `eval_into` must CLEAR its buffer. Catches the reuse path appending to a
    /// previous result, which in a per-pixel loop grows without bound and
    /// returns the first pixel's colour forever.
    #[test]
    fn eval_into_clears_the_output_buffer() {
        let f = load_dict(&[
            (b"FunctionType", Object::Integer(2)),
            (b"Domain", arr(&[0.0, 1.0])),
            (b"N", Object::Integer(1)),
        ])
        .unwrap();
        let mut buf = vec![9.0, 9.0, 9.0];
        f.eval_into(&[0.25], &mut buf).unwrap();
        close(&buf, &[0.25]);
        f.eval_into(&[0.75], &mut buf).unwrap();
        close(&buf, &[0.75]);
    }

    // -- Type 4 (PostScript calculator), §7.10.5 — structure ----------------

    /// Load a type 4 whose program text is `src`, with `m` inputs and `n`
    /// outputs both declared over `[0, 1]`.
    fn load_ps(src: &str, m: usize, n: usize) -> Result<PdfFunction, FunctionError> {
        let domain: Vec<f64> = (0..m).flat_map(|_| [0.0, 1.0]).collect();
        let range: Vec<f64> = (0..n).flat_map(|_| [-1e300, 1e300]).collect();
        load_stream(
            &[
                (b"FunctionType", Object::Integer(4)),
                (b"Domain", arr(&domain)),
                (b"Range", arr(&range)),
            ],
            src.as_bytes(),
        )
    }

    /// Run `src` (1 input, `n` outputs) at `x`.
    fn run_ps(src: &str, x: f64, n: usize) -> Result<Vec<f64>, FunctionError> {
        load_ps(src, 1, n)?.eval(&[x])
    }

    /// The two halves of §7.10.5.1's stack contract, tested with **zero
    /// operators** so nothing else can be responsible: the inputs *are* the
    /// initial stack, and what remains *is* the output. Catches an evaluator
    /// that starts from an empty stack and expects the program to fetch its
    /// arguments some other way.
    #[test]
    fn type4_empty_program_passes_inputs_through_as_outputs() {
        let f = load_ps("{ }", 2, 2).unwrap();
        close(&f.eval(&[0.25, 0.75]).unwrap(), &[0.25, 0.75]);
    }

    /// §7.10.5.1: "It shall be an error for the number of remaining operands to
    /// differ from the number of output variables specified by `Range`."
    /// Catches a reader that pads or truncates the result to fit, which for a
    /// tint transform means silently inventing or dropping an ink.
    #[test]
    fn type4_output_arity_mismatch_is_refused() {
        let err = run_ps("{ }", 0.5, 2).unwrap_err();
        assert_eq!(
            err,
            FunctionError::OutputArity {
                expected: 2,
                got: 1
            }
        );
    }

    /// The second half of the same sentence: "…or for any of them to be objects
    /// other than numbers." Catches a boolean being coerced to 0/1 on the way
    /// out.
    #[test]
    fn type4_boolean_left_in_output_position_is_refused() {
        let err = run_ps("{ true }", 0.5, 2).unwrap_err();
        assert_eq!(err, FunctionError::NonNumericOutput { index: 1 });
    }

    /// §7.10.5.1: "The entire code stream defining the function shall be
    /// enclosed in braces." Catches an implicit-block tolerance, which would
    /// make a truncated stream run as a valid program.
    #[test]
    fn type4_program_without_enclosing_braces_is_refused() {
        let err = load_ps("1 2 add", 1, 1).unwrap_err();
        assert!(matches!(err, FunctionError::PostScriptSyntax { .. }));
    }

    /// Catches an unterminated program being accepted as if the closing brace
    /// were implied at end of stream.
    #[test]
    fn type4_unterminated_program_is_refused() {
        let err = load_ps("{ 1 2", 1, 1).unwrap_err();
        assert!(matches!(err, FunctionError::PostScriptSyntax { .. }));
    }

    /// Catches content after the closing brace being ignored — which would hide
    /// a concatenated or partially-overwritten stream.
    #[test]
    fn type4_trailing_content_after_the_program_is_refused() {
        let err = load_ps("{ } 5", 1, 1).unwrap_err();
        assert!(matches!(err, FunctionError::PostScriptSyntax { .. }));
    }

    /// Table 42 is the complete operator set. Catches an unknown token being
    /// skipped, which leaves the stack at the wrong depth and surfaces as a
    /// baffling arity error rather than as the real defect.
    #[test]
    fn type4_unknown_operator_is_refused_by_name() {
        let err = load_ps("{ 2 wibble }", 1, 1).unwrap_err();
        assert_eq!(err, FunctionError::UnknownOperator("wibble".to_owned()));
    }

    /// §7.10.5.1's brace construct is "purely syntactic": a block can only be
    /// the operand of `if` or `ifelse`. Catches a parser that treats a block as
    /// a pushable value.
    #[test]
    fn type4_block_not_followed_by_if_or_ifelse_is_refused() {
        for src in ["{ { 1 } 2 }", "{ { 1 } }", "{ { 1 } add }"] {
            let err = load_ps(src, 1, 1).unwrap_err();
            assert!(
                matches!(err, FunctionError::PostScriptSyntax { .. }),
                "{src} should be a syntax error, got {err:?}"
            );
        }
    }

    /// Catches `ifelse` being fed one block, or three blocks being accepted.
    #[test]
    fn type4_wrong_block_count_for_conditionals_is_refused() {
        for src in ["{ true { 1 } ifelse }", "{ true { 1 } { 2 } { 3 } ifelse }"] {
            let err = load_ps(src, 1, 1).unwrap_err();
            assert!(
                matches!(err, FunctionError::PostScriptSyntax { .. }),
                "{src} should be a syntax error, got {err:?}"
            );
        }
        let err = load_ps("{ true if }", 1, 1).unwrap_err();
        assert!(matches!(err, FunctionError::PostScriptSyntax { .. }));
    }

    /// The parser recurses one frame per `{`. Catches a missing depth guard,
    /// which on adversarial input is a native stack overflow — an abort, which
    /// `pdfcer-core`'s panic-free policy treats as seriously as an `unwrap`.
    #[test]
    fn type4_excessive_brace_nesting_is_refused() {
        let depth = MAX_PS_NESTING + 2;
        let src = format!("{}{}", "{ ".repeat(depth), " }".repeat(depth));
        let err = load_ps(&src, 1, 1).unwrap_err();
        assert_eq!(
            err,
            FunctionError::PostScriptNestingTooDeep {
                limit: MAX_PS_NESTING
            }
        );
    }

    /// §7.10.5.1: "it shall be an error to overflow the stack", with a
    /// `shall`-minimum capacity of 100. Catches a `Vec` that simply grows,
    /// which accepts programs no conforming reader accepts.
    #[test]
    fn type4_stack_overflow_at_one_hundred_entries_is_refused() {
        // One input already occupies an entry, so 100 more literals overflow.
        let src = format!("{{ {} }}", "0 ".repeat(PS_STACK_LIMIT));
        let err = run_ps(&src, 0.5, 1).unwrap_err();
        assert_eq!(
            err,
            FunctionError::StackOverflow {
                limit: PS_STACK_LIMIT
            }
        );
        // Exactly at the limit is fine — the boundary is inclusive.
        let ok = format!("{{ {} }}", "0 ".repeat(PS_STACK_LIMIT - 1));
        load_ps(&ok, 1, PS_STACK_LIMIT)
            .unwrap()
            .eval(&[0.5])
            .unwrap();
    }

    /// §7.10.5.2 lists stack underflow as a reader-detected error. Catches an
    /// operator reading a missing operand as zero.
    #[test]
    fn type4_stack_underflow_is_refused() {
        let err = run_ps("{ add add }", 0.5, 1).unwrap_err();
        assert!(matches!(err, FunctionError::StackUnderflow { .. }));
    }

    /// `/Range` is Required for type 4 — it is the only source of *n*, and
    /// without it the output-arity check has nothing to check against.
    #[test]
    fn type4_missing_range_is_refused() {
        let err = load_stream(
            &[
                (b"FunctionType", Object::Integer(4)),
                (b"Domain", arr(&[0.0, 1.0])),
            ],
            b"{ }",
        )
        .unwrap_err();
        assert_eq!(
            err,
            FunctionError::MissingEntry {
                key: "Range",
                function_type: 4
            }
        );
    }

    /// A type 4 written as a bare dictionary has no program.
    #[test]
    fn type4_as_a_bare_dictionary_is_refused() {
        let err = load_dict(&[
            (b"FunctionType", Object::Integer(4)),
            (b"Domain", arr(&[0.0, 1.0])),
            (b"Range", arr(&[0.0, 1.0])),
        ])
        .unwrap_err();
        assert_eq!(err, FunctionError::NotAStream { function_type: 4 });
    }

    /// §7.10.5.2's "type error" class, at the conditional. Catches a number
    /// being treated as truthy, which C-family instincts make the natural
    /// mistake — PostScript has no truthiness.
    #[test]
    fn type4_non_boolean_condition_is_refused() {
        let err = run_ps("{ 1 { 2 } if }", 0.5, 1).unwrap_err();
        assert_eq!(
            err,
            FunctionError::PostScriptType {
                op: "if",
                detail: "the condition operand is not a boolean"
            }
        );
        let err = run_ps("{ 1.5 { 2 } { 3 } ifelse }", 0.5, 1).unwrap_err();
        assert!(matches!(
            err,
            FunctionError::PostScriptType { op: "ifelse", .. }
        ));
    }

    /// The `%` comment and the white-space rules come from the shared lexer
    /// (§7.2), not from a second tokenizer written for this sub-language.
    /// Catches a hand-rolled tokenizer being introduced later that disagrees
    /// with the object lexer.
    #[test]
    fn type4_comments_and_irregular_whitespace_are_skipped() {
        let f = load_ps("{% leading comment\n\t 1\r\n%another\n}", 1, 2).unwrap();
        close(&f.eval(&[0.5]).unwrap(), &[0.5, 1.0]);
    }

    /// pdfcer's step cap (policy, not spec — see [`MAX_PS_STEPS`]). Catches the
    /// counter never being consulted, which would leave a multi-megabyte
    /// program running its full length once per pixel.
    ///
    /// The program is deliberately neutral (`1 pop` leaves the stack alone), so
    /// the ONLY thing that can stop it is the cap.
    #[test]
    fn type4_step_cap_stops_a_pathologically_long_program() {
        let pairs = MAX_PS_STEPS / 2 + 8;
        let src = format!("{{ {} }}", "1 pop ".repeat(pairs));
        let err = run_ps(&src, 0.5, 1).unwrap_err();
        assert_eq!(
            err,
            FunctionError::StepLimit {
                limit: MAX_PS_STEPS
            }
        );
    }

    /// An integer literal outside the 32-bit range has no representation in
    /// this sub-language. Catches it being saturated into a wrong constant that
    /// then computes silently.
    #[test]
    fn type4_out_of_range_integer_literal_is_refused() {
        let err = load_ps("{ 99999999999 }", 1, 2).unwrap_err();
        assert!(matches!(err, FunctionError::PostScriptSyntax { .. }));
    }

    // -- Type 4 — operator semantics (ISO 32000-1 Annex B; PLRM3 §8.2) ------

    /// Evaluate `body` with the one dummy input already discarded, so the
    /// assertions are about the operators and nothing else.
    fn ps(body: &str, n: usize) -> Result<Vec<f64>, FunctionError> {
        run_ps(&format!("{{ pop {body} }}"), 0.0, n)
    }

    /// `body` must fail; return the error.
    #[track_caller]
    fn ps_err(body: &str, n: usize) -> FunctionError {
        ps(body, n).expect_err("expected a refusal")
    }

    /// **`ifelse`**, the required conditional test, driven by a real comparison
    /// rather than a literal. Catches inverted branch selection — which a
    /// `true`-only test cannot see, because both branches of a wrongly-wired
    /// `ifelse` still run *something*.
    #[test]
    fn type4_ifelse_selects_the_branch_the_condition_names() {
        close(&ps("0.6 0.5 gt { 11 } { 22 } ifelse", 1).unwrap(), &[11.0]);
        close(&ps("0.4 0.5 gt { 11 } { 22 } ifelse", 1).unwrap(), &[22.0]);
        // `if` with no else: the block runs or it does not, and nothing else
        // changes.
        close(&ps("7 true { 1 add } if", 1).unwrap(), &[8.0]);
        close(&ps("7 false { 1 add } if", 1).unwrap(), &[7.0]);
        // Nested, so the recursive descent through blocks is exercised.
        close(
            &ps(
                "3 dup 2 gt { dup 5 gt { 100 } { 200 } ifelse } { 300 } ifelse exch pop",
                1,
            )
            .unwrap(),
            &[200.0],
        );
    }

    /// §7.10.5.1's own worked EXAMPLE — the DoubleDot halftone spot function,
    /// a genuine 2-in/1-out program straight out of the standard. Catches any
    /// defect that only shows up when several operators compose, and pins
    /// `sin`'s units: the `360 mul` before each `sin` is only meaningful in
    /// degrees.
    #[test]
    fn type4_evaluates_the_specs_doubledot_example() {
        let f = load_stream(
            &[
                (b"FunctionType", Object::Integer(4)),
                (b"Domain", arr(&[-1.0, 1.0, -1.0, 1.0])),
                (b"Range", arr(&[-1.0, 1.0])),
            ],
            b"{ 360 mul sin 2 div exch 360 mul sin 2 div add }",
        )
        .unwrap();
        assert_eq!(f.inputs(), 2);
        assert_eq!(f.outputs(), 1);
        // sin(360y)/2 + sin(360x)/2.
        close(&f.eval(&[0.0, 0.0]).unwrap(), &[0.0]);
        close(&f.eval(&[0.25, 0.0]).unwrap(), &[0.5]);
        close(&f.eval(&[0.25, 0.25]).unwrap(), &[1.0]);
        close(&f.eval(&[0.75, 0.0]).unwrap(), &[-0.5]);
    }

    /// `sin`/`cos` take **degrees** (Annex B). Catches a radian implementation,
    /// which for a halftone spot function produces a smoothly wrong screen
    /// rather than an obvious failure.
    #[test]
    fn type4_sin_and_cos_take_degrees_not_radians() {
        close(&ps("90 sin", 1).unwrap(), &[1.0]);
        close(&ps("0 cos", 1).unwrap(), &[1.0]);
        close(&ps("180 cos", 1).unwrap(), &[-1.0]);
        // sin(180°) is 0; sin(180 rad) is −0.801.
        close(&ps("180 sin", 1).unwrap(), &[0.0]);
    }

    /// `num den atan → angle` in degrees, normalised to `[0, 360)` and never
    /// negative. Catches a bare `atan2().to_degrees()`, which answers −90.0
    /// where the spec requires 270.0, and catches a one-operand reading.
    #[test]
    fn type4_atan_is_two_operand_degrees_and_never_negative() {
        close(&ps("0 1 atan", 1).unwrap(), &[0.0]);
        close(&ps("1 0 atan", 1).unwrap(), &[90.0]);
        close(&ps("-100 0 atan", 1).unwrap(), &[270.0]);
        close(&ps("4 4 atan", 1).unwrap(), &[45.0]);
        // Zero over zero has no defined angle.
        assert!(matches!(
            ps_err("0 0 atan", 1),
            FunctionError::UndefinedResult { op: "atan", .. }
        ));
    }

    /// `base exponent exp` is **pow**, not `e^x` (Annex B). Catches the `e^x`
    /// reading, which produces 8103.08 where the answer is 3.
    #[test]
    fn type4_exp_is_pow_not_e_to_the_x() {
        close(&ps("9 0.5 exp", 1).unwrap(), &[3.0]);
        close(&ps("2 10 exp", 1).unwrap(), &[1024.0]);
    }

    /// `log` is base 10 and `ln` is natural (Annex B states both). Catches the
    /// two being swapped, which is a 2.3× error that still looks like a curve.
    #[test]
    fn type4_log_is_base_ten_and_ln_is_natural() {
        close(&ps("100 log", 1).unwrap(), &[2.0]);
        close(&ps("1 ln", 1).unwrap(), &[0.0]);
        close(&ps("2.718281828459045 ln", 1).unwrap(), &[1.0]);
        // Both need a positive operand.
        assert!(matches!(
            ps_err("0 ln", 1),
            FunctionError::PostScriptRange { op: "ln", .. }
        ));
        assert!(matches!(
            ps_err("-1 log", 1),
            FunctionError::PostScriptRange { op: "log", .. }
        ));
    }

    /// `add`/`sub`/`mul` keep an integer result integral; `div` never does.
    /// The type is observed by feeding the result to `idiv`, which is
    /// integer-only — the only way an external caller can see the distinction
    /// at all, and the reason it must be modelled.
    #[test]
    fn type4_arithmetic_preserves_integers_but_div_always_produces_a_real() {
        // 2 + 3 = the INTEGER 5, so `idiv` accepts it.
        close(&ps("2 3 add 2 idiv", 1).unwrap(), &[2.0]);
        close(&ps("10 4 sub 3 idiv", 1).unwrap(), &[2.0]);
        close(&ps("3 4 mul 5 idiv", 1).unwrap(), &[2.0]);
        // 4 div 2 is the REAL 2.0 even though it is exact, so `idiv` refuses.
        close(&ps("4 2 div", 1).unwrap(), &[2.0]);
        assert!(matches!(
            ps_err("4 2 div 2 idiv", 1),
            FunctionError::PostScriptType { op: "idiv", .. }
        ));
    }

    /// The overflow half of the same rule: an integer sum that does not fit
    /// becomes a **real**, not a wrapped negative. Catches `wrapping_add`,
    /// which would turn 4×10⁹ into −294,967,296 and hand that to a colour
    /// component.
    #[test]
    fn type4_integer_overflow_spills_to_real_rather_than_wrapping() {
        close(&ps("2000000000 2000000000 add", 1).unwrap(), &[4.0e9]);
        // And the result really is a real now, so `idiv` refuses it.
        assert!(matches!(
            ps_err("2000000000 2000000000 add 2 idiv", 1),
            FunctionError::PostScriptType { op: "idiv", .. }
        ));
    }

    /// **The rounding operators are type-preserving, not integer-producing.**
    /// `3.7 floor` is the real `3.0`; only `cvi` converts. Catches the
    /// intuitive-but-wrong "floor returns an integer", which would make pdfcer
    /// accept `3.7 floor 2 idiv` — a program every conforming reader rejects.
    #[test]
    fn type4_rounding_operators_preserve_their_operands_type() {
        close(&ps("3.7 floor", 1).unwrap(), &[3.0]);
        close(&ps("3.2 ceiling", 1).unwrap(), &[4.0]);
        close(&ps("-4.8 truncate", 1).unwrap(), &[-4.0]);
        close(&ps("-4.8 floor", 1).unwrap(), &[-5.0]);
        close(&ps("-4.8 ceiling", 1).unwrap(), &[-4.0]);
        // A real in, a real out — `idiv` refuses it.
        assert!(matches!(
            ps_err("3.7 floor 2 idiv", 1),
            FunctionError::PostScriptType { op: "idiv", .. }
        ));
        // An integer in, an integer out — `idiv` accepts it.
        close(&ps("99 floor 2 idiv", 1).unwrap(), &[49.0]);
    }

    /// `round` breaks ties toward the **greater** value, so `-6.5` rounds to
    /// `-6.0`. Catches `f64::round`, which breaks ties away from zero and
    /// answers `-7.0` — the one rounding call where the obvious choice is
    /// wrong.
    #[test]
    fn type4_round_breaks_ties_toward_the_greater_value() {
        close(&ps("6.5 round", 1).unwrap(), &[7.0]);
        close(&ps("-6.5 round", 1).unwrap(), &[-6.0]);
        close(&ps("3.2 round", 1).unwrap(), &[3.0]);
        close(&ps("-3.2 round", 1).unwrap(), &[-3.0]);
    }

    /// `idiv` truncates toward zero and `mod` takes the sign of the
    /// **dividend** — PLRM3 calls it "a remainder operation rather than a true
    /// modulo". Catches a floored-division implementation, which answers −3 and
    /// +1 where the spec requires −2 and −2.
    #[test]
    fn type4_idiv_truncates_toward_zero_and_mod_follows_the_dividend() {
        close(&ps("-5 2 idiv", 1).unwrap(), &[-2.0]);
        close(&ps("5 2 idiv", 1).unwrap(), &[2.0]);
        close(&ps("-5 3 mod", 1).unwrap(), &[-2.0]);
        close(&ps("5 -3 mod", 1).unwrap(), &[2.0]);
        close(&ps("5 3 mod", 1).unwrap(), &[2.0]);
    }

    /// `cvi` is the only real→integer conversion, and it truncates toward zero.
    /// Catches a `floor`-based conversion, which answers −48 for −47.8.
    #[test]
    fn type4_cvi_truncates_toward_zero_and_yields_an_integer() {
        close(&ps("-47.8 cvi", 1).unwrap(), &[-47.0]);
        close(&ps("47.8 cvi", 1).unwrap(), &[47.0]);
        // The result really is an integer, so `idiv` accepts it.
        close(&ps("-47.8 cvi 2 idiv", 1).unwrap(), &[-23.0]);
        // Out of the integer range is a range error, not a saturation to
        // i32::MAX (which would be a plausible-looking wrong number).
        assert!(matches!(
            ps_err("3000000000.0 cvi", 1),
            FunctionError::PostScriptRange { op: "cvi", .. }
        ));
    }

    /// `not` on an **integer** is the ones complement, not a zero test: PLRM3's
    /// own example is `52 not → -53`. Catches a boolean-only `not` that coerces,
    /// which would silently answer `false` there.
    #[test]
    fn type4_not_on_an_integer_is_the_ones_complement() {
        close(&ps("52 not", 1).unwrap(), &[-53.0]);
        close(&ps("0 not", 1).unwrap(), &[-1.0]);
        close(&ps("true not { 1 } { 2 } ifelse", 1).unwrap(), &[2.0]);
        // A real has no complement.
        assert!(matches!(
            ps_err("1.5 not", 1),
            FunctionError::PostScriptType { op: "not", .. }
        ));
    }

    /// `and`/`or`/`xor` are polymorphic over `bool|int` — Annex B's own
    /// notation. Catches a boolean-only implementation (which cannot compute
    /// `52 7 and → 4`) and an integer-only one (which cannot compute
    /// `true false and`).
    #[test]
    fn type4_bitwise_operators_are_polymorphic_over_booleans_and_integers() {
        close(&ps("52 7 and", 1).unwrap(), &[4.0]);
        close(&ps("17 5 or", 1).unwrap(), &[21.0]);
        close(&ps("7 3 xor", 1).unwrap(), &[4.0]);
        close(&ps("true false and { 1 } { 2 } ifelse", 1).unwrap(), &[2.0]);
        close(&ps("true false or { 1 } { 2 } ifelse", 1).unwrap(), &[1.0]);
        close(&ps("true true xor { 1 } { 2 } ifelse", 1).unwrap(), &[2.0]);
    }

    /// A mixed boolean/integer pair is a `typecheck`, with no coercion in
    /// either direction. Catches an implementation that maps `true` to 1.
    #[test]
    fn type4_mixed_boolean_and_integer_bitwise_operands_are_refused() {
        assert!(matches!(
            ps_err("true 1 and", 1),
            FunctionError::PostScriptType { op: "and", .. }
        ));
        assert!(matches!(
            ps_err("1.0 2.0 or", 1),
            FunctionError::PostScriptType { op: "or", .. }
        ));
    }

    /// Annex B types `eq`/`ne` as `any any` but `gt`/`ge`/`lt`/`le` as
    /// `num num`. Catches the four relationals accepting booleans, and catches
    /// `eq` erroring on a boolean-versus-number comparison that should simply
    /// be `false`.
    #[test]
    fn type4_equality_accepts_any_operands_but_ordering_requires_numbers() {
        // An integer and a real compare equal when their values match.
        close(&ps("4.0 4 eq { 1 } { 0 } ifelse", 1).unwrap(), &[1.0]);
        close(&ps("4 5 ne { 1 } { 0 } ifelse", 1).unwrap(), &[1.0]);
        // Boolean against boolean.
        close(&ps("true true eq { 1 } { 0 } ifelse", 1).unwrap(), &[1.0]);
        // Boolean against a number: false, NOT an error.
        close(&ps("true 1 eq { 1 } { 0 } ifelse", 1).unwrap(), &[0.0]);
        // Ordering rejects booleans.
        for src in [
            "true false gt",
            "true false lt",
            "true false ge",
            "true false le",
        ] {
            assert!(
                matches!(ps_err(src, 1), FunctionError::PostScriptType { .. }),
                "{src} should be a type error"
            );
        }
        close(&ps("1 2 lt { 1 } { 0 } ifelse", 1).unwrap(), &[1.0]);
        close(&ps("2 2 ge { 1 } { 0 } ifelse", 1).unwrap(), &[1.0]);
    }

    /// `bitshift`: positive shifts left, negative shifts right, and the right
    /// shift is **logical** — PLRM3, "bits shifted in are 0". Catches an
    /// arithmetic right shift, which answers −4 for `-8 -1 bitshift` where the
    /// spec's zero-fill gives a large positive number.
    #[test]
    fn type4_bitshift_is_left_on_positive_and_logically_right_on_negative() {
        close(&ps("7 3 bitshift", 1).unwrap(), &[56.0]);
        close(&ps("142 -3 bitshift", 1).unwrap(), &[17.0]);
        // Zero-fill, not sign-extension: 0xFFFFFFF8 >> 1 = 0x7FFFFFFC.
        close(&ps("-8 -1 bitshift", 1).unwrap(), &[2_147_483_644.0]);
        // A shift at or past the word width empties the register.
        close(&ps("1 32 bitshift", 1).unwrap(), &[0.0]);
        close(&ps("1 -32 bitshift", 1).unwrap(), &[0.0]);
    }

    /// The integer-only operators refuse a real rather than truncating it —
    /// §7.10.5.2's "type error" class. Catches a helpful coercion that makes
    /// pdfcer compute where a conforming reader raises.
    #[test]
    fn type4_integer_only_operators_refuse_reals() {
        for src in [
            "3.0 2 idiv",
            "3.0 2 mod",
            "3.0 2 bitshift",
            "3 2.0 bitshift",
        ] {
            assert!(
                matches!(ps_err(src, 1), FunctionError::PostScriptType { .. }),
                "{src} should be a type error"
            );
        }
    }

    /// `dup`, `exch` and `pop`, the three unambiguous stack operators.
    #[test]
    fn type4_dup_exch_and_pop() {
        close(&ps("10 dup", 2).unwrap(), &[10.0, 10.0]);
        close(&ps("10 20 exch", 2).unwrap(), &[20.0, 10.0]);
        close(&ps("10 20 pop", 1).unwrap(), &[10.0]);
    }

    /// `index` is **zero-based from the top**, so `0 index` is exactly `dup`.
    /// Catches one-based counting and counting from the bottom — both of which
    /// return a real value from the stack and so fail silently.
    #[test]
    fn type4_index_counts_from_the_top_starting_at_zero() {
        close(
            &ps("10 20 30 0 index", 4).unwrap(),
            &[10.0, 20.0, 30.0, 30.0],
        );
        close(
            &ps("10 20 30 1 index", 4).unwrap(),
            &[10.0, 20.0, 30.0, 20.0],
        );
        close(
            &ps("10 20 30 2 index", 4).unwrap(),
            &[10.0, 20.0, 30.0, 10.0],
        );
        // Reaching past the bottom is a range error, not a wrap.
        assert!(matches!(
            ps_err("10 5 index", 1),
            FunctionError::PostScriptRange { op: "index", .. }
        ));
    }

    /// `copy` duplicates the top *n*, and `0 copy` is an explicitly legal
    /// no-op. Catches the count being included in the copied region, and
    /// catches `0 copy` being special-cased into an error.
    #[test]
    fn type4_copy_duplicates_the_top_n_and_zero_is_a_legal_no_op() {
        close(&ps("10 20 2 copy", 4).unwrap(), &[10.0, 20.0, 10.0, 20.0]);
        close(
            &ps("10 20 30 1 copy", 4).unwrap(),
            &[10.0, 20.0, 30.0, 30.0],
        );
        close(&ps("10 20 0 copy", 2).unwrap(), &[10.0, 20.0]);
        assert!(matches!(
            ps_err("10 20 5 copy", 1),
            FunctionError::PostScriptRange { op: "copy", .. }
        ));
    }

    /// `roll` rotates the top *n* by *j*, with a **positive `j` moving elements
    /// upward**. Catches a rotation in the wrong direction — which agrees at
    /// `j = 0` and disagrees at every other `j`, so `j = ±1` is the whole test.
    #[test]
    fn type4_roll_rotates_upward_for_a_positive_count() {
        close(&ps("10 20 30 3 1 roll", 3).unwrap(), &[30.0, 10.0, 20.0]);
        close(&ps("10 20 30 3 -1 roll", 3).unwrap(), &[20.0, 30.0, 10.0]);
        close(&ps("10 20 30 3 0 roll", 3).unwrap(), &[10.0, 20.0, 30.0]);
        // A rotation of a full turn is the identity, so `j` wraps modulo `n`.
        close(&ps("10 20 30 3 3 roll", 3).unwrap(), &[10.0, 20.0, 30.0]);
        close(&ps("10 20 30 3 4 roll", 3).unwrap(), &[30.0, 10.0, 20.0]);
        // Only the top n move.
        close(
            &ps("10 20 30 40 2 1 roll", 4).unwrap(),
            &[10.0, 20.0, 40.0, 30.0],
        );
    }

    /// `n = 0` leaves `j mod n` undefined (ambiguity `F-A4`); pdfcer treats it as
    /// the identity, consistent with `0 copy`. Catches a division by zero in
    /// the modulo.
    #[test]
    fn type4_roll_with_a_zero_window_is_the_identity() {
        close(&ps("10 20 0 1 roll", 2).unwrap(), &[10.0, 20.0]);
    }

    /// A negative count operand is a `rangecheck` for all three counted stack
    /// operators. Catches a negative being cast to a huge `usize`.
    #[test]
    fn type4_negative_count_operands_are_range_errors() {
        for src in ["10 20 -1 copy", "10 -1 index", "10 20 -1 1 roll"] {
            assert!(
                matches!(ps_err(src, 1), FunctionError::PostScriptRange { .. }),
                "{src} should be a range error"
            );
        }
    }

    /// §7.10.5.2's remaining two error classes, at their canonical examples:
    /// "a range error (for example, applying `sqrt` to a negative number)" and
    /// "an undefined result (for example, dividing by 0)".
    #[test]
    fn type4_sqrt_of_a_negative_and_division_by_zero_are_refused() {
        assert!(matches!(
            ps_err("-1 sqrt", 1),
            FunctionError::PostScriptRange { op: "sqrt", .. }
        ));
        assert!(matches!(
            ps_err("1 0 div", 1),
            FunctionError::UndefinedResult { op: "div", .. }
        ));
        assert!(matches!(
            ps_err("1 0 idiv", 1),
            FunctionError::UndefinedResult { op: "idiv", .. }
        ));
        assert!(matches!(
            ps_err("1 0 mod", 1),
            FunctionError::UndefinedResult { op: "mod", .. }
        ));
        // 0 to a negative power has no finite value.
        assert!(matches!(
            ps_err("0 -1 exp", 1),
            FunctionError::UndefinedResult { op: "exp", .. }
        ));
        close(&ps("4 sqrt", 1).unwrap(), &[2.0]);
    }

    /// `neg` and `abs` preserve their operand's type, which the `idiv` probe
    /// makes visible. Catches them being routed through `f64` unconditionally.
    #[test]
    fn type4_neg_and_abs_preserve_the_operand_type() {
        close(&ps("5 neg", 1).unwrap(), &[-5.0]);
        close(&ps("-5.5 abs", 1).unwrap(), &[5.5]);
        close(&ps("-5 abs 2 idiv", 1).unwrap(), &[2.0]);
        close(&ps("5 neg 2 idiv", 1).unwrap(), &[-2.0]);
    }

    /// `cvr` turns an integer into a real, which the `idiv` probe then refuses.
    /// Catches `cvr` being implemented as a no-op — harmless-looking, but it
    /// means a program that deliberately leaves the integer domain does not.
    #[test]
    fn type4_cvr_converts_an_integer_to_a_real() {
        close(&ps("7 cvr", 1).unwrap(), &[7.0]);
        assert!(matches!(
            ps_err("7 cvr 2 idiv", 1),
            FunctionError::PostScriptType { op: "idiv", .. }
        ));
    }

    /// Every one of Table 42's 42 entries is recognised. Catches an operator
    /// being left out of the token map — which would surface as
    /// `UnknownOperator` on a perfectly valid file, months later.
    #[test]
    fn type4_every_table_42_operator_is_recognised() {
        const TABLE_42: [&str; 42] = [
            "abs", "add", "atan", "ceiling", "cos", "cvi", "cvr", "div", "exp", "floor", "idiv",
            "ln", "log", "mod", "mul", "neg", "round", "sin", "sqrt", "sub", "truncate", "and",
            "bitshift", "eq", "false", "ge", "gt", "le", "lt", "ne", "not", "or", "true", "xor",
            "if", "ifelse", "copy", "dup", "exch", "index", "pop", "roll",
        ];
        for token in TABLE_42 {
            let structural = matches!(token, "true" | "false" | "if" | "ifelse");
            assert_eq!(
                PsOperator::from_token(token.as_bytes()).is_some(),
                !structural,
                "{token} classified wrongly"
            );
        }
        // And the round trip through `name` is exact for the 38 dispatched
        // operators, so an error message can never blame the wrong operator.
        for token in TABLE_42 {
            if let Some(op) = PsOperator::from_token(token.as_bytes()) {
                assert_eq!(op.name(), token);
            }
        }
    }
}
