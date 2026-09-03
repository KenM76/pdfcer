//! `CompatibleOverprint` — ISO 32000-1 §11.7.4.3, Table 149.
//!
//! # What this module is
//!
//! Overprint is the prepress behaviour where painting one ink does **not**
//! erase the inks already on the sheet. Paint cyan, then paint magenta over
//! it with overprint on, and the press lays magenta ink on top of cyan ink:
//! the result is blue. With overprint **off** — the normal PDF model — the
//! magenta paint *replaces* all four colorants, so the cyan is knocked out
//! and the result is magenta.
//!
//! In the transparent imaging model the standard expresses this not as a
//! special case in the painting code but as a **blend mode**:
//! `CompatibleOverprint`, whose per-component blend function
//! `B(c_b, c_s)` is given by Table 149. That is the formulation implemented
//! here, and it is the useful one, because it puts overprint on exactly the
//! same footing as `Multiply` or `Darken` — a function of the backdrop
//! component and the source component, evaluated per colour component.
//!
//! # Why pdfcer implements this at all, when the standard says it need not
//!
//! **pdfcer is not obliged to do any of this.** The spec RAG's consolidator
//! (`iso32000__ref__spot_colour_overprint.md`) records two sourced facts
//! that between them make overprint simulation entirely optional:
//!
//! - **`OP-N1` (a negative result, not a gap) — ★ EDITION-SCOPED, corrected
//!   2026-08-19:** **ISO 32000-1** never describes overprint preview or
//!   simulation on a non-separating device. Confirmed by measurement rather
//!   than by absence of memory: `simulat*` returns **7 hits in all 756
//!   pages** and every one is unrelated (ink `/Solidities`, an
//!   obsolete-masking NOTE, halftone "patterns of pixels", two annotation
//!   border styles, one mouse-device sentence); `overprint preview` returns
//!   **zero**.
//!
//!   **This paragraph previously said "the standard", unqualified, and that
//!   is wrong for PDF 2.0.** `OP-N1` is **rescoped, not retracted**:
//!   **ISO 32000-2:2020 §10.8.3 "Separation simulation"** specifies a
//!   four-step algorithm for exactly this, and **§12.11.2 Table 275**'s
//!   `SeparationSimulation` requirement row carries **NOTE 5**: *"This is
//!   sometimes referred to as "Overprint Preview"."* So in 2.0 the feature
//!   has a standard name, a standard algorithm and a standard capability
//!   flag. See `iso32000__s__10.8.md`.
//!
//!   ★ **What survives, and what does not.** The *conclusion* survives — §10.8.3
//!   is a `should`, so a compositor without it is still conformant, and this
//!   module is still a policy choice. What does not survive is the reason
//!   given: "the standard is silent" was load-bearing for the claim that
//!   pdfcer may pick **any** plausible behaviour. Under 2.0 it may not —
//!   §10.8.3 is a `should` **on the outcome**, so shipping a simulation that
//!   does not match its four-step result is worse-placed than shipping none.
//!   That constrains `Pass 97.2` (the collapse), not this module's Table 149
//!   arithmetic, which §10.8.3 does not touch.
//!
//!   Note the shape of the original error, because it is the expensive kind:
//!   a measured negative result over **one edition** was written down as a
//!   fact about **the format**. The measurement was sound; the scope on it
//!   was not stated, so it read as universal.
//! - **§8.6.7 directly:** *"If overprinting is not supported, the value of
//!   the overprint parameter shall be ignored."* Ignoring `/OP` and `/op`
//!   is **conformant**, and it is what pdfcer did until this module existed.
//!
//! So this is `C2` in that document's obliged-vs-choosing table: a **policy
//! choice**, deliberately made, to render what a press would produce rather
//! than what the standard's floor permits. Two reasons:
//!
//! 1. The print-conformance suite's overprint patches are authored so that a
//!    renderer which ignores overprint shows a visible trap X. Ten of the
//!    suite's 51 patches test overprint directly. A conformant renderer that
//!    ignores it fails all ten, and the operator sees ten red Xs.
//! 2. The operator's stated goal is prepress-credible output. "Conformant"
//!    and "correct on a press" are different bars here, and the second is
//!    the one that matters for the files this project is aimed at.
//!
//! Because it is a choice rather than an obligation, it is **disclosable**
//! under project rule 4 and is reported through the render diagnostics
//! rather than applied silently.
//!
//! # The polarity trap, stated once so it is not re-derived
//!
//! Table 149 is written in **subtractive tint** values: `0.0` means *no
//! colorant* (lightest) and `1.0` means *full colorant* (darkest). That is
//! the opposite of the additive convention used for RGB.
//!
//! The standard is explicit that this is a presentational convenience:
//!
//! > "Colour component values are represented in these tables as
//! > **subtractive tint values**… In reality, however, `CompatibleOverprint`
//! > (like all blend modes) shall treat colour components as **additive**
//! > values; subtractive components shall be **complemented before and
//! > after** application of the blend function." — §11.7.4.5
//!
//! **This module works in TINT throughout**, matching the table as written,
//! and does not complement anything. That is a deliberate simplification and
//! it is safe *only* because every function in Table 149 is one of
//! `c_s`, `c_b`, or the constant `0.0` — a selection among existing values,
//! never an arithmetic combination of them. Complementing before and after a
//! function that merely *chooses* an operand yields the identical choice.
//! If a future edit makes any cell arithmetic (it will not; the table is
//! closed), the complement steps become mandatory.
//!
//! # What Table 149 keys on
//!
//! Three inputs decide each cell:
//!
//! 1. **The source colour space** — specifically whether it is `DeviceCMYK`
//!    named *directly* (not via an image sample), some other process space,
//!    a `Separation`/`DeviceN`, or a **transparency group** rather than an
//!    elementary object.
//! 2. **The affected component** of the **group's** colour space — process
//!    or spot, and if spot, whether the source space *names* it.
//! 3. **The overprint parameters** — `OP`/`op` (a boolean) and `OPM`
//!    (`0` or `1`).
//!
//! Note the second input carefully, because it is the subtlest thing in the
//! clause and the standard spends a whole NOTE on it: in **Table 148** (the
//! opaque model) the components are *actual device colorants*; in **Table
//! 149** they are the components of the **group's** colour space, which
//! "is not necessarily the same as that of the output device (and can even
//! be something like `CalRGB` or `ICCBased`)". Consequently "the process
//! colour components of the group colour space **cannot** be treated as if
//! they were spot colours".
//!
//! # The group row, and why it is not a shortcut
//!
//! The final row of Table 149 — a source that is itself a transparency
//! group — evaluates to `c_s` in **every** column, i.e. `CompatibleOverprint`
//! degenerates to `Normal`. This is not an omission to be improved upon
//! later. The standard's reasoning:
//!
//! > "Since no information is retained about which components were actually
//! > painted within the group, compatible overprinting is not possible in
//! > this case; the `CompatibleOverprint` blend mode **reverts to Normal**,
//! > with no consideration of the overprint and overprint mode parameters."
//!
//! # Known ambiguity, surfaced rather than decided here
//!
//! **`OP-A3`:** the standard defines *two* overprint parameters (stroking
//! and non-stroking) but never says which one indexes the `OP` column of
//! Tables 148/149. The consolidator records this as unresolved in **both**
//! §8.6.7 and §11.7.4. This module therefore takes `op` as an explicit
//! argument and refuses to guess: the caller — which knows whether it is
//! filling or stroking — supplies the matching parameter. See
//! [`ComponentRule`] for where that choice is made visible.

use crate::color::{ColorSpace, Colorant};
use pdfcer_core::settings::{CmykIntent, OverprintZeroTintScope};

/// A spot colorant's name and its tint curve — the pair a plane is
/// allocated from (`Pass 238.0`). Shared by the image decoder and the
/// shading ramp so the two carry identical shapes into `spot_index`.
pub(crate) type SpotColorant = (
    std::sync::Arc<[u8]>,
    std::sync::Arc<crate::cmyk_buffer::SpotLut>,
);

/// Resolve every colorant in `colorants` to a plane, all or nothing.
///
/// The all-or-nothing rule is the fill path's: a spot's ink reaches the page
/// by its plane OR flattened into the process channels, never both, so a
/// caller that gets an empty `Vec` back paints flattened exactly as before
/// and one that gets a full one deposits every spot. `spot_index` runs each
/// closure only on the colorant's first allocation on the page.
#[must_use]
pub(crate) fn resolve_spot_planes(
    buf: &mut crate::cmyk_buffer::CmykBuffer,
    colorants: &[SpotColorant],
) -> Vec<usize> {
    let mut planes = Vec::with_capacity(colorants.len());
    for (name, lut) in colorants {
        match buf.spot_index(name, || (**lut).clone()) {
            Some(plane) => planes.push(plane),
            None => return Vec::new(),
        }
    }
    planes
}

/// One spot colorant's tint curve — the colorant ALONE on white paper,
/// sampled across `0..=1` through the space's own tint transform.
///
/// # Why this is one function and not two
///
/// The interpreter builds this curve for a path fill and the image decoder
/// builds it for a `Separation`/`DeviceN` image, and the two MUST agree:
/// `CmykBuffer::spot_index` runs the builder only on the first allocation of
/// a colorant on a page, so whichever caller gets there first defines the
/// curve every later paint of that colorant collapses through. Two
/// implementations that differed by a rounding step would make a spot fill
/// and a spot image of the same tint disagree — the exact defect
/// `devicen_image_ink`'s agreement tests exist to catch.
///
/// `component` is the colorant's index in the space's declaration order and
/// `arity` the space's operand count; every other operand is held at `0.0`,
/// which is §10.8.3 step (b)'s *"background matte of all white"*. A transform
/// that will not evaluate answers white, not black — white is multiply's
/// identity, so an unevaluable colorant paints nothing rather than a solid
/// black rectangle nobody asked for (`SpotLut::transparent` documents the
/// same choice).
#[must_use]
pub(crate) fn spot_lut(
    space: &ColorSpace,
    component: usize,
    arity: usize,
    intent: CmykIntent,
) -> crate::cmyk_buffer::SpotLut {
    crate::cmyk_buffer::SpotLut::build(|t| {
        let mut probe = vec![0.0_f32; arity.max(component + 1)];
        if let Some(slot) = probe.get_mut(component) {
            *slot = t;
        }
        // Diagnostics discarded on purpose: the probe is not a paint, and a
        // transform that fails here fails identically for the paint that
        // follows, which is where it is counted.
        let mut scratch = crate::color::ColorDiagnostics::default();
        space
            .to_rgb(&probe, intent, &mut scratch)
            .map_or([1.0, 1.0, 1.0], |rgb| [rgb.r, rgb.g, rgb.b])
    })
}

/// Which of Table 149's source-space rows applies.
///
/// This is deliberately a *classification of the row*, not of the colour
/// space, because Table 149's rows do not partition colour spaces cleanly:
/// `DeviceCMYK` appears in two different rows depending on **how** it was
/// specified, and a transparency group is not a colour space at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceKind {
    /// `DeviceCMYK`, specified directly and **not** in a sampled image.
    ///
    /// The "not in a sampled image" qualifier is load-bearing and is the
    /// reason this variant exists separately from [`Self::OtherProcess`]:
    /// it is the **only** row where `OPM 1` behaves differently from
    /// `OPM 0`. A CMYK *image* falls into the other row, where the two
    /// overprint modes are identical.
    DeviceCmykDirect,
    /// Four process tints stated directly, but through a space that is **not**
    /// `DeviceCMYK` for Table 149 Row 1's purposes -- today, an `ICCBased`
    /// with `/N 4` whose `/Alternate` resolves to `DeviceCMYK` (`Pass 165.0`).
    ///
    /// # ★★ Why this is a separate variant rather than either neighbour
    ///
    /// The two questions [`SourceKind`] is asked look like one and are not:
    ///
    /// 1. **Which Table 149 row applies?** [`Self::DeviceCmykDirect`] is the
    ///    only row where `OPM 1` differs from `OPM 0`, and §8.6.7 scopes that
    ///    rule to a `DeviceCMYK` source. An `ICCBased` is not one, so
    ///    reclassifying it into that variant would silently change what
    ///    overprint preserves -- a spec deviation, not a fix.
    /// 2. **Can the authored tints be READ?** Yes: the operands of an
    ///    `ICCBased` `/N 4` space *are* CMYK tints, sitting right there.
    ///
    /// Answering (2) by moving the source into (1)'s variant was the tempting
    /// one-line fix and it would have traded a colour bug for an overprint bug.
    /// This variant answers **yes to (2) and no to (1)**: it behaves exactly as
    /// [`Self::OtherProcess`] in [`cmyk_group_rules`], and exactly as
    /// [`Self::DeviceCmykDirect`] in [`authored_tints`].
    ///
    /// # What it fixes
    ///
    /// Without it, `authored_tints` returned `None` for these spaces and the
    /// caller fell through to `rgb_to_cmyk` on the already-flattened paint --
    /// a max-GCR transform its own doc calls *"chosen for exact
    /// round-tripping, not for accuracy"*. Measured on a PDF/X-4 patch:
    /// authored `.75 0 1 0` came back as `(0.7382, 0, 0.5942, 0.2906)` --
    /// **yellow collapsed 1.0 -> 0.59 and black invented at 0.29** -- rendering
    /// `(24, 140, 108)` where the authored tints give `(47, 181, 73)`.
    ///
    /// `authored_tints`'s own doc already said `None` was for *"an `ICCBased`
    /// that did not resolve to CMYK"*. The **implementation never made that
    /// distinction**; this variant is what makes the documented behaviour real.
    ProcessCmykIndirect,
    /// Any other process colour space — including `DeviceCMYK` reached any
    /// other way (an image sample, an `ICCBased` with four components,
    /// `DeviceRGB`, `DeviceGray`, `CalRGB`, `Lab`).
    OtherProcess,
    /// A `Separation` or `DeviceN` space, carrying the colorant names it
    /// actually specifies.
    ///
    /// The names matter because Table 149 splits spot components into
    /// *named in the source space* (painted normally) and *not named*
    /// (left to the backdrop under overprint). A `Separation` names
    /// exactly one colorant; a `DeviceN` names several.
    SeparationOrDeviceN {
        /// The colorants, in the order the space declares them.
        ///
        /// Held as [`Colorant`] rather than raw names so `/All` keeps its
        /// meaning. §8.6.6.4 says `/All` "shall refer collectively to all
        /// colorants available on an output device", which for Table 149's
        /// "named in source space" test means it names EVERY component --
        /// a distinction a bare name string would erase.
        names: Vec<Colorant>,
    },
    /// The source is a transparency **group**, not an elementary object.
    ///
    /// Reverts to `Normal` in every column — see the module docs.
    Group,
}

/// Which component of the **group's** colour space is being computed.
///
/// "Of the group's colour space" is the whole subtlety of Table 149 versus
/// Table 148; see the module documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Component {
    /// One of cyan, magenta, yellow or black.
    ///
    /// Distinguished from [`Self::OtherProcess`] because the
    /// `DeviceCMYK`-direct row treats *only* these four specially under
    /// `OPM 1`.
    ProcessCmyk,
    /// A process component that is not one of C, M, Y, K — for instance a
    /// red, green or blue component of an RGB group colour space.
    OtherProcess,
    /// A spot colorant, identified by name so the "named in source space"
    /// test can be applied.
    Spot(String),
}

/// The value `B(c_b, c_s)` selects, before it is applied.
///
/// Returned rather than a bare `f32` so a caller can *report* what overprint
/// did without having to compare numbers and infer it — a paint that
/// happens to leave a component unchanged because `c_s == c_b` is not the
/// same event as one that deliberately preserved the backdrop, and rule 4's
/// disclosure needs to tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentRule {
    /// Paint the source tint: `B = c_s`. The ordinary, non-overprint result.
    Source,
    /// Preserve the backdrop tint: `B = c_b`. This is overprint *acting* —
    /// the component survives because the source did not claim it.
    Backdrop,
    /// Paint zero tint: `B = 0.0`. An explicit erase, not a no-op.
    ///
    /// This cell surprises people and is **not** a spec defect. `SP-N2` in
    /// the consolidator records it as a confirmed negative result: a
    /// `Separation` paint with overprint **off** *erases* the process
    /// colorants it does not name, and the standard states this twice
    /// (Table 148, and Table 149's `c_s (= 0.0)` notation). Recorded so it
    /// is not "fixed" by a later reader who assumes it must be a typo.
    Zero,
}

impl ComponentRule {
    /// Resolve the rule to an actual tint value given the two operands.
    ///
    /// Kept separate from [`compatible_overprint`] so the *decision* can be
    /// tested against Table 149 without threading sample values through
    /// every case — the table specifies which operand wins, and that is the
    /// part worth pinning.
    #[must_use]
    pub fn apply(self, backdrop: f32, source: f32) -> f32 {
        match self {
            Self::Source => source,
            Self::Backdrop => backdrop,
            Self::Zero => 0.0,
        }
    }

    /// Whether this rule leaves the backdrop showing through.
    ///
    /// The predicate a caller uses to answer "did overprint change what the
    /// operator sees here?" without re-deriving the table.
    #[must_use]
    pub const fn preserves_backdrop(self) -> bool {
        matches!(self, Self::Backdrop)
    }
}

/// Evaluate Table 149 for one component.
///
/// # Arguments
///
/// * `source` — which row of Table 149 the painting operation falls in.
/// * `component` — which component of the **group** colour space is being
///   computed (not of the output device; see the module docs).
/// * `op` — the overprint parameter. **Which** of the two parameters
///   (stroking or non-stroking) this is, is the caller's decision: the
///   standard does not say, and that silence is recorded as `OP-A3`.
/// * `opm` — the overprint mode, `0` or `1`. Values other than these two
///   have **no specified behaviour** (`OP-N2`); this function treats any
///   non-`1` value as mode `0`, which is the conservative reading because
///   mode 0 is the mode that changes less.
///
/// # Returns
///
/// The [`ComponentRule`] Table 149 selects. Call [`ComponentRule::apply`]
/// with the actual backdrop and source tints to get the blended value.
///
/// # Examples
///
/// Overprint off is always just the source, for a process component:
///
/// ```
/// use pdfcer_render::overprint::{compatible_overprint, Component, ComponentRule, SourceKind};
///
/// let rule = compatible_overprint(
///     &SourceKind::DeviceCmykDirect,
///     &Component::ProcessCmyk,
///     false,
///     0,
/// );
/// assert_eq!(rule, ComponentRule::Source);
/// ```
///
/// Mode 1 is where `DeviceCMYK` starts preserving the backdrop — and only
/// for components whose source tint is zero, which is why the source tint
/// is an input here:
///
/// ```
/// use pdfcer_render::overprint::{compatible_overprint_cmyk, Component, ComponentRule, SourceKind};
///
/// // Source cyan is 0.0 under OPM 1 => the backdrop's cyan survives.
/// let rule = compatible_overprint_cmyk(
///     &SourceKind::DeviceCmykDirect,
///     &Component::ProcessCmyk,
///     true,
///     1,
///     0.0,
/// );
/// assert_eq!(rule, ComponentRule::Backdrop);
///
/// // A non-zero source cyan paints normally even under OPM 1.
/// let rule = compatible_overprint_cmyk(
///     &SourceKind::DeviceCmykDirect,
///     &Component::ProcessCmyk,
///     true,
///     1,
///     0.6,
/// );
/// assert_eq!(rule, ComponentRule::Source);
/// ```
#[must_use]
pub fn compatible_overprint(
    source: &SourceKind,
    component: &Component,
    op: bool,
    opm: u8,
) -> ComponentRule {
    // The `OPM 1` cell of the `DeviceCMYK`-direct row is the ONLY cell in
    // the whole table that depends on the source tint value, so the
    // tint-free entry point routes through the tint-aware one with a
    // non-zero placeholder. Any non-zero value gives the same answer for
    // every other cell, and for that one cell it gives the "paint source"
    // half — which is the correct default for a caller that has not told us
    // the tint.
    compatible_overprint_cmyk(source, component, op, opm, 1.0)
}

/// Evaluate Table 149 for one component, given the source tint.
///
/// Identical to [`compatible_overprint`] except that the `DeviceCMYK`-direct
/// / `OPM 1` cell — the single cell in Table 149 whose result depends on a
/// *value* rather than only on a classification — can be resolved.
///
/// That cell reads:
///
/// > `c_s` if `c_s` ≠ 0.0 / `c_b` if `c_s` = 0.0
///
/// which is the rule that makes `OPM 1` ("nonzero overprint mode") useful:
/// a `DeviceCMYK` paint with a zero in some channel leaves that channel's
/// backdrop alone instead of erasing it, so `0 0 0 1 k` prints black text
/// **over** a coloured background rather than knocking a black-shaped hole
/// in it.
#[must_use]
pub fn compatible_overprint_cmyk(
    source: &SourceKind,
    component: &Component,
    op: bool,
    opm: u8,
    source_tint: f32,
) -> ComponentRule {
    // Table 149's last row, taken first because it overrides everything:
    // a group source reverts to Normal "with no consideration of the
    // overprint and overprint mode parameters".
    if matches!(source, SourceKind::Group) {
        return ComponentRule::Source;
    }

    match (source, component) {
        // --- Row 1: DeviceCMYK, specified directly, not in a sampled image.
        (SourceKind::DeviceCmykDirect, Component::ProcessCmyk) => {
            if op && opm == 1 {
                // The one value-dependent cell in the table.
                if source_tint == 0.0 {
                    ComponentRule::Backdrop
                } else {
                    ComponentRule::Source
                }
            } else {
                // OP false, and OP true / OPM 0, are both plain `c_s`.
                ComponentRule::Source
            }
        }
        // Row 1, second line: a process component that is not C, M, Y or K
        // — e.g. painting DeviceCMYK into an RGB group. Always the source.
        (SourceKind::DeviceCmykDirect, Component::OtherProcess) => ComponentRule::Source,

        // --- Row 2: any other process colour space, process component.
        //
        // `ProcessCmykIndirect` sits here DELIBERATELY and not in Row 1: its
        // tints are readable but it is not a `DeviceCMYK` source, and §8.6.7
        // scopes `OPM 1`'s zero-tint rule to one. See that variant's docs.
        (
            SourceKind::OtherProcess | SourceKind::ProcessCmykIndirect,
            Component::ProcessCmyk | Component::OtherProcess,
        ) => {
            // Note there is NO OPM distinction here. This is exactly why
            // `DeviceCmykDirect` is a separate variant: a CMYK image lands
            // in this row and must NOT get the mode-1 behaviour.
            ComponentRule::Source
        }

        // --- Spot components under a process source space (rows 1 and 2,
        //     third line). A process paint does not name any spot colorant,
        //     so with overprint off it erases them and with overprint on it
        //     leaves them alone.
        (
            SourceKind::DeviceCmykDirect
            | SourceKind::OtherProcess
            | SourceKind::ProcessCmykIndirect,
            Component::Spot(_),
        ) => {
            if op {
                ComponentRule::Backdrop
            } else {
                ComponentRule::Zero
            }
        }

        // --- Row 3: Separation / DeviceN.
        (
            SourceKind::SeparationOrDeviceN { .. },
            Component::ProcessCmyk | Component::OtherProcess,
        ) => {
            // A process component under a Separation/DeviceN source. With
            // overprint OFF this is `c_s (= 0.0)` -- the erase that `SP-N2`
            // records as deliberate and twice-stated.
            if op {
                ComponentRule::Backdrop
            } else {
                ComponentRule::Zero
            }
        }
        (SourceKind::SeparationOrDeviceN { names }, Component::Spot(name)) => {
            // `/All` names every colorant by definition (§8.6.6.4), so it
            // satisfies "named in source space" for any component. `/None`
            // never paints at all and is suppressed upstream, so it names
            // nothing here.
            let named = names.iter().any(|n| match n {
                Colorant::All => true,
                Colorant::None => false,
                // Bytes on both sides. `Component::Spot` still carries a
                // `String` because it is constructed only in this file's
                // tests -- production code never builds one, since the
                // spot plane it describes does not exist yet. When that
                // lands, this comparison is where the identity rule has to
                // be enforced, so it is written byte-wise now.
                Colorant::Named(n) => n.as_ref() == name.as_bytes(),
            });
            if named {
                // Named in the source space: painted normally in every
                // column, overprint or not.
                ComponentRule::Source
            } else if op {
                ComponentRule::Backdrop
            } else {
                ComponentRule::Zero
            }
        }

        // Unreachable: `Group` returned above.
        (SourceKind::Group, _) => ComponentRule::Source,
    }
}

/// The paint's colour as **subtractive tints**, taken from the operands
/// the file actually wrote — or `None` when the space does not state them.
///
/// # Why this is not "convert the colour to CMYK"
///
/// Because for two of the three source kinds the tints are already present
/// and a conversion would destroy them:
///
/// - **`DeviceCMYK`, specified directly.** The operands *are* the tints.
///   Going via RGB and back would erase the very component identity that
///   both Table 149 and §11.3.4's subtractive blend depend on: a
///   `0 0 0 1 k` black returns as `C = M = Y = 0, K = 1` only by luck of
///   the conversion, and pdfcer has measured `0 1 0 0` coming back as
///   `(0, 0.995, 0.409, 0.071)`.
/// - **`Separation` / `DeviceN`.** These state their tints DIRECTLY, one
///   operand per declared colorant, in `names` order (§8.6.6.5: the
///   operands "shall" be interpreted in names-array order). Where a
///   colorant *is* a process colorant, that operand *is* the process tint.
///
///   ★ Deriving this from the flattened RGB instead — which pdfcer did
///   first — is wrong for a space naming a spot ALONGSIDE a process
///   colorant: the flattened RGB carries the spot's contribution, and
///   reconstructing CMYK from it smears the spot into the process
///   channels. suite `PCS2_030` is built entirely from that shape.
///
/// # `None`, and what the caller must do with it
///
/// Returned for [`SourceKind::OtherProcess`] — `DeviceRGB`, `DeviceGray`,
/// `CalRGB`, `Lab`, an `ICCBased` that did not resolve to CMYK, a CMYK
/// image sample. Those spaces state no tints, so there is nothing to read
/// and the caller must **convert** rather than read: [`rgb_to_cmyk`] on the
/// already-resolved paint colour. That conversion is §11.6.6's required
/// "convert the source to the group's colour space" and is not equivalent
/// to an authored value — which is the whole reason this function
/// distinguishes the two cases instead of always returning something.
///
/// # Why it lives here rather than in the interpreter
///
/// Two callers need exactly this answer and must not be able to disagree:
/// `Interpreter::paint_overprint` (Table 149's source tints) and
/// `Interpreter::authored_cmyk` (the colorant buffer's paint colour). One
/// implementation of one rule, per this crate's standing habit of putting
/// a formula in the single place both clauses can reach.
#[must_use]
pub fn authored_tints(kind: &SourceKind, comps: &[f32]) -> Option<[f32; 4]> {
    match kind {
        // `ProcessCmykIndirect` reads IDENTICALLY to `DeviceCmykDirect` here
        // -- the whole point of the variant is that the tints are stated even
        // though the overprint row is not Row 1.
        SourceKind::DeviceCmykDirect | SourceKind::ProcessCmykIndirect if comps.len() == 4 => {
            Some([comps[0], comps[1], comps[2], comps[3]])
        }
        SourceKind::SeparationOrDeviceN { names } => {
            let mut t = [0.0_f32; 4];
            for (i, n) in names.iter().enumerate() {
                let Some(v) = comps.get(i) else { break };
                match n {
                    crate::color::Colorant::All => t = [*v; 4],
                    crate::color::Colorant::None => {}
                    crate::color::Colorant::Named(name) => {
                        // ★ This was a VERBATIM INLINE COPY of
                        // `process_channel`, four arms and all. Two copies of
                        // one mapping are two things that can drift, and this
                        // one had already drifted in type -- it took a `&str`
                        // while the other took bytes. Consolidated rather than
                        // patched twice.
                        if let Some(ch) = process_channel(name) {
                            t[ch] = *v;
                        }
                    }
                }
            }
            Some(t)
        }
        // `Group` joins the `None` row for a stronger reason than the
        // others: §11.7.4.5 NOTE 2 says compatible overprinting is
        // UNAVAILABLE for a group and "the special overprinting blend mode
        // reverts to Normal". A group result has no authored tints to read
        // because it is not an authored colour at all.
        SourceKind::DeviceCmykDirect
        | SourceKind::ProcessCmykIndirect
        | SourceKind::OtherProcess
        | SourceKind::Group => None,
    }
}

/// The SPOT colorants a source states, with their tints — the half
/// [`authored_tints`] deliberately drops.
///
/// # ★★ What this is for, and why it is a separate function
///
/// [`authored_tints`] answers Table 149's question: *"which of the four
/// PROCESS channels did this source state a tint into?"* A spot colorant
/// has no process channel, so a `Separation /PANTONE 265 C` answers
/// `[0, 0, 0, 0]` there — correctly, and the tint is **discarded**.
///
/// That discard is exactly right for overprint's process-channel
/// arithmetic and exactly wrong for a per-colorant buffer, where the spot
/// tint is the thing being carried. This function returns what the other
/// one throws away. The two are deliberately not merged: they answer
/// different questions and one of them is Table 149's, which must not
/// acquire a spot dimension by accident.
///
/// # The identity rules, all three of which come from §8.6.6.4
///
/// - **`/None` never appears.** *"Painting operations in a `Separation`
///   space with this colorant name shall have no effect on the current
///   page"* — so it has no ink, and a plane for it would be a plane that
///   must never be painted. Excluded here rather than suppressed later.
/// - **`/All` never appears either**, and this is the subtle one. It
///   *"shall refer collectively to all colorants available on an output
///   device"* and applies its tint *"to all available colorants at once"*.
///   That is a **broadcast**, not a colorant: it belongs in every plane
///   including ones not yet allocated, and giving it a plane of its own
///   would make it one ink among many — the opposite of what it means.
///   [`authored_tints`] already broadcasts it across the four process
///   channels; broadcasting it across spot planes is step 4's problem and
///   is **not** silently half-done here.
/// - **A name that IS a process colorant is not a spot.** `/Cyan`,
///   `/Magenta`, `/Yellow`, `/Black` (case-folded, a documented pdfcer
///   choice) already have channels, and §8.6.6.5's `NChannel` rule makes
///   the point normatively: components *"shall be evaluated
///   individually … only the ones **not present on the output device**
///   shall use the alternate colour space"*. A simulated CMYK device has
///   those four.
///
/// # Returns
///
/// Triples of `(component index, colorant name bytes, tint)`, in the order
/// the space declares its components, for every component that is a spot.
/// Empty for every process space, which is the 98.6 % case in a 4,023-file
/// corpus — so the allocation is skipped entirely there.
///
/// ★ **The component index is returned rather than recoverable by name**,
/// and that is not a convenience. A caller that needs it — to sample this
/// colorant's tint curve with every OTHER component pinned at zero, per
/// ISO 32000-2 §10.8.3 step (b) — would otherwise search the name list,
/// and a `DeviceN` that names the same colorant twice (malformed, but
/// nothing rejects it) would hand back the first match for both. Sampling
/// the wrong component produces a perfectly plausible curve for the wrong
/// ink, which is the failure mode that would never be reported as a bug.
///
/// The name is borrowed from `kind`, not cloned: the caller either looks
/// it up in an existing roster (no allocation) or hands it to
/// `CmykBuffer::spot_index`, which clones once on the transition.
///
/// A component with no corresponding operand is skipped rather than
/// defaulted — a `DeviceN` naming three colorants and supplied two
/// operands is malformed, and inventing a tint for the third would paint
/// ink the document never asked for.
#[must_use]
pub fn authored_spots<'a>(kind: &'a SourceKind, comps: &[f32]) -> Vec<(usize, &'a [u8], f32)> {
    let SourceKind::SeparationOrDeviceN { names } = kind else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, colorant) in names.iter().enumerate() {
        let Some(&tint) = comps.get(i) else { break };
        // `/All` and `/None` are handled by NOT being here; see the docs.
        if let crate::color::Colorant::Named(name) = colorant
            && process_channel(name).is_none()
        {
            out.push((i, &**name, tint));
        }
    }
    out
}

/// Whether `kind` states a tint into at least one SPOT colorant.
///
/// A cheap pre-test for the paint path, which wants to skip the whole
/// spot-plane apparatus on the overwhelming majority of sources. Kept
/// beside [`authored_spots`] so the two cannot disagree about what counts
/// as a spot — a predicate that drifts from the function it guards is a
/// paint that silently stops depositing.
#[must_use]
pub fn names_a_spot_colorant(kind: &SourceKind) -> bool {
    let SourceKind::SeparationOrDeviceN { names } = kind else {
        return false;
    };
    names.iter().any(|colorant| {
        matches!(colorant, crate::color::Colorant::Named(name) if process_channel(name).is_none())
    })
}

/// Classify a [`ColorSpace`] into its Table 149 row.
///
/// `in_image_sample` distinguishes the two `DeviceCMYK` rows: the standard
/// separates "`DeviceCMYK`, specified directly, **not in a sampled image**"
/// from "any process colour space (**including other cases of
/// `DeviceCMYK`**)", and the difference is real — only the first gets
/// `OPM 1` behaviour.
///
/// # Returns
///
/// [`None`] when the space cannot be classified — a `Pattern` space, whose
/// colour comes from the pattern's own content stream rather than from a
/// colour operand, so there is no single source colour for Table 149 to
/// key on. The caller should paint normally and, if overprint was
/// requested, disclose that it could not be honoured.
#[must_use]
pub fn classify(
    space: &ColorSpace,
    in_image_sample: bool,
    scope: OverprintZeroTintScope,
) -> Option<SourceKind> {
    // Does `scope` let a non-CMYK process space be treated as the CMYK it
    // converts to?
    //
    // ★ `!in_image_sample` is not a copied guard, it is the SAME rule the
    // `DeviceCmyk` arm below applies for the same reason. Table 149 gives
    // `DeviceCmykDirect` the qualifier "and not in a sampled image", and a
    // CMYK IMAGE falls to `OtherProcess` where `OPM 0` and `OPM 1` are
    // identical. A grey image is that case's exact analogue, so upgrading it
    // would give a grey image an overprint behaviour a CMYK image does not
    // get — inverting the standard's own ordering.
    let converts = !in_image_sample
        && match scope {
            OverprintZeroTintScope::DeviceCmykOnly => false,
            OverprintZeroTintScope::GreyAsKOnly => matches!(space, ColorSpace::DeviceGray),
            OverprintZeroTintScope::AllProcessSpaces => true,
            // ★ `#[non_exhaustive]` makes this arm compulsory, and the
            // compulsory-looking choice is the wrong one. `_ => true` would
            // make an unrecognised future scope silently WIDEN what overprint
            // preserves — the paint would stop writing components it used to
            // write, on a file the operator did not change.
            //
            // `false` is the conservative reading: §8.6.7 to the letter, which
            // is what pdfcer did before this Pass existed. An unknown scope
            // therefore renders as the pre-`Pass 143.0` behaviour rather than
            // as a guess, and the scope actually in use is reported on
            // `pdfcer render-page`'s metrics line either way, so the
            // divergence is visible rather than inferred from the pixels.
            _ => false,
        };
    match space {
        ColorSpace::DeviceCmyk => Some(if in_image_sample {
            SourceKind::OtherProcess
        } else {
            SourceKind::DeviceCmykDirect
        }),
        // ★ `Pass 143.0` — a DIVERGENCE from ISO 32000-1, offered as a
        // SETTING. (It said "toward Acrobat" until `Pass 206.0`. It diverges
        // toward Acrobat over a SPOT backdrop and AWAY from Acrobat over
        // process components, so the direction is a property of the geometry
        // rather than of the setting.) (Called "the §8.6.7 ambiguity" until
        // `Pass 174.6`: §8.6.7's next sentence excludes "conversions from
        // some other colour space" by name, and Tables 148/149 row 2
        // tabulate the case and give it OPM-0 behaviour. ISO 32000-2 deletes
        // both, so the question is edition-gated and only OPEN in 2.0.)
        //
        // The literal reading gives these `OtherProcess`, whose Table 149 row
        // is `[Source; 4]` in all three columns — so the paint replaces the
        // backdrop in every component and a 50 % grey KNOCKS A SPOT BACKDROP
        // OUT. This setting converts grey to K-only `DeviceCMYK` first and
        // then applies `OPM 1`, so its zero C, M and Y preserve the backdrop.
        // Acrobat does the same over a spot backdrop and does NOT over process
        // components — measured 2026-09-01, and the agreement over a spot is
        // for the wrong reason (pdfcer flattens the spot into C/M/Y, which this
        // mis-assignment then happens to preserve).
        //
        // See `OverprintZeroTintScope` for the measurement and for why the
        // default is nevertheless unchanged. (This said "the default is
        // Acrobat's"; §11.7.4.5 Table 149 places a DeviceGray source in the
        // process-space row, so the literal reading is both conforming AND the
        // one that matches Acrobat on the geometry that can tell.)
        // **No conversion happens here**
        // — the caller has already resolved the paint to CMYK tints, and for
        // equal RGB `rgb_to_cmyk` yields `[0, 0, 0, 1-g]` exactly. Only the
        // classification moves.
        ColorSpace::DeviceGray | ColorSpace::DeviceRgb if converts => {
            Some(SourceKind::DeviceCmykDirect)
        }
        ColorSpace::DeviceGray | ColorSpace::DeviceRgb => Some(SourceKind::OtherProcess),
        ColorSpace::Separation { colorant, .. } => Some(SourceKind::SeparationOrDeviceN {
            names: vec![colorant.clone()],
        }),
        ColorSpace::DeviceN { names, .. } => Some(SourceKind::SeparationOrDeviceN {
            names: names.to_vec(),
        }),
        // ★ §8.6.6.3 — AN `Indexed` SPACE'S COLOUR VALUES ARE IN ITS BASE.
        //
        // Without this arm an `/Indexed [/DeviceN [/Cyan] /DeviceCMYK …]`
        // space fell to the catch-all below and Table 149 decided what
        // survives from a colorant list it never read. `PCS1_190` is
        // authored on exactly that discriminator.
        //
        // ★ THIS ARM WAS INERT FOR FIVE DAYS AND IS NOT ANY MORE.
        // Measured 2026-08-21: `/Indexed` is PRESENT in four of the suite's
        // overprint patches and was REACHABLE IN NONE OF THEM, because every
        // one of those spaces is an IMAGE colour space and `composite` had
        // no image call site — pre- and post-fix binaries reported identical
        // overprint counters on all four. `Pass 130.2` built that call site
        // (`Canvas::fill_image_overprint`) and three of the four patches
        // went from FAIL to pass. The note is corrected rather than deleted
        // because the shape of it recurs: **present-in-the-file and
        // reachable-by-the-renderer are different claims**, and the first
        // reads as the second unless it says so.
        //
        // ★★ AND THE CALLER OWES THE OTHER HALF. This arm fixes the ROW;
        // the TINTS handed to `cmyk_group_rules` must independently be the
        // palette-looked-up base components rather than the raw index, via
        // `ColorSpace::indexed_entry`. Classifying from the base while
        // still reading tints from the index is a worse state than either
        // half alone — a correct rule applied to a meaningless number —
        // which is why the obligation is stated here, where somebody
        // adding a call to `classify` will read it.
        ColorSpace::Indexed { base, .. } => classify(base, in_image_sample, scope),
        // ★★ `Pass 165.0` -- an `ICCBased` `/N 4` over `DeviceCMYK` states real
        // CMYK tints, and they were being thrown away and re-derived from the
        // flattened paint colour.
        //
        // Deliberately NARROW. Only `/N 4` with a `DeviceCMYK` alternate, and
        // only outside an image sample -- the same qualifier Table 149 gives
        // Row 1 and for the same reason. Every other `ICCBased` (1- or
        // 3-component, or a non-CMYK alternate) still falls to the catch-all
        // and is unchanged, so this cannot quietly widen the `Pass 143.0`
        // grey/RGB upgrade to spaces that never had it.
        ColorSpace::IccBased {
            n: 4, alternate, ..
        } if !in_image_sample && matches!(**alternate, ColorSpace::DeviceCmyk) => {
            Some(SourceKind::ProcessCmykIndirect)
        }
        _ => Some(SourceKind::OtherProcess),
    }
}

/// Convert an additive sRGB triple to subtractive CMYK tints.
///
/// Uses the standard maximum-GCR ("100% grey component replacement")
/// formulation: pull as much neutral density as possible into `K`, then
/// express what remains in `C`, `M`, `Y`.
///
/// ```text
/// K = 1 - max(R, G, B)
/// C = (1 - R - K) / (1 - K)      (and likewise M from G, Y from B)
/// ```
///
/// # Why a naive conversion is the RIGHT choice here, not a shortcut
///
/// This is not a colorimetric transform and does not pretend to be — it
/// carries no ICC profile, no black generation curve and no ink limit. It is
/// chosen because of a property that matters far more for overprint than
/// accuracy would: **it is exactly inverted by [`cmyk_to_rgb`]**.
///
/// `cmyk_to_rgb(rgb_to_cmyk(c)) == c` for every colour, because
/// `R = (1 - C)(1 - K)` reduces to `R` by construction. So a pixel that is
/// merely *read* and written back is unchanged to the last bit, and only the
/// components overprint actually alters can move. A "better" conversion that
/// did not round-trip would smear colour across every pixel an overprint
/// paint touched, including the ones Table 149 says to leave alone — turning
/// a correctness feature into a source of drift.
///
/// The round trip does **not** preserve the original *component split*: a
/// backdrop painted `C=0.5 M=0.4 Y=0.4 K=0` reads back as
/// `C=0.167 M=0 Y=0 K=0.4`, the same colour by a different route. That is
/// acceptable, and the reason is worth stating because it looks like it
/// should break: each output channel depends on a disjoint pair of inputs
/// (`R` on `C` and `K`, `G` on `M` and `K`, `B` on `Y` and `K`), so
/// overprinting a single ink changes exactly one output channel and leaves
/// the other two at their round-tripped — hence original — values.
///
/// The honest limitation: pdfcer has no separated CMYK buffer, so the
/// backdrop's component split is *reconstructed* from the composite rather
/// than remembered. Where a document overprints two inks in sequence over a
/// rich backdrop, the reconstruction can differ from a true separated
/// pipeline. This is disclosed rather than hidden — see the render
/// diagnostics — and a real n-channel buffer remains the eventual fix.
#[must_use]
pub fn rgb_to_cmyk(r: f32, g: f32, b: f32) -> [f32; 4] {
    let k = 1.0 - r.max(g).max(b);
    if k >= 1.0 {
        // Pure black: the divisions below are 0/0. Every chromatic
        // component is meaningless here, so report none of them.
        return [0.0, 0.0, 0.0, 1.0];
    }
    let inv = 1.0 - k;
    [
        (1.0 - r - k) / inv,
        (1.0 - g - k) / inv,
        (1.0 - b - k) / inv,
        k,
    ]
}

/// Convert subtractive CMYK tints back to an additive sRGB triple.
///
/// The exact inverse of [`rgb_to_cmyk`]; see that function for why the
/// round-trip property is the point.
#[must_use]
pub fn cmyk_to_rgb(cmyk: [f32; 4]) -> (f32, f32, f32) {
    let k = 1.0 - cmyk[3];
    (
        (1.0 - cmyk[0]) * k,
        (1.0 - cmyk[1]) * k,
        (1.0 - cmyk[2]) * k,
    )
}

/// Which CMYK channel a colorant name refers to, if any.
///
/// `None` for a genuine spot colorant. The comparison is
/// **case-insensitive** and that is a *choice*, not a rule: `SEP-A1` records
/// that ISO 32000-1 defines **no** colorant-name matching or normalisation
/// rule at all (`PANTONE 185 C` vs `185C` is undecidable by the standard).
/// Matching the four process names case-insensitively is the least
/// surprising behaviour and is what a press operator would expect; anything
/// more aggressive would start silently merging real spot inks.
#[must_use]
fn process_channel(name: &[u8]) -> Option<usize> {
    // Takes BYTES because `Colorant::Named` carries bytes: a colorant name is
    // an identity and a lossy decode makes distinct names compare equal. See
    // that variant's documentation.
    //
    // ★ The ASCII case-insensitivity is a pdfcer CHOICE, not a spec rule --
    // ISO 32000 defines no case-folding for colorant names, and the corpus
    // note `SEP-A1` records that explicitly. It is kept because real files
    // spell these names inconsistently, and it is ASCII-only so it cannot
    // fold two distinct non-ASCII names together.
    match name.to_ascii_lowercase().as_slice() {
        b"cyan" => Some(0),
        b"magenta" => Some(1),
        b"yellow" => Some(2),
        b"black" => Some(3),
        _ => None,
    }
}

/// Resolve Table 149 for all four channels of a `DeviceCMYK` group.
///
/// pdfcer composites against a `DeviceCMYK` group colour space, so the four
/// "components of the group colour space" Table 149 speaks of are exactly
/// C, M, Y and K.
///
/// # The `Separation`/`DeviceN` interpretation, stated because it is a choice
///
/// Table 149's `Separation`/`DeviceN` row splits components into *process*
/// and *spot, named in source space*. It has **no** row for a process
/// component that the source space names — yet naming one is legal and
/// common: `/DeviceN [/Black] /DeviceCMYK …` is precisely how a document
/// overprints black. Read literally, `Black` is a process component of the
/// group, so it would take the process row and be **preserved from the
/// backdrop** — i.e. a DeviceN black paint would paint nothing at all.
///
/// That is plainly not the intent, and the standard knows the area is
/// unsettled: `DN-A1` records that a CMYK `NChannel` naming `Cyan` "hits two
/// contradictory rules". pdfcer resolves it the way a press does — **a
/// colorant the source space names is painted, whether or not it happens to
/// be a process colorant** — which is the same principle as the "spot
/// colorant named in source space" row, applied to the case the table
/// omits.
///
/// `/All` names every colorant by definition (§8.6.6.4) and therefore paints
/// every channel.
/// Does this source name a colorant pdfcer has no plate for?
///
/// The condition `Pass 195.0`'s mixed-case widening turns on, exposed so the
/// shading route can ask the same question before narrowing that widening back
/// down. Kept beside `cmyk_group_rules` so the two cannot drift: a caller that
/// asks this and a rule that acts on it must agree about what "unplatable"
/// means.
#[must_use]
pub fn names_unplatable_spot(source: &SourceKind) -> bool {
    match source {
        SourceKind::SeparationOrDeviceN { names } => names.iter().any(|n| match n {
            Colorant::Named(name) => process_channel(name).is_none(),
            // `/All` names every colorant, so nothing is unplatable; `/None`
            // names none.
            Colorant::All | Colorant::None => false,
        }),
        _ => false,
    }
}

/// Does this source NAME the colorant for process channel `channel`?
///
/// Distinct from "does it put ink there": a named process colorant is claimed
/// by the source and Table 149 gives it `c_s` regardless of the value, which is
/// why the shading narrowing must not withdraw it just because a particular
/// ramp happens to be zero there. A shading that names `/Cyan` and ramps cyan
/// from 0 to 1 still CLAIMS cyan at `t = 0`.
#[must_use]
pub fn names_process_channel(source: &SourceKind, channel: usize) -> bool {
    match source {
        SourceKind::SeparationOrDeviceN { names } => names.iter().any(|n| match n {
            Colorant::Named(name) => process_channel(name) == Some(channel),
            Colorant::All => true,
            Colorant::None => false,
        }),
        _ => false,
    }
}

#[must_use]
pub fn cmyk_group_rules(
    source: &SourceKind,
    source_cmyk: [f32; 4],
    op: bool,
    opm: u8,
) -> [ComponentRule; 4] {
    cmyk_group_rules_with_planes(source, source_cmyk, op, opm, false)
}

/// [`cmyk_group_rules`] for a caller that knows whether the source's SPOT
/// colorants have **planes of their own** (`Pass 238.0`).
///
/// # Why the table needs to be told
///
/// The mixed-case widening at the bottom of the `SeparationOrDeviceN` arm —
/// `[Source; 4]` for a source naming both a process colorant and a spot —
/// exists because, with no plane, a spot's ink can only reach the page
/// flattened into the process channels, and `Backdrop` rules would throw it
/// away (`Pass 195.0`'s measured missing green). Its own comment says the
/// per-channel version *"belongs with the per-spot-colorant plane, where the
/// paint colour is in scope."*
///
/// With `spots_plated == true` the spot's ink is going to its plane, so the
/// widening is no longer a rescue — it is a knockout of backdrop C, M and Y
/// the source never named. Table 149's row is applied as written: named
/// process channels `Source`, unnamed process channels `Backdrop` (or `Zero`
/// with overprint off), and the spot handled by the plane. Measured on a
/// print-conformance duotone (`[/DeviceN [/Black <spot>]]` image under
/// `/OP true`): with the widening the image knocked out the cyan and yellow
/// of the check marks beneath it and the spot never landed, because the
/// all-`Source` rules made the caller take the flattened route; without it
/// the marks survive and the spot deposits.
///
/// `false` reproduces [`cmyk_group_rules`] exactly, for callers that do not
/// deposit spot planes yet (shadings, meshes).
#[must_use]
pub fn cmyk_group_rules_with_planes(
    source: &SourceKind,
    source_cmyk: [f32; 4],
    op: bool,
    opm: u8,
    spots_plated: bool,
) -> [ComponentRule; 4] {
    match source {
        // Reverts to Normal in every column — no consideration of op/opm.
        SourceKind::Group => [ComponentRule::Source; 4],

        SourceKind::DeviceCmykDirect => {
            let mut out = [ComponentRule::Source; 4];
            for (i, rule) in out.iter_mut().enumerate() {
                *rule = compatible_overprint_cmyk(
                    source,
                    &Component::ProcessCmyk,
                    op,
                    opm,
                    source_cmyk[i],
                );
            }
            out
        }

        // "Any process colour space" — every process component is `c_s`,
        // in all three columns. Overprint is inert, which is why a
        // DeviceRGB or DeviceGray paint with /OP true changes nothing.
        //
        // ★ `ProcessCmykIndirect` belongs HERE, not in the arm above. Its
        // tints are readable (that is what `authored_tints` is for) but it is
        // not a `DeviceCMYK` source, and §8.6.7 scopes `OPM 1`'s zero-tint
        // rule to one. Putting it in the `DeviceCmykDirect` arm would trade a
        // colour bug for an overprint bug.
        SourceKind::OtherProcess | SourceKind::ProcessCmykIndirect => [ComponentRule::Source; 4],

        SourceKind::SeparationOrDeviceN { names } => {
            let mut out = [if op {
                ComponentRule::Backdrop
            } else {
                // SP-N2: overprint OFF erases the colorants the source does
                // not name. Stated twice in the standard; not a typo.
                ComponentRule::Zero
            }; 4];
            let mut named_process = false;
            let mut named_unplatable_spot = false;
            for n in names {
                match n {
                    Colorant::All => return [ComponentRule::Source; 4],
                    Colorant::None => {}
                    Colorant::Named(name) => {
                        if let Some(ch) = process_channel(name) {
                            out[ch] = ComponentRule::Source;
                            named_process = true;
                        } else {
                            // A colorant pdfcer has no plate for. Its ink is
                            // still in `source_cmyk` -- flattened through the
                            // tint transform -- but no NAME maps it to a
                            // channel, so nothing above will write it.
                            named_unplatable_spot = true;
                        }
                    }
                }
            }

            // ★★★ THE MIXED CASE: a source naming BOTH a process colorant and
            // a spot pdfcer cannot plate (`Pass 195.0`).
            //
            // The rules above are computed from colorant NAMES; the colour
            // they are applied to is `source_cmyk`, which is ALREADY FLATTENED
            // through the tint transform. For a spot, those two disagree: the
            // spot's ink lands in whichever process channels its tint
            // transform writes, and its NAME maps to none of them -- so every
            // channel carrying its colour stays `Backdrop` and the spot's
            // entire contribution is silently discarded.
            //
            // MEASURED on a `/DeviceN [<spot green> /Cyan] /DeviceCMYK` axial
            // shading painted with `/OP true /op true /OPM 1`: the tint
            // transform yields (0.5, 0, 1.0, 0) at one end, so the green lives
            // in Y -- and Y was exactly the channel thrown away. The band
            // rendered as a pure cyan ramp with the green end missing, which
            // is what the operator reported as "missing an entire colour in
            // the colour band".
            //
            // ★ Why this is NOT the existing spot-only refusal. The guard that
            // catches a spot-only source asks whether the source names a
            // process colorant; here `/Cyan` DOES, so the refusal never fired.
            // A guard written for one shape is a claim about a class -- the
            // mixed shape fell through the middle.
            //
            // ★★★ WHY THIS IS `[Source; 4]` AND NOT THE PER-CHANNEL VERSION,
            // WHICH WAS TRIED FIRST AND MEASURED TO DO NOTHING.
            //
            // The obviously better fix is to mark `Source` only on the channels
            // the flattened tint actually writes -- for the measured spot, C
            // and Y -- leaving M and K on `Backdrop` so an overprint cannot
            // ERASE backdrop ink the spot never claimed. That version was
            // written, built and measured, and it changed the render by
            // exactly zero.
            //
            // The reason is architectural and is worth recording, because it is
            // invisible from this function's signature: **`source_cmyk` is not
            // the paint's colour here.** These rules are computed ONCE per
            // graphics state, and for precisely the mixed spaces the probe
            // showed the call arriving with `source_cmyk = [0, 0, 0, 0]` -- a
            // placeholder. The real colour only exists per-sample, later, in
            // the ramp. So a per-channel refinement at THIS site can only ever
            // read zeros and widen nothing.
            //
            // ⇒ `[Source; 4]` is not the better answer, it is the reachable
            // one. The honest cost: it writes the source's M and K, which are 0
            // for this shading, so it knocks out backdrop magenta and black
            // that the spot never claimed. No patch in the conformance corpus
            // detects that, and the per-channel version belongs with the
            // per-spot-colorant plane, where the paint colour is in scope.
            //
            // ★★ AND WHY A SPOT-ONLY SOURCE IS DELIBERATELY LEFT ALONE: the
            // unconditional widening was measured and REGRESSES two patches
            // (page mean |diff| 20.25 -> 22.02 and 20.60 -> 23.77). Preserving
            // the backdrop for a spot-only paint is the decided behaviour this
            // module documents at length; this widening is scoped to the mixed
            // case that behaviour was never written for.
            // ★ And with a plane, the spot is no longer unplatable — see
            // `cmyk_group_rules_with_planes`. The widening was the reachable
            // answer for a four-plane buffer; the table's own answer is
            // reachable now.
            if named_process && named_unplatable_spot && !spots_plated {
                return [ComponentRule::Source; 4];
            }
            out
        }
    }
}

/// Composite one overprinting paint into `pixmap` through `coverage`.
///
/// # Contract
///
/// * `coverage` is the anti-aliased coverage of the path being painted,
///   already intersected with any clip in force. It must be the same
///   dimensions as `pixmap`.
/// * `source_cmyk` is the source colour expressed as subtractive tints.
/// * `alpha` is the constant alpha (`CA`/`ca`) for this paint.
/// * `region` is a device-space `(x0, y0, x1, y1)` bounding box, already
///   clamped to the pixmap, limiting the scan to where coverage can be
///   non-zero.
///
/// # Returns
///
/// The number of pixels whose value actually changed — the measurement the
/// caller discloses. Zero is meaningful and is **not** a failure: it means
/// overprint was requested and turned out to be a no-op on this geometry,
/// which is a different fact from "overprint was not applied".
///
/// # Why this writes pixels directly rather than going through a `Paint`
///
/// `tiny_skia` composites in RGBA. Table 149 selects **per colour
/// component in a subtractive space**, and there is no RGBA blend mode that
/// expresses "keep the backdrop's cyan but take the source's magenta" —
/// that is precisely the operation `CompatibleOverprint` exists to perform
/// and precisely what a three-channel additive pipeline cannot say. So the
/// blend is done here, per pixel, in CMYK.
pub fn composite(
    pixmap: &mut tiny_skia::Pixmap,
    coverage: &tiny_skia::Mask,
    rules: [ComponentRule; 4],
    source_cmyk: [f32; 4],
    alpha: f32,
    region: (u32, u32, u32, u32),
) -> u32 {
    let width = pixmap.width();
    let (x0, y0, x1, y1) = region;
    let cov = coverage.data();
    let mut changed = 0_u32;

    for y in y0..y1 {
        for x in x0..x1 {
            let idx = (y * width + x) as usize;
            let c = f32::from(cov[idx]) / 255.0;
            if c <= 0.0 {
                continue;
            }
            // Coverage and constant alpha both scale HOW MUCH of the
            // overprint result replaces what is there — antialiased edges
            // and a `ca` of 0.5 attenuate identically.
            let t = c * alpha;

            let px = pixmap.pixels()[idx];
            // `tiny_skia` stores premultiplied; demultiply to get the
            // colour Table 149 reasons about. A fully transparent pixel
            // has no meaningful colour, so it is treated as white paper —
            // which is what an unpainted sheet is.
            let a = f32::from(px.alpha()) / 255.0;
            let (br, bg, bb) = if a <= 0.0 {
                (1.0, 1.0, 1.0)
            } else {
                (
                    f32::from(px.red()) / 255.0 / a,
                    f32::from(px.green()) / 255.0 / a,
                    f32::from(px.blue()) / 255.0 / a,
                )
            };

            let backdrop = rgb_to_cmyk(br, bg, bb);
            let mut out = [0.0_f32; 4];
            for i in 0..4 {
                out[i] = rules[i].apply(backdrop[i], source_cmyk[i]).clamp(0.0, 1.0);
            }
            let (nr, ng, nb) = cmyk_to_rgb(out);

            // Interpolate between the backdrop and the overprint result by
            // `t`, so partial coverage and partial alpha behave the way
            // every other paint in the renderer does.
            let fr = br + (nr - br) * t;
            let fg = bg + (ng - bg) * t;
            let fb = bb + (nb - bb) * t;
            // The paint is opaque where it lands: overprint adds ink, it
            // does not make the sheet more transparent. Alpha rises toward
            // full by the same `t`.
            let fa = a + (1.0 - a) * t;

            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let enc = |v: f32| -> u8 { (v * fa * 255.0 + 0.5).clamp(0.0, 255.0) as u8 };
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let ea = (fa * 255.0 + 0.5).clamp(0.0, 255.0) as u8;

            if let Some(np) =
                tiny_skia::PremultipliedColorU8::from_rgba(enc(fr), enc(fg), enc(fb), ea)
            {
                if np != px {
                    changed += 1;
                }
                pixmap.pixels_mut()[idx] = np;
            }
        }
    }
    changed
}

/// Whether this source space names at least one **process** colorant.
///
/// `/All` counts — §8.6.6.4 makes it refer collectively to every colorant on
/// the device, so it names all four. `/None` never counts. A process colour
/// space trivially counts. A `Separation`/`DeviceN` counts only if one of its
/// declared colorant names matches `Cyan`, `Magenta`, `Yellow` or `Black`.
///
/// # ★★ WHY THIS EXISTS: `authored_tints` ANSWERS A QUESTION THAT IS NOT
/// "WHAT COLOUR IS THIS PAINT"
///
/// [`authored_tints`] reads the operands the file wrote and reports them as
/// process tints. For a spot colorant there is **no process channel to report
/// into**, so it contributes nothing and the function returns `[0, 0, 0, 0]`.
/// That is the correct and intended answer to *"which process tints did this
/// source state?"* — the question Table 149 asks — and it is a **catastrophic**
/// answer to *"what ink does this paint put on the sheet?"*, because zero ink
/// is blank paper.
///
/// The two questions were being answered by one function. Measured: a
/// `/Separation /SpotInk /DeviceCMYK` square filled over white paper on a page
/// whose group colour space is `/DeviceCMYK` rendered **completely invisible**,
/// with overprint OFF, no diagnostic, and every counter green. The same square
/// on an additive page rendered correctly, so the defect was invisible to any
/// check that did not set a page group — and a page group is exactly what a
/// print-bound PDF carries.
///
/// So a caller asking for a PAINT colour must test this first and fall back to
/// the space's own tint transform (already resolved into the paint's sRGB)
/// when it is `false`. A caller asking Table 149's question must not.
#[must_use]
pub fn names_a_process_colorant(kind: &SourceKind) -> bool {
    match kind {
        SourceKind::DeviceCmykDirect
        | SourceKind::ProcessCmykIndirect
        | SourceKind::OtherProcess
        | SourceKind::Group => true,
        SourceKind::SeparationOrDeviceN { names } => names.iter().any(|n| match n {
            Colorant::All => true,
            Colorant::None => false,
            Colorant::Named(name) => process_channel(name).is_some(),
        }),
    }
}

/// Whether Table 149 leaves **any** component to the backdrop — i.e.
/// whether honouring overprint on this source changes anything at all.
///
/// `false` means the source space specifies every component of the group, so
/// `CompatibleOverprint` degenerates to Normal and an **ordinary paint is
/// already the correct answer**. A caller that takes the native route anyway
/// gets the same pixels for more work; a caller that COUNTS it as an
/// effective overprint reports a divergence where the table promises none,
/// and an operator diffing that number between files will chase it.
///
/// # Why this is a function and not four lines at each call site
///
/// Because there are now three call sites — the path/glyph painter, the
/// shading painter and the image painter — and the three-way outcome they
/// each have to distinguish (inert / erasing / mixed) is *the same reading of
/// the same table*. `Pass 130.3` exists because one of them was missing the
/// second branch, and a whole gradient bar disappeared from a page.
#[must_use]
pub fn changes_anything(rules: [ComponentRule; 4]) -> bool {
    rules.iter().any(|r| r.preserves_backdrop())
}

/// Whether Table 149 leaves **every** component to the backdrop — the
/// spot-only case, where compositing natively would **erase the paint**.
///
/// # ★★ WHY THIS IS A REFUSAL AND NOT A RESULT
///
/// A `Separation`/`DeviceN` source that names no *process* colorant at all —
/// `/Separation /PANTONE 185 C`, or `/DeviceN [/PANTONE 265 C /Suite Green]`
/// — has, under Table 149's third row, every one of the group's four
/// components in the "not named in source space" column. Under `OP true`
/// that column is `c_b`. So the literal answer is: **preserve the entire
/// backdrop, and paint nothing into the process planes.**
///
/// That is *correct for a press*, where the ink goes on its own plate and the
/// four process plates genuinely are untouched. It is **catastrophic for a
/// renderer with four process planes and no spot plane**, because the mark
/// simply vanishes — not shifted, not desaturated, *absent*. And it vanishes
/// SILENTLY: every counter reads as a successful composite, because the
/// composite did succeed at doing what the table said.
///
/// Measured, and this is what the function is named after: a print-
/// conformance patch whose spot-colour gradient bar is a `ShadingType 2` in
/// `[/DeviceN [/PANTONE 265 C /Suite Green] /DeviceCMYK …]` under `/OP true`
/// rendered as **bare white paper, 451 × 29 device pixels of it**, while
/// `shadings_painted` said 1 and `overprint_shadings_unsupported` said 0.
/// The check marks drawn *on top of* the bar were all present and correct,
/// which made the page look like a rendering that had merely lost a
/// background rather than one that had followed a rule off a cliff.
///
/// # ★★★ AND YET THE ANSWER IS TO COMPOSITE IT ANYWAY. DO NOT "FIX" THIS.
///
/// The obvious repairs are to paint the flattened tint normally, or to
/// composite an ink union (`max(c_b, c_s)`) so that nothing is ever knocked
/// out. **Both were built, both were ablated across the whole
/// print-conformance suite on 2026-08-26, and both are REFUTED:**
///
/// | behaviour for a spot-only source under `OP true` | suite failures |
/// |---|---|
/// | preserve the backdrop — what pdfcer does | **4** |
/// | ink union, `max(c_b, c_s)` | 6 |
/// | paint the flattened tint normally | 8 |
///
/// The suite ships **three patches whose entire subject** is that a *white*
/// spot set to overprint must not knock out what lies under it, and both
/// alternatives break all three. Knocking out is the one thing overprint
/// promises never to do, so those two are refuted by the artefact rather than
/// merely out-scored, and neither should be re-attempted without an oracle
/// stronger than this one.
///
/// So a caller that sees `true` here should **let the composite run** — the
/// result marks nothing, which is Table 149's answer — and **disclose that it
/// did**, because an object that marks nothing is exactly what an operator
/// cannot see. The real fix is the per-colorant buffer, which is filed and is
/// not reachable from any of these call sites.
///
/// ★ What IS a defect in this area, and is fixed, is a different function
/// being asked this one's question: [`authored_tints`] reports a spot-only
/// source as `[0, 0, 0, 0]` — correct for *"which process tints did the file
/// state?"* and blank paper when used as a **paint colour**. That made a spot
/// invisible on a subtractive page *with overprint off*, which no reading of
/// Table 149 sanctions. See [`names_a_process_colorant`].
#[must_use]
pub fn erases_the_paint(rules: [ComponentRule; 4]) -> bool {
    rules.iter().all(|r| r.preserves_backdrop())
}

/// [`composite`]'s twin for a paint whose source colour **differs per
/// pixel** — an overprinting `Separation`/`DeviceN` *image*.
///
/// # Why a second function rather than a `[f32; 4]` that happens to vary
///
/// Because the varying part is the only part that varies. `rules` are still
/// computed **once** by the caller, and that is a fact about Table 149 rather
/// than an optimisation: row 3's selection depends on **which colorants the
/// source space names**, never on their tints, and an image has one colour
/// space for all its samples. (Row 1 — `DeviceCMYK` specified directly under
/// `/OPM 1` — *is* tint-dependent, which is why that source kind must not be
/// routed here; it also cannot arrive, because
/// [`classify`] sends a sampled image to [`SourceKind::OtherProcess`] by
/// Table 149's own scope note.)
///
/// This mirrors [`crate::cmyk_buffer::CmykBuffer::composite_overprint_varying`]
/// exactly, for the same reason [`composite`] mirrors its solid sibling: a
/// document can contain the same mark on a subtractive page and on an
/// additive one, and the two must not disagree about what overprint did to
/// it. What the sRGB path loses — and it is real and disclosed elsewhere —
/// is that the backdrop's component split is *reconstructed* by
/// [`rgb_to_cmyk`] rather than remembered.
///
/// # Contract
///
/// * `source_at(x, y)` returns the pixel's authored process tints and its
///   **coverage** (the image's own per-texel alpha, already rasterised onto
///   the device grid), or [`None`] where the image does not land.
/// * `alpha` is the constant alpha (`ca`), multiplied into coverage exactly
///   as [`composite`] multiplies it.
/// * `region` is a device-space `(x0, y0, x1, y1)`, already clamped.
///
/// # Returns
///
/// The number of pixels whose value actually changed. Zero is meaningful and
/// is not a failure — see [`composite`].
pub fn composite_varying(
    pixmap: &mut tiny_skia::Pixmap,
    rules: [ComponentRule; 4],
    alpha: f32,
    region: (u32, u32, u32, u32),
    mut source_at: impl FnMut(u32, u32) -> Option<([f32; 4], f32)>,
) -> u32 {
    let width = pixmap.width();
    let (x0, y0, x1, y1) = region;
    let mut changed = 0_u32;

    for y in y0..y1 {
        for x in x0..x1 {
            let Some((source, coverage)) = source_at(x, y) else {
                continue;
            };
            let t = alpha.clamp(0.0, 1.0) * coverage.clamp(0.0, 1.0);
            if t <= 0.0 {
                continue;
            }
            let idx = (y * width + x) as usize;
            let Some(&px) = pixmap.pixels().get(idx) else {
                continue;
            };
            // The identical demultiply-and-white-paper convention
            // `composite` uses, stated there and not repeated here.
            let a = f32::from(px.alpha()) / 255.0;
            let (br, bg, bb) = if a <= 0.0 {
                (1.0, 1.0, 1.0)
            } else {
                (
                    f32::from(px.red()) / 255.0 / a,
                    f32::from(px.green()) / 255.0 / a,
                    f32::from(px.blue()) / 255.0 / a,
                )
            };
            let backdrop = rgb_to_cmyk(br, bg, bb);
            let mut out = [0.0_f32; 4];
            for i in 0..4 {
                out[i] = rules[i].apply(backdrop[i], source[i]).clamp(0.0, 1.0);
            }
            let (nr, ng, nb) = cmyk_to_rgb(out);
            let fr = br + (nr - br) * t;
            let fg = bg + (ng - bg) * t;
            let fb = bb + (nb - bb) * t;
            let fa = a + (1.0 - a) * t;

            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let enc = |v: f32| -> u8 { (v * fa * 255.0 + 0.5).clamp(0.0, 255.0) as u8 };
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let ea = (fa * 255.0 + 0.5).clamp(0.0, 255.0) as u8;

            if let Some(np) =
                tiny_skia::PremultipliedColorU8::from_rgba(enc(fr), enc(fg), enc(fb), ea)
            {
                if np != px {
                    changed += 1;
                }
                if let Some(slot) = pixmap.pixels_mut().get_mut(idx) {
                    *slot = np;
                }
            }
        }
    }
    changed
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn spot(name: &str) -> Component {
        Component::Spot(name.to_owned())
    }

    /// ★ §8.6.6.3 — an `Indexed` space classifies as its BASE.
    ///
    /// Without this, `/Indexed [/DeviceN [/Cyan] /DeviceCMYK …]` fell to
    /// `OtherProcess` and Table 149 decided what survives from a colorant
    /// list it never read. `PCS1_190` is authored on exactly that
    /// discriminator: its a/b pair's `DeviceN` omits the backdrop's
    /// colorants and its c/d pair includes them at 0 %.
    ///
    /// ★ **This test guarded a correct rule that no suite pixel exercised,
    /// for five days, and that was the reason to KEEP it rather than to
    /// discount it.** Measured 2026-08-21: `/Indexed` is present in four of
    /// the suite's overprint patches and was reachable in none of them,
    /// because every one of those spaces is an *image* colour space and
    /// `composite` had no image call site. `Pass 130.2` built one; three of
    /// the four now pass. A green test over an unreachable rule is not a
    /// wasted test — it is the half of the fix that was already done when
    /// the other half arrived.
    #[test]
    fn indexed_classifies_as_its_base_space() {
        let base = ColorSpace::DeviceN {
            names: std::sync::Arc::from(
                vec![crate::color::Colorant::Named(b"Cyan".as_slice().into())].into_boxed_slice(),
            ),
            alternate: std::sync::Arc::new(ColorSpace::DeviceCmyk),
            tint: None,
        };
        let indexed = ColorSpace::Indexed {
            base: std::sync::Arc::new(base),
            hival: 1,
            lookup: std::sync::Arc::from(vec![0_u8, 255].into_boxed_slice()),
        };
        assert_eq!(
            classify(&indexed, false, OverprintZeroTintScope::default()),
            Some(sep(&["Cyan"])),
            "an Indexed space must classify as the space its palette entries \
             are written in"
        );
    }

    /// An `ICCBased` `/N 4` over `DeviceCMYK` builds the space the way the
    /// interpreter does, for the tests below.
    fn iccbased_cmyk() -> ColorSpace {
        ColorSpace::IccBased {
            n: 4,
            alternate: std::sync::Arc::new(ColorSpace::DeviceCmyk),
            alternate_explicit: true,
            // These tests are about OVERPRINT CLASSIFICATION, which reads `n`
            // and the alternate only. No profile: a classification that
            // changed with one would be a defect, and `None` is also the
            // honest value for a hand-built space that never had a stream.
            profile: None,
        }
    }

    /// ★★ `Pass 165.0` — an `ICCBased` `/N 4` states real CMYK tints, and they
    /// must be READABLE without becoming a `DeviceCMYK` source.
    ///
    /// Before this, the space classified as [`SourceKind::OtherProcess`],
    /// `authored_tints` returned `None`, and the caller fell through to
    /// `rgb_to_cmyk` on the already-flattened paint — a max-GCR transform its
    /// own doc calls *"chosen for exact round-tripping, not for accuracy"*.
    ///
    /// Measured on a PDF/X-4 patch: authored `.75 0 1 0` came back as
    /// `(0.7382, 0, 0.5942, 0.2906)` — **yellow collapsed 1.0 → 0.59 and black
    /// invented at 0.29** — rendering `(24, 140, 108)` where the authored tints
    /// give `(47, 181, 73)`. Acrobat renders `(59, 171, 51)`, so the authored
    /// value is also the closer one.
    #[test]
    fn iccbased_cmyk_states_its_tints_and_they_are_read() {
        let kind = classify(&iccbased_cmyk(), false, OverprintZeroTintScope::default())
            .expect("an ICCBased CMYK space classifies");
        assert_eq!(
            kind,
            SourceKind::ProcessCmykIndirect,
            "an ICCBased /N 4 over DeviceCMYK must classify as the variant \
             whose tints are readable"
        );
        assert_eq!(
            authored_tints(&kind, &[0.75, 0.0, 1.0, 0.0]),
            Some([0.75, 0.0, 1.0, 0.0]),
            "the operands ARE the tints; returning None here is what threw the \
             authored value away and re-derived it from its own approximation"
        );
    }

    /// ★★★ …and it must **not** become a `DeviceCMYK` source, because that is
    /// the one Table 149 row where `OPM 1` differs from `OPM 0`.
    ///
    /// This is the test that stops the tempting one-line fix. Reclassifying
    /// `ICCBased` as [`SourceKind::DeviceCmykDirect`] would have made
    /// `authored_tints` work *and* silently changed what overprint preserves:
    /// §8.6.7 scopes the zero-tint rule to a `DeviceCMYK` source, which an
    /// `ICCBased` is not. That trades a colour bug for an overprint bug, and
    /// nothing else in the suite would have caught it.
    #[test]
    fn iccbased_cmyk_does_not_get_device_cmyk_overprint_semantics() {
        let icc = classify(&iccbased_cmyk(), false, OverprintZeroTintScope::default())
            .expect("classifies");
        let device = classify(
            &ColorSpace::DeviceCmyk,
            false,
            OverprintZeroTintScope::default(),
        )
        .expect("classifies");
        assert_ne!(icc, device, "the two must not collapse into one variant");

        // A zero component under OP true / OPM 1: the DeviceCMYK source
        // preserves the backdrop, the ICCBased source must not.
        let tints = [0.0, 0.5, 0.5, 0.0];
        let icc_rules = cmyk_group_rules(&icc, tints, true, 1);
        let device_rules = cmyk_group_rules(&device, tints, true, 1);

        assert_eq!(
            device_rules[0],
            ComponentRule::Backdrop,
            "control: a DeviceCMYK source with OPM 1 preserves a zero component"
        );
        assert_eq!(
            icc_rules,
            [ComponentRule::Source; 4],
            "an ICCBased source is Table 149 Row 2 -- every component is the \
             source, in all three columns. If this ever reads Backdrop, the \
             colour fix has been made by breaking overprint."
        );
    }

    /// The narrowness is deliberate and is asserted, so a later widening is a
    /// decision rather than an accident.
    ///
    /// Only `/N 4` over `DeviceCMYK`, and only outside an image sample — the
    /// same qualifier Table 149 gives Row 1, for the same reason. A 3-component
    /// `ICCBased`, or one whose alternate is not CMYK, states no CMYK tints and
    /// must still bridge.
    #[test]
    fn the_iccbased_arm_is_narrow_on_purpose() {
        let scope = OverprintZeroTintScope::default();

        let rgb_alt = ColorSpace::IccBased {
            n: 3,
            alternate: std::sync::Arc::new(ColorSpace::DeviceRgb),
            alternate_explicit: true,
            profile: None,
        };
        assert_eq!(
            classify(&rgb_alt, false, scope),
            Some(SourceKind::OtherProcess),
            "a 3-component ICCBased states no CMYK tints"
        );

        assert_eq!(
            classify(&iccbased_cmyk(), true, scope),
            Some(SourceKind::OtherProcess),
            "IN A SAMPLED IMAGE it stays OtherProcess -- the same qualifier \
             Table 149 gives Row 1, and a CMYK image already lands there"
        );

        assert_eq!(
            authored_tints(&SourceKind::ProcessCmykIndirect, &[0.5, 0.5, 0.5]),
            None,
            "three operands are not four tints, whatever the space said"
        );
    }

    /// …and `DeviceCMYK` under `Indexed` is `OtherProcess`, not
    /// `DeviceCmykDirect`.
    ///
    /// Table 149 separates "`DeviceCMYK`, specified directly, **not in a
    /// sampled image**" from every other `DeviceCMYK` case, and only the
    /// first gets `OPM 1` behaviour. An `/Indexed` operand is an index, so
    /// the CMYK was not "specified directly" by the operator — it was
    /// looked up. Getting this backwards would turn `OPM 1` on for palette
    /// colours the document never wrote as CMYK operands.
    #[test]
    fn indexed_over_device_cmyk_is_not_the_direct_row() {
        let indexed = ColorSpace::Indexed {
            base: std::sync::Arc::new(ColorSpace::DeviceCmyk),
            hival: 0,
            lookup: std::sync::Arc::from(vec![0_u8; 4].into_boxed_slice()),
        };
        // `in_image_sample` is what the caller says about the CONTEXT, and
        // an Indexed palette entry reached through a path fill is not an
        // image sample — so this is the honest call and it is the one the
        // recursion makes.
        assert_eq!(
            classify(&indexed, true, OverprintZeroTintScope::default()),
            Some(SourceKind::OtherProcess)
        );
    }

    fn sep(names: &[&str]) -> SourceKind {
        SourceKind::SeparationOrDeviceN {
            names: names
                .iter()
                .map(|n| Colorant::Named(n.as_bytes().into()))
                .collect(),
        }
    }

    /// ★ Table 149, transcribed cell by cell.
    ///
    /// The whole point of this module is to be Table 149 and nothing else,
    /// so the test IS the table: every row, every one of the three columns.
    /// A cell that disagrees with the standard is a wrong pixel on a press,
    /// and no downstream test would localise it.
    ///
    /// The `DeviceCMYK` / `OPM 1` row is covered separately below because
    /// it is the one cell that depends on a value rather than a class.
    #[test]
    fn table_149_transcribed() {
        // (source, component, OP false, OP+OPM0, OP+OPM1)
        let cases: Vec<(
            SourceKind,
            Component,
            ComponentRule,
            ComponentRule,
            ComponentRule,
        )> = vec![
            // Row 1: DeviceCMYK direct.
            (
                SourceKind::DeviceCmykDirect,
                Component::OtherProcess,
                ComponentRule::Source,
                ComponentRule::Source,
                ComponentRule::Source,
            ),
            (
                SourceKind::DeviceCmykDirect,
                spot("PANTONE 185 C"),
                ComponentRule::Zero,
                ComponentRule::Backdrop,
                ComponentRule::Backdrop,
            ),
            // Row 2: any other process space.
            (
                SourceKind::OtherProcess,
                Component::ProcessCmyk,
                ComponentRule::Source,
                ComponentRule::Source,
                ComponentRule::Source,
            ),
            (
                SourceKind::OtherProcess,
                Component::OtherProcess,
                ComponentRule::Source,
                ComponentRule::Source,
                ComponentRule::Source,
            ),
            (
                SourceKind::OtherProcess,
                spot("Varnish"),
                ComponentRule::Zero,
                ComponentRule::Backdrop,
                ComponentRule::Backdrop,
            ),
            // Row 3: Separation / DeviceN.
            (
                sep(&["Spot1"]),
                Component::ProcessCmyk,
                ComponentRule::Zero,
                ComponentRule::Backdrop,
                ComponentRule::Backdrop,
            ),
            (
                sep(&["Spot1"]),
                spot("Spot1"),
                ComponentRule::Source,
                ComponentRule::Source,
                ComponentRule::Source,
            ),
            (
                sep(&["Spot1"]),
                spot("Spot2"),
                ComponentRule::Zero,
                ComponentRule::Backdrop,
                ComponentRule::Backdrop,
            ),
            // Last row: a group source reverts to Normal everywhere.
            (
                SourceKind::Group,
                Component::ProcessCmyk,
                ComponentRule::Source,
                ComponentRule::Source,
                ComponentRule::Source,
            ),
            (
                SourceKind::Group,
                spot("Spot1"),
                ComponentRule::Source,
                ComponentRule::Source,
                ComponentRule::Source,
            ),
        ];

        for (src, comp, off, on0, on1) in cases {
            assert_eq!(
                compatible_overprint(&src, &comp, false, 0),
                off,
                "Table 149 OP-false column, source {src:?}, component {comp:?}",
            );
            assert_eq!(
                compatible_overprint(&src, &comp, true, 0),
                on0,
                "Table 149 OP-true/OPM-0 column, source {src:?}, component {comp:?}",
            );
            assert_eq!(
                compatible_overprint(&src, &comp, true, 1),
                on1,
                "Table 149 OP-true/OPM-1 column, source {src:?}, component {comp:?}",
            );
        }
    }

    /// ★ The one value-dependent cell: DeviceCMYK direct, CMYK component,
    /// OP true, OPM 1 — `c_s` if `c_s` ≠ 0, else `c_b`.
    ///
    /// This is the cell that makes `0 0 0 1 k` overprinting black text work,
    /// and getting it backwards produces the exact opposite of the intended
    /// effect (a knockout hole instead of an overprint), which is why it is
    /// pinned separately rather than folded into the table above.
    #[test]
    fn opm_one_preserves_the_backdrop_only_where_the_source_is_zero() {
        let s = SourceKind::DeviceCmykDirect;
        let c = Component::ProcessCmyk;

        assert_eq!(
            compatible_overprint_cmyk(&s, &c, true, 1, 0.0),
            ComponentRule::Backdrop,
            "a zero source component under OPM 1 must leave the backdrop alone — \
             this is the whole purpose of nonzero overprint mode",
        );
        for tint in [0.001_f32, 0.5, 1.0] {
            assert_eq!(
                compatible_overprint_cmyk(&s, &c, true, 1, tint),
                ComponentRule::Source,
                "a nonzero source component ({tint}) must paint normally even under OPM 1",
            );
        }

        // And OPM 0 does NOT get this behaviour, which is the distinction
        // between the two modes.
        assert_eq!(
            compatible_overprint_cmyk(&s, &c, true, 0, 0.0),
            ComponentRule::Source,
            "OPM 0 paints all four components regardless of value — if this ever \
             returns Backdrop, mode 0 and mode 1 have been conflated",
        );
    }

    /// A CMYK **image** is not the same row as a CMYK fill.
    ///
    /// `classify`'s `in_image_sample` flag exists solely for this, and it is
    /// easy to drop: both are "DeviceCMYK", and only the standard's phrase
    /// "not in a sampled image" separates them. Getting it wrong gives
    /// images a mode-1 behaviour they must not have.
    #[test]
    fn a_cmyk_image_sample_does_not_get_mode_one_behaviour() {
        let direct = classify(
            &ColorSpace::DeviceCmyk,
            false,
            OverprintZeroTintScope::default(),
        )
        .unwrap();
        let sampled = classify(
            &ColorSpace::DeviceCmyk,
            true,
            OverprintZeroTintScope::default(),
        )
        .unwrap();
        assert_eq!(direct, SourceKind::DeviceCmykDirect);
        assert_eq!(sampled, SourceKind::OtherProcess);

        assert_eq!(
            compatible_overprint_cmyk(&direct, &Component::ProcessCmyk, true, 1, 0.0),
            ComponentRule::Backdrop,
        );
        assert_eq!(
            compatible_overprint_cmyk(&sampled, &Component::ProcessCmyk, true, 1, 0.0),
            ComponentRule::Source,
            "a sampled CMYK image falls in the 'any other process space' row, where \
             OPM 1 and OPM 0 are identical",
        );
    }

    /// `OP-N2`: values of `OPM` other than 0 and 1 have no specified
    /// behaviour. This pins the conservative reading rather than leaving it
    /// to whatever the `match` happens to do.
    #[test]
    fn an_unspecified_overprint_mode_falls_back_to_mode_zero() {
        let s = SourceKind::DeviceCmykDirect;
        let c = Component::ProcessCmyk;
        for opm in [2_u8, 3, 255] {
            assert_eq!(
                compatible_overprint_cmyk(&s, &c, true, opm, 0.0),
                ComponentRule::Source,
                "OPM {opm} is unspecified (OP-N2); mode 0 is the conservative reading \
                 because it is the mode that changes less",
            );
        }
    }

    /// `SP-N2`, guarded explicitly: a `Separation` paint with overprint OFF
    /// **erases** process colorants. Stated twice in the standard and
    /// recorded as a confirmed negative result precisely so nobody "fixes"
    /// it into `Source`.
    #[test]
    fn a_separation_paint_without_overprint_erases_process_colorants() {
        assert_eq!(
            compatible_overprint(&sep(&["Spot1"]), &Component::ProcessCmyk, false, 0),
            ComponentRule::Zero,
            "SP-N2: this cell is 'paint 0.0', not 'paint source' and not 'do not \
             paint'. It is stated in both Table 148 and Table 149 and is NOT a typo",
        );
    }

    /// ★ The round trip is EXACT, which is the property the whole
    /// compositing approach rests on.
    ///
    /// pdfcer has no separated CMYK buffer, so an overprint paint must
    /// reconstruct the backdrop's CMYK from the composited RGB, blend, and
    /// convert back. If that round trip were lossy, every pixel an
    /// overprint paint merely *touched* would shift colour — including the
    /// ones Table 149 says to leave alone. The feature would introduce
    /// drift everywhere it was used.
    ///
    /// If this test ever fails, `composite` is unsafe to run and the right
    /// response is to stop using it, not to widen the tolerance.
    #[test]
    fn rgb_survives_a_cmyk_round_trip_exactly() {
        let mut worst = 0.0_f32;
        for r in 0..=16 {
            for g in 0..=16 {
                for b in 0..=16 {
                    let (rf, gf, bf) = (r as f32 / 16.0, g as f32 / 16.0, b as f32 / 16.0);
                    let (or, og, ob) = cmyk_to_rgb(rgb_to_cmyk(rf, gf, bf));
                    worst = worst
                        .max((or - rf).abs())
                        .max((og - gf).abs())
                        .max((ob - bf).abs());
                }
            }
        }
        assert!(
            worst < 1e-5,
            "RGB -> CMYK -> RGB must be exact; worst error {worst} over 4913 colours. \
             A lossy round trip would smear colour across every pixel an overprint \
             paint touched, including the ones Table 149 preserves",
        );
    }

    /// Pure black is the singular case of the conversion (the divisions are
    /// 0/0) and must not produce NaN.
    #[test]
    fn pure_black_converts_without_dividing_by_zero() {
        let cmyk = rgb_to_cmyk(0.0, 0.0, 0.0);
        assert_eq!(cmyk, [0.0, 0.0, 0.0, 1.0]);
        assert!(
            cmyk.iter().all(|v| v.is_finite()),
            "no NaN from the K=1 case"
        );
        let (r, g, b) = cmyk_to_rgb(cmyk);
        assert!(r.abs() < 1e-6 && g.abs() < 1e-6 && b.abs() < 1e-6);
    }

    /// ★ The behaviour the whole feature exists for: cyan, then magenta
    /// overprinted, gives blue rather than magenta.
    ///
    /// This is the one-sentence description of overprint, so it is worth
    /// having as a test in exactly those terms. Without overprint the
    /// magenta paint replaces all four colorants and the cyan is gone.
    #[test]
    fn magenta_over_cyan_gives_blue_only_with_overprint() {
        let src = SourceKind::DeviceCmykDirect;
        let backdrop_cyan = [1.0, 0.0, 0.0, 0.0];
        let source_magenta = [0.0, 1.0, 0.0, 0.0];

        // OPM 1: the source's zero cyan preserves the backdrop's full cyan.
        let rules = cmyk_group_rules(&src, source_magenta, true, 1);
        let mut out = [0.0_f32; 4];
        for i in 0..4 {
            out[i] = rules[i].apply(backdrop_cyan[i], source_magenta[i]);
        }
        assert_eq!(
            out,
            [1.0, 1.0, 0.0, 0.0],
            "cyan + overprinted magenta must retain BOTH inks — that is what a \
             press does and what the trap X in the suite patches detects",
        );

        // Overprint off: the paint knocks out everything it does not set.
        let rules = cmyk_group_rules(&src, source_magenta, false, 0);
        for i in 0..4 {
            out[i] = rules[i].apply(backdrop_cyan[i], source_magenta[i]);
        }
        assert_eq!(
            out,
            [0.0, 1.0, 0.0, 0.0],
            "without overprint the magenta paint replaces the cyan entirely",
        );
    }

    /// A `DeviceN` naming a PROCESS colorant paints it, and preserves the
    /// rest under overprint.
    ///
    /// Table 149 has no row for this — it splits only *spot* components by
    /// whether the source names them — yet `/DeviceN [/Black] /DeviceCMYK`
    /// is exactly how a file overprints black. Read literally the K channel
    /// would take the "process component" row and be preserved from the
    /// backdrop, i.e. the paint would paint nothing. `DN-A1` records the
    /// area as contradictory. This pins pdfcer's resolution so it is a
    /// decision on record rather than an accident of match order.
    #[test]
    fn a_devicen_naming_black_paints_black_and_preserves_the_rest() {
        let src = SourceKind::SeparationOrDeviceN {
            names: vec![Colorant::Named(b"Black".as_slice().into())],
        };
        let rules = cmyk_group_rules(&src, [0.0, 0.0, 0.0, 1.0], true, 0);
        assert_eq!(
            rules,
            [
                ComponentRule::Backdrop,
                ComponentRule::Backdrop,
                ComponentRule::Backdrop,
                ComponentRule::Source,
            ],
            "the named colorant is painted; the three it does not name survive",
        );

        // Case-insensitively, because SEP-A1 says the standard defines no
        // matching rule at all and this is the least surprising one.
        let lower = SourceKind::SeparationOrDeviceN {
            names: vec![Colorant::Named(b"black".as_slice().into())],
        };
        assert_eq!(
            cmyk_group_rules(&lower, [0.0, 0.0, 0.0, 1.0], true, 0),
            rules
        );
    }

    /// A genuine spot name claims no process channel.
    #[test]
    fn a_real_spot_colorant_claims_no_process_channel() {
        let src = SourceKind::SeparationOrDeviceN {
            names: vec![Colorant::Named(b"PANTONE 185 C".as_slice().into())],
        };
        assert_eq!(
            cmyk_group_rules(&src, [0.0, 0.0, 0.0, 0.0], true, 0),
            [ComponentRule::Backdrop; 4],
            "a spot ink is not a process channel, so under overprint the whole \
             CMYK backdrop survives",
        );
    }

    /// `/All` paints every channel (§8.6.6.4).
    #[test]
    fn separation_all_paints_every_channel() {
        let src = SourceKind::SeparationOrDeviceN {
            names: vec![Colorant::All],
        };
        assert_eq!(
            cmyk_group_rules(&src, [0.5; 4], true, 1),
            [ComponentRule::Source; 4],
            "/All refers collectively to ALL colorants, so nothing is preserved",
        );
    }

    /// `apply` resolves a rule to the right operand.
    #[test]
    fn rules_resolve_to_the_operand_they_name() {
        assert!((ComponentRule::Source.apply(0.25, 0.75) - 0.75).abs() < f32::EPSILON);
        assert!((ComponentRule::Backdrop.apply(0.25, 0.75) - 0.25).abs() < f32::EPSILON);
        assert!(ComponentRule::Zero.apply(0.25, 0.75).abs() < f32::EPSILON);

        assert!(ComponentRule::Backdrop.preserves_backdrop());
        assert!(!ComponentRule::Source.preserves_backdrop());
        assert!(
            !ComponentRule::Zero.preserves_backdrop(),
            "Zero ERASES the backdrop; reporting it as preserved would invert the \
             disclosure the operator reads",
        );
    }

    // -----------------------------------------------------------------
    // `authored_spots` — the half `authored_tints` drops (`Pass 227.0`)
    // -----------------------------------------------------------------

    /// Would catch: a spot colorant's tint being dropped, which is the
    /// state of affairs this function exists to end.
    ///
    /// The companion assertion is the point: `authored_tints` must STILL
    /// answer all-zero for the same input. The two functions split one
    /// source between them and neither may start answering the other's
    /// question — Table 149's process arithmetic acquiring a spot
    /// dimension by accident is a spec deviation, not a fix.
    #[test]
    fn a_separation_states_its_spot_tint_and_no_process_tint() {
        let src = SourceKind::SeparationOrDeviceN {
            names: vec![Colorant::Named(b"PANTONE 265 C".as_slice().into())],
        };
        assert_eq!(
            authored_spots(&src, &[0.5]),
            vec![(0, &b"PANTONE 265 C"[..], 0.5)]
        );
        assert_eq!(
            authored_tints(&src, &[0.5]),
            Some([0.0, 0.0, 0.0, 0.0]),
            "the process half must be unchanged -- a spot states no process tint"
        );
    }

    /// Would catch: a mixed `DeviceN` handing its process component to the
    /// spot path or its spot component to the process path.
    ///
    /// ★ This is the exact shape of the backdrop in the patch this work
    /// targets: `/DeviceN [/Black <spot>] /DeviceCMYK` filled `0.5 1 scn`.
    /// Black is a process channel and must go to `authored_tints`; the
    /// spot has no channel and must come out here. Getting the split wrong
    /// is what smears a spot across C/M/Y.
    #[test]
    fn a_mixed_devicen_splits_its_components_between_the_two_readers() {
        let src = SourceKind::SeparationOrDeviceN {
            names: vec![
                Colorant::Named(b"Black".as_slice().into()),
                Colorant::Named(b"Suite Green".as_slice().into()),
            ],
        };
        assert_eq!(
            authored_spots(&src, &[0.5, 1.0]),
            vec![(1, &b"Suite Green"[..], 1.0)],
            "only the component with no process channel"
        );
        assert_eq!(
            authored_tints(&src, &[0.5, 1.0]),
            Some([0.0, 0.0, 0.0, 0.5]),
            "Black is a PROCESS channel and stays on the process side"
        );
    }

    /// Would catch: `/None` being given an ink plane.
    ///
    /// §8.6.6.4: painting in a `Separation` space with this colorant name
    /// *"shall have no effect on the current page"*. A plane for it would
    /// be a plane that must never be painted — excluded here rather than
    /// suppressed downstream, so no later code has to remember.
    #[test]
    fn the_none_colorant_never_becomes_a_plane() {
        let src = SourceKind::SeparationOrDeviceN {
            names: vec![Colorant::None],
        };
        assert!(authored_spots(&src, &[1.0]).is_empty());
        assert!(!names_a_spot_colorant(&src));
    }

    /// Would catch: `/All` being given a plane of its own, which would
    /// make a BROADCAST into one ink among many — the opposite of what it
    /// means.
    ///
    /// §8.6.6.4 says `/All` *"shall refer collectively to all colorants
    /// available on an output device"* and applies its tint *"to all
    /// available colorants at once"*. `authored_tints` already broadcasts
    /// it across the four process channels; broadcasting it across spot
    /// planes is later work and must not be half-done by accident here.
    #[test]
    fn the_all_colorant_is_a_broadcast_not_a_plane() {
        let src = SourceKind::SeparationOrDeviceN {
            names: vec![Colorant::All],
        };
        assert!(
            authored_spots(&src, &[0.75]).is_empty(),
            "/All must not acquire a plane"
        );
        assert!(!names_a_spot_colorant(&src));
        // ...and the existing broadcast across process channels is intact.
        assert_eq!(authored_tints(&src, &[0.75]), Some([0.75; 4]));
    }

    /// Would catch: the process-colorant test losing its case folding, or
    /// gaining it for non-ASCII.
    ///
    /// The folding is a pdfcer CHOICE (`SEP-A1`: the standard defines no
    /// matching rule), and it is ASCII-only so it cannot fold two distinct
    /// non-ASCII names together. Pinned here because this function and
    /// `authored_tints` must agree about what "is a process colorant"
    /// means — a component counted by both, or by neither, is ink
    /// duplicated or ink lost.
    #[test]
    fn process_colorant_names_fold_in_ascii_only() {
        for spelling in [&b"black"[..], b"Black", b"BLACK", b"bLaCk"] {
            let src = SourceKind::SeparationOrDeviceN {
                names: vec![Colorant::Named(spelling.into())],
            };
            assert!(
                authored_spots(&src, &[1.0]).is_empty(),
                "{} is a process colorant however it is spelled",
                String::from_utf8_lossy(spelling)
            );
        }
        // A name that merely CONTAINS a process name is its own colorant.
        let src = SourceKind::SeparationOrDeviceN {
            names: vec![Colorant::Named(b"Blackcurrant".as_slice().into())],
        };
        assert_eq!(
            authored_spots(&src, &[1.0]),
            vec![(0, &b"Blackcurrant"[..], 1.0)]
        );
    }

    /// Would catch: a missing operand being defaulted to a tint, which
    /// would paint ink the document never asked for.
    ///
    /// A `DeviceN` naming three colorants and supplied two operands is
    /// malformed. §8.6.6.5 states no recovery, so the honest response is
    /// to carry what was stated and stop — not to invent the rest.
    #[test]
    fn a_component_with_no_operand_is_skipped_not_defaulted() {
        let src = SourceKind::SeparationOrDeviceN {
            names: vec![
                Colorant::Named(b"one".as_slice().into()),
                Colorant::Named(b"two".as_slice().into()),
                Colorant::Named(b"three".as_slice().into()),
            ],
        };
        assert_eq!(
            authored_spots(&src, &[0.25, 0.5]),
            vec![(0, &b"one"[..], 0.25), (1, &b"two"[..], 0.5)],
            "two operands, two colorants -- the third is not invented"
        );
    }

    /// Would catch: a process space being walked for spots at all, which
    /// is the 98.6 % case in a 4,023-file corpus and must cost nothing.
    #[test]
    fn a_process_space_states_no_spots() {
        for kind in [
            SourceKind::DeviceCmykDirect,
            SourceKind::ProcessCmykIndirect,
            SourceKind::OtherProcess,
            SourceKind::Group,
        ] {
            assert!(authored_spots(&kind, &[0.1, 0.2, 0.3, 0.4]).is_empty());
            assert!(!names_a_spot_colorant(&kind));
        }
    }

    /// Would catch: the cheap predicate drifting from the function it
    /// guards — a paint that silently stops depositing.
    #[test]
    fn the_predicate_agrees_with_the_function_it_guards() {
        let cases = [
            vec![Colorant::Named(b"spot".as_slice().into())],
            vec![Colorant::Named(b"Cyan".as_slice().into())],
            vec![Colorant::All],
            vec![Colorant::None],
            vec![
                Colorant::Named(b"Black".as_slice().into()),
                Colorant::Named(b"spot".as_slice().into()),
            ],
            vec![],
        ];
        for names in cases {
            let src = SourceKind::SeparationOrDeviceN {
                names: names.clone(),
            };
            // Enough operands that no component is skipped for want of one,
            // so the two can only disagree about what COUNTS as a spot.
            let comps = vec![1.0_f32; names.len()];
            assert_eq!(
                names_a_spot_colorant(&src),
                !authored_spots(&src, &comps).is_empty(),
                "predicate and function disagree for {names:?}"
            );
        }
    }
}
