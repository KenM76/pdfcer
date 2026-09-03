//! # Colour spaces for content streams (ISO 32000-1 §8.6)
//!
//! The colour-space model behind the `cs`/`CS` and `sc`/`scn`/`SC`/`SCN`
//! operators (Table 74). Spec sources, all in the PDF-spec RAG at
//! `D:\Dev\Rag-Specialized\PDF_Spec\`:
//!
//! | Clause | RAG file | What it governs here |
//! |---|---|---|
//! | §8.6.3, §8.6.4, §8.6.8, Table 74 | `iso32000__s__8.6.md` | families, device spaces, the operators, the per-space initial colour |
//! | §8.6.5.5, Tables 66/67 | `color__iccbased.md` | `ICCBased` and its `/N`-driven fallback |
//! | §8.6.6.3 | `color__indexed.md` | `Indexed` palettes, `hival`, the normative index clamp |
//! | §8.6.6.4 | `color__separation.md` | `Separation`, `/All`, `/None`, the additive-device rule |
//! | §8.6.6.5, Tables 71–73 | `color__devicen.md` | `DeviceN`/`NChannel` |
//! | §8.6.5.1–.4, Tables 63–65 | `color__cie_based.md` | `CalGray`, `CalRGB`, `Lab` |
//!
//! ## The defect this module exists to fix
//!
//! Before this module, [`crate::interpret`] **recognised and deferred**
//! `cs`, `CS`, `sc`, `scn`, `SC` and `SCN`. Only `g`/`rg`/`k` (and their
//! stroking twins) were honoured. The consequence was not a missing
//! feature — it was **silently wrong pixels**: a content stream that
//! selected a colour space and then set a colour kept whatever colour was
//! previously in force and painted real marks with it. On a CAD drawing
//! using spot colours — the exact workload this project was started for —
//! every line came out in a stale colour, most often black, with no
//! diagnostic anywhere saying so.
//!
//! That is why the headline test in this module asserts **pixels**, not
//! interpreter state: state can be right while the paint is stale, and it
//! was the paint that was wrong.
//!
//! ## What is implemented, and what is disclosed instead
//!
//! "Fuzzy, never sneaky" (project rule 4) applies to a renderer as a
//! disclosure obligation: every place pdfcer cannot do the exact thing gets
//! a **counted** diagnostic in [`ColorDiagnostics`], never a silent
//! approximation, and never a silent fallback to `DeviceGray`.
//!
//! | Space | Treatment |
//! |---|---|
//! | `DeviceGray`/`DeviceRGB`/`DeviceCMYK` | Exact, via [`pdfcer_core::color`] — the one conversion site in the project (see [`crate::gstate`]'s module docs for why). |
//! | `CalGray`, `CalRGB` | Full §8.6.5.2/.3 decode to CIE XYZ, then XYZ→sRGB (see "White points" below). |
//! | `Lab` | Full §8.6.5.4 decode, including the piecewise `g(x)`. |
//! | `ICCBased` | **The spec's own fallback**, not an approximation pdfcer invented — see [`ColorSpace::IccBased`]. |
//! | `Indexed` | Full §8.6.6.3, string *and* stream lookups, normative index clamp. |
//! | `Separation`, `DeviceN` | Parsed in full, **tint transform evaluated** via [`pdfcer_core::function`] — [`ColorDiagnostics::tint_transforms_applied`] counts the successes, [`ColorDiagnostics::tint_transform_not_applied`] the residue (a missing or unusable `/tintTransform`). |
//! | `Pattern` | Recognised. `PatternType 2` (shading) is **painted**; tiling patterns and unresolvable names are not, and the residue is counted. See [`ColorDiagnostics::patterns_unpainted`]. |
//!
//! ### Why `Separation`/`DeviceN` stop short of the transform
//!
//! §8.6.6.4's rule S4 makes this simple in principle: pdfcer is an
//! **additive** device, so a `Separation` space *never* applies a colorant
//! directly and *always* reverts to `alternateSpace` via `tintTransform`.
//! There is no colorant-matching step to implement at all. What is missing
//! was only the second half — a §7.10 function evaluator (types 0/2/3/4).
//! **That evaluator landed**: it lives in `pdfcer-core` as
//! [`pdfcer_core::function`], deliberately in ONE place because images
//! (`DeviceN` samples), shadings and tint transforms all need it, and two
//! evaluators that disagree would paint the same colour two ways in one
//! document. [`separation_to_rgb`] and [`device_n_to_rgb`] call it, so a
//! spot colour with a usable `/tintTransform` now paints **the document's
//! own colour**, and [`ColorDiagnostics::tint_transforms_applied`] counts
//! each one.
//!
//! What survives is the **residue**, and it is a file property rather than
//! a pdfcer one: a `Separation`/`DeviceN` space whose `/tintTransform` is
//! absent, malformed, or of the wrong arity. Those fall back to the
//! alternate space's **neutral** interpretation of the tint
//! ([`ColorSpace::neutral_from_tint`]) — the lightness ordering is right,
//! the hue is not reproduced — and
//! [`ColorDiagnostics::tint_transform_not_applied`] counts every one.
//!
//! ## White points, and where pdfcer is choosing rather than matching
//!
//! §8.6.5 defines CIE-based spaces as a decode to **CIE 1931 XYZ**. It
//! does not define how a reader gets from XYZ to a monitor's RGB — that is
//! clause 10 / colour-management territory, and pdfcer has no colour
//! management engine. So [`xyz_to_srgb`] is **pdfcer's engineering choice**,
//! stated as such:
//!
//! 1. Bradford chromatic adaptation from the space's own `WhitePoint` to
//!    D65 (PDF CIE spaces are overwhelmingly D50; sRGB is defined at D65,
//!    and skipping the adaptation gives a visible warm cast).
//! 2. The standard sRGB (IEC 61966-2-1) XYZ→linear-RGB matrix.
//! 3. The sRGB transfer function.
//!
//! No rendering intent (§8.6.5.8) and no gamut mapping beyond a clamp are
//! applied; out-of-gamut XYZ is clipped per channel.
//!
//! ## Scope: what is NOT here
//!
//! - **`DefaultGray`/`DefaultRGB`/`DefaultCMYK`** (§8.6.5.6): a resource
//!   `/ColorSpace` entry under those names redirects `g`/`rg`/`k` to a
//!   CIE-based substitute. Not implemented; device operators go straight to
//!   the device space. A documented near-parity deviation, per
//!   `iso32000__s__8.6.md`.
//! - **Overprint** (§8.6.7): §8.6.7 says "If overprinting is not
//!   supported, the value of the overprint parameter **shall be ignored**",
//!   so ignoring it on an additive display is *conformant*, not a
//!   deviation. Nothing to disclose.
//! - **Image colour spaces**: [`crate::image`] has its own resolver, with a
//!   different job — it converts a whole palette or a whole sample plane
//!   and cares about `/Decode`, `BitsPerComponent` and codec geometry. The
//!   two are deliberately not merged; this one resolves once per `cs` and
//!   converts one colour at a time.
//!
//! ## The colour space is graphics state, and `q`/`Q` must save it
//!
//! Table 52 puts the colour *space* in the graphics state alongside the
//! colour. [`ColorState`] therefore carries its own `q`/`Q` stack, pushed
//! and popped in lockstep with [`crate::gstate::GStateStack`]. Without it,
//! `q /CS0 cs 1 scn Q 0.5 sc` would interpret the trailing `sc` in a space
//! the `Q` had already discarded.
//!
//! **Known limitation, stated rather than discovered:** a form XObject
//! invoked by `Do` gets a *fresh* [`ColorState`] (initial `DeviceGray`),
//! not the caller's. The colour *values* are inherited correctly — they
//! live in [`crate::gstate::GraphicsState`] — so painting is unaffected
//! unless the form issues `sc`/`scn` without first issuing its own `cs`,
//! which is ill-formed in practice because the operand count depends on the
//! space. §8.10.1 says the form inherits the whole graphics state; closing
//! this gap means threading the state through `run_nested`, which is a
//! change to the interpreter's signature rather than to this module.

use std::collections::HashMap;
use std::sync::Arc;

use pdfcer_core::filters;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, Object};
use pdfcer_core::settings::CmykIntent;
use pdfcer_core::view::DocumentView;

use crate::gstate::Rgb;

/// Maximum nesting accepted while resolving a colour-space object.
///
/// A legitimate space nests two or three deep (`Indexed` over `ICCBased`,
/// or `Separation` whose alternate is `ICCBased`), and each named-resource
/// hop adds one. Eight is generous headroom; the guard exists because a
/// self-referential resource — `/ColorSpace << /CS0 [/Indexed /CS0 0 <00>] >>`
/// — is otherwise unbounded recursion on attacker-supplied input
/// (ARCHITECTURE.md §10).
pub const MAX_COLOR_SPACE_DEPTH: usize = 8;

/// Cap on distinct strings retained in [`ColorDiagnostics::notes`].
///
/// Matches [`crate::interpret`]'s own sample cap and exists for the same
/// reason: the list is shown to a human and shipped in a CLI batch report,
/// so an unbounded list from a hostile page is both useless and an
/// allocation vector.
const MAX_NOTES: usize = 12;

/// CIE D65 white point, the illuminant sRGB is defined against
/// (IEC 61966-2-1). Used as the adaptation target in [`xyz_to_srgb`].
const D65: [f32; 3] = [0.950_47, 1.0, 1.088_83];

/// The ICC profile connection space's illuminant — D50 as ICC.1 §7.2.16
/// encodes it in every profile header (`0.9642, 1.0, 0.8249`). The white
/// [`ColorSpace::to_pcs_xyz`] adapts to, because a destination profile's
/// B2A table is defined against it.
const PCS_D50: [f32; 3] = [0.964_2, 1.0, 0.824_9];

/// A colorant name in a `Separation` or `DeviceN` space (§8.6.6.4/.5).
///
/// `All` and `None` are singled out because the standard gives them
/// normative behaviour that **overrides** the alternate space and tint
/// transform entirely: a conforming reader "shall ignore the
/// `alternateSpace` and `tintTransform` parameters" for both, on every
/// device, "even if the devices are not capable of supporting any others"
/// (§8.6.6.4). They are not merely two more names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Colorant {
    /// `/All` — "shall refer collectively to all colorants available on an
    /// output device"; painting "shall apply tint values to all available
    /// colorants at once" (§8.6.6.4). Intended for registration targets.
    All,
    /// `/None` — "shall not produce any visible output. Painting
    /// operations in a `Separation` space with this colorant name shall
    /// have no effect on the current page" (§8.6.6.4).
    ///
    /// Honoured by suppressing the paint, not by painting white: white
    /// would erase a backdrop that must show through.
    None,
    /// Any other colorant name. "Such colorant names are arbitrary, and
    /// there may be any number of them" (§8.6.6.4).
    ///
    /// # ★★ Why BYTES and not `String`
    ///
    /// This was `Named(String)`, built with `String::from_utf8_lossy`, and
    /// that is **not an identity-preserving conversion**: every distinct
    /// invalid byte sequence maps to the same `U+FFFD`, so two different
    /// colorants compare EQUAL. Found in the real corpus — a census of 4,023
    /// files turned up colorant names carrying `U+FFFD`, in more than one
    /// file.
    ///
    /// A colorant name is an **identity**, and the standard says so in a way
    /// that leaves no room: §8.6.6.4's device test consults *only* the name
    /// ("shall determine whether the device has an available colorant
    /// corresponding to the name"), and §7.3.5 NOTE 4 states that names
    /// differing in bytes are distinct names **even if they render
    /// identically**. No case folding and no Unicode normalisation is
    /// specified. UTF-8 is a *should* for DISPLAY, not a rule for equality.
    ///
    /// ⇒ **Lossy is fine for showing a name to an operator; it is never fine
    /// for deciding whether two names are the same.** The diagnostic paths in
    /// this module still use `from_utf8_lossy` deliberately, and that is the
    /// correct split rather than an oversight.
    ///
    /// ★ This was HARMLESS when it was found, because nothing was keyed on a
    /// colorant name — and it stops being harmless the moment the
    /// per-spot-colorant plane lands, since two colliding names would then
    /// share one ink plane and silently composite as one colour. Fixed
    /// *before* that work rather than during it, so the plane is not
    /// debugging this at the same time.
    Named(Box<[u8]>),
}

impl Colorant {
    /// Classify a raw `/Name` from a colour-space array.
    ///
    /// The bytes arrive already `#xx`-decoded by the lexer, which is the
    /// comparison form §7.3.5 specifies, so they are stored verbatim.
    fn parse(bytes: &[u8]) -> Self {
        match bytes {
            b"All" => Self::All,
            b"None" => Self::None,
            other => Self::Named(other.into()),
        }
    }
}

/// The three device spaces, for the `g`/`rg`/`k` family.
///
/// Table 74 is explicit that `G`, `RG` and `K` "set the stroking colour
/// space to `DeviceGray`/`DeviceRGB`/`DeviceCMYK` **and** set the colour" —
/// they are not colour-only operators. A renderer that updates the colour
/// but leaves the *space* pointing at the previous selection will
/// mis-interpret the operand count and polarity of the next `sc`/`scn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceSpace {
    /// `g` / `G` — §8.6.4.2.
    Gray,
    /// `rg` / `RG` — §8.6.4.3.
    Rgb,
    /// `k` / `K` — §8.6.4.4.
    Cmyk,
}

/// A resolved colour space (§8.6.3's three families).
///
/// Resolution happens **once, at `cs`/`CS` time** rather than lazily at
/// paint time, because the number of operands `sc`/`scn` consumes is a
/// property of the space: the stream cannot even be parsed correctly
/// without it (`color__iccbased.md`, "Gotchas").
#[derive(Debug, Clone, PartialEq)]
pub enum ColorSpace {
    /// `DeviceGray` (§8.6.4.2). One component; **0.0 is black, 1.0 white**
    /// — the same direction as RGB and the opposite of CMYK.
    DeviceGray,
    /// `DeviceRGB` (§8.6.4.3). Three additive components, 0.0–1.0.
    DeviceRgb,
    /// `DeviceCMYK` (§8.6.4.4). Four subtractive components, 0.0–1.0.
    /// Converted through the operator's chosen intent — §8.6.4.4 mandates
    /// no conversion at all, so the answer is the operator's (R169).
    DeviceCmyk,
    /// `CalGray` (§8.6.5.2, Table 63) — a calibrated one-component grey.
    CalGray {
        /// `WhitePoint` `[Xw Yw Zw]`, required by Table 63. Table 63 also
        /// constrains it: `Yw` **shall be 1.0** and `Xw`/`Zw` **shall be
        /// positive**, so `Y = A^Gamma` unconditionally.
        white: [f32; 3],
        /// `Gamma`, default 1.0; Table 63 says it "shall be positive".
        gamma: f32,
    },
    /// `CalRGB` (§8.6.5.3, Table 64) — three components through per-channel
    /// gamma and a 3×3 matrix into XYZ.
    CalRgb {
        /// `WhitePoint` `[Xw Yw Zw]`, required by Table 64.
        white: [f32; 3],
        /// `Gamma` `[Ga Gb Gc]`, default `[1 1 1]`.
        gamma: [f32; 3],
        /// `Matrix` `[XA YA ZA XB YB ZB XC YC ZC]`, default the identity.
        /// Note the layout: the array is grouped by *input* component, so
        /// `X = XA·A + XB·B + XC·C` reads entries 0, 3 and 6.
        matrix: [f32; 9],
    },
    /// `Lab` (§8.6.5.4, Table 65) — CIE 1976 L\*a\*b\*.
    Lab {
        /// `WhitePoint` `[Xw Yw Zw]`, required by Table 65.
        white: [f32; 3],
        /// `Range` `[amin amax bmin bmax]`, default `[-100 100 -100 100]`.
        /// L\* is always 0–100 and is not covered by `Range`.
        range: [f32; 4],
    },
    /// `ICCBased` (§8.6.5.5) resolved through its **specified fallback**.
    ///
    /// # This is the standard's own path, not pdfcer's invention
    ///
    /// This module contains no ICC colour-management engine.
    ///
    /// ★ That sentence used to read "pdfcer has no ICC colour-management
    /// engine and this module does not contain one", and the first half
    /// stopped being true at `Pass 199.2`, when iccce was wired in. The
    /// SECOND half is still exactly true and is the load-bearing one: colour
    /// management happens on the ink-compositing path, using the profile
    /// carried on [`Self::IccBased`], and this resolution step still produces
    /// the spec's fallback so that a page with no `/OutputIntent` — or a
    /// profile that will not parse — renders at all.
    ///
    /// §8.6.5.5 / Table 66 anticipate exactly that reader and
    /// say what it shall do, in the `Alternate` row, verbatim: an alternate
    /// colour space "**shall be used in case the one specified in the
    /// stream data is not supported**"; and "**If this entry is omitted and
    /// the conforming reader does not understand the ICC profile data, the
    /// colour space that shall be used is `DeviceGray`, `DeviceRGB`, or
    /// `DeviceCMYK`, depending on whether the value of `N` is 1, 3, or 4,
    /// respectively.**"
    ///
    /// Table 66 also settles what the fallback must *not* do: "**There
    /// shall not be conversion of source colour values, such as a tint
    /// transformation, when using the alternate colour space.**" The
    /// fallback is a **reinterpretation**, not a conversion — a 3-component
    /// `ICCBased` value `(0.2, 0.4, 0.6)` becomes `DeviceRGB (0.2, 0.4,
    /// 0.6)` unchanged, with out-of-range components clamped to the
    /// alternate's range ("the nearest values within the range of the
    /// alternate space shall be substituted").
    ///
    /// ★ **This variant's own [`Self::to_rgb`] is that reinterpretation and
    /// nothing more** — but it is no longer the whole story, and this
    /// paragraph used to say it was ("pdfcer substitutes unconditionally").
    /// Since `Pass 199.2` a paint in this space on a subtractive page is
    /// converted through its embedded profile to the output intent
    /// (`Interpreter::authored_cmyk`); since `Pass 240.0` an `N 3` paint on
    /// ANY page is converted through it to the screen
    /// (`Interpreter::display_managed_rgb`), and images take both routes
    /// through `image::Space::IccRgb`. Those conversions live on the
    /// interpreter, which holds the bridge cache and the graphics state's
    /// intent; this type holds the profile bytes for them. What is still
    /// substituted, and still counted by [`ColorDiagnostics::icc_alternate_used`]
    /// / [`ColorDiagnostics::icc_device_fallback_used`]: `N 1` and `N 4`
    /// paints on a page with no output intent, shadings and meshes in this
    /// space, and any profile iccce cannot model. The counters tick at
    /// RESOLUTION and say which fallback structure was built, not whether a
    /// paint was then managed — `Diagnostics::icc_managed_paints` answers
    /// that.
    IccBased {
        /// Table 66 `/N` — "shall be 1, 3, or 4". It is what determines how
        /// many operands `sc`/`scn` consume, which is why it is required
        /// and why a reader need never parse the profile to get it.
        n: usize,
        /// The space actually used: the resolved `/Alternate`, or the
        /// device space implied by `/N`.
        alternate: Arc<ColorSpace>,
        /// `true` if `/Alternate` was present and usable — i.e. which of
        /// the two sentences of Table 66 applied. Kept because the two
        /// fallbacks have different fidelity and an operator reading the
        /// diagnostics is entitled to know which one ran.
        alternate_explicit: bool,
        /// The **decoded ICC profile bytes** from the `ICCBased` stream
        /// itself, when they decoded.
        ///
        /// # Why this is carried at all, when `/N` and `/Alternate` already
        /// make the space paintable
        ///
        /// Because the alternate is a *fallback*, and a fallback is only
        /// correct when nobody can do better. The two fields above answer
        /// "how many operands does `sc` take" and "what do I paint if I
        /// cannot colour-manage"; neither can answer "what colour IS this",
        /// which needs the profile. Until this field existed the profile was
        /// parsed for its `/N` and then **discarded**, so a document that
        /// went to the trouble of embedding a calibrated source description
        /// was rendered as though it had not.
        ///
        /// # Why `Option`, and why that is not a defect
        ///
        /// A stream whose filters fail to decode still yields a usable space
        /// through `/N` — Table 66's fallback is *designed* to survive a
        /// profile the reader cannot read. Refusing the whole colour space
        /// because its profile was corrupt would make pdfcer strictly worse
        /// than the spec's own recovery path. `None` therefore means "render
        /// through the alternate", which is exactly what happened before
        /// this field was added.
        ///
        /// # Why `Arc<[u8]>` rather than `Vec<u8>`
        ///
        /// `ColorSpace` is cloned freely — it is resolved once per named
        /// resource and then handed to every operation that paints in it.
        /// A profile is commonly 500 B–3 kB and occasionally far larger, so
        /// a `Vec` would be memcpy'd on every clone for no benefit; the
        /// bytes are immutable once parsed.
        profile: Option<Arc<[u8]>>,
    },
    /// `[/Indexed base hival lookup]` (§8.6.6.3) — "a colour map or colour
    /// table of arbitrary colours in some other space".
    Indexed {
        /// The base space the table's entries are interpreted in. "Shall be
        /// any device or CIE-based colour space or (PDF 1.3) a `Separation`
        /// or `DeviceN` space, but shall not be a `Pattern` space or
        /// another `Indexed` space."
        base: Arc<ColorSpace>,
        /// "The **maximum valid index value**… shall be no greater than
        /// 255." A maximum, not a count: the table has `hival + 1` entries,
        /// and an off-by-one here truncates the last palette colour.
        hival: u8,
        /// The raw lookup bytes, `m × (hival + 1)` long where `m` is the
        /// base space's component count. Always 8 bits per component
        /// regardless of the base space's natural precision.
        lookup: Arc<[u8]>,
    },
    /// `[/Separation name alternateSpace tintTransform]` (§8.6.6.4).
    ///
    /// The tint transform is deliberately **not stored**: this module does
    /// not evaluate functions, and holding a function object it cannot run
    /// would invite a future reader to assume it does. See the module docs.
    Separation {
        /// The colorant this space represents, or `/All` / `/None`.
        colorant: Colorant,
        /// `tintTransform` (§7.10), loaded — one input, one output per
        /// component of `alternate`.
        ///
        /// `None` when the entry is absent or would not load. That is a
        /// malformed file (the element is Required), and the fallback is
        /// the neutral stand-in with the shortfall counted, rather than
        /// refusing a space whose colorant name is still useful.
        tint: Option<Arc<pdfcer_core::function::PdfFunction>>,
        /// `alternateSpace` — "may be any device or CIE-based colour space
        /// but may not be another special colour space", so the recursion
        /// here is exactly one level deep.
        alternate: Arc<ColorSpace>,
    },
    /// `[/DeviceN names alternateSpace tintTransform]` (+ optional
    /// `attributes`) — §8.6.6.5.
    ///
    /// `NChannel` (`/Subtype /NChannel`, PDF 1.6) is deliberately treated as
    /// plain `DeviceN`, which §8.6.6.5 explicitly blesses: "Conforming
    /// readers that do not support PDF 1.6 shall treat these colour spaces
    /// as normal `DeviceN` colour spaces and shall use the tint
    /// transformation function as appropriate", and "`alternateSpace` and
    /// `tintTransform` shall always be provided". Reading `/Colorants`,
    /// `/Process` and `/MixingHints` is optional even for `NChannel`.
    DeviceN {
        /// One entry per component, in `names`-array order — the order
        /// `scn` operands and image samples are given in.
        names: Arc<[Colorant]>,
        /// As `Separation`: any device or CIE-based space, never another
        /// special space.
        alternate: Arc<ColorSpace>,
        /// `tintTransform` (§7.10), loaded — one input per name, one
        /// output per component of `alternate`. See `Separation::tint`.
        tint: Option<Arc<pdfcer_core::function::PdfFunction>>,
    },
    /// `Pattern` (§8.6.6.2) — `/Pattern` alone, or `[/Pattern underlying]`
    /// for an uncoloured tiling pattern whose colour comes from
    /// `underlying`.
    ///
    /// Recognised, never painted. Table 74's own initial value for this
    /// space is "a pattern object that causes nothing to be painted", so
    /// painting nothing is the spec's own degradation rather than an
    /// invented one — but painting nothing where the document asked for a
    /// gradient or a hatch is still a visible shortfall, so it is counted
    /// ([`ColorDiagnostics::patterns_unpainted`]).
    Pattern {
        /// The underlying space of an uncoloured tiling pattern, if the
        /// array form was used.
        underlying: Option<Arc<ColorSpace>>,
    },
}

impl ColorSpace {
    /// Number of colour components `sc`/`scn` consumes for this space
    /// (§8.6, "Operand counts").
    ///
    /// `Pattern` returns 0: its operand is a *name*, and for an uncoloured
    /// tiling pattern the leading numbers belong to the underlying space,
    /// not to `Pattern` itself.
    #[must_use]
    pub fn components(&self) -> usize {
        match self {
            Self::DeviceGray | Self::CalGray { .. } | Self::Indexed { .. } => 1,
            Self::DeviceRgb | Self::CalRgb { .. } | Self::Lab { .. } => 3,
            Self::DeviceCmyk => 4,
            Self::IccBased { n, .. } => *n,
            Self::Separation { .. } => 1,
            Self::DeviceN { names, .. } => names.len(),
            Self::Pattern { .. } => 0,
        }
    }

    /// Valid range of component `i`, used for two things the spec ties
    /// together: clamping the initial colour into range, and scaling an
    /// `Indexed` lookup byte ("0 corresponds to the minimum value in the
    /// range for that component, and 255 corresponds to the maximum",
    /// §8.6.6.3).
    ///
    /// Everything is `0.0..=1.0` except `Lab` (L\* is 0–100, a\*/b\* come
    /// from `/Range`) and `Indexed` (0..=`hival`).
    #[must_use]
    pub fn component_range(&self, i: usize) -> (f32, f32) {
        match self {
            Self::Lab { range, .. } => {
                let [amin, amax, bmin, bmax] = *range;
                match i {
                    0 => (0.0, 100.0),
                    1 => (amin, amax),
                    _ => (bmin, bmax),
                }
            }
            Self::Indexed { hival, .. } => (0.0, f32::from(*hival)),
            // Table 66's `/Range` defaults to [0 1]×N and pdfcer does not
            // read it: the alternate space's own range is what the
            // substitution clamps to, which is this arm.
            Self::IccBased { alternate, .. } => alternate.component_range(i),
            _ => (0.0, 1.0),
        }
    }

    /// The colour `cs`/`CS` installs when it selects this space.
    ///
    /// §8.6.8 is explicit that the operator "shall **also** set the current
    /// stroking colour to its initial value", and the values are *not*
    /// uniform — this is the trap the clause exists to close:
    ///
    /// | Space | Initial |
    /// |---|---|
    /// | `DeviceGray`, `DeviceRGB`, `CalGray`, `CalRGB` | all 0.0 (black) |
    /// | `DeviceCMYK` | `[0 0 0 1]` — **black**, where all-zeros would be white |
    /// | `Lab`, `ICCBased` | all 0.0, clamped into range |
    /// | `Indexed` | 0 |
    /// | `Separation`, `DeviceN` | **1.0** per component — full colorant, the *darkest* value |
    /// | `Pattern` | "a pattern object that causes nothing to be painted" (empty) |
    #[must_use]
    pub fn initial_color(&self) -> Vec<f32> {
        match self {
            Self::DeviceCmyk => vec![0.0, 0.0, 0.0, 1.0],
            Self::Separation { .. } => vec![1.0],
            Self::DeviceN { names, .. } => vec![1.0; names.len()],
            Self::Pattern { .. } => Vec::new(),
            other => (0..other.components())
                .map(|i| {
                    let (lo, hi) = other.component_range(i);
                    // "unless that falls outside the space's Range, in
                    // which case the nearest valid value" (§8.6.8).
                    clamp_between(0.0, lo, hi)
                })
                .collect(),
        }
    }

    /// Would painting in this space put marks on the page at all?
    ///
    /// `false` only where the standard or pdfcer's own limits say nothing is
    /// drawn: a `Pattern` space (which has no solid colour to paint — the
    /// pattern itself is drawn by the interpreter's own route), a
    /// `Separation /None` ("shall have no effect on the current page"), and
    /// a `DeviceN` whose components are **all** `/None` ("shall always
    /// discard its output … shall never revert to the alternate colour
    /// space", §8.6.6.5).
    ///
    /// A partially-`None` `DeviceN` still paints: §8.6.6.5 says reversion
    /// happens if at least one non-`None` component exists, and the `None`
    /// components are still passed to the transform.
    #[must_use]
    pub fn paints(&self) -> bool {
        match self {
            Self::Pattern { .. } => false,
            Self::Separation { colorant, .. } => *colorant != Colorant::None,
            Self::DeviceN { names, .. } => !names.iter().all(|c| *c == Colorant::None),
            _ => true,
        }
    }

    /// Convert a colour in this space to sRGB for painting.
    ///
    /// Returns `None` only where there is deliberately nothing to paint
    /// (`Pattern`, `Separation /None`, an all-`None` `DeviceN`) — the caller
    /// leaves the graphics state's colour alone in that case, because the
    /// paint is suppressed by [`ColorSpace::paints`] instead.
    ///
    /// Missing components read as 0.0 rather than panicking: a malformed
    /// operand run is a tolerated structural oddity in this renderer, not a
    /// reason to abandon the page (§7.8.2 calls it an error; a viewer that
    /// gives up over one is conformant and useless).
    /// The **authored colorants** for `comps`, when this space has some —
    /// the colorant-preserving twin of [`Self::to_rgb`].
    ///
    /// # Why this exists, and what it is NOT
    ///
    /// [`Self::to_rgb`] is lossy by construction: it answers *"what does
    /// this look like on a screen?"*, and once answered there is no way
    /// back to which inks were asked for. That is fine for painting and
    /// fatal for **overprint**, which §11.7.4.3 defines in terms of *"all
    /// colour components **specified in the current colour space**"* — a
    /// question about the authored space, not about the appearance.
    ///
    /// So this returns `Some([c, m, y, k])` only where those four numbers
    /// are the ones the *file* asked for:
    ///
    /// * `DeviceCmyk` — the components themselves.
    /// * `Separation` / `DeviceN` **whose alternate resolves to
    ///   `DeviceCmyk`** — the tint transform's own output, taken *before*
    ///   anything converts it. This is the case suite `PCS 1.0` exercises:
    ///   `/DeviceN [/Cyan /Magenta]` over a `DeviceCMYK` alternate.
    ///
    /// **`None` everywhere else, and `None` is not a failure** — it is the
    /// honest answer that this space has no colorants to preserve, and the
    /// caller must fall back to the `to_rgb` route rather than invent
    /// four numbers. In particular `DeviceGray` and `DeviceRgb` return
    /// `None`: a grey *could* be mapped to `K` and an RGB *could* be
    /// mapped through a conversion, but neither is what the file
    /// specified, and §11.7.4.3 would then let pdfcer claim components the
    /// document never named. That is the exact error this function exists
    /// to avoid, so it is refused rather than approximated.
    ///
    /// # `Colorant::All` and `Colorant::None`
    ///
    /// Both return `None` here. `/None` paints nothing at all (the caller
    /// suppresses it upstream), and `/All` means *"every colorant at this
    /// tint"* — which is a statement about the output device's full ink
    /// set, not about four specific values, and pdfcer's four-plane buffer
    /// cannot express it faithfully. Answering `None` keeps that gap
    /// visible instead of silently rendering `/All` as four equal inks.
    #[must_use]
    pub fn to_cmyk(&self, comps: &[f32], diag: &mut ColorDiagnostics) -> Option<[f32; 4]> {
        match self {
            Self::DeviceCmyk => Some([
                comp(comps, 0),
                comp(comps, 1),
                comp(comps, 2),
                comp(comps, 3),
            ]),
            Self::Separation {
                colorant,
                alternate,
                tint,
            } => {
                if matches!(colorant, Colorant::None | Colorant::All) {
                    return None;
                }
                tint_to_cmyk(tint.as_deref(), alternate, comps, diag)
            }
            Self::DeviceN {
                names,
                alternate,
                tint,
            } => {
                if names.iter().all(|c| matches!(c, Colorant::None)) {
                    return None;
                }
                tint_to_cmyk(tint.as_deref(), alternate, comps, diag)
            }
            // `Indexed` recurses into its base for the same reason
            // `BlendSpace::of` does: §8.6.6.3 puts the colour values in the
            // BASE space, so asking the index is a category error.
            Self::Indexed { .. } => {
                let (base, entry) = self.indexed_entry(comps)?;
                base.to_cmyk(&entry, diag)
            }
            _ => None,
        }
    }

    /// The colour as **CIE XYZ relative to the ICC profile connection
    /// space's D50 white**, for the three CIE-based families
    /// (`Pass 242.0`).
    ///
    /// # Why this exists
    ///
    /// A `Lab`, `CalRGB` or `CalGray` colour has no colorants and no
    /// embedded profile, so on a page that composites in ink it used to
    /// reach the colorant buffer by the worst route available: to sRGB
    /// through [`xyz_to_srgb`], then back to four inks through the
    /// max-GCR `rgb_to_cmyk` round trip. A colorimetric colour was thereby
    /// separated by a formula that knows nothing about the press. Measured
    /// on a print-conformance patch: a `Lab (60, 0, 0)` backdrop separated
    /// to `K = 0.43` alone, where the document's own output intent separates
    /// it to roughly `(0.38, 0.31, 0.31, 0.18)` — and a `ColorBurn` over the
    /// K-only version burned to solid black.
    ///
    /// The document's `/OutputIntent` destination profile IS a separation
    /// engine for exactly this input: its B2A table maps PCS values to
    /// device ink. This method produces the PCS value; `IccBridgeCache::
    /// pcs_to_ink` runs the table. So a CIE colour on an ink page takes the
    /// same route Acrobat takes for it, and the same route an `ICCBased`
    /// paint already takes here — profile connection space in, ink out.
    ///
    /// # The white point
    ///
    /// Each space declares its own `/WhitePoint` (Tables 63–65), and the
    /// XYZ these decoders produce is relative to it. The PCS is defined at
    /// D50 (ICC.1 §7.2.16), so the result is Bradford-adapted from the
    /// declared white to D50 — the same adaptation [`xyz_to_srgb`] performs
    /// towards D65 for the screen. For the overwhelmingly common
    /// D50-declared space the adaptation is the identity.
    ///
    /// `None` for every other family: a device or `ICCBased` space has its
    /// own route, and a `Separation`/`DeviceN` answers through its
    /// alternate. `Indexed` is resolved by the caller through
    /// [`Self::indexed_entry`] first, exactly as [`Self::to_cmyk`]'s callers
    /// do.
    #[must_use]
    pub fn to_pcs_xyz(&self, comps: &[f32]) -> Option<[f32; 3]> {
        let (xyz, white) = match self {
            Self::CalGray { white, gamma } => {
                (cal_gray_to_xyz(comp(comps, 0), *white, *gamma), *white)
            }
            Self::CalRgb {
                white,
                gamma,
                matrix,
            } => {
                let abc = [comp(comps, 0), comp(comps, 1), comp(comps, 2)];
                (cal_rgb_to_xyz(abc, *gamma, matrix), *white)
            }
            Self::Lab { white, range } => {
                let [amin, amax, bmin, bmax] = *range;
                let l = comp(comps, 0).clamp(0.0, 100.0);
                let a = clamp_between(comp(comps, 1), amin, amax);
                let b = clamp_between(comp(comps, 2), bmin, bmax);
                (lab_to_xyz([l, a, b], *white), *white)
            }
            _ => return None,
        };
        Some(bradford_adapt(xyz, white, PCS_D50))
    }

    #[must_use]
    pub fn to_rgb(
        &self,
        comps: &[f32],
        intent: CmykIntent,
        diag: &mut ColorDiagnostics,
    ) -> Option<Rgb> {
        match self {
            Self::DeviceGray => Some(Rgb::from_gray(comp(comps, 0))),
            Self::DeviceRgb => Some(Rgb::from_rgb(
                comp(comps, 0),
                comp(comps, 1),
                comp(comps, 2),
            )),
            Self::DeviceCmyk => Some(Rgb::from_cmyk(
                intent,
                comp(comps, 0),
                comp(comps, 1),
                comp(comps, 2),
                comp(comps, 3),
            )),
            Self::CalGray { white, gamma } => Some(xyz_to_srgb(
                cal_gray_to_xyz(comp(comps, 0), *white, *gamma),
                *white,
            )),
            Self::CalRgb {
                white,
                gamma,
                matrix,
            } => {
                let abc = [comp(comps, 0), comp(comps, 1), comp(comps, 2)];
                Some(xyz_to_srgb(cal_rgb_to_xyz(abc, *gamma, matrix), *white))
            }
            Self::Lab { white, range } => {
                let [amin, amax, bmin, bmax] = *range;
                let l = comp(comps, 0).clamp(0.0, 100.0);
                let a = clamp_between(comp(comps, 1), amin, amax);
                let b = clamp_between(comp(comps, 2), bmin, bmax);
                Some(xyz_to_srgb(lab_to_xyz([l, a, b], *white), *white))
            }
            // §8.6.5.5: reinterpretation, NOT conversion. The components
            // pass through untouched except for the mandated clamp into the
            // alternate's range.
            Self::IccBased { alternate, .. } => {
                let clamped: Vec<f32> = (0..alternate.components())
                    .map(|i| {
                        let (lo, hi) = alternate.component_range(i);
                        clamp_between(comp(comps, i), lo, hi)
                    })
                    .collect();
                alternate.to_rgb(&clamped, intent, diag)
            }
            Self::Indexed {
                base,
                hival,
                lookup,
            } => Some(indexed_to_rgb(
                comp(comps, 0),
                base,
                *hival,
                lookup,
                intent,
                diag,
            )),
            Self::Separation {
                colorant,
                alternate,
                tint,
            } => separation_to_rgb(
                colorant,
                alternate,
                tint.as_deref(),
                comp(comps, 0),
                intent,
                diag,
            ),
            Self::DeviceN {
                names,
                alternate,
                tint,
            } => device_n_to_rgb(names, alternate, tint.as_deref(), comps, intent, diag),
            Self::Pattern { .. } => None,
        }
    }

    /// §8.6.6.3 — resolve an `Indexed` selection to the colour values it
    /// **selects**, expressed in the base space.
    ///
    /// # Why anything that reasons about COLORANTS must call this first
    ///
    /// An `Indexed` operand is an **index into a table**, not a colour.
    /// §8.6.6.3 is explicit that the values live in the base space:
    /// *"a colour map or colour table of arbitrary colours in some other
    /// space"*. Everything that paints already goes through
    /// [`Self::to_rgb`], which does the lookup internally and therefore
    /// never sees the problem — so the defect this method exists for is
    /// invisible on screen and shows up only where something asks a
    /// question about the space itself.
    ///
    /// Overprint is exactly that. §11.7.4.3's Table 149 keys on **which
    /// colorants the source names**, and an `/Indexed [/DeviceN [/Cyan]
    /// /DeviceCMYK …]` space names one. Without this resolution the
    /// classifier sees `Indexed`, falls to "some other process space", and
    /// Table 149 decides what survives from a colorant list it never read.
    /// suite's `PCS1_190` is authored on precisely that discriminator —
    /// its a/b pair's `DeviceN` **omits** the backdrop's colorants and its
    /// c/d pair **includes** them at 0 %, and the patch's own ReadMe says
    /// the colorant LIST, not the tint values, decides the outcome.
    ///
    /// ★ **`/Indexed` IS PRESENT IN FOUR OF THE SUITE'S OVERPRINT PATCHES
    /// AND IS NO LONGER INERT.** This paragraph carried a measured
    /// negative from 2026-08-21 — *"REACHABLE in none of them: every one of
    /// those spaces is an image colour space, `overprint::composite` has no
    /// image call site, and pre- and post-fix binaries report identical
    /// overprint counters on all four"* — and every word of it was true when
    /// written. `Pass 130.2` built the image call site
    /// (`Canvas::fill_image_overprint`), and three of those four patches
    /// went from FAIL to pass on the strength of this arm reading the
    /// `/Indexed` base's colorant list. The claim is corrected rather than
    /// deleted because the SHAPE of it recurs: *present in the file* and
    /// *reachable by the renderer* are different claims, and the first reads
    /// as the second unless it says so.
    ///
    /// ★★ **It was the THIRD copy of that claim and the last one
    /// corrected**, which is worth more than either correction. The other
    /// two are in `crate::overprint` — the `classify` arm and its test. This
    /// one was missed because **the sweep's boundary was the file and the
    /// claim's boundary was the feature**, and this is the definition site of
    /// the function the whole fix hangs on: the one place a reader arrives at
    /// without meaning to.
    ///
    /// # Returns
    ///
    /// `None` when `self` is not `Indexed` — the caller keeps what it had,
    /// which makes this safe to call unconditionally. `Some((base, comps))`
    /// otherwise, with `comps` the palette entry mapped into the base
    /// space's own component ranges.
    ///
    /// An index outside `0..=hival` is **clamped**, which §8.6.6.3
    /// requires; a lookup table shorter than the entry implies yields the
    /// base space's all-zero components. Neither case touches
    /// [`ColorDiagnostics`] — this is a *classification* helper, and
    /// [`Self::to_rgb`] already counts both on the painting path. Counting
    /// them twice would make one malformed palette read as two separate
    /// findings.
    #[must_use]
    pub fn indexed_entry(&self, comps: &[f32]) -> Option<(&Self, Vec<f32>)> {
        let Self::Indexed {
            base,
            hival,
            lookup,
        } = self
        else {
            return None;
        };
        let index = comps.first().copied().unwrap_or(0.0);
        // §8.6.6.3: "shall be … clamped to the range 0 to hival".
        let clamped = index.round().clamp(0.0, f32::from(*hival));
        let m = base.components();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let offset = (clamped as usize).saturating_mul(m);
        let entry = lookup.get(offset..offset.saturating_add(m));
        let out = (0..m)
            .map(|i| {
                let byte = entry.and_then(|e| e.get(i)).copied().unwrap_or(0);
                let (lo, hi) = base.component_range(i);
                // The table is ALWAYS 8 bits per component whatever the
                // base space's natural precision is (§8.6.6.3), so the
                // byte is a fraction of the base component's own range,
                // not a value in it. That matters for `Lab`, whose `a`/`b`
                // range is negative at one end.
                (byte as f32 / 255.0).mul_add(hi - lo, lo)
            })
            .collect();
        Some((base.as_ref(), out))
    }

    /// This space's **neutral** rendering of a subtractive tint in
    /// `0.0..=1.0`, where 0.0 is the lightest achievable colour and 1.0 the
    /// darkest (§8.6.6.4's polarity, which is CMYK's and the opposite of
    /// `DeviceGray`/`DeviceRGB`'s).
    ///
    /// This is the stand-in used while the §7.10 function evaluator is
    /// unavailable, and it is a **stand-in, not an approximation of the
    /// document's intent**: a real tint transform maps one tint onto a
    /// specific hue, and pdfcer cannot know that hue without running the
    /// function. What this preserves is only the ordering — 0 is paper,
    /// 1 is fully inked — so a spot-coloured drawing reads correctly as
    /// line-work rather than vanishing or turning solid black at every
    /// tint. Every use is counted in
    /// [`ColorDiagnostics::tint_transform_not_applied`].
    ///
    /// Hue is deliberately *not* guessed. Choosing, say, "full tint of an
    /// unknown colorant is red" would be exactly the invented value rule 4
    /// forbids.
    #[must_use]
    pub fn neutral_from_tint(&self, tint: f32) -> Vec<f32> {
        let t = tint.clamp(0.0, 1.0);
        match self {
            Self::DeviceCmyk => vec![0.0, 0.0, 0.0, t],
            Self::DeviceGray | Self::CalGray { .. } => vec![1.0 - t],
            Self::DeviceRgb | Self::CalRgb { .. } => vec![1.0 - t; 3],
            Self::Lab { .. } => vec![100.0 * (1.0 - t), 0.0, 0.0],
            Self::IccBased { alternate, .. } => alternate.neutral_from_tint(t),
            // The alternate of a Separation/DeviceN "may not be another
            // special colour space", so these arms are unreachable for a
            // conformant file; a neutral grey is the honest answer for a
            // non-conformant one.
            _ => vec![1.0 - t],
        }
    }
}

/// Counted disclosures from colour handling — the "fuzzy, never sneaky"
/// half of this module.
///
/// Every field answers a question an operator actually asks about a page
/// that looks wrong, and they are deliberately **not** lumped: "the colour
/// space would not resolve", "this ICCBased space took its /Alternate rather
/// than being colour-managed" and "the spot colour's tint transform was not
/// evaluated" lead to three different next actions (rule R27).
///
/// ★ The middle phrase used to read "pdfcer has no ICC engine", which became
/// false at `Pass 199.2`. It is corrected rather than deleted because the
/// DISTINCTION it was drawing survives: a space taking its `/Alternate` is
/// still a different diagnosis from a space that failed to resolve, and the
/// counters here still separate them. Only the reason changed — from "the
/// engine does not exist" to "the engine did not run for this space".
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ColorDiagnostics {
    /// `cs`/`CS` named a colour space pdfcer could **not** resolve — a name
    /// absent from the resource `/ColorSpace` subdictionary, a malformed
    /// array, an unknown family, or nesting past
    /// [`MAX_COLOR_SPACE_DEPTH`].
    ///
    /// The space is left **unset**, deliberately: an unresolvable space
    /// must not silently become `DeviceGray`, because that would paint
    /// black marks that look exactly like a correct render. Subsequent
    /// `sc`/`scn` in that state change nothing and are counted in
    /// [`Self::colors_not_set`].
    ///
    /// Counts **distinct resource names**, not occurrences — the same
    /// policy as [`crate::Diagnostics::fonts_unsupported`], and for the
    /// same reason: one broken resource used ten thousand times is one
    /// problem.
    pub spaces_unresolved: usize,
    /// `sc`/`scn`/`SC`/`SCN` that could not be turned into a colour, so the
    /// **previous colour stayed in force**: the space was unresolved, or
    /// the operand count did not match the space's component count.
    ///
    /// Counts occurrences, because each one is a mark potentially painted
    /// in a stale colour — which is the exact defect this module was
    /// written to end, and the honest report of the residue is a number.
    pub colors_not_set: usize,
    /// `ICCBased` spaces rendered through an explicit `/Alternate`
    /// (§8.6.5.5, Table 66). Distinct resolutions, not paints.
    ///
    /// Not a shortfall counter so much as a fidelity one: this is the
    /// spec's own fallback and it is visually close for the sRGB-like
    /// profiles that dominate real files, but pdfcer is not colour-managing
    /// and an operator chasing a colour difference should know.
    pub icc_alternate_used: usize,
    /// `ICCBased` spaces with **no** usable `/Alternate`, rendered as the
    /// device space implied by `/N` (1 → Gray, 3 → RGB, 4 → CMYK) — the
    /// second sentence of Table 66's `Alternate` row. Distinct resolutions.
    pub icc_device_fallback_used: usize,
    /// Colour conversions in a `Separation` or `DeviceN` space where the
    /// **tint transform was not applied** — because the document's
    /// `/tintTransform` is absent, malformed, or of the wrong arity for
    /// the space's component count.
    ///
    /// This counts a property of the **file**, not a gap in pdfcer: the
    /// §7.10 evaluator ([`pdfcer_core::function`]) is wired into both
    /// [`separation_to_rgb`] and [`device_n_to_rgb`], and a conformant
    /// space paints the document's own colour and increments
    /// [`Self::tint_transforms_applied`] instead. The field doc formerly
    /// said "because pdfcer has no §7.10 function evaluator yet", which
    /// stayed behind when the evaluator landed.
    ///
    /// The colour painted is [`ColorSpace::neutral_from_tint`] in the
    /// alternate space: right lightness, **wrong hue**. A non-zero value
    /// here on a drawing whose spot colours look grey is the explanation.
    pub tint_transform_not_applied: usize,
    /// Tint transforms that WERE evaluated (§7.10). The positive twin of
    /// `tint_transform_not_applied` — reported so a shell can say the
    /// spot colours on a page are the document's own and not pdfcer's
    /// stand-in, which is the difference an operator checking a brand
    /// colour needs.
    pub tint_transforms_applied: usize,
    /// `Separation /All` conversions. §8.6.6.4 says painting "shall apply
    /// tint values to all available colorants at once"; on an additive
    /// display pdfcer renders that as a neutral of luminance `1 − tint`,
    /// which is a **choice** (the standard describes an ink behaviour, not
    /// a screen appearance), so it is disclosed rather than assumed.
    pub separation_all_approximated: usize,
    /// Paint operations suppressed because the colour space was
    /// `Separation /None` or an all-`/None` `DeviceN` (§8.6.6.4/.5).
    ///
    /// A census, not a shortfall: this is pdfcer obeying the standard. It is
    /// counted because a page that is missing content for a *conformant*
    /// reason is otherwise indistinguishable from one that failed, which is
    /// exactly the distinction the diagnostics exist to make.
    pub separation_none_suppressed: usize,
    /// `cs`/`CS` selections of a `Pattern` colour space.
    pub pattern_spaces_selected: usize,
    /// `scn`/`SCN` operations that named a **pattern pdfcer did not
    /// paint** — the REMAINDER after shading patterns are painted.
    ///
    /// ★ This read "which pdfcer does not paint (tiling and shading
    /// patterns, §8.7, are later work)" long after `PatternType 2` shading
    /// patterns started painting. A comment ~300 lines below in this same
    /// file exists **specifically to refute that sentence** ("NOT 'pdfcer
    /// does not paint patterns' — it does, for `PatternType 2`"), and the
    /// CLI's own stderr note already described this counter correctly. So
    /// three copies of the fact existed, two right and one wrong, and the
    /// wrong one was the field's own doc — the copy every consumer reads
    /// first. `R212`.
    ///
    /// What it now counts: TILING patterns, a `/Pattern` name with no
    /// matching entry, and any shading pattern refused for its own reason.
    /// Nothing is drawn in their place — deliberately, since Table 74's
    /// initial `Pattern` colour "causes nothing to be painted" and an
    /// invented solid fill would be worse than a gap.
    pub patterns_unpainted: usize,
    /// `Indexed` lookups whose index was outside `0..=hival` and was
    /// clamped. The clamp is **normative** (§8.6.6.3: "if it is outside the
    /// range 0 to `hival`, it shall be adjusted to the nearest value within
    /// that range") and it is a clamp, not a modulo — so this is a census
    /// of a conformant behaviour, useful only because a stream that keeps
    /// hitting it is usually a producer bug.
    pub indexed_index_clamped: usize,
    /// `Indexed` lookups that fell past the end of a **short** lookup
    /// table. Producers routinely trim trailing unused entries, so this is
    /// tolerated rather than fatal; the colour painted is black, and it is
    /// counted so a wrongly-black palette entry has a named cause.
    pub indexed_lookup_short: usize,
    /// First few distinct human-readable reasons, capped at
    /// [`MAX_NOTES`] — named separately from
    /// [`crate::Diagnostics::sample_ops`] because "which colour space would
    /// not resolve?" is a different operator question from "which operator
    /// is missing?".
    pub notes: Vec<String>,
}

impl ColorDiagnostics {
    /// Record a distinct reason for the notes list.
    pub(crate) fn note(&mut self, reason: &str) {
        if self.notes.len() < MAX_NOTES && !self.notes.iter().any(|s| s == reason) {
            self.notes.push(reason.to_owned());
        }
    }

    /// Fold a nested form XObject's colour diagnostics into this one.
    ///
    /// Additive for the same reason [`crate::Diagnostics::merge`] is: every
    /// counter answers a "how many, on this page" question, and the page
    /// includes whatever its forms painted.
    pub fn merge(&mut self, other: Self) {
        self.spaces_unresolved += other.spaces_unresolved;
        self.colors_not_set += other.colors_not_set;
        self.icc_alternate_used += other.icc_alternate_used;
        self.icc_device_fallback_used += other.icc_device_fallback_used;
        self.tint_transform_not_applied += other.tint_transform_not_applied;
        self.tint_transforms_applied += other.tint_transforms_applied;
        self.separation_all_approximated += other.separation_all_approximated;
        self.separation_none_suppressed += other.separation_none_suppressed;
        self.pattern_spaces_selected += other.pattern_spaces_selected;
        self.patterns_unpainted += other.patterns_unpainted;
        self.indexed_index_clamped += other.indexed_index_clamped;
        self.indexed_lookup_short += other.indexed_lookup_short;
        for note in other.notes {
            self.note(&note);
        }
    }
}

/// One half of the colour state — stroking or non-stroking.
///
/// The two are fully independent (§8.6: the uppercase/lowercase operator
/// pairs map onto them one-for-one), so they are the same type twice rather
/// than a struct with doubled fields.
#[derive(Debug, Clone)]
struct Half {
    /// The current space, or `None` when `cs`/`CS` failed to resolve one.
    /// `None` is **not** `DeviceGray`: it means "pdfcer does not know what
    /// the components mean", and the only honest response to a subsequent
    /// `sc` is to change nothing and say so.
    space: Option<Arc<ColorSpace>>,
    /// Whether painting in the current colour puts marks on the page at all
    /// ([`ColorSpace::paints`]). Part of the graphics state, so it is saved
    /// and restored by `q`/`Q` along with everything else here.
    paints: bool,
    /// The `/Pattern` resource name the last `scn`/`SCN` named, if any
    /// (§8.6.6.2, Table 74).
    ///
    /// Kept as the NAME rather than a resolved pattern for two reasons.
    /// The resource dictionary a name resolves against belongs to the
    /// content stream being interpreted, and a form XObject's `scn` names
    /// the FORM's `/Pattern` sub-dictionary — resolving eagerly here would
    /// bind the name against whichever resources happened to be current
    /// when the colour was set. And a pattern that is set and never used
    /// costs nothing this way, which matters because setting a pattern
    /// colour is cheap and common while resolving one is neither.
    ///
    /// It lives in `Half` so `q`/`Q` save and restore it with the rest of
    /// the colour state, which is what §8.4.2 requires of the colour and
    /// therefore of the pattern that stands in for it.
    pattern: Option<std::sync::Arc<[u8]>>,
    /// The operands of the last colour-setting operator, in the current
    /// space's own component order — NOT converted to sRGB.
    ///
    /// # Why the components are kept and not just the resulting `Rgb`
    ///
    /// Overprint is defined per COMPONENT. §11.7.4.3's `CompatibleOverprint`
    /// blend chooses, for each colour component independently, between the
    /// source value and the backdrop value; §8.6.7's overprint mode 1 makes
    /// that choice depend on whether the component's own value is zero. An
    /// sRGB triple has no DeviceCMYK components to choose between, so a
    /// renderer that converts at colour-set time has destroyed the
    /// information overprint operates on before painting begins.
    ///
    /// Kept in `Half` rather than beside the paint so `q`/`Q` save and
    /// restore them with the rest of the colour state (§8.4.2), which is
    /// the same argument that puts `space` and `pattern` here.
    ///
    /// Empty when the space did not resolve or the operand count disagreed
    /// with it — in both cases pdfcer refused to change the colour, so there
    /// is no source component to record.
    components: Vec<f32>,
}

impl Default for Half {
    /// §8.6.4 / Table 52: the initial colour space is `DeviceGray` and the
    /// initial colour is black, in both the stroking and non-stroking
    /// halves.
    fn default() -> Self {
        Self {
            space: Some(Arc::new(ColorSpace::DeviceGray)),
            paints: true,
            pattern: None,
            // Table 52: the initial colour is black, which in DeviceGray is
            // the single component 0.0 — not an empty operand list.
            components: vec![0.0],
        }
    }
}

/// The colour-space half of the graphics state, with its own `q`/`Q` stack
/// and a per-content-stream resolution cache.
///
/// # Why the stack is here and not in [`crate::gstate::GraphicsState`]
///
/// It belongs in the graphics state conceptually (Table 52), and one day it
/// may move there. It lives here today because the *colour* — the thing
/// that actually paints — already lives in `GraphicsState` and is already
/// saved and restored correctly; what this adds is the interpretation
/// context for the next `sc`, which no existing field carries. Keeping it
/// in one self-contained type means the interpreter's `q`/`Q` arms gain one
/// line each instead of the graphics state gaining three fields that every
/// `q` would then clone.
#[derive(Debug, Clone, Default)]
pub struct ColorState {
    fill: Half,
    stroke: Half,
    stack: Vec<(Half, Half)>,
    /// Resolved spaces keyed by resource name.
    ///
    /// Resolution walks arrays, resolves indirect references and may decode
    /// a lookup stream, while a content stream re-selects the same few
    /// spaces constantly — the same argument that gave `Tf` a cache. The
    /// cache also makes [`ColorDiagnostics::spaces_unresolved`] count
    /// DISTINCT names, which is what that counter means.
    ///
    /// Scoped to one content stream, never shared with a nested form: the
    /// key is a resource *name*, and `/CS0` in a form's `/Resources` is a
    /// different space from `/CS0` on the page.
    cache: HashMap<Vec<u8>, Option<Arc<ColorSpace>>>,
}

impl ColorState {
    /// The §8.6.4 initial state: `DeviceGray` in both halves.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `q` — push a copy of both halves (Table 52: the colour space is a
    /// graphics-state parameter).
    ///
    /// Unbounded here on purpose: the interpreter calls this only when
    /// [`crate::gstate::GStateStack::push`] succeeded, so
    /// [`crate::gstate::MAX_Q_DEPTH`] already bounds it, and a second,
    /// independent limit could only ever desynchronise the two stacks.
    pub fn push(&mut self) {
        self.stack.push((self.fill.clone(), self.stroke.clone()));
    }

    /// `Q` — restore. A no-op on an empty stack, matching the interpreter's
    /// tolerance of an unbalanced `Q` (the spec says balanced; producers
    /// disagree).
    pub fn pop(&mut self) {
        if let Some((fill, stroke)) = self.stack.pop() {
            self.fill = fill;
            self.stroke = stroke;
        }
    }

    /// Does painting in the current colour put marks on the page?
    ///
    /// Consulted by the interpreter at each fill and each stroke, which is
    /// why it is split by `stroking` rather than being one flag: a stream
    /// may legitimately fill in a real colour while stroking in
    /// `Separation /None`, and `B` paints both halves in one operator.
    #[must_use]
    pub fn paints(&self, stroking: bool) -> bool {
        if stroking {
            self.stroke.paints
        } else {
            self.fill.paints
        }
    }

    /// The `/Pattern` resource name selected in one half, if the current
    /// colour is a pattern (§8.6.6.2).
    ///
    /// `Some` here and [`ColorState::paints`] `false` are the same fact
    /// seen from two sides: there is no solid colour to fill with, and
    /// there is a pattern the paint site should try to draw instead.
    #[must_use]
    pub fn pattern(&self, stroking: bool) -> Option<&[u8]> {
        let half = if stroking { &self.stroke } else { &self.fill };
        half.pattern.as_deref()
    }

    /// The current space and the source colour's own components for one
    /// half — what `CompatibleOverprint` needs and `Rgb` cannot supply.
    ///
    /// `None` when no colour is resolvable (unresolved space, refused
    /// operand count, or a pattern selection, which has components in no
    /// device space at all).
    #[must_use]
    pub fn device_color(&self, stroking: bool) -> Option<(&ColorSpace, &[f32])> {
        let half = if stroking { &self.stroke } else { &self.fill };
        if half.pattern.is_some() || half.components.is_empty() {
            return None;
        }
        Some((half.space.as_deref()?, &half.components))
    }

    /// The current space of one half, for tests and diagnostics surfaces.
    #[must_use]
    pub fn space(&self, stroking: bool) -> Option<&ColorSpace> {
        let half = if stroking { &self.stroke } else { &self.fill };
        half.space.as_deref()
    }

    /// `g`/`G`, `rg`/`RG`, `k`/`K` — set the space **and** signal that the
    /// caller is setting the colour itself (Table 74: these operators do
    /// both).
    ///
    /// The colour conversion stays at the operator, where it already was;
    /// what this fixes is the *space*, so that a later `sc` is interpreted
    /// in the device space the document just selected rather than in a
    /// `Separation` space three operators upstream.
    pub fn set_device(&mut self, space: DeviceSpace, comps: &[f32], stroking: bool) {
        let resolved = Arc::new(match space {
            DeviceSpace::Gray => ColorSpace::DeviceGray,
            DeviceSpace::Rgb => ColorSpace::DeviceRgb,
            DeviceSpace::Cmyk => ColorSpace::DeviceCmyk,
        });
        let half = if stroking {
            &mut self.stroke
        } else {
            &mut self.fill
        };
        half.space = Some(resolved);
        half.paints = true;
        // `g`/`rg`/`k` set the space AND the colour in one operator, so the
        // components arrive here rather than through `set`. Recording them
        // is what lets the paint site answer `CompatibleOverprint`'s
        // per-component question; without it the commonest colour operators
        // in any PDF would be exactly the ones overprint could not see.
        half.components.clear();
        half.components.extend_from_slice(comps);
        // A new space replaces any pattern selection (§8.6.8: the operator
        // "shall also set the current colour to its initial value"). Not
        // clearing this is how a `scn /P1` followed by a `cs` naming a
        // NON-painting space (`Separation /None`) would paint the old
        // gradient where the standard requires nothing at all.
        half.pattern = None;
    }

    /// `cs` / `CS` — select a colour space by name and install its initial
    /// colour (§8.6.8).
    ///
    /// Returns the colour the caller should write into the graphics state,
    /// or `None` when there is nothing honest to write: the space did not
    /// resolve (leave the previous colour, and it is counted), or the space
    /// paints nothing.
    ///
    /// The name resolution follows §8.6 exactly, including the rule that
    /// makes it *unconditional*: "The names `DeviceGray`, `DeviceRGB`,
    /// `DeviceCMYK`, and `Pattern` **always** identify the corresponding
    /// colour spaces directly; they **never** refer to resources in the
    /// `ColorSpace` subdictionary." A resource named `/DeviceRGB` is
    /// unreachable, and looking it up would be a bug.
    pub fn select(
        &mut self,
        doc: &DocumentView<'_>,
        resources: &Dict,
        name: Option<&[u8]>,
        stroking: bool,
        intent: CmykIntent,
        diag: &mut ColorDiagnostics,
    ) -> Option<Rgb> {
        let Some(name) = name else {
            // §7.8.2 says an operator has exactly its operands; a `cs` with
            // no name is malformed. Nothing is guessed.
            diag.spaces_unresolved += 1;
            diag.note("cs/CS with no name operand: colour space left unset");
            self.set_unresolved(stroking);
            return None;
        };
        let resolved = self.lookup(doc, resources, name, diag);
        let Some(space) = resolved else {
            self.set_unresolved(stroking);
            return None;
        };
        if matches!(*space, ColorSpace::Pattern { .. }) {
            diag.pattern_spaces_selected += 1;
            // NOT "pdfcer does not paint patterns" — it does, for
            // `PatternType 2`. Selecting the SPACE is all this sees; which
            // kind of pattern gets named, and whether it is drawn, is
            // decided at the paint site and counted there.
            diag.note("Pattern colour space selected");
        }
        let initial = space.initial_color();
        let paints = space.paints();
        if !paints
            && matches!(
                *space,
                ColorSpace::Separation { .. } | ColorSpace::DeviceN { .. }
            )
        {
            diag.separation_none_suppressed += 1;
            diag.note("Separation//None or all-/None DeviceN: painting suppressed per 8.6.6.4");
        }
        let half = if stroking {
            &mut self.stroke
        } else {
            &mut self.fill
        };
        half.space = Some(Arc::clone(&space));
        half.paints = paints;
        half.pattern = None;
        space.to_rgb(&initial, intent, diag)
    }

    /// `sc` / `scn` / `SC` / `SCN` — set the colour components in the
    /// current space (Table 74).
    ///
    /// `pattern` carries `scn`'s optional trailing pattern name. Its
    /// presence is what distinguishes the two `scn` shapes; the operand
    /// arity cannot, because an uncoloured tiling pattern takes numbers
    /// *and* a name.
    ///
    /// Returns the colour to install, or `None` to leave the previous
    /// colour alone — in which case the reason has been counted.
    pub fn set(
        &mut self,
        comps: &[f32],
        pattern: Option<&[u8]>,
        stroking: bool,
        intent: CmykIntent,
        diag: &mut ColorDiagnostics,
    ) -> Option<Rgb> {
        // A pattern name wins outright: whatever numbers precede it belong
        // to an uncoloured tiling pattern's underlying space, and pdfcer
        // paints neither kind.
        if let Some(name) = pattern {
            let half = if stroking {
                &mut self.stroke
            } else {
                &mut self.fill
            };
            // `paints` stays FALSE. It gates the SOLID-colour paint, and a
            // pattern has no solid colour to paint — letting it stay true
            // would fill the path with whatever RGB happened to be current,
            // which is the one outcome worse than painting nothing.
            //
            // The `patterns_unpainted` counter used to be incremented right
            // here, which meant it counted patterns SELECTED rather than
            // patterns that failed to paint. Now that a shading pattern can
            // actually be drawn, those are different numbers and only the
            // second one is a shortfall, so the count moved to the paint
            // site (`Interpreter::fill_with_pattern`).
            half.paints = false;
            half.pattern = Some(std::sync::Arc::from(name));
            return None;
        }

        let half = if stroking { &self.stroke } else { &self.fill };
        let Some(space) = half.space.clone() else {
            // The space never resolved, so these numbers have no meaning.
            // Painting them as grey would be the silent default this
            // module exists to refuse.
            diag.colors_not_set += 1;
            return None;
        };

        // An operand count that disagrees with the space is a producer
        // error with no spec-defined recovery. Missing components would
        // read as 0.0 and *surplus* ones would be ignored — either way the
        // painted colour would be a guess, so it is refused and counted.
        if comps.len() != space.components() {
            diag.colors_not_set += 1;
            diag.note(&format!(
                "sc/scn given {} operand(s) for a {}-component space; colour unchanged",
                comps.len(),
                space.components()
            ));
            return None;
        }

        let rgb = space.to_rgb(comps, intent, diag);
        if rgb.is_none() {
            diag.colors_not_set += 1;
        } else {
            let half = if stroking {
                &mut self.stroke
            } else {
                &mut self.fill
            };
            half.components.clear();
            half.components.extend_from_slice(comps);
        }
        rgb
    }

    /// Put one half into the "space did not resolve" state.
    ///
    /// `paints` stays `true` deliberately. Suppressing the paint would blank
    /// content over a *resource* problem, which is a far worse failure than
    /// painting it in the previous colour — and the previous colour is at
    /// least a colour the document itself chose. The dishonesty being fixed
    /// was the silence, not the pixel.
    fn set_unresolved(&mut self, stroking: bool) {
        let half = if stroking {
            &mut self.stroke
        } else {
            &mut self.fill
        };
        half.space = None;
        half.paints = true;
        half.pattern = None;
    }

    /// Resolve a `cs`/`CS` name to a space, memoized per resource name.
    fn lookup(
        &mut self,
        doc: &DocumentView<'_>,
        resources: &Dict,
        name: &[u8],
        diag: &mut ColorDiagnostics,
    ) -> Option<Arc<ColorSpace>> {
        if let Some(hit) = self.cache.get(name) {
            return hit.clone();
        }
        let resolved = resolve_named(doc, name, resources, 0, diag);
        if resolved.is_none() {
            // Counted here, once per distinct name, because this is the
            // only place that knows the cache missed.
            diag.spaces_unresolved += 1;
            diag.note(&format!(
                "colour space /{} did not resolve; colour operators in it are ignored",
                String::from_utf8_lossy(name)
            ));
        }
        self.cache.insert(name.to_vec(), resolved.clone());
        resolved
    }
}

/// Resolve a colour space given as a **name** (the `cs`/`CS` operand form,
/// and the form a nested `base`/`alternateSpace` may take).
///
/// §8.6: "If the colour space is one that can be specified by a name and no
/// additional parameters … the name may be specified directly. Otherwise,
/// it shall be a name defined in the `ColorSpace` subdictionary of the
/// current resource dictionary."
///
/// The inline-image abbreviations (`/G`, `/RGB`, `/CMYK`, `/I` — Table 93)
/// are deliberately **not** accepted: §8.9.7 scopes them to inline images.
/// A content stream using one lands in the resource-lookup path and, if the
/// resource is absent, is counted as unresolved rather than silently
/// guessed at.
fn resolve_named(
    doc: &DocumentView<'_>,
    name: &[u8],
    resources: &Dict,
    depth: usize,
    diag: &mut ColorDiagnostics,
) -> Option<Arc<ColorSpace>> {
    if depth > MAX_COLOR_SPACE_DEPTH {
        diag.note("colour space nested past the depth guard");
        return None;
    }
    match name {
        b"DeviceGray" => Some(Arc::new(ColorSpace::DeviceGray)),
        b"DeviceRGB" => Some(Arc::new(ColorSpace::DeviceRgb)),
        b"DeviceCMYK" => Some(Arc::new(ColorSpace::DeviceCmyk)),
        b"Pattern" => Some(Arc::new(ColorSpace::Pattern { underlying: None })),
        other => {
            let entry = resources
                .get(b"ColorSpace")
                .map(|o| doc.resolve(o))
                .and_then(Object::as_dict)
                .and_then(|cs| cs.get(other))
                .map(|o| doc.resolve(o))?;
            resolve_object(doc, entry, resources, depth + 1, diag)
        }
    }
}

/// Resolve a colour space given as an arbitrary object — a name, a
/// one-element array (`[/DeviceRGB]`, which producers emit), or one of the
/// parameterised array forms.
///
/// `pub(crate)` rather than private because [`crate::shading`] needs it: a
/// shading dictionary's `/ColorSpace` (§8.7.4.3, Table 78) is an arbitrary
/// colour-space object exactly as a `cs` operand's resource entry is, and
/// resolving it a second way would let the same array produce two different
/// colours in one document. One resolver, per this module's own rule about
/// the function evaluator.
pub(crate) fn resolve_object(
    doc: &DocumentView<'_>,
    obj: &Object,
    resources: &Dict,
    depth: usize,
    diag: &mut ColorDiagnostics,
) -> Option<Arc<ColorSpace>> {
    if depth > MAX_COLOR_SPACE_DEPTH {
        diag.note("colour space nested past the depth guard");
        return None;
    }
    match obj {
        Object::Name(n) => resolve_named(doc, n.as_bytes(), resources, depth, diag),
        Object::Array(items) => match items.split_first() {
            // `[/DeviceRGB]` and friends: an array wrapping just the name.
            Some((first, [])) => {
                resolve_object(doc, doc.resolve(first), resources, depth + 1, diag)
            }
            Some((first, rest)) => {
                let family = doc.resolve(first).as_name()?.as_bytes();
                resolve_family(doc, family, rest, resources, depth, diag)
            }
            None => {
                diag.note("empty colour space array");
                None
            }
        },
        _ => {
            diag.note("colour space is neither a name nor an array");
            None
        }
    }
}

/// Dispatch on the family name of a parameterised colour-space array.
fn resolve_family(
    doc: &DocumentView<'_>,
    family: &[u8],
    args: &[Object],
    resources: &Dict,
    depth: usize,
    diag: &mut ColorDiagnostics,
) -> Option<Arc<ColorSpace>> {
    match family {
        b"CalGray" => resolve_cal_gray(doc, args, diag),
        b"CalRGB" => resolve_cal_rgb(doc, args, diag),
        b"Lab" => resolve_lab(doc, args, diag),
        b"ICCBased" => resolve_icc_based(doc, args, resources, depth, diag),
        b"Indexed" => resolve_indexed(doc, args, resources, depth, diag),
        b"Separation" => resolve_separation(doc, args, resources, depth, diag),
        b"DeviceN" => resolve_device_n(doc, args, resources, depth, diag),
        b"Pattern" => {
            let underlying = args
                .first()
                .map(|o| doc.resolve(o))
                .and_then(|o| resolve_object(doc, o, resources, depth + 1, diag));
            Some(Arc::new(ColorSpace::Pattern { underlying }))
        }
        // A device name inside an array with arguments is malformed, but
        // `[/DeviceRGB]` was already handled above, so anything reaching
        // here is genuinely unknown.
        other => {
            diag.note(&format!(
                "unknown colour space family /{}",
                String::from_utf8_lossy(other)
            ));
            None
        }
    }
}

/// `[/CalGray dict]` — §8.6.5.2, Table 63.
///
/// `BlackPoint` is not parsed, and that is conformant rather than lazy: it
/// appears in **none** of the three CIE transforms. §8.6.5.3 gives its only
/// stated role as controlling "the overall effect of the CIE-based gamut
/// mapping function described in 10.2" — and §10.2 hands that function to
/// the implementation. Same for `CalRGB` and `Lab` below.
fn resolve_cal_gray(
    doc: &DocumentView<'_>,
    args: &[Object],
    diag: &mut ColorDiagnostics,
) -> Option<Arc<ColorSpace>> {
    let dict = args
        .first()
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)?;
    let white = white_point(doc, dict).unwrap_or_else(|| {
        // `WhitePoint` is Required by Table 62. A missing one is a producer
        // bug with no spec-defined recovery; D65 keeps the conversion
        // defined and the note says the value is pdfcer's, not the file's.
        diag.note("CalGray without the required /WhitePoint; D65 assumed");
        D65
    });
    let gamma = number(doc, dict, b"Gamma").unwrap_or(1.0);
    Some(Arc::new(ColorSpace::CalGray { white, gamma }))
}

/// `[/CalRGB dict]` — §8.6.5.3, Table 64.
fn resolve_cal_rgb(
    doc: &DocumentView<'_>,
    args: &[Object],
    diag: &mut ColorDiagnostics,
) -> Option<Arc<ColorSpace>> {
    let dict = args
        .first()
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)?;
    let white = white_point(doc, dict).unwrap_or_else(|| {
        diag.note("CalRGB without the required /WhitePoint; D65 assumed");
        D65
    });
    let gamma = numbers(doc, dict, b"Gamma")
        .and_then(|v| <[f32; 3]>::try_from(v.as_slice()).ok())
        .unwrap_or([1.0, 1.0, 1.0]);
    let matrix = numbers(doc, dict, b"Matrix")
        .and_then(|v| <[f32; 9]>::try_from(v.as_slice()).ok())
        .unwrap_or([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    Some(Arc::new(ColorSpace::CalRgb {
        white,
        gamma,
        matrix,
    }))
}

/// `[/Lab dict]` — §8.6.5.4, Table 65.
fn resolve_lab(
    doc: &DocumentView<'_>,
    args: &[Object],
    diag: &mut ColorDiagnostics,
) -> Option<Arc<ColorSpace>> {
    let dict = args
        .first()
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)?;
    let white = white_point(doc, dict).unwrap_or_else(|| {
        diag.note("Lab without the required /WhitePoint; D65 assumed");
        D65
    });
    let range = numbers(doc, dict, b"Range")
        .and_then(|v| <[f32; 4]>::try_from(v.as_slice()).ok())
        .unwrap_or([-100.0, 100.0, -100.0, 100.0]);
    Some(Arc::new(ColorSpace::Lab { white, range }))
}

/// `[/ICCBased stream]` — §8.6.5.5, Table 66. See [`ColorSpace::IccBased`]
/// for why this is the specified behaviour rather than a shortcut.
fn resolve_icc_based(
    doc: &DocumentView<'_>,
    args: &[Object],
    resources: &Dict,
    depth: usize,
    diag: &mut ColorDiagnostics,
) -> Option<Arc<ColorSpace>> {
    let stream_obj = args.first().map(|o| doc.resolve(o));
    let dict = stream_obj.and_then(Object::as_dict)?;

    // The profile bytes themselves. Decoded eagerly here because this is the
    // only place the stream is in hand — `resolve_object` returns a
    // `ColorSpace`, not the object it came from, so a later consumer that
    // wanted the profile would have no way back to it.
    //
    // A decode failure is deliberately swallowed to `None` rather than
    // propagated: see the `profile` field's own documentation for why
    // refusing the colour space would be worse than the spec's own fallback.
    let profile: Option<Arc<[u8]>> = match stream_obj {
        Some(Object::Stream(s)) => doc
            .slice(s.data_span)
            .and_then(|raw| pdfcer_core::filters::decode_stream(&s.dict, raw).ok())
            .map(Arc::from),
        _ => None,
    };

    // `/N` is Required and constrained to 1, 3 or 4, which is exactly what
    // makes the fallback total: there is no unknown-component-count case
    // for a conformant file.
    let n = dict
        .get(b"N")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_int)
        .and_then(|v| usize::try_from(v).ok())
        .filter(|n| matches!(n, 1 | 3 | 4));

    // `/Alternate` first, so a file that supplied one gets it even when
    // `/N` is missing or nonsense.
    let alternate = dict
        .get(b"Alternate")
        .map(|o| doc.resolve(o))
        .and_then(|o| resolve_object(doc, o, resources, depth + 1, diag))
        // Table 66: the alternate "may be any valid colour space (except a
        // Pattern colour space)". A Pattern alternate is refused rather than
        // used, because it would make the space unpaintable for a reason
        // the document did not ask for.
        .filter(|space| !matches!(**space, ColorSpace::Pattern { .. }))
        // And it must have N components. `color__iccbased.md` records the
        // genuine spec tension here: a Separation/DeviceN alternate would
        // need a tint transform, which Table 66's own sentence forbids. The
        // recommendation there — skip it and take the N-based device
        // fallback — is what the component check produces anyway, since
        // Separation has 1 component and rarely matches N.
        .filter(|space| n.is_none_or(|n| space.components() == n));

    let n = n.or_else(|| alternate.as_ref().map(|s| s.components()))?;
    match alternate {
        Some(alternate) => {
            diag.icc_alternate_used += 1;
            // ★ This note used to end "pdfcer has no ICC engine". That stopped
            // being true when iccce was wired in: the profile is now carried
            // on the space and used for terminal conversions. The counter is
            // kept — the alternate IS still what paints in the additive path —
            // but the parenthetical was a claim about pdfcer's capabilities,
            // and a stale capability claim in an operator-visible diagnostic
            // is worse than no parenthetical at all.
            diag.note("ICCBased rendered through its /Alternate (8.6.5.5)");
            Some(Arc::new(ColorSpace::IccBased {
                n,
                alternate,
                alternate_explicit: true,
                profile,
            }))
        }
        None => {
            let device = match n {
                1 => ColorSpace::DeviceGray,
                3 => ColorSpace::DeviceRgb,
                4 => ColorSpace::DeviceCmyk,
                // Unreachable: `n` came from the 1/3/4 filter or from an
                // alternate that passed the component check.
                _ => return None,
            };
            diag.icc_device_fallback_used += 1;
            diag.note("ICCBased with no /Alternate: device space by /N (8.6.5.5, Table 66)");
            Some(Arc::new(ColorSpace::IccBased {
                n,
                alternate: Arc::new(device),
                alternate_explicit: false,
                profile,
            }))
        }
    }
}

/// `[/Indexed base hival lookup]` — §8.6.6.3.
fn resolve_indexed(
    doc: &DocumentView<'_>,
    args: &[Object],
    resources: &Dict,
    depth: usize,
    diag: &mut ColorDiagnostics,
) -> Option<Arc<ColorSpace>> {
    let base = args
        .first()
        .map(|o| doc.resolve(o))
        .and_then(|o| resolve_object(doc, o, resources, depth + 1, diag))?;
    if matches!(
        *base,
        ColorSpace::Indexed { .. } | ColorSpace::Pattern { .. }
    ) {
        // §8.6.6.3: the base "shall not be a Pattern space or another
        // Indexed space".
        diag.note("Indexed over Indexed/Pattern is forbidden by 8.6.6.3");
        return None;
    }

    // `hival` is a MAXIMUM INDEX with a normative ceiling of 255, which is
    // exactly what `u8::try_from` enforces — a `hival` of 256 or −1 is
    // refused rather than silently saturated.
    let hival = args
        .get(1)
        .map(|o| doc.resolve(o))
        .and_then(Object::as_int)
        .and_then(|v| u8::try_from(v).ok())?;

    // The lookup may be a byte STRING (PDF 1.2, the form §8.6.6.3's own
    // example uses) or a STREAM. A reader that handles only the stream case
    // fails on the spec's own example.
    let lookup_obj = args.get(2).map(|o| doc.resolve(o))?;
    let lookup: Vec<u8> = match lookup_obj {
        Object::String(bytes) => bytes.clone(),
        Object::Stream(stream) => {
            // `doc.slice`, not `span.slice(doc.bytes())`: on a session view
            // the payload may live in the staging half and there is no
            // single buffer to index (decision 018 §4).
            let raw = doc.slice(stream.data_span)?;
            filters::decode_stream(&stream.dict, raw).ok()?
        }
        _ => {
            diag.note("Indexed lookup is neither a string nor a stream");
            return None;
        }
    };

    Some(Arc::new(ColorSpace::Indexed {
        base,
        hival,
        lookup: Arc::from(lookup.into_boxed_slice()),
    }))
}

/// `[/Separation name alternateSpace tintTransform]` — §8.6.6.4.
///
/// The `tintTransform` element is **required to be present** and is
/// deliberately not read: it is a §7.10 function, and this module has no
/// evaluator (see the module docs). A file that omits it is still resolved,
/// because on an additive device pdfcer's rendering does not depend on it
/// today, and refusing the space would lose the colorant name too.
fn resolve_separation(
    doc: &DocumentView<'_>,
    args: &[Object],
    resources: &Dict,
    depth: usize,
    diag: &mut ColorDiagnostics,
) -> Option<Arc<ColorSpace>> {
    let colorant = args
        .first()
        .map(|o| doc.resolve(o))
        .and_then(Object::as_name)
        .map(|n| Colorant::parse(n.as_bytes()))?;
    let alternate = alternate_space(doc, args.get(1), resources, depth, diag);
    let tint = load_tint(doc, args.get(2), alternate.components());
    Some(Arc::new(ColorSpace::Separation {
        colorant,
        alternate,
        tint,
    }))
}

/// `[/DeviceN names alternateSpace tintTransform]` (+ `attributes`) —
/// §8.6.6.5.
fn resolve_device_n(
    doc: &DocumentView<'_>,
    args: &[Object],
    resources: &Dict,
    depth: usize,
    diag: &mut ColorDiagnostics,
) -> Option<Arc<ColorSpace>> {
    let names: Vec<Colorant> = args
        .first()
        .map(|o| doc.resolve(o))
        .and_then(Object::as_array)?
        .iter()
        .filter_map(|o| {
            doc.resolve(o)
                .as_name()
                .map(|n| Colorant::parse(n.as_bytes()))
        })
        .collect();
    if names.is_empty() {
        diag.note("DeviceN with an empty or unreadable /names array");
        return None;
    }
    // Annex C's implementation limit. A longer array is a hostile or broken
    // file; refusing it is cheaper than carrying it.
    if names.len() > 32 {
        diag.note("DeviceN with more than the Annex C limit of 32 components");
        return None;
    }
    let alternate = alternate_space(doc, args.get(1), resources, depth, diag);
    let tint = load_tint(doc, args.get(2), alternate.components());
    Some(Arc::new(ColorSpace::DeviceN {
        names: Arc::from(names.into_boxed_slice()),
        alternate,
        tint,
    }))
}

/// Load a `tintTransform` and check it fits the space it serves.
///
/// # Why the arity is checked here rather than at evaluation
///
/// §8.6.6.4/.5 require the function to take **one input per colorant** and
/// produce **one output per component of the alternate space**. A function
/// that does not is a malformed file, and the cheap moment to find out is
/// once at load rather than per pixel — a mismatch discovered during
/// evaluation would either paint something wrong or cost a branch on every
/// sample.
///
/// Returns `None` on any problem, which routes the space to the neutral
/// stand-in with the shortfall counted. That is the same posture the rest
/// of this module takes: a colour shown by mistake is visible and
/// arguable; a page silently painted from a function pdfcer mis-read is
/// neither.
fn load_tint(
    doc: &DocumentView<'_>,
    obj: Option<&Object>,
    alternate_components: usize,
) -> Option<Arc<pdfcer_core::function::PdfFunction>> {
    let function = pdfcer_core::function::PdfFunction::load(doc, obj?).ok()?;
    (function.outputs() == alternate_components).then(|| Arc::new(function))
}

/// Convert tint components through a `tintTransform` into the alternate
/// space, then to RGB.
///
/// # This is the function the whole §7.10 evaluator was built for
///
/// Until it existed, `Separation` and `DeviceN` rendered a **neutral** —
/// the right lightness and no hue at all — because the mapping from a tint
/// to a colour lives entirely inside the document's own function and
/// cannot be guessed. A spot-coloured drawing read as a grey one.
///
/// Now the document's own answer is used. `neutral_from_tint` remains as
/// the fallback for a file whose transform is missing or malformed, and
/// that case is still counted.
/// Run a `/tintTransform` and keep its output **as colorants**, when the
/// alternate space is `DeviceCmyk`.
///
/// The four numbers this returns are the same ones [`tint_through`] computes
/// and then hands to `alternate.to_rgb(...)`. The only difference is that
/// this stops one step earlier — which is the whole of what overprint needs
/// and the whole of what the RGB route destroys.
///
/// `None` when there is no transform, when it fails to evaluate, or when the
/// alternate is not `DeviceCmyk`. A four-component `ICCBased` alternate
/// arrives here already resolved to `DeviceCmyk` by `crate::color`'s own
/// `/Alternate` handling (§8.6.5.5 / Table 66), so it needs no arm of its own
/// — the same reasoning `BlendSpace::of` documents.
fn tint_to_cmyk(
    transform: Option<&pdfcer_core::function::PdfFunction>,
    alternate: &ColorSpace,
    comps: &[f32],
    diag: &mut ColorDiagnostics,
) -> Option<[f32; 4]> {
    if !matches!(alternate, ColorSpace::DeviceCmyk) {
        return None;
    }
    let f = transform?;
    let inputs: Vec<f64> = comps.iter().map(|v| f64::from(*v)).collect();
    let Ok(out) = f.eval(&inputs) else {
        // Counted through the same channel the RGB route uses, so a file
        // with a broken transform reports one failure rather than two.
        diag.tint_transform_not_applied += 1;
        return None;
    };
    if out.len() < 4 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some([out[0] as f32, out[1] as f32, out[2] as f32, out[3] as f32])
}

fn tint_through(
    function: &pdfcer_core::function::PdfFunction,
    alternate: &ColorSpace,
    tints: &[f32],
    intent: CmykIntent,
    diag: &mut ColorDiagnostics,
) -> Option<Rgb> {
    let inputs: Vec<f64> = tints.iter().map(|t| f64::from(*t)).collect();
    // A function that refuses is a malformed one, and the refusal is the
    // evaluator's job to make; this layer's job is not to paper over it.
    let out = function.eval(&inputs).ok()?;
    let comps: Vec<f32> = out.iter().map(|v| *v as f32).collect();
    diag.tint_transforms_applied += 1;
    alternate.to_rgb(&comps, intent, diag)
}

/// Resolve the `alternateSpace` of a `Separation`/`DeviceN`.
///
/// Falls back to `DeviceGray` when absent or unresolvable, and that is the
/// one place in this module where a fallback to grey is right rather than
/// dishonest: the alternate is only ever consulted through
/// [`ColorSpace::neutral_from_tint`] today, which renders a *neutral* in
/// whatever space it is given, so grey changes nothing an operator could
/// observe. The fact that the tint transform did not run is already
/// disclosed by [`ColorDiagnostics::tint_transform_not_applied`]; a second
/// counter here would report the same shortfall twice.
fn alternate_space(
    doc: &DocumentView<'_>,
    obj: Option<&Object>,
    resources: &Dict,
    depth: usize,
    diag: &mut ColorDiagnostics,
) -> Arc<ColorSpace> {
    obj.map(|o| doc.resolve(o))
        .and_then(|o| resolve_object(doc, o, resources, depth + 1, diag))
        .filter(|space| {
            // "may not be another special colour space (Pattern, Indexed,
            // Separation, or DeviceN)" — §8.6.6.4/.5.
            !matches!(
                **space,
                ColorSpace::Pattern { .. }
                    | ColorSpace::Indexed { .. }
                    | ColorSpace::Separation { .. }
                    | ColorSpace::DeviceN { .. }
            )
        })
        .unwrap_or_else(|| Arc::new(ColorSpace::DeviceGray))
}

/// `[Xw Yw Zw]` from a CIE-based colour space dictionary.
///
/// Tables 63/64/65 all constrain it identically: "The numbers `Xw` and `Zw`
/// shall be positive, and `Yw` shall be equal to 1.0."
///
/// Only the positivity is enforced, and only as a **safety** check — a
/// non-positive component makes the Bradford adaptation degenerate and
/// would emit NaNs, which paint as transparent black and are far harder to
/// diagnose than a missing white point. `Yw != 1.0` is *not* rejected: it is
/// non-conformant but harmless here, because the adaptation normalises
/// against whatever white the file declares, and refusing the space would
/// lose a colour the document can still be rendered in.
fn white_point(doc: &DocumentView<'_>, dict: &Dict) -> Option<[f32; 3]> {
    let v = numbers(doc, dict, b"WhitePoint")?;
    let w = <[f32; 3]>::try_from(v.as_slice()).ok()?;
    let [x, y, z] = w;
    (x > 0.0 && y > 0.0 && z > 0.0).then_some(w)
}

/// A numeric array entry, resolved and widened to `f32`.
fn numbers(doc: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> Option<Vec<f32>> {
    let items = doc.resolve(dict.get(key)?).as_array()?;
    Some(
        items
            .iter()
            .filter_map(|o| doc.resolve(o).as_number().map(|v| v as f32))
            .collect(),
    )
}

/// A scalar numeric entry, resolved and widened to `f32`.
fn number(doc: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> Option<f32> {
    doc.resolve(dict.get(key)?).as_number().map(|v| v as f32)
}

/// Component `i` of an operand run, or 0.0 if it is absent.
///
/// A free function rather than indexing so that a short operand run is a
/// tolerated oddity rather than a panic on attacker-supplied input.
fn comp(comps: &[f32], i: usize) -> f32 {
    comps.get(i).copied().unwrap_or(0.0)
}

/// Clamp into `[lo, hi]` without assuming the bounds are ordered — a
/// `/Range` of `[100 -100]` is malformed but must not panic
/// (`f32::clamp` panics when `min > max`).
fn clamp_between(v: f32, lo: f32, hi: f32) -> f32 {
    v.clamp(lo.min(hi), lo.max(hi))
}

/// §8.6.6.3's index → colour lookup.
///
/// The index rule is one of the few places the standard makes clamping
/// **normative**: "The index value should be an integer in the range 0 to
/// `hival`. If the value is a real number, it shall be rounded to the
/// nearest integer; if it is outside the range 0 to `hival`, it shall be
/// adjusted to the nearest value within that range." It is a clamp, not a
/// modulo — index 300 with `hival 255` is 255, not 44.
///
/// Each table byte is "scaled to the range of the corresponding colour
/// component in the base colour space", which is why this reads
/// [`ColorSpace::component_range`] rather than dividing by 255 and stopping:
/// an `Indexed` over `Lab` stores L\* as 0–255 meaning 0–100.
fn indexed_to_rgb(
    index: f32,
    base: &ColorSpace,
    hival: u8,
    lookup: &[u8],
    intent: CmykIntent,
    diag: &mut ColorDiagnostics,
) -> Rgb {
    let rounded = index.round();
    let clamped = rounded.clamp(0.0, f32::from(hival));
    if (clamped - rounded).abs() > f32::EPSILON {
        diag.indexed_index_clamped += 1;
        diag.note("Indexed index outside 0..=hival; clamped per 8.6.6.3");
    }
    let m = base.components();
    let offset = (clamped as usize).saturating_mul(m);
    let Some(entry) = lookup.get(offset..offset.saturating_add(m)) else {
        // Producers routinely trim trailing unused entries. Black is the
        // documented degradation (`color__indexed.md`: "treat out-of-bounds
        // reads as black, + diagnostic. Do not reject").
        diag.indexed_lookup_short += 1;
        diag.note("Indexed lookup table shorter than hival; entry painted black");
        return Rgb::BLACK;
    };
    let comps: Vec<f32> = (0..m)
        .map(|i| {
            let byte = f32::from(entry.get(i).copied().unwrap_or(0)) / 255.0;
            let (lo, hi) = base.component_range(i);
            lo + byte * (hi - lo)
        })
        .collect();
    base.to_rgb(&comps, intent, diag).unwrap_or(Rgb::BLACK)
}

/// §8.6.6.4 tint → colour, through the document's own tint transform.
///
/// # The structure, and why the three arms differ
///
/// pdfcer is an **additive** device, so §8.6.6.4 rule S4 applies without
/// exception: a `Separation` never applies its colorant directly and
/// always reverts to `alternateSpace` via `tintTransform`. There is no
/// colorant-matching step to implement.
///
/// - [`Colorant::None`] paints nothing (the caller suppresses via
///   [`ColorSpace::paints`]), so there is no colour to report.
/// - [`Colorant::All`] explicitly **ignores** the alternate space and the
///   transform, per the clause; the screen appearance is pdfcer's choice
///   and is disclosed as one.
/// - [`Colorant::Named`] evaluates the document's `/tintTransform` via
///   [`pdfcer_core::function`] and hands the result to
///   [`ColorSpace::to_rgb`]. Only a file whose transform is missing,
///   malformed or wrongly-shaped falls through to the neutral stand-in,
///   and that fall-through is counted.
fn separation_to_rgb(
    colorant: &Colorant,
    alternate: &ColorSpace,
    transform: Option<&pdfcer_core::function::PdfFunction>,
    tint: f32,
    intent: CmykIntent,
    diag: &mut ColorDiagnostics,
) -> Option<Rgb> {
    match colorant {
        // "shall not produce any visible output" — the caller suppresses
        // the paint via `ColorSpace::paints`; there is no colour to report.
        Colorant::None => None,
        Colorant::All => {
            // "shall apply tint values to all available colorants at once",
            // with the alternate space and tint transform explicitly
            // ignored. All colorants at tint t on an additive display is
            // pdfcer's call, not the standard's: a neutral of luminance
            // 1 − t, which reaches black at full tint as a registration
            // target should.
            diag.separation_all_approximated += 1;
            diag.note(
                "Separation//All rendered as a neutral of luminance 1-tint (pdfcer's choice)",
            );
            Some(Rgb::from_gray((1.0 - tint).clamp(0.0, 1.0)))
        }
        Colorant::Named(_) => {
            // The document's own answer, when it gave one.
            if let Some(f) = transform
                && let Some(rgb) = tint_through(f, alternate, &[tint], intent, diag)
            {
                return Some(rgb);
            }
            // Only a file whose transform is missing, malformed or
            // wrongly-shaped reaches here — and it is still counted, so
            // "these spot colours are pdfcer's stand-in" stays something a
            // shell can say.
            diag.tint_transform_not_applied += 1;
            diag.note(
                "Separation tint transform missing or unusable; lightness preserved, hue is not the document's",
            );
            alternate.to_rgb(&alternate.neutral_from_tint(tint), intent, diag)
        }
    }
}

/// §8.6.6.5 tints → colour, through the document's own tint transform.
///
/// # The normal path
///
/// All components are passed to `/tintTransform` **in `names` order,
/// including the `/None` ones**, because that is what the clause requires
/// — the transform's input arity is the space's full component count, and
/// dropping a channel would silently mis-index every colorant after it.
/// The result goes to the alternate space. This is what a conformant file
/// takes.
///
/// # The fallback, and why it maximises rather than averages
///
/// A file whose transform is absent, malformed or of the wrong arity falls
/// back to a single effective tint: the **maximum** over the non-`/None`
/// components. That is pdfcer's choice and it is the conservative one: in a
/// subtractive space the darkest colorant dominates the appearance, so an
/// all-zero `DeviceN` renders as paper and a single fully-applied colorant
/// renders dark, which preserves the drawing's figure/ground. Averaging
/// would wash a single strong colorant out in a six-channel space.
///
/// `/None` components are excluded from **that maximum** because they
/// "shall never be painted on the page" (§8.6.6.5) — including them would
/// darken the result for ink that is not there. Note the asymmetry: they
/// are excluded from the fallback's maximum and included in the
/// transform's input, and both are what the clause asks for.
///
/// A space whose components are **all** `/None` returns `None` outright:
/// the clause says such a space "shall always discard its output" and
/// "shall never revert to the alternate colour space", so there is no
/// fallback to reach.
fn device_n_to_rgb(
    names: &[Colorant],
    alternate: &ColorSpace,
    transform: Option<&pdfcer_core::function::PdfFunction>,
    comps: &[f32],
    intent: CmykIntent,
    diag: &mut ColorDiagnostics,
) -> Option<Rgb> {
    let mut tint = 0.0f32;
    let mut any = false;
    for (i, name) in names.iter().enumerate() {
        if *name == Colorant::None {
            continue;
        }
        any = true;
        tint = tint.max(comp(comps, i));
    }
    if !any {
        // All components are `/None`: "shall always discard its output …
        // shall never revert to the alternate colour space".
        return None;
    }
    // The document's own answer, when it gave one. All components are
    // passed in `names` order, including `/None` ones -- SS8.6.6.5 requires
    // the transform to receive them all. The max-tint fallback below is
    // only for a file that supplied no usable transform.
    if let Some(f) = transform
        && let Some(rgb) = tint_through(f, alternate, comps, intent, diag)
    {
        return Some(rgb);
    }
    // The note names the FILE's shortfall, not pdfcer's. Its previous
    // wording — "NOT evaluated (no 7.10 function evaluator yet)" — was
    // written before `pdfcer_core::function` existed and was left behind
    // when the evaluator was wired in directly above. It is the one stale
    // sentence in this file that a shell PRINTS, so an operator chasing a
    // grey spot colour was told to wait for a feature that had shipped
    // instead of to check the document's own `/tintTransform`. Its
    // `Separation` twin in `separation_to_rgb` was updated at the time;
    // this one was not.
    diag.tint_transform_not_applied += 1;
    diag.note(
        "DeviceN tint transform missing or unusable; lightness preserved, \
         hue is not the document's",
    );
    alternate.to_rgb(&alternate.neutral_from_tint(tint), intent, diag)
}

/// §8.6.5.2's `CalGray` decode: `A` through `Gamma`, scaled by the white
/// point.
///
/// `X = Xw · A^Gamma`, `Y = Yw · A^Gamma`, `Z = Zw · A^Gamma` — the value is
/// achromatic by construction, which is why one exponentiation serves all
/// three channels.
fn cal_gray_to_xyz(a: f32, white: [f32; 3], gamma: f32) -> [f32; 3] {
    let [xw, yw, zw] = white;
    // The clamp is pdfcer's, not the standard's: Table 64 gives `CalRGB` an
    // explicit "component values falling outside that range shall be
    // adjusted to the nearest valid value without error indication" and
    // Table 63 gives `CalGray` no such sentence (spec ambiguity `CIE-A1`).
    // It is applied anyway because a negative base with a fractional
    // exponent is NaN, and a NaN paints as transparent black — a failure
    // mode far harder to diagnose than a clamped grey.
    let ag = a.clamp(0.0, 1.0).powf(gamma);
    [xw * ag, yw * ag, zw * ag]
}

/// §8.6.5.3's `CalRGB` decode: per-channel gamma, then the 3×3 `Matrix`.
///
/// Note the `Matrix` layout — `[XA YA ZA XB YB ZB XC YC ZC]`, grouped by
/// *input* component — so the X row of the multiplication reads entries 0,
/// 3 and 6. Reading it as three XYZ rows instead transposes the matrix,
/// which is silent on the default identity and wrong on every real one.
fn cal_rgb_to_xyz(abc: [f32; 3], gamma: [f32; 3], matrix: &[f32; 9]) -> [f32; 3] {
    let [a, b, c] = abc;
    let [ga, gb, gc] = gamma;
    let a = a.clamp(0.0, 1.0).powf(ga);
    let b = b.clamp(0.0, 1.0).powf(gb);
    let c = c.clamp(0.0, 1.0).powf(gc);
    let [xa, ya, za, xb, yb, zb, xc, yc, zc] = *matrix;
    [
        xa * a + xb * b + xc * c,
        ya * a + yb * b + yc * c,
        za * a + zb * b + zc * c,
    ]
}

/// §8.6.5.4's `Lab` decode.
///
/// ```text
/// M = (L* + 16) / 116
/// L = M + a* / 500
/// N = M − b* / 200
/// X = Xw · g(L)      Y = Yw · g(M)      Z = Zw · g(N)
///
///           ⎧ x³                          if x ≥ 6/29
/// g(x)  =   ⎨
///           ⎩ (108/841) · (x − 4/29)      otherwise
/// ```
///
/// The piecewise branch is the part that gets dropped by implementations
/// working from half-remembered CIE formulas, and what it buys is NOT a
/// small correction near the breakpoint: the linear segment is the
/// *tangent* to `x³` at `6/29`, so the two agree to first order there. What
/// it buys is at the bottom — the linear branch reaches **exactly zero at
/// `x = 4/29`**, which is precisely where `L* = 0` lands. Drop it and
/// `Lab (0, 0, 0)` decodes to `(0.00266·Xw, …)` instead of black. Use these
/// exact rationals, not the `903.3`/`7.787` approximations that circulate
/// for the same function: they do not agree.
///
/// (An earlier draft of this comment claimed a factor-of-forty divergence
/// at `x = 0.05`. That was wrong in a way worth leaving recorded: below
/// `4/29` the linear branch is NEGATIVE, so the two branches are not
/// comparable as magnitudes at all. The claim was never measured; the
/// tangency and the exact zero at `4/29` were, and both are pinned by
/// tests.)
fn lab_to_xyz(lab: [f32; 3], white: [f32; 3]) -> [f32; 3] {
    let [l_star, a_star, b_star] = lab;
    let m = (l_star + 16.0) / 116.0;
    let l = m + a_star / 500.0;
    let n = m - b_star / 200.0;
    let [xw, yw, zw] = white;
    [xw * lab_g(l), yw * lab_g(m), zw * lab_g(n)]
}

/// §8.6.5.4's `g(x)`.
fn lab_g(x: f32) -> f32 {
    // 6/29 ≈ 0.206897; 108/841 ≈ 0.128419; 4/29 ≈ 0.137931.
    const BREAK: f32 = 6.0 / 29.0;
    const SLOPE: f32 = 108.0 / 841.0;
    const OFFSET: f32 = 4.0 / 29.0;
    if x >= BREAK {
        x * x * x
    } else {
        SLOPE * (x - OFFSET)
    }
}

/// CIE XYZ → sRGB, **pdfcer's engineering choice** (module docs).
///
/// # "pdfcer's choice" is a sourced statement, not a hedge
///
/// ISO 32000-1 stops at XYZ, and it stops there **deliberately**. §10.1(a)
/// makes the conversion to device colour a `shall`; §10.2 decomposes it
/// into a gamut-mapping and a colour-mapping function, both `shall` — and
/// then closes: *"The gamut mapping and colour mapping functions **are part
/// of the implementation of the conforming reader**."* Its NOTE 2 adds that
/// the conversion "is complex, and the theory on which it is based is
/// **beyond the scope of this specification**."
///
/// That the omission is deliberate rather than an oversight is provable
/// from the next sub-clause: **§10.3 does give explicit `shall` formulas**
/// for device↔device conversion (`gray = 0.3·red + 0.59·green + 0.11·blue`,
/// gray↔CMYK, RGB↔CMYK). The standard writes conversion arithmetic when it
/// means to. It did not write this one.
///
/// **Chromatic adaptation is not merely optional — it is unmentioned.**
/// A search of all 756 pages of ISO 32000-1 returns zero hits for
/// "chromatic adaptation", "Bradford" and "von Kries"
/// (`color__cie_based.md`, verification record). Adapting and not adapting
/// are therefore *both* conformant, which makes this an operator-facing
/// ambiguity of exactly the kind pdfcer turns into a setting (`CIE-A6`/
/// `CIE-A7` in the ambiguity register). It is hard-coded to "adapt" today
/// and named here so the next Pass can lift it into `RenderPolicy` rather
/// than rediscover the question.
///
/// So: *"L\*a\*b\* → XYZ per §8.6.5.4"* is a claim pdfcer may make.
/// *"XYZ → sRGB as specified by ISO 32000-1"* is a sentence pdfcer must
/// never write. The three steps below are pdfcer choosing a defensible
/// answer under §10.2's grant, and they are documented here rather than
/// buried:
///
/// 1. **Bradford chromatic adaptation** from `source_white` to D65. PDF CIE
///    spaces are overwhelmingly D50; sRGB is defined at D65. Skipping this
///    leaves a visible warm cast on every `Lab` and `CalRGB` colour.
/// 2. The **sRGB (IEC 61966-2-1) XYZ→linear-RGB matrix**, defined at D65 —
///    which is what makes step 1 a precondition rather than a refinement.
///    Note that `IEC/3WD 61966-2.1:1999` *is* in ISO 32000-1's clause 3
///    (Normative references), but no `shall` in the standard ever invokes
///    it for output conversion, so citing it does not turn this step into
///    a specified one.
/// 3. The **sRGB transfer function** (the 12.92 linear toe plus the
///    2.4-exponent segment), not a plain 1/2.2 power.
///
/// Out-of-gamut results are clipped per channel. No rendering intent
/// (§8.6.5.8) and no gamut compression are applied; a saturated `Lab`
/// colour outside sRGB will clip rather than desaturate.
fn xyz_to_srgb(xyz: [f32; 3], source_white: [f32; 3]) -> Rgb {
    let [x, y, z] = bradford_adapt(xyz, source_white, D65);
    // IEC 61966-2-1 XYZ (D65) → linear sRGB.
    let r = 3.240_454_2 * x - 1.537_138_5 * y - 0.498_531_4 * z;
    let g = -0.969_266 * x + 1.876_010_8 * y + 0.041_556_0 * z;
    let b = 0.055_643_4 * x - 0.204_025_9 * y + 1.057_225_2 * z;
    Rgb {
        r: srgb_encode(r),
        g: srgb_encode(g),
        b: srgb_encode(b),
    }
}

/// The sRGB transfer function (IEC 61966-2-1), with the linear toe.
fn srgb_encode(linear: f32) -> f32 {
    let v = linear.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Bradford chromatic adaptation between two white points.
///
/// The von Kries transform in the Bradford cone-response space: convert
/// both white points and the colour to cone responses, scale each cone
/// channel by the ratio of the destination white's response to the source
/// white's, and convert back. This is the standard CIE-recommended
/// adaptation and it is pdfcer's choice (the module docs say why there is a
/// choice to make at all).
///
/// A degenerate source white (a zero cone response) leaves the colour
/// unadapted rather than producing infinities — `white_point` already
/// refuses non-positive components, so this is belt-and-braces against a
/// pathological but positive white.
fn bradford_adapt(xyz: [f32; 3], from: [f32; 3], to: [f32; 3]) -> [f32; 3] {
    let src = bradford_cone(from);
    let dst = bradford_cone(to);
    let cone = bradford_cone(xyz);
    let [sl, sm, ss] = src;
    let [dl, dm, ds] = dst;
    let [cl, cm, cs] = cone;
    let ratio = |d: f32, s: f32, c: f32| if s.abs() < f32::EPSILON { c } else { c * d / s };
    bradford_cone_inverse([ratio(dl, sl, cl), ratio(dm, sm, cm), ratio(ds, ss, cs)])
}

/// XYZ → Bradford LMS cone responses.
fn bradford_cone([x, y, z]: [f32; 3]) -> [f32; 3] {
    [
        0.895_1 * x + 0.266_4 * y - 0.161_4 * z,
        -0.750_2 * x + 1.713_5 * y + 0.036_7 * z,
        0.038_9 * x - 0.068_5 * y + 1.029_6 * z,
    ]
}

/// Bradford LMS cone responses → XYZ (the inverse of [`bradford_cone`]).
fn bradford_cone_inverse([l, m, s]: [f32; 3]) -> [f32; 3] {
    [
        0.986_992_9 * l - 0.147_054_3 * m + 0.159_962_7 * s,
        0.432_305_3 * l + 0.518_360_3 * m + 0.049_291_2 * s,
        -0.008_528_7 * l + 0.040_042_8 * m + 0.968_486_7 * s,
    ]
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
    use pdfcer_core::document::Document;
    use pdfcer_core::page_tree;

    use crate::{RenderOptions, RenderedPage, render_page_with};

    /// ★★★ Two colorant names that differ only in INVALID UTF-8 bytes must not
    /// compare equal.
    ///
    /// # The defect this pins
    ///
    /// `Colorant::parse` built a `String` with `String::from_utf8_lossy`, which
    /// maps **every** invalid byte sequence to `U+FFFD`. Two documents naming
    /// two different colorants therefore produced the *same* `Colorant`.
    ///
    /// §7.3.5 NOTE 4 is explicit that names differing in bytes are distinct
    /// names even if they render identically, and §8.6.6.4's device test
    /// consults only the name — so this is an identity, not a label.
    ///
    /// # Why it is worth a test when nothing currently keys on it
    ///
    /// Because the per-spot-colorant plane will, and at that point a collision
    /// stops being invisible and starts meaning **two inks share one plate**.
    /// The corpus census that found this turned up `U+FFFD` in real colorant
    /// names in more than one file, so the input is not hypothetical.
    ///
    /// Verified to fail before the fix: both sides became `"Spot\u{FFFD}"` and
    /// the assertion below compared equal.
    #[test]
    fn colorant_names_differing_only_in_invalid_bytes_stay_distinct() {
        // Two different invalid continuation bytes after the same prefix.
        let a = Colorant::parse(b"Spot\xC0");
        let b = Colorant::parse(b"Spot\xC1");
        assert_ne!(
            a, b,
            "★ two DIFFERENT colorant names compared equal. A lossy decode \
             folds every invalid byte sequence onto U+FFFD, and a colorant \
             name is an identity (7.3.5 NOTE 4, 8.6.6.4) — so this would give \
             two inks one plane once the spot buffer exists"
        );
        // And the valid-UTF-8 path is unchanged, so the fix did not buy
        // distinctness by mangling ordinary names.
        assert_eq!(
            Colorant::parse(b"PANTONE 185 C"),
            Colorant::parse(b"PANTONE 185 C")
        );
        assert_ne!(Colorant::parse(b"Cyan"), Colorant::parse(b"cyan"));
        // `/All` and `/None` are still classified, not treated as names.
        assert_eq!(Colorant::parse(b"All"), Colorant::All);
        assert_eq!(Colorant::parse(b"None"), Colorant::None);
    }

    /// Build an offset-consistent classic PDF from `(number, body)` pairs.
    fn build(objects: &[(u32, String)]) -> Vec<u8> {
        let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets: Vec<(u32, usize)> = Vec::new();
        for (num, body) in objects {
            offsets.push((*num, buf.len()));
            buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
        }
        let xref_at = buf.len();
        let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
        buf.extend_from_slice(format!("xref\n0 {}\n", max_num + 1).as_bytes());
        buf.extend_from_slice(b"0000000000 65535 f \n");
        for num in 1..=max_num {
            match offsets.iter().find(|(n, _)| *n == num) {
                Some((_, off)) => {
                    buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
                }
                None => buf.extend_from_slice(b"0000000000 65535 f \n"),
            }
        }
        buf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R /ID [<0102> <0304>] >>\n\
                 startxref\n{xref_at}\n%%EOF\n",
                max_num + 1
            )
            .as_bytes(),
        );
        buf
    }

    /// Render a 100×100 page whose content is `content` and whose
    /// `/Resources` dictionary is `resources`, plus any `extra` objects the
    /// resources refer to (numbered from 5 upward).
    fn render(content: &str, resources: &str, extra: &[(u32, String)]) -> RenderedPage {
        let mut objects: Vec<(u32, String)> = vec![
            (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
            (
                2,
                format!(
                    "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] \
                     /Resources {resources} >>"
                ),
            ),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_owned(),
            ),
            (
                4,
                format!(
                    "<< /Length {} >>\nstream\n{content}endstream",
                    content.len()
                ),
            ),
        ];
        objects.extend(extra.iter().cloned());
        let doc = Document::from_bytes(build(&objects)).expect("fixture parses");
        let page = page_tree::pages(&doc).expect("page tree").remove(0);
        render_page_with(&doc, &page, 1.0, &RenderOptions::default()).expect("render")
    }

    /// The demultiplied RGB of one device pixel.
    fn pixel(rendered: &RenderedPage, x: u32, y: u32) -> (u8, u8, u8) {
        let p = rendered
            .pixmap
            .pixel(x, y)
            .expect("pixel in range")
            .demultiply();
        (p.red(), p.green(), p.blue())
    }

    /// A rectangle covering the page's middle, so `pixel(_, 50, 50)` is
    /// always inside it.
    const RECT: &str = "10 10 80 80 re f\n";

    fn near(got: u8, want: u8, tol: u8) -> bool {
        got.abs_diff(want) <= tol
    }

    // ---- the headline defect -------------------------------------------

    /// **THE DEFECT.** `cs` + `scn` must change the colour that is actually
    /// PAINTED — not merely some interpreter state.
    ///
    /// Until 2026-08-10 `cs`/`CS`/`sc`/`scn`/`SC`/`SCN` were all in
    /// `interpret.rs`'s "recognized, deferred to later slices" arm. A stream
    /// that selected a colour space and set a colour therefore kept whatever
    /// colour was previously in force — on a fresh page, black — and painted
    /// real marks with it, with no diagnostic. On a CAD drawing using spot
    /// colours, every line came out wrong.
    ///
    /// The assertion is on PIXELS on purpose: an implementation can track
    /// the space correctly and still paint stale, and it was the paint that
    /// was wrong.
    #[test]
    fn cs_then_scn_changes_the_painted_colour() {
        let rendered = render(
            &format!("/CS0 cs 1 0 0 scn\n{RECT}"),
            "<< /ColorSpace << /CS0 [/ICCBased 5 0 R] >> >>",
            &[(
                5,
                "<< /N 3 /Alternate /DeviceRGB /Length 0 >>\nstream\nendstream".to_owned(),
            )],
        );
        let (r, g, b) = pixel(&rendered, 50, 50);
        assert!(
            near(r, 255, 1) && near(g, 0, 1) && near(b, 0, 1),
            "cs + scn must paint red, got ({r}, {g}, {b}) — a black pixel here is \
             the deferred-operator defect: the colour operators ran, changed \
             nothing, and the fill used the initial black"
        );
    }

    /// The control that gives the test above its meaning: without the `cs`,
    /// the same `scn` operands cannot mean red, and the fill stays black.
    /// Without this, the test above would also pass on a renderer that
    /// hard-coded "`scn` means DeviceRGB".
    #[test]
    fn scn_without_a_matching_space_does_not_paint_that_colour() {
        let rendered = render(&format!("1 0 0 scn\n{RECT}"), "<< >>", &[]);
        assert_eq!(
            pixel(&rendered, 50, 50),
            (0, 0, 0),
            "three operands in the initial DeviceGray space are an arity \
             mismatch; the colour must stay at its initial black"
        );
    }

    /// `cs`/`CS` "shall **also** set the current colour to its initial
    /// value" (§8.6.8), and the values are not uniform: all-zeros for most
    /// spaces but `[0 0 0 1]` for `DeviceCMYK`, where all-zeros would be
    /// **white**. Painting white where the document expects black is the
    /// exact trap that clause exists to close.
    #[test]
    fn cs_installs_the_per_space_initial_colour() {
        // Start from white so "did the initial colour get installed?" is
        // observable at all.
        let rendered = render(&format!("1 1 1 rg /DeviceCMYK cs\n{RECT}"), "<< >>", &[]);
        let (r, _, _) = pixel(&rendered, 50, 50);
        assert!(
            r < 60,
            "DeviceCMYK's initial colour is [0 0 0 1] — solid black ink, a warm \
             near-black after conversion — not white; got r={r}"
        );
    }

    // ---- Indexed (§8.6.6.3) --------------------------------------------

    /// An `Indexed` lookup **at** `hival` must return the last palette
    /// entry, and one **past** it must clamp to that same entry rather than
    /// wrapping or reading out of bounds.
    ///
    /// `hival` is a maximum INDEX, not a count, so the table has `hival + 1`
    /// entries — the off-by-one that truncates the last colour. And the
    /// out-of-range rule is one of the few places §8.6.6.3 makes clamping
    /// **normative**, and it is a clamp, not a modulo: index 5 with
    /// `hival 2` is 2, not 1.
    #[test]
    fn indexed_lookup_at_hival_and_past_it() {
        let space = ColorSpace::Indexed {
            base: Arc::new(ColorSpace::DeviceRgb),
            hival: 2,
            // red, green, blue
            lookup: Arc::from(vec![255, 0, 0, 0, 255, 0, 0, 0, 255].into_boxed_slice()),
        };
        let mut diag = ColorDiagnostics::default();
        let at = space
            .to_rgb(&[2.0], CmykIntent::default(), &mut diag)
            .unwrap();
        assert_eq!(
            (at.r, at.g, at.b),
            (0.0, 0.0, 1.0),
            "index == hival is blue"
        );
        assert_eq!(diag.indexed_index_clamped, 0, "2 is in range");

        let past = space
            .to_rgb(&[5.0], CmykIntent::default(), &mut diag)
            .unwrap();
        assert_eq!(
            (past.r, past.g, past.b),
            (0.0, 0.0, 1.0),
            "index 5 clamps to hival 2 (a clamp, NOT a modulo — a modulo would \
             give index 1, green)"
        );
        assert_eq!(diag.indexed_index_clamped, 1, "the clamp is disclosed");
    }

    /// A real operand "shall be rounded to the nearest integer" (§8.6.6.3),
    /// and a negative one clamps to 0 — both normative, both silent
    /// otherwise.
    #[test]
    fn indexed_rounds_reals_and_clamps_negatives() {
        let space = ColorSpace::Indexed {
            base: Arc::new(ColorSpace::DeviceRgb),
            hival: 2,
            lookup: Arc::from(vec![255, 0, 0, 0, 255, 0, 0, 0, 255].into_boxed_slice()),
        };
        let mut diag = ColorDiagnostics::default();
        let rounded = space
            .to_rgb(&[0.6], CmykIntent::default(), &mut diag)
            .unwrap();
        assert_eq!((rounded.r, rounded.g), (0.0, 1.0), "0.6 rounds to index 1");
        let negative = space
            .to_rgb(&[-3.0], CmykIntent::default(), &mut diag)
            .unwrap();
        assert_eq!((negative.r, negative.g), (1.0, 0.0), "-3 clamps to index 0");
        assert_eq!(diag.indexed_index_clamped, 1, "only the negative clamped");
    }

    /// Producers routinely trim trailing unused palette entries. A short
    /// table must not be a panic and must not be an out-of-bounds read: the
    /// entry paints black and says so.
    #[test]
    fn indexed_short_lookup_paints_black_and_is_counted() {
        let space = ColorSpace::Indexed {
            base: Arc::new(ColorSpace::DeviceRgb),
            hival: 3,
            // Only two entries for a four-entry table.
            lookup: Arc::from(vec![255, 0, 0, 0, 255, 0].into_boxed_slice()),
        };
        let mut diag = ColorDiagnostics::default();
        let got = space
            .to_rgb(&[3.0], CmykIntent::default(), &mut diag)
            .unwrap();
        assert_eq!((got.r, got.g, got.b), (0.0, 0.0, 0.0));
        assert_eq!(diag.indexed_lookup_short, 1);
    }

    /// An `Indexed` space really does select through the whole render path,
    /// including a hex-string lookup — the form §8.6.6.3's own example uses,
    /// and the one a stream-only reader fails on.
    #[test]
    fn indexed_selects_a_palette_entry_on_the_page() {
        let rendered = render(
            &format!("/CS0 cs 2 scn\n{RECT}"),
            "<< /ColorSpace << /CS0 [/Indexed /DeviceRGB 2 <FF000000FF000000FF>] >> >>",
            &[],
        );
        let (r, g, b) = pixel(&rendered, 50, 50);
        assert!(
            near(r, 0, 1) && near(g, 0, 1) && near(b, 255, 1),
            "index 2 of [red, green, blue] is blue, got ({r}, {g}, {b})"
        );
    }

    // ---- ICCBased (§8.6.5.5) -------------------------------------------

    /// `ICCBased` with an `/Alternate` renders through it, unchanged —
    /// Table 66: "There shall not be conversion of source colour values …
    /// when using the alternate colour space." A reinterpretation, not a
    /// conversion.
    #[test]
    fn iccbased_falls_back_to_its_alternate() {
        let rendered = render(
            &format!("/CS0 cs 0 0 1 scn\n{RECT}"),
            "<< /ColorSpace << /CS0 [/ICCBased 5 0 R] >> >>",
            &[(
                5,
                "<< /N 3 /Alternate /DeviceRGB /Length 0 >>\nstream\nendstream".to_owned(),
            )],
        );
        assert_eq!(pixel(&rendered, 50, 50), (0, 0, 255));
        assert_eq!(
            rendered.diagnostics.color.icc_alternate_used, 1,
            "the ICC fallback is disclosed, not silent"
        );
        assert_eq!(rendered.diagnostics.color.icc_device_fallback_used, 0);
    }

    /// With **no** `/Alternate`, Table 66's second sentence applies: the
    /// space used "shall be `DeviceGray`, `DeviceRGB`, or `DeviceCMYK`,
    /// depending on whether the value of `N` is 1, 3, or 4".
    ///
    /// `/N 1` is the discriminating case: a renderer that assumed three
    /// components would see an arity mismatch and paint nothing at all.
    #[test]
    fn iccbased_without_an_alternate_falls_back_to_the_device_space_for_n() {
        let rendered = render(
            &format!("/CS0 cs 0.5 scn\n{RECT}"),
            "<< /ColorSpace << /CS0 [/ICCBased 5 0 R] >> >>",
            &[(5, "<< /N 1 /Length 0 >>\nstream\nendstream".to_owned())],
        );
        let (r, g, b) = pixel(&rendered, 50, 50);
        assert!(
            near(r, 127, 2) && near(g, 127, 2) && near(b, 127, 2),
            "N 1 with no /Alternate is DeviceGray, so 0.5 is mid grey; got ({r}, {g}, {b})"
        );
        assert_eq!(rendered.diagnostics.color.icc_device_fallback_used, 1);
        assert_eq!(rendered.diagnostics.color.icc_alternate_used, 0);
    }

    /// A soft mask's `/BC` must convert by the SAME route painted content
    /// does.
    ///
    /// §11.6.5.2 makes the `/BC` backdrop's *magnitude* the mask value
    /// everywhere outside the mask group's `/BBox`, so it is not merely a
    /// polarity question. When `/BC` went through an inline naive
    /// complement while painted content went through the calibrated grid,
    /// one luminosity computation was fed by two different CMYK->sRGB
    /// routes and a `DeviceCMYK` mask disagreed with itself across its own
    /// bounding box.
    ///
    /// This pins the agreement rather than the numbers: if the calibrated
    /// table is ever retuned, this test still holds, which is the property
    /// worth having.
    #[test]
    fn a_soft_mask_backdrop_converts_by_the_painted_content_route() {
        for intent in [CmykIntent::NeutralBlack, CmykIntent::Calibrated] {
            for (c, m, y, k) in [
                (0.0, 0.0, 0.0, 1.0),
                (1.0, 1.0, 1.0, 1.0),
                (0.5, 0.4, 0.4, 0.0),
                (0.0, 0.0, 0.0, 0.0),
            ] {
                let painted = Rgb::from_cmyk(intent, c, m, y, k);
                let naive = Rgb {
                    r: (1.0 - c) * (1.0 - k),
                    g: (1.0 - m) * (1.0 - k),
                    b: (1.0 - y) * (1.0 - k),
                };
                // The point of the test is that these are DIFFERENT for at
                // least one intent, so routing `/BC` through the naive form
                // was a real divergence and not a harmless simplification.
                if intent == CmykIntent::Calibrated && (c, m, y, k) == (0.5, 0.4, 0.4, 0.0) {
                    assert!(
                        (painted.r - naive.r).abs() > 1e-4
                            || (painted.g - naive.g).abs() > 1e-4
                            || (painted.b - naive.b).abs() > 1e-4,
                        "if these ever agree, the divergence this test guards against has gone away and the test is vacuous",
                    );
                }
            }
        }
    }

    /// `/N 4` with no `/Alternate` lands on `DeviceCMYK`, which is the arm
    /// where the polarity differs: `0 0 0 0` is white, not black.
    #[test]
    fn iccbased_n4_falls_back_to_cmyk_with_cmyk_polarity() {
        let rendered = render(
            &format!("/CS0 cs 0 0 0 0 scn\n{RECT}"),
            "<< /ColorSpace << /CS0 [/ICCBased 5 0 R] >> >>",
            &[(5, "<< /N 4 /Length 0 >>\nstream\nendstream".to_owned())],
        );
        let (r, _, _) = pixel(&rendered, 50, 50);
        assert!(
            r > 250,
            "no ink in DeviceCMYK is paper white, not black; got r={r}"
        );
    }

    // ---- CIE-based spaces (§8.6.5.2–.4) --------------------------------

    /// A `Lab` value with a known answer.
    ///
    /// `L* = 100, a* = b* = 0` is the space's own white point by
    /// construction, and `L* = 50, a* = b* = 0` is the canonical mid grey:
    /// `g((50+16)/116) = 0.5690³ = 0.1842` relative luminance, which the
    /// sRGB transfer function encodes as ≈0.466.
    ///
    /// Neutral values are the right thing to pin here because they are
    /// invariant to the one part of the pipeline pdfcer *chose* rather than
    /// read from the standard (the Bradford adaptation and the sRGB matrix
    /// — see [`xyz_to_srgb`]): an adaptation maps source white exactly onto
    /// destination white, so the achromatic axis stays achromatic whatever
    /// white point the file declares.
    #[test]
    fn lab_converts_a_known_value() {
        // D50, the white point PDF CIE spaces overwhelmingly declare.
        let space = ColorSpace::Lab {
            white: [0.9642, 1.0, 0.8249],
            range: [-100.0, 100.0, -100.0, 100.0],
        };
        let mut diag = ColorDiagnostics::default();
        let white = space
            .to_rgb(&[100.0, 0.0, 0.0], CmykIntent::default(), &mut diag)
            .unwrap();
        assert!(
            (white.r - 1.0).abs() < 0.01 && (white.g - 1.0).abs() < 0.01,
            "L*=100 is the white point: {white:?}"
        );

        let grey = space
            .to_rgb(&[50.0, 0.0, 0.0], CmykIntent::default(), &mut diag)
            .unwrap();
        for c in [grey.r, grey.g, grey.b] {
            assert!(
                (c - 0.466).abs() < 0.01,
                "L*=50 encodes to ~0.466 in sRGB: {grey:?}"
            );
        }

        let black = space
            .to_rgb(&[0.0, 0.0, 0.0], CmykIntent::default(), &mut diag)
            .unwrap();
        assert!(
            black.r < 0.01 && black.g < 0.01 && black.b < 0.01,
            "{black:?}"
        );
    }

    /// `to_pcs_xyz` adapts the space's declared white to the PCS's D50, and
    /// the fixture that proves it must NOT declare D50 — on a D50 space the
    /// adaptation is the identity and a missing adaptation passes
    /// (`R225`; the three-ways `Lab` fixture is D50 and could not see this,
    /// which a sabotage sweep showed the same hour).
    ///
    /// `Lab (100, 0, 0)` is the space's own white by definition, so its PCS
    /// value must be the PCS white EXACTLY — for a D65-declared space that
    /// means the answer is D50, and an unadapted implementation returns D65
    /// instead: `Z` differs by 0.26, a third of its range.
    #[test]
    fn to_pcs_xyz_adapts_the_declared_white_to_d50() {
        let d65_lab = ColorSpace::Lab {
            white: D65,
            range: [-100.0, 100.0, -100.0, 100.0],
        };
        let got = d65_lab
            .to_pcs_xyz(&[100.0, 0.0, 0.0])
            .expect("Lab has a PCS answer");
        for (i, (g, w)) in got.iter().zip(PCS_D50.iter()).enumerate() {
            assert!(
                (g - w).abs() < 2e-3,
                "component {i}: the D65 white adapted to {got:?}, expected D50 {PCS_D50:?}"
            );
        }
        // A D50-declared space is the identity case, and a device space has
        // no PCS answer at all.
        let d50_lab = ColorSpace::Lab {
            white: PCS_D50,
            range: [-100.0, 100.0, -100.0, 100.0],
        };
        let same = d50_lab.to_pcs_xyz(&[100.0, 0.0, 0.0]).unwrap();
        for (g, w) in same.iter().zip(PCS_D50.iter()) {
            assert!((g - w).abs() < 1e-5);
        }
        assert_eq!(ColorSpace::DeviceRgb.to_pcs_xyz(&[1.0, 1.0, 1.0]), None);
        assert_eq!(ColorSpace::DeviceCmyk.to_pcs_xyz(&[0.0; 4]), None);
    }

    /// The `a*` and `b*` axes must move the hue in the documented
    /// directions: `+a*` toward red, `+b*` toward yellow. This is what
    /// catches a transposed matrix or a sign slip in `L = M + a*/500`,
    /// neither of which the neutral test above can see.
    #[test]
    fn lab_axes_point_the_right_way() {
        let space = ColorSpace::Lab {
            white: [0.9642, 1.0, 0.8249],
            range: [-100.0, 100.0, -100.0, 100.0],
        };
        let mut diag = ColorDiagnostics::default();
        let red = space
            .to_rgb(&[50.0, 60.0, 0.0], CmykIntent::default(), &mut diag)
            .unwrap();
        // `+a*` with `b* = 0` lands magenta-ward, not on a pure red: the
        // a* axis is red-green, and with no yellow contribution the blue
        // channel stays up. The invariant is "red dominates, green is
        // suppressed", not a full channel ordering.
        assert!(
            red.r > red.g && red.r > red.b && red.b > red.g,
            "+a* raises red and suppresses green: {red:?}"
        );
        let yellow = space
            .to_rgb(&[80.0, 0.0, 70.0], CmykIntent::default(), &mut diag)
            .unwrap();
        assert!(
            yellow.r > yellow.b && yellow.g > yellow.b,
            "+b* is yellow-ward: {yellow:?}"
        );
    }

    /// `g(x)`'s piecewise branch is the part implementations drop. Below the
    /// 6/29 breakpoint the linear segment must be used, and the two halves
    /// must meet: `g` is continuous at the breakpoint by construction.
    #[test]
    fn lab_g_is_piecewise_and_continuous() {
        let b = 6.0f32 / 29.0;
        assert!((lab_g(b) - b * b * b).abs() < 1e-6, "cubic side at 6/29");
        assert!(
            (lab_g(b - 1e-5) - lab_g(b)).abs() < 1e-4,
            "the linear segment meets the cubic at the breakpoint"
        );
        // The linear branch is the TANGENT to x^3 at the breakpoint (slope
        // 108/841 = 3*(6/29)^2), which is what makes the join smooth rather
        // than merely continuous.
        let slope = (lab_g(b) - lab_g(b - 0.01)) / 0.01;
        assert!(
            (slope - 3.0 * b * b).abs() < 1e-3,
            "the linear segment is the tangent at 6/29, slope {slope}"
        );
        // And the consequence the branch actually buys: it reaches exactly
        // zero at 4/29, which is where `L* = 0` lands. The cubic alone gives
        // 0.00266 there, so black would not be black.
        let zero = 4.0f32 / 29.0;
        assert!(lab_g(zero).abs() < 1e-7, "g(4/29) is exactly 0");
        assert!(
            zero * zero * zero > 1e-3,
            "the cubic at 4/29 is NOT zero - that is the whole point"
        );
    }

    /// `CalGray` decodes through `Gamma` and lands on a neutral. Its own
    /// white point is the reference, so a `CalGray` of 1.0 is white however
    /// exotic that white point is.
    #[test]
    fn cal_gray_honours_gamma() {
        let mut diag = ColorDiagnostics::default();
        let linear = ColorSpace::CalGray {
            white: [0.9642, 1.0, 0.8249],
            gamma: 1.0,
        };
        let gamma_two = ColorSpace::CalGray {
            white: [0.9642, 1.0, 0.8249],
            gamma: 2.0,
        };
        let a = linear
            .to_rgb(&[0.5], CmykIntent::default(), &mut diag)
            .unwrap();
        let b = gamma_two
            .to_rgb(&[0.5], CmykIntent::default(), &mut diag)
            .unwrap();
        assert!(
            b.r < a.r,
            "0.5^2 < 0.5^1, so a gamma of 2 is darker: {b:?} vs {a:?}"
        );
        let white = linear
            .to_rgb(&[1.0], CmykIntent::default(), &mut diag)
            .unwrap();
        assert!(
            (white.r - 1.0).abs() < 0.01,
            "A=1 is the white point: {white:?}"
        );
    }

    /// `CalRGB`'s `Matrix` is grouped by INPUT component
    /// (`[XA YA ZA XB YB ZB XC YC ZC]`), so reading it as three XYZ rows
    /// transposes it — silent on the default identity, wrong on every real
    /// one. A matrix that routes only the B input to XYZ pins the
    /// orientation.
    #[test]
    fn cal_rgb_matrix_is_grouped_by_input_component() {
        let mut diag = ColorDiagnostics::default();
        // XA YA ZA | XB YB ZB | XC YC ZC — the B column alone carries the
        // white point, so only the middle input should produce anything.
        let space = ColorSpace::CalRgb {
            white: [0.9642, 1.0, 0.8249],
            gamma: [1.0, 1.0, 1.0],
            matrix: [
                0.0, 0.0, 0.0, // A contributes nothing
                0.9642, 1.0, 0.8249, // B is the whole white point
                0.0, 0.0, 0.0, // C contributes nothing
            ],
        };
        let from_a = space
            .to_rgb(&[1.0, 0.0, 0.0], CmykIntent::default(), &mut diag)
            .unwrap();
        let from_b = space
            .to_rgb(&[0.0, 1.0, 0.0], CmykIntent::default(), &mut diag)
            .unwrap();
        assert!(from_a.r < 0.01, "the A column is all zeros: {from_a:?}");
        assert!(
            (from_b.r - 1.0).abs() < 0.01 && (from_b.g - 1.0).abs() < 0.01,
            "the B column is the white point, so B=1 is white: {from_b:?}"
        );
    }

    /// §8.6.5.3's own EXAMPLE, used as the column-major oracle.
    ///
    /// The clause's example dictionary pairs
    /// `/Matrix [0.4497 0.2446 0.0252  0.3163 0.6720 0.1412  0.1845 0.0833
    /// 0.9227]` with `/WhitePoint [0.9505 1.0000 1.0890]` — and those nine
    /// numbers sum, **column-wise**, to exactly that white point
    /// (0.9505 / 0.9999 / 1.0891). So `(1, 1, 1)` in this space must be the
    /// white point, i.e. white.
    ///
    /// Loaded row-major the same nine numbers sum to (0.7195, 1.1295,
    /// 1.1905), which is not the white point and not anything else — the
    /// error is invisible on the default identity matrix and wrong on every
    /// real one, which is exactly why it earns a dedicated test.
    #[test]
    fn cal_rgb_example_matrix_maps_one_one_one_to_white() {
        let mut diag = ColorDiagnostics::default();
        let space = ColorSpace::CalRgb {
            white: [0.9505, 1.0, 1.0890],
            gamma: [1.0, 1.0, 1.0],
            matrix: [
                0.4497, 0.2446, 0.0252, // the A (red) primary's XYZ
                0.3163, 0.6720, 0.1412, // the B (green) primary's XYZ
                0.1845, 0.0833, 0.9227, // the C (blue) primary's XYZ
            ],
        };
        let white = space
            .to_rgb(&[1.0, 1.0, 1.0], CmykIntent::default(), &mut diag)
            .unwrap();
        assert!(
            (white.r - 1.0).abs() < 0.01
                && (white.g - 1.0).abs() < 0.01
                && (white.b - 1.0).abs() < 0.01,
            "the EXAMPLE matrix's columns sum to its white point, so (1,1,1) \
             is white; a transposed load gives something else entirely: {white:?}"
        );
    }

    /// `Lab (0, 0, 0)` is **exactly** XYZ (0, 0, 0) for any white point,
    /// because `L* = 0` makes `M = 16/116 = 4/29`, which is the exact zero
    /// of `g`'s linear branch. A free oracle that pins the `4/29` offset:
    /// get it wrong and black stops being black.
    #[test]
    fn lab_zero_is_exactly_black_for_any_white_point() {
        for white in [[0.9642, 1.0, 0.8249], [0.9505, 1.0, 1.0890]] {
            let xyz = lab_to_xyz([0.0, 0.0, 0.0], white);
            for c in xyz {
                assert!(c.abs() < 1e-6, "Lab(0,0,0) is XYZ zero: {xyz:?}");
            }
        }
    }

    /// `CalGray(1.0)` with `Gamma 1` is exactly the white point, and
    /// `CalRGB(1,1,1)` under the default identity matrix is the sum of its
    /// columns — both stated by §8.6.5.2/.3's transforms directly, and both
    /// cheap regression anchors on the `powf` handling.
    #[test]
    fn cal_gray_one_is_exactly_the_white_point() {
        let white = [0.9642, 1.0, 0.8249];
        let xyz = cal_gray_to_xyz(1.0, white, 1.0);
        assert_eq!(xyz, white);
        // Gamma has no effect at A = 1 (1^G == 1 for every G).
        assert_eq!(cal_gray_to_xyz(1.0, white, 2.2), white);
    }

    // ---- Separation / DeviceN (§8.6.6.4–.5) ----------------------------

    /// ★ **A `Separation` renders the document's own colour, through its
    /// own tint transform.**
    ///
    /// The fixture's transform is a Type 2 exponential onto
    /// `[0.84 0 0.44 0.21]` CMYK — a green. Before `pdfcer_core::function`
    /// existed this module could only render a NEUTRAL of the right
    /// lightness, because the mapping from tint to hue lives entirely
    /// inside that function and cannot be guessed. A spot-coloured
    /// drawing came out grey.
    ///
    /// # This test previously asserted the opposite, and that is the point
    ///
    /// It read `tint_transform_not_applied == 2` and carried a comment
    /// saying it was "the counter that must go to zero — without any other
    /// change to this module's surface — when `pdfcer_core::function` is
    /// wired in".
    ///
    /// That is exactly what happened. The counter went to zero, this test
    /// failed, and the failure was the notification that the integration
    /// worked. A test written to fail on success is worth more than a
    /// comment predicting it, because only one of the two interrupts you.
    #[test]
    fn separation_renders_its_documents_own_colour_through_the_tint_transform() {
        let rendered = render(
            &format!("/CS0 cs 1 scn\n{RECT}"),
            "<< /ColorSpace << /CS0 [/Separation /LogoGreen /DeviceCMYK 5 0 R] >> >>",
            &[(
                5,
                "<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0.84 0 0.44 0.21] /N 1 >>"
                    .to_owned(),
            )],
        );
        let (r, g, b) = pixel(&rendered, 50, 50);
        // Green: the transform's C1 has no magenta and heavy cyan/yellow,
        // so the green channel must dominate. A neutral — the old
        // behaviour — would have r == g == b, which this rules out.
        assert!(
            g > r && g > b,
            "the document's transform yields a GREEN; a neutral stand-in would be \
             grey. got r={r} g={g} b={b}"
        );
        assert_eq!(
            rendered.diagnostics.color.tint_transform_not_applied, 0,
            "nothing was approximated"
        );
        assert!(
            rendered.diagnostics.color.tint_transforms_applied >= 1,
            "and the evaluation is reported, so a shell can say the spot colours \
             on this page are the document's own"
        );
    }

    /// **A `Separation` whose transform is missing still paints, and says
    /// it approximated.**
    ///
    /// The companion to the test above, and the reason the neutral
    /// stand-in was kept rather than deleted once the evaluator landed.
    /// `tintTransform` is a Required element, so this file is malformed —
    /// but the colorant name is still meaningful and the drawing is still
    /// worth showing, at the right lightness, with the shortfall counted.
    ///
    /// Refusing to paint would lose real content over a missing function;
    /// painting silently would claim a hue pdfcer invented.
    #[test]
    fn a_separation_with_no_tint_transform_falls_back_and_is_counted() {
        let rendered = render(
            &format!("/CS0 cs 1 scn\n{RECT}"),
            "<< /ColorSpace << /CS0 [/Separation /LogoGreen /DeviceCMYK] >> >>",
            &[],
        );
        let (r, g, b) = pixel(&rendered, 50, 50);
        // ★ NEUTRAL IN CMYK, NOT NEUTRAL IN sRGB — and the difference arrived
        // with `Pass 153.0`.
        //
        // The stand-in is built achromatic: equal C, M and Y with the tint in
        // K. What it RENDERS as depends on `CmykIntent`, and until 2026-08-28
        // the default was `NeutralBlack`, which forces a pure-K colour to an
        // exactly neutral grey — so `r == g == b` held on the nose and the
        // test asserted it.
        //
        // The default is now `Calibrated`, whose whole documented consequence
        // is that CMYK neutrals are *slightly cool* (solid K renders `#231F20`,
        // not `#000000`). So exact equality is now false for a correct render,
        // and this test failed on the intent change rather than on any change
        // to the fallback it is about.
        //
        // Asserted as a measured spread rather than relaxed to nothing: the
        // claim worth keeping is that pdfcer invents no HUE for a missing tint
        // transform, and a handful of levels of calibrated coolness is not a
        // hue. The observed spread is 4 (35 vs 31); 12 leaves room for table
        // revisions without admitting a green or a magenta.
        let spread = i32::from(r.max(g).max(b)) - i32::from(r.min(g).min(b));
        assert!(
            spread <= 12,
            "the stand-in must invent no hue: got r={r} g={g} b={b}, spread {spread}"
        );
        assert!(r < 60, "and full tint is still the darkest value");
        assert!(
            rendered.diagnostics.color.tint_transform_not_applied >= 1,
            "the approximation is disclosed"
        );
    }

    /// `Separation /None` "shall not produce any visible output. Painting
    /// operations … shall have no effect on the current page" (§8.6.6.4).
    ///
    /// This is a **correctness** rule, not a fidelity one: content the
    /// author marked invisible becoming visible is worse than a colour being
    /// slightly off. Painting white would be equally wrong — it would erase
    /// the backdrop.
    #[test]
    fn separation_none_paints_nothing() {
        let rendered = render(
            // Paint a black rectangle first, then try to paint over it in
            // /None. The first must survive.
            &format!("0 g 20 20 60 60 re f /CS0 cs 1 scn\n{RECT}"),
            "<< /ColorSpace << /CS0 [/Separation /None /DeviceGray 5 0 R] >> >>",
            &[(
                5,
                "<< /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >>".to_owned(),
            )],
        );
        assert_eq!(
            pixel(&rendered, 50, 50),
            (0, 0, 0),
            "the /None fill must not overpaint the black rectangle"
        );
        assert_eq!(rendered.diagnostics.color.separation_none_suppressed, 1);
    }

    /// A `DeviceN` takes one operand per name (§8.6.6.5). The maximum over
    /// the non-`/None` components is pdfcer's stand-in for the transform, so
    /// a single strong colorant darkens the result rather than being
    /// averaged away.
    #[test]
    fn device_n_takes_one_operand_per_name_and_discloses_the_transform() {
        let rendered = render(
            &format!("/CS0 cs 1 0 0 scn\n{RECT}"),
            "<< /ColorSpace << /CS0 [/DeviceN [/Spot1 /Spot2 /Spot3] /DeviceCMYK 5 0 R] >> >>",
            &[(
                5,
                "<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0 0 1] /N 1 >>".to_owned(),
            )],
        );
        let (r, _, _) = pixel(&rendered, 50, 50);
        assert!(r < 60, "max tint 1.0 is fully inked; got r={r}");
        // Two, not one: `cs` installs the space's all-1.0 initial colour
        // (§8.6.8) and `scn` then sets one, and BOTH are colours that could
        // have been painted, so both conversions are disclosed.
        assert_eq!(rendered.diagnostics.color.tint_transform_not_applied, 2);
        assert_eq!(
            rendered.diagnostics.color.colors_not_set, 0,
            "three operands for a three-name DeviceN is the right arity"
        );
    }

    // ---- Pattern (§8.6.6.2) --------------------------------------------

    /// `scn` may name a **pattern**, and a pattern pdfcer cannot draw paints
    /// NOTHING and is counted — rather than drawing a solid fill in a stale
    /// colour, which is what the deferred arm used to do and is worse than
    /// a gap.
    ///
    /// The pattern here is unpaintable for a specific reason: its shading
    /// carries no `/Function`, which §8.7.4.5.3 requires of a type-2
    /// shading, so the model refuses to load. That is deliberate now that
    /// `PatternType 2` fills ARE painted (see
    /// `crates/pdfcer-render/tests/shading_pattern_anchoring.rs`) — this
    /// test's subject is the REFUSAL path, and it would silently stop
    /// testing that if the fixture were ever made well-formed.
    #[test]
    fn an_unpaintable_pattern_is_recognised_and_left_unpainted() {
        let rendered = render(
            &format!("0 g 20 20 60 60 re f /Pattern cs /P0 scn\n{RECT}"),
            "<< /Pattern << /P0 5 0 R >> >>",
            &[(
                5,
                "<< /PatternType 2 /Shading << /ShadingType 2 /ColorSpace /DeviceRGB \
                 /Coords [0 0 100 100] >> >>"
                    .to_owned(),
            )],
        );
        assert_eq!(
            pixel(&rendered, 50, 50),
            (0, 0, 0),
            "nothing is painted in a Pattern space, so the black rectangle survives"
        );
        assert_eq!(rendered.diagnostics.color.pattern_spaces_selected, 1);
        assert_eq!(rendered.diagnostics.color.patterns_unpainted, 1);
    }

    // ---- refusals, counted rather than guessed -------------------------

    /// **An unresolvable colour space must not silently become
    /// `DeviceGray`.** It is counted, the previous colour stays in force,
    /// and the subsequent `sc` is counted too.
    ///
    /// Defaulting to `DeviceGray` would paint marks that look exactly like a
    /// correct render — the worst possible failure mode for a renderer whose
    /// whole diagnostics contract is that the operator can tell a faithful
    /// raster from a partial one.
    #[test]
    fn an_unresolvable_space_is_counted_not_defaulted_to_gray() {
        let rendered = render(
            // Blue first, then a space that does not exist, then a value
            // that WOULD be mid grey if the space had silently become
            // DeviceGray.
            &format!("0 0 1 rg /NoSuchCS cs 0.5 sc\n{RECT}"),
            "<< /ColorSpace << /Other /DeviceRGB >> >>",
            &[],
        );
        assert_eq!(
            pixel(&rendered, 50, 50),
            (0, 0, 255),
            "the previous colour stays in force; a grey pixel here means the \
             unresolved space silently became DeviceGray"
        );
        assert_eq!(rendered.diagnostics.color.spaces_unresolved, 1);
        assert_eq!(rendered.diagnostics.color.colors_not_set, 1);
        assert!(
            rendered
                .diagnostics
                .color
                .notes
                .iter()
                .any(|n| n.contains("NoSuchCS")),
            "the note names the space, so an operator can fix the file: {:?}",
            rendered.diagnostics.color.notes
        );
    }

    /// ★ §8.6.6.3 — an `Indexed` operand is an INDEX, and
    /// [`ColorSpace::indexed_entry`] is what turns it into the colour the
    /// palette selects, in the base space.
    ///
    /// This is a *classification* path, not a painting one: everything that
    /// paints already resolves the palette inside `to_rgb`, which is
    /// exactly why the missing resolution was invisible on screen and
    /// visible only to overprint, which asks a question about the space
    /// rather than about the pixel.
    #[test]
    fn indexed_entry_returns_the_palette_colour_in_the_base_space() {
        // Two entries in DeviceRGB: black, then a known non-neutral.
        let space = ColorSpace::Indexed {
            base: std::sync::Arc::new(ColorSpace::DeviceRgb),
            hival: 1,
            lookup: std::sync::Arc::from(vec![0_u8, 0, 0, 255, 128, 0].into_boxed_slice()),
        };
        let (base, comps) = space.indexed_entry(&[1.0]).expect("Indexed resolves");
        assert!(matches!(base, ColorSpace::DeviceRgb));
        assert!((comps[0] - 1.0).abs() < 1e-3, "{comps:?}");
        assert!((comps[1] - 128.0 / 255.0).abs() < 1e-3, "{comps:?}");
        assert!((comps[2] - 0.0).abs() < 1e-3, "{comps:?}");

        // A non-Indexed space returns None, so the caller can call this
        // unconditionally and keep what it had.
        assert!(
            ColorSpace::DeviceRgb
                .indexed_entry(&[1.0, 0.0, 0.0])
                .is_none()
        );
    }

    /// §8.6.6.3's clamp, on the classification path too.
    ///
    /// The painting path already clamps and COUNTS the clamp; this one
    /// clamps and deliberately does not count, because counting in both
    /// would make one malformed palette read as two separate findings.
    #[test]
    fn indexed_entry_clamps_an_out_of_range_index_without_double_counting() {
        let space = ColorSpace::Indexed {
            base: std::sync::Arc::new(ColorSpace::DeviceGray),
            hival: 1,
            lookup: std::sync::Arc::from(vec![0_u8, 255].into_boxed_slice()),
        };
        let (_, high) = space.indexed_entry(&[9.0]).expect("Indexed resolves");
        assert!((high[0] - 1.0).abs() < 1e-3, "index 9 clamps to hival 1");
        let (_, low) = space.indexed_entry(&[-4.0]).expect("Indexed resolves");
        assert!((low[0] - 0.0).abs() < 1e-3, "a negative index clamps to 0");
    }

    /// A lookup table shorter than the entry it is asked for yields the
    /// base space's all-zero components rather than reading past the end.
    ///
    /// Producers routinely trim trailing unused entries, so this is a real
    /// file shape and not a fuzzing artefact.
    #[test]
    fn indexed_entry_survives_a_short_lookup_table() {
        let space = ColorSpace::Indexed {
            base: std::sync::Arc::new(ColorSpace::DeviceRgb),
            hival: 7,
            lookup: std::sync::Arc::from(vec![0_u8, 0, 0].into_boxed_slice()),
        };
        let (_, comps) = space.indexed_entry(&[5.0]).expect("Indexed resolves");
        assert_eq!(comps.len(), 3);
        assert!(comps.iter().all(|v| v.abs() < 1e-6), "{comps:?}");
    }

    /// An operand count that disagrees with the space has no spec-defined
    /// recovery, so pdfcer refuses rather than padding with zeros — padding
    /// would paint a colour nobody asked for.
    #[test]
    fn a_wrong_operand_count_leaves_the_colour_unchanged() {
        let rendered = render(
            &format!("0 0 1 rg /DeviceCMYK cs 0.2 0.3 sc\n{RECT}"),
            "<< >>",
            &[],
        );
        let (r, _, _) = pixel(&rendered, 50, 50);
        assert!(
            r < 60,
            "cs installed DeviceCMYK's initial [0 0 0 1]; the malformed sc must \
             have changed nothing, so the fill is still black ink (got r={r})"
        );
        assert_eq!(rendered.diagnostics.color.colors_not_set, 1);
    }

    /// The colour SPACE is graphics state (Table 52), so `q`/`Q` must save
    /// and restore it — not just the colour. Without the parallel stack, a
    /// trailing `sc` would be read in a space the `Q` had already discarded.
    #[test]
    fn q_and_q_save_and_restore_the_colour_space() {
        let mut state = ColorState::new();
        assert_eq!(state.space(false), Some(&ColorSpace::DeviceGray));
        state.push();
        state.set_device(DeviceSpace::Cmyk, &[0.0, 0.0, 0.0, 1.0], false);
        assert_eq!(state.space(false), Some(&ColorSpace::DeviceCmyk));
        state.pop();
        assert_eq!(
            state.space(false),
            Some(&ColorSpace::DeviceGray),
            "Q restores the space the q captured"
        );
    }

    /// The stroking and non-stroking halves are fully independent — the
    /// uppercase/lowercase operator split maps onto two separate parameters,
    /// and `B` paints both in one operator.
    #[test]
    fn the_two_halves_are_independent() {
        let mut state = ColorState::new();
        state.set_device(DeviceSpace::Cmyk, &[0.0, 0.0, 0.0, 1.0], true);
        assert_eq!(state.space(true), Some(&ColorSpace::DeviceCmyk));
        assert_eq!(
            state.space(false),
            Some(&ColorSpace::DeviceGray),
            "CS must not disturb the non-stroking space"
        );
    }

    /// Component counts drive `sc`/`scn` parsing, so they are pinned per
    /// family (§8.6, "Operand counts"). `Pattern` is 0 because its operand
    /// is a name.
    #[test]
    fn component_counts_match_the_operand_table() {
        assert_eq!(ColorSpace::DeviceGray.components(), 1);
        assert_eq!(ColorSpace::DeviceRgb.components(), 3);
        assert_eq!(ColorSpace::DeviceCmyk.components(), 4);
        assert_eq!(
            ColorSpace::Lab {
                white: [0.9642, 1.0, 0.8249],
                range: [-100.0, 100.0, -100.0, 100.0],
            }
            .components(),
            3
        );
        assert_eq!(
            ColorSpace::Indexed {
                base: Arc::new(ColorSpace::DeviceCmyk),
                hival: 3,
                lookup: Arc::from(vec![0u8; 16].into_boxed_slice()),
            }
            .components(),
            1,
            "an Indexed colour is ONE index, whatever the base space is"
        );
        assert_eq!(ColorSpace::Pattern { underlying: None }.components(), 0);
    }

    /// §8.6.8's initial-colour table, including the rows that are not
    /// all-zeros and are therefore the ones that get implemented wrong.
    #[test]
    fn initial_colours_follow_table_74() {
        assert_eq!(ColorSpace::DeviceGray.initial_color(), vec![0.0]);
        assert_eq!(
            ColorSpace::DeviceCmyk.initial_color(),
            vec![0.0, 0.0, 0.0, 1.0],
            "black via K, where all-zeros would be white"
        );
        assert_eq!(
            ColorSpace::Separation {
                colorant: Colorant::Named(b"LogoGreen".as_slice().into()),
                tint: None,
                alternate: Arc::new(ColorSpace::DeviceCmyk),
            }
            .initial_color(),
            vec![1.0],
            "full colorant — the DARKEST value, not the lightest"
        );
        // Lab's zeros are clamped into /Range, which for a one-sided range
        // is not zero at all.
        assert_eq!(
            ColorSpace::Lab {
                white: [0.9642, 1.0, 0.8249],
                range: [20.0, 80.0, -100.0, 100.0],
            }
            .initial_color(),
            vec![0.0, 20.0, 0.0]
        );
    }

    /// The four reserved names "ALWAYS identify the corresponding colour
    /// spaces directly; they NEVER refer to resources in the `ColorSpace`
    /// subdictionary" (§8.6). A resource shadowing `/DeviceRGB` must be
    /// unreachable — looking it up would be a bug, and a file could
    /// weaponise it.
    #[test]
    fn reserved_names_never_reach_the_resource_dictionary() {
        let rendered = render(
            &format!("/DeviceRGB cs 1 0 0 scn\n{RECT}"),
            // A hostile resource that would make /DeviceRGB one-component.
            "<< /ColorSpace << /DeviceRGB /DeviceGray >> >>",
            &[],
        );
        assert_eq!(
            pixel(&rendered, 50, 50),
            (255, 0, 0),
            "/DeviceRGB is the device space, never the shadowing resource"
        );
        assert_eq!(rendered.diagnostics.color.colors_not_set, 0);
    }

    /// A self-referential resource must terminate. `/CS0` whose definition
    /// is an `Indexed` over `/CS0` is unbounded recursion on untrusted input
    /// without the depth guard (ARCHITECTURE.md §10).
    #[test]
    fn a_self_referential_colour_space_terminates() {
        let rendered = render(
            &format!("/CS0 cs 0 scn\n{RECT}"),
            "<< /ColorSpace << /CS0 [/Indexed /CS0 1 <0000>] >> >>",
            &[],
        );
        assert_eq!(
            rendered.diagnostics.color.spaces_unresolved, 1,
            "the cycle is refused and counted, not followed"
        );
    }

    /// `cs`/`CS`/`sc`/`scn` must no longer land in the interpreter's
    /// "recognized, deferred" arm. This asserts on the *other* side of the
    /// fix — the diagnostic that used to fire — so a revert cannot pass by
    /// leaving the operators wired up but dead.
    #[test]
    fn the_colour_operators_are_no_longer_deferred() {
        let rendered = render(&format!("/DeviceRGB cs 0 1 0 sc\n{RECT}"), "<< >>", &[]);
        assert_eq!(
            rendered.diagnostics.deferred_ops, 0,
            "no colour operator may be counted as deferred any more: {:?}",
            rendered.diagnostics.sample_ops
        );
        assert_eq!(pixel(&rendered, 50, 50), (0, 255, 0));
    }
}
