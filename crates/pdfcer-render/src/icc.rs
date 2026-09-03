//! # The ICC bridge — pdfcer's one and only door to colour conversion
//!
//! This module is a thin adapter over **iccce** (`github.com/KenM76/iccce`,
//! MIT), the sibling project that owns *all* colour conversion by
//! `docs/ARCHITECTURE.md` decision 064. Nothing here implements colour
//! science. Everything here is about deciding *which* conversion to ask for,
//! *when*, and what to do when the answer is unavailable.
//!
//! ## The problem this exists to fix
//!
//! When a page composites in ink (it has a `/Group` with a subtractive
//! `/CS`), every additive paint that lands on it has to become CMYK at some
//! point. Until this module existed that job was done by
//! [`crate::overprint::rgb_to_cmyk`] — and that function is **not wrong**, it
//! is *wrongly used*. It is a deliberately simple, deliberately
//! **invertible** max-GCR formula, and it exists so that
//! `snapshot_srgb_backdrop` and `composite_srgb` can make a round trip that
//! returns where it started. Round-tripping is the whole point of it.
//!
//! A **terminal** conversion — the last thing that happens to a colour before
//! it is written into the colorant buffer for good — has the opposite
//! requirement. It should be *accurate*, and it has no obligation to be
//! reversible. Using the round-trip transform for the terminal conversion was
//! measured on a conformance patch at **~92 levels** of error against
//! Acrobat's output, where a real CMM lands within **~3**.
//!
//! ⇒ ★ **The bug was never in the arithmetic; it was in which function was
//! called.** Three hypotheses about pdfcer's blend maths were raised and each
//! was refuted by ablation before the real cause was found. Recorded because
//! the shape recurs: a correct function used for the wrong job produces
//! numbers that look like an arithmetic bug and are not one.
//!
//! ## What must NOT change, and why it is stated as a prohibition
//!
//! `overprint::rgb_to_cmyk` **stays exactly where it is** on the
//! `snapshot_srgb_backdrop` ↔ `composite_srgb` path. This module is not a
//! search-and-replace of that function. Substituting an accurate,
//! non-invertible CMM into a round trip would make the return leg fail to
//! return — the backdrop would drift a little on every composite, which is a
//! far worse and much harder-to-see defect than the one being fixed. The two
//! call sites want two different functions and always did.
//!
//! ## Where the profiles come from
//!
//! | end | source | fallback if absent |
//! |---|---|---|
//! | **source** | the `ICCBased` stream's own decoded profile, carried on [`crate::color::ColorSpace::IccBased`] | none — no bridge is built |
//! | **destination** | the document catalog's `/OutputIntents` → `/DestOutputProfile` | none — no bridge is built |
//! | **destination (display)** | iccce's **constructed sRGB** (`iccce_cmm::builtin::srgb`, `Destination::None`) — the screen itself | none needed; see below |
//!
//! ### ★ The second destination, and why it is not the asymmetry being broken
//!
//! The paragraph below this one says pdfcer must never invent a **source**
//! characterisation. It says nothing against a built-in **destination**, and
//! iccce ships exactly one on purpose: the screen every additive render
//! lands on IS sRGB by this crate's own convention (`color::xyz_to_srgb`
//! already assumes it for `Lab`/`CalRGB`). So when a document embeds a
//! profile that says what its RGB numbers mean, converting those numbers
//! **to the display** needs no `/OutputIntent` at all — the destination is
//! known by construction. [`IccBridge::build_to_srgb`] is that route, and
//! [`IccBridgeCache::get_srgb`] hands it out.
//!
//! ★★ It is a DIFFERENT route from the ink one, and the difference was
//! measured before this was written (`docs/NEXT_SESSION.md` §D item 1,
//! 2026-09-02): routing an `ICCBased /N 3` image through source →
//! `/OutputIntent` CMYK → the terminal CMYK→sRGB conversion landed **3×
//! worse** than not managing it at all, because that terminal conversion is
//! separately ~10 levels off and the detour paid for it twice. Source → sRGB
//! directly is one transform, and it is the one a colour-managed viewer
//! actually runs for the screen.
//!
//! Both ends are genuinely optional in a real document, and when either is
//! missing the honest answer is **not to colour-manage**, falling back to the
//! previous behaviour rather than inventing a profile. That fallback is the
//! reason [`IccBridge::build`] returns `Option` rather than `Result`: a
//! document with no output intent is not malformed, it simply has not said
//! what device it targets, and guessing would be exactly the "sneaky"
//! behaviour `CLAUDE.md` rule 4 forbids.
//!
//! ### ★ Why there is no built-in-sRGB source fallback
//!
//! It would be easy to write "if the source has no profile, assume sRGB", and
//! it would be wrong twice over. iccce deliberately exposes a built-in sRGB as
//! a **destination** only, and pdfcer is not entitled to invent a source
//! characterisation the document never made. A `DeviceRGB` fill genuinely has
//! no colorimetric meaning until something assigns one; pretending otherwise
//! would replace a known-approximate conversion with a differently-approximate
//! one while *looking* authoritative. So `DeviceRGB` keeps the old transform,
//! and only `ICCBased` — where the document did the work of saying what its
//! numbers mean — is colour-managed.
//!
//! ## Caching, and why the key is the whole dependency set
//!
//! Building a `Chain` parses two profiles and composes their transforms; doing
//! that per *paint* would be absurd. [`IccBridge`] is therefore built once and
//! shared behind an `Arc`.
//!
//! ★ The cache key is **(source profile bytes, destination profile bytes,
//! rendering intent)** — all three. This project has already shipped a defect
//! (standing rule R237) where a memo's key omitted one of its dependencies, so
//! every verb computed over it silently addressed the wrong object. The intent
//! is the field most likely to be dropped from a key by someone who reasons
//! "it is just an enum" — but a perceptual and a relative-colorimetric chain
//! between the same two profiles are **different transforms**, and sharing one
//! entry between them would make a document's `ri` operator do nothing while
//! the counter said it had been honoured.

use pdfcer_core::color::RenderingIntent;

/// A built, ready-to-use source→destination colour transform.
///
/// Cheap to clone (it is a handle); expensive to build, which is why callers
/// hold an `Arc<IccBridge>` for the life of a page rather than constructing
/// one per paint.
pub(crate) struct IccBridge {
    chain: iccce_cmm::transform::Chain,
    /// How many components the SOURCE takes, from the PDF's `/N`. Kept so a
    /// caller handing over the wrong number of operands is refused rather
    /// than silently converted from a truncated or zero-padded colour.
    src_components: usize,
    /// How many components the destination expects. Kept so a caller can
    /// refuse a mismatched buffer rather than index past the end of a slice
    /// that a malformed profile made shorter than assumed.
    dst_components: usize,
}

impl std::fmt::Debug for IccBridge {
    /// Hand-written because `iccce_cmm::transform::Chain` is an opaque transform with no
    /// useful `Debug`, and a derived impl would either fail to compile or dump
    /// megabytes of lookup table into a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IccBridge")
            .field("src_components", &self.src_components)
            .field("dst_components", &self.dst_components)
            .finish_non_exhaustive()
    }
}

impl IccBridge {
    /// Build a transform from an embedded source profile to the document's
    /// output-intent destination profile.
    ///
    /// Returns `None` — never an error — when the transform cannot be built.
    /// See the module docs: an absent or unparseable profile means "do not
    /// colour-manage", which is a legitimate document state and not a failure
    /// pdfcer should surface as one. The caller falls back to the previous
    /// conversion, and the *disclosure* obligation (rule 4) is met by the
    /// render diagnostics counting how many paints were bridged without a
    /// profile, not by an error.
    ///
    /// # Why the intent is a parameter rather than read from the profile
    ///
    /// ISO 32000-1 §8.6.5.8 makes the rendering intent a property of the
    /// *graphics state* (`ri`, or `/RI` in an `ExtGState`), not of the
    /// profile. The profile carries a *default* intent; the content stream
    /// overrides it. Reading the profile's would silently ignore the operator's
    /// `ri`, which is the exact failure the cache-key note above warns about.
    pub(crate) fn build(
        src_profile: &[u8],
        src_components: usize,
        dst_profile: &[u8],
        intent: RenderingIntent,
    ) -> Option<Self> {
        let src = iccce_profile::Profile::parse(src_profile).ok()?;
        let dst = iccce_profile::Profile::parse(dst_profile).ok()?;
        let chain = iccce_cmm::transform::Chain::new(&src, &dst, to_iccce_intent(intent)).ok()?;
        // A destination profile that is not 4-component is a perfectly valid
        // profile; it is just not one this bridge can feed a CMYK buffer.
        // Probing with a mid-grey is cheaper and more honest than trusting a
        // header field, because it exercises the transform that will actually
        // run.
        //
        // The probe's WIDTH comes from the PDF's `/N`, not from the profile.
        // Table 66 makes `/N` Required and constrains it to 1, 3 or 4, so the
        // document has already stated the component count authoritatively --
        // and a disagreement between `/N` and the embedded profile is a
        // malformed file whose colour we should decline to manage rather than
        // resolve by picking a side.
        let probe = vec![0.5_f64; src_components];
        let dst_components = chain.convert(&probe).ok()?.len();
        Some(Self {
            chain,
            src_components,
            dst_components,
        })
    }

    /// Build a transform from an embedded source profile to **iccce's
    /// constructed sRGB** — the display route.
    ///
    /// # Why this exists beside [`Self::build`]
    ///
    /// [`Self::build`] answers *"what ink does this colour become on the
    /// document's named output device?"* and needs an `/OutputIntent` to
    /// answer it. This one answers *"what does this colour look like on the
    /// screen?"*, and the screen is sRGB by construction, so it needs no
    /// destination from the document at all. See the module docs for why
    /// that is not the built-in-*source* substitution this crate refuses.
    ///
    /// `Destination::None` is iccce's spelling for "I looked, there is no
    /// destination profile, construct sRGB" — and the chain records that it
    /// did (`DestinationProvenance::BuiltInSrgb`), so the substitution is a
    /// disclosed fact rather than a hidden one.
    ///
    /// Returns `None` — never an error — on an unparseable profile or a
    /// source model iccce cannot derive, for the same reason [`Self::build`]
    /// does: the caller falls back to Table 66's reinterpretation and the
    /// render counts the paint as unmanaged.
    pub(crate) fn build_to_srgb(
        src_profile: &[u8],
        src_components: usize,
        intent: RenderingIntent,
    ) -> Option<Self> {
        let src = iccce_profile::Profile::parse(src_profile).ok()?;
        let chain = iccce_cmm::transform::Chain::with_destination(
            &src,
            iccce_cmm::transform::Destination::None,
            to_iccce_intent(intent),
        )
        .ok()?;
        // Same probe as `build`, same reason: exercise the transform that
        // will actually run rather than trust a header field. The constructed
        // destination is three-component by construction, so this also pins
        // the width `convert_to_rgb` checks against.
        let probe = vec![0.5_f64; src_components];
        let dst_components = chain.convert(&probe).ok()?.len();
        Some(Self {
            chain,
            src_components,
            dst_components,
        })
    }

    /// Convert the source space's OWN components to an **encoded sRGB**
    /// triple — what the screen path paints.
    ///
    /// The output is sRGB *device* values from iccce's constructed profile,
    /// which means the transfer function has already been applied: these
    /// are the same encoded numbers `Rgb::from_rgb` expects, not linear
    /// light. Clamped for the same reason [`Self::convert_components`]
    /// clamps: an out-of-gamut source colour is entitled to land outside the
    /// unit interval, and the raster stores bytes.
    ///
    /// Returns `None` when the component count does not match what the chain
    /// was built for, or when this bridge was built to a four-component
    /// destination — a bridge built by [`Self::build`] must never be asked
    /// for a display colour, because its output is ink.
    pub(crate) fn convert_to_rgb(&self, comps: &[f32]) -> Option<crate::gstate::Rgb> {
        if self.dst_components != 3 || comps.len() != self.src_components {
            return None;
        }
        let input: Vec<f64> = comps.iter().map(|c| f64::from(*c)).collect();
        let out = self.chain.convert(&input).ok()?;
        let get = |i: usize| -> f32 { out.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0) as f32 };
        Some(crate::gstate::Rgb::from_rgb(get(0), get(1), get(2)))
    }

    /// Convert the source space's OWN components to CMYK tints in `0.0..=1.0`.
    ///
    /// # Why components rather than an sRGB triple
    ///
    /// Because the sRGB triple is already a lossy answer to the question being
    /// asked. By the time a colour has been flattened to `rgba` it has been
    /// through the alternate space and quantised to bytes; converting *that*
    /// would colour-manage pdfcer's approximation instead of the document's
    /// actual numbers. The `ICCBased` operands are what the file wrote and what
    /// the embedded profile describes, so they are what the chain should see.
    ///
    /// Returns `None` when the component count does not match what the chain
    /// was built for, or when the destination is not four-component — in both
    /// cases the caller falls back rather than writing a wrong-width result.
    pub(crate) fn convert_components(&self, comps: &[f32]) -> Option<[f32; 4]> {
        if self.dst_components != 4 || comps.len() != self.src_components {
            return None;
        }
        let input: Vec<f64> = comps.iter().map(|c| f64::from(*c)).collect();
        let out = self.chain.convert(&input).ok()?;
        let get = |i: usize| -> f32 {
            // Clamped because a CMM is entitled to return values slightly
            // outside the unit interval for out-of-gamut input, and the
            // colorant buffer stores tints as bytes.
            out.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0) as f32
        };
        Some([get(0), get(1), get(2), get(3)])
    }
}

/// Map pdfcer's rendering intent onto iccce's.
///
/// # Why there IS a catch-all arm, when the obvious thing is to forbid one
///
/// This function was first written with no `_` arm, on the reasoning that if
/// either enum gained a variant the compiler should stop the build rather than
/// let a new intent be silently rendered as the wrong one. That reasoning is
/// sound and the code still would not compile: `RenderingIntent` is
/// `#[non_exhaustive]`, so a match on it from *outside* `pdfcer-core` is
/// required to have a catch-all. The exhaustiveness this wanted is
/// unavailable here by construction.
///
/// So the arm is present, and it resolves to **relative colorimetric** —
/// which is not an arbitrary pick but ISO 32000-1 §8.6.5.8 Table 70's own
/// stated default for `/RI`. A future variant therefore degrades to the
/// behaviour a file with no `ri` operator would already have got, rather than
/// to whichever variant happened to be listed first.
///
/// ⇒ The transferable point: `#[non_exhaustive]` moves a compile-time
/// guarantee to a runtime choice, and the choice then has to be *defended* in
/// prose because no gate will ever check it again.
fn to_iccce_intent(intent: RenderingIntent) -> iccce_cmm::matrix_trc::Intent {
    use iccce_cmm::matrix_trc::Intent;
    match intent {
        RenderingIntent::Perceptual => Intent::Perceptual,
        RenderingIntent::RelativeColorimetric => Intent::MediaRelative,
        RenderingIntent::Saturation => Intent::Saturation,
        RenderingIntent::AbsoluteColorimetric => Intent::Absolute,
        _ => Intent::MediaRelative,
    }
}

/// Whether two profile handles carry the same profile.
///
/// # Why pointer identity is not enough, and why it is still tried first
///
/// Every consumer decodes its `ICCBased` stream afresh: the graphics-state
/// path at each `cs`, the image path at each decode. Each decode is a new
/// `Arc`, so a cache keyed on `Arc::ptr_eq` alone missed on EVERY image and
/// rebuilt the chain — parsing the profile again and appending a fresh
/// entry that nothing would ever hit. On a page of fifty photographs sharing
/// one embedded sRGB profile that was fifty parses and fifty entries. Worse
/// on the graphics-state side with a 1.2 MB CMYK profile, re-parsed at each
/// `cs`.
///
/// Comparing bytes fixes that, and `ptr_eq` stays as the fast path: the
/// paint loop asks once per paint through a handle it already holds, and a
/// pointer compare keeps that at a few instructions. The byte compare runs
/// only when the handle differs, which is once per decode or per `cs`, and
/// it short-circuits on length before touching the bytes.
///
/// Byte-equal profiles ARE the same transform: a `Chain` is a pure function
/// of the two profiles and the intent, so nothing about the handle's
/// identity can make two byte-equal profiles convert differently.
fn same_profile(a: &std::sync::Arc<[u8]>, b: &std::sync::Arc<[u8]>) -> bool {
    std::sync::Arc::ptr_eq(a, b) || (a.len() == b.len() && **a == **b)
}

/// A destination-only chain: profile connection space in, the output
/// intent's four inks out (`Pass 242.0`). Held behind an `Arc` by the cache
/// and by every `Lab`/`CalRGB`/`CalGray` image decoded on the page.
pub(crate) struct PcsBridge {
    chain: iccce_cmm::transform::Chain,
}

impl std::fmt::Debug for PcsBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PcsBridge").finish_non_exhaustive()
    }
}

impl PcsBridge {
    /// Separate a D50-relative XYZ into ink. Clamped for the same reason
    /// [`IccBridge::convert_components`] clamps.
    pub(crate) fn to_ink(&self, xyz: [f32; 3]) -> Option<[f32; 4]> {
        let out = self
            .chain
            .pcs_to_destination(iccce_color::Xyz {
                x: f64::from(xyz[0]),
                y: f64::from(xyz[1]),
                z: f64::from(xyz[2]),
            })
            .ok()?;
        let get = |i: usize| -> f32 { out.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0) as f32 };
        Some([get(0), get(1), get(2), get(3)])
    }
}

/// Every bridge one colour space can have, resolved once and applied in
/// front of [`crate::color::ColorSpace::to_rgb`] / `to_cmyk` (`Pass 243.0`).
///
/// # Why this exists
///
/// Three Passes put three routes onto three object types one at a time —
/// `ICCBased` fills (`199.2`), `ICCBased` images (`214.0`/`240.0`), CIE
/// colours (`242.0`) — and each time the shading and mesh readers stayed
/// behind, because their colour is resolved inside `shading.rs`/`mesh.rs`
/// through a bare `ColorSpace` that knows nothing about the page's bridge
/// cache. The fill's and image's route decisions are each spelled out at
/// their own call sites; a fourth copy of that ladder in the ramp builder
/// and a fifth in the vertex reader would be the "two answers to one
/// question" this crate's colour design exists to prevent.
///
/// So the ladder is HERE, once, and the readers call [`Self::to_rgb`] and
/// [`Self::to_cmyk`] instead of the space's own. The order is the fill
/// path's order (`Interpreter::authored_cmyk`, `display_managed_rgb`):
///
/// | space | `to_rgb` | `to_cmyk` |
/// |---|---|---|
/// | `ICCBased N 3` | profile → sRGB | profile → output intent, else `None` |
/// | `ICCBased N 4` | the space's own (`from_cmyk`) | profile → output intent, else `None` |
/// | `Lab` / `CalRGB` / `CalGray` | the space's own (`xyz_to_srgb`) | PCS → output intent, else `None` |
/// | everything else | the space's own | the space's own |
///
/// An empty bundle ([`Self::none`]) is the identity — every method falls
/// through to the space — which is what a document with no bridges gets,
/// so nothing about a plain `DeviceRGB` gradient moves.
#[derive(Debug, Clone, Default)]
pub struct ColorBridges {
    display: Option<std::sync::Arc<IccBridge>>,
    ink: Option<std::sync::Arc<IccBridge>>,
    pcs: Option<std::sync::Arc<PcsBridge>>,
}

impl ColorBridges {
    /// No bridges: every conversion is the space's own.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Whether any route here differs from the space's own.
    #[must_use]
    pub fn is_managed(&self) -> bool {
        self.display.is_some() || self.ink.is_some() || self.pcs.is_some()
    }

    /// The screen answer: the display bridge for an `ICCBased N 3` source,
    /// the space's own otherwise.
    #[must_use]
    pub fn to_rgb(
        &self,
        space: &crate::color::ColorSpace,
        comps: &[f32],
        intent: pdfcer_core::settings::CmykIntent,
        diag: &mut crate::color::ColorDiagnostics,
    ) -> Option<crate::gstate::Rgb> {
        if let Some(bridge) = &self.display
            && let Some(rgb) = bridge.convert_to_rgb(comps)
        {
            return Some(rgb);
        }
        space.to_rgb(comps, intent, diag)
    }

    /// The ink answer, in the fill path's ladder: the document's own
    /// `DeviceCMYK` answer first, then the profile bridge, then the PCS
    /// bridge, then `None` — which the caller bridges through `rgb_to_cmyk`
    /// exactly as before.
    #[must_use]
    pub fn to_cmyk(
        &self,
        space: &crate::color::ColorSpace,
        comps: &[f32],
        diag: &mut crate::color::ColorDiagnostics,
    ) -> Option<[f32; 4]> {
        if let Some(own) = space.to_cmyk(comps, diag) {
            return Some(own);
        }
        if let Some(bridge) = &self.ink
            && let Some(cmyk) = bridge.convert_components(comps)
        {
            return Some(cmyk);
        }
        if let Some(bridge) = &self.pcs
            && let Some(xyz) = space.to_pcs_xyz(comps)
        {
            return bridge.to_ink(xyz);
        }
        None
    }
}

/// A page-lifetime cache of built [`IccBridge`]es.
pub(crate) struct IccBridgeCache {
    /// The destination profile for this document, decoded once from
    /// `/OutputIntents` -> `/DestOutputProfile`. `None` means the document
    /// named no output device, so nothing here can be colour-managed.
    dest: Option<std::sync::Arc<[u8]>>,
    entries: std::cell::RefCell<Vec<CacheEntry>>,
    /// Destination-only chains for the PCS route (`Pass 242.0`), one per
    /// rendering intent — a chain's B2A selection depends on the intent,
    /// so two intents are two chains. `None` inside means the destination
    /// would not build a chain at all, cached so a broken profile is tried
    /// once rather than per paint.
    pcs: std::cell::RefCell<Vec<(RenderingIntent, Option<std::sync::Arc<PcsBridge>>)>>,
    /// Tallies, in `Cell` because the conversion happens behind `&self`.
    ///
    /// They live here rather than on the renderer's `Diagnostics` for a
    /// mechanical reason worth stating: the conversion is reached from
    /// `authored_cmyk(&self, ..)`, and widening that to `&mut self` would
    /// cascade through ten paint call sites to add a counter. Folding these
    /// into the real diagnostics once, at the end of the run, costs nothing
    /// and keeps the public `Diagnostics` free of interior mutability.
    managed: std::cell::Cell<usize>,
    unmanaged: std::cell::Cell<usize>,
}

/// Which destination a cached bridge was built to.
///
/// ★ Part of the cache KEY, and it must be: the same source profile at the
/// same intent builds two genuinely different transforms depending on whether
/// it is headed for the output device or the screen, and a cache that could
/// not tell them apart would hand an ink transform to a display lookup. That
/// is the R237 shape — a memo whose key omits one of its dependencies — in
/// its third field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BridgeDest {
    /// Source → the document's `/OutputIntent` profile ([`IccBridge::build`]).
    OutputIntent,
    /// Source → iccce's constructed sRGB ([`IccBridge::build_to_srgb`]).
    Srgb,
}

struct CacheEntry {
    /// ★ Held as an owned `Arc`, not a raw pointer, and that is a
    /// CORRECTNESS requirement rather than convenience. Lookup tries
    /// `Arc::ptr_eq` first (see [`same_profile`]), so if the cache did not
    /// keep the allocation alive a freed profile's address could be recycled
    /// by a later one and two different profiles would compare equal --
    /// silently converting a page with the wrong transform. Owning a clone
    /// makes the address stable for as long as the key can be consulted, and
    /// keeps the bytes for the equality fallback.
    src: std::sync::Arc<[u8]>,
    src_components: usize,
    intent: RenderingIntent,
    dest: BridgeDest,
    bridge: Option<std::sync::Arc<IccBridge>>,
}

impl std::fmt::Debug for IccBridgeCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IccBridgeCache")
            .field("has_dest", &self.dest.is_some())
            .field("entries", &self.entries.borrow().len())
            .finish()
    }
}

impl IccBridgeCache {
    pub(crate) fn new(dest: Option<std::sync::Arc<[u8]>>) -> Self {
        Self {
            dest,
            entries: std::cell::RefCell::new(Vec::new()),
            pcs: std::cell::RefCell::new(Vec::new()),
            managed: std::cell::Cell::new(0),
            unmanaged: std::cell::Cell::new(0),
        }
    }

    /// Separate a **profile-connection-space** colour into the output
    /// intent's ink — the route for a colour that has colorimetry but no
    /// profile and no colorants: `Lab`, `CalRGB`, `CalGray`
    /// (`Pass 242.0`).
    ///
    /// # Why a destination-only chain, and why it is cached per intent
    ///
    /// iccce exposes exactly this for named colours
    /// (`Chain::convert_pcs_to_device`), but that entry point builds a
    /// chain per call — parsing the destination profile every time, which
    /// for a 1.4 MB press profile is not a per-paint cost anyone can pay.
    /// So the destination-only chain (`Chain::new(dst, dst, intent)`, the
    /// same construction iccce's own function uses) is built once per
    /// intent here and `pcs_to_destination` is called on it.
    ///
    /// Per INTENT, not once: ISO 32000-1 §8.6.5.8 makes the rendering
    /// intent a graphics-state property, and a destination profile's B2A0/
    /// B2A1/B2A2 tables are genuinely different separations. iccce's named-
    /// colour entry point hard-codes media-relative because Table 66 fixes
    /// that for `ncl2`; a `Lab` fill under `/Perceptual ri` has no such
    /// clause and gets the intent the stream asked for.
    ///
    /// `xyz` is relative to D50 — [`crate::color::ColorSpace::to_pcs_xyz`]
    /// produces exactly that. `None` when the document names no output
    /// device, when the destination is not four-component, or when the
    /// profile will not model; the caller then falls back to the
    /// `rgb_to_cmyk` bridge it used before, and counts the paint as
    /// unmanaged.
    pub(crate) fn pcs_to_ink(&self, xyz: [f32; 3], intent: RenderingIntent) -> Option<[f32; 4]> {
        self.pcs_bridge(intent)?.to_ink(xyz)
    }

    /// The destination-only chain for one intent, as a shareable handle —
    /// what an image decode holds for the life of a decoded `Lab`/`CalRGB`/
    /// `CalGray` image (`image::Space::Special`), so a texel and a fill of
    /// one colour separate through one chain. Built once per intent; see
    /// [`Self::pcs_to_ink`].
    pub(crate) fn pcs_bridge(&self, intent: RenderingIntent) -> Option<std::sync::Arc<PcsBridge>> {
        let dest = self.dest.as_ref()?;
        let mut chains = self.pcs.borrow_mut();
        if let Some((_, hit)) = chains.iter().find(|(i, _)| *i == intent) {
            return hit.clone();
        }
        let bridge = iccce_profile::Profile::parse(dest)
            .ok()
            .and_then(|profile| {
                iccce_cmm::transform::Chain::new(&profile, &profile, to_iccce_intent(intent)).ok()
            })
            .filter(|chain| chain.output_channels() == 4)
            .map(|chain| std::sync::Arc::new(PcsBridge { chain }));
        chains.push((intent, bridge.clone()));
        bridge
    }

    /// Whether this document named an output device at all.
    pub(crate) fn has_destination(&self) -> bool {
        self.dest.is_some()
    }

    /// Record that a paint was converted by the engine.
    pub(crate) fn note_managed(&self) {
        self.managed.set(self.managed.get().saturating_add(1));
    }

    /// Record that a paint that could have been managed was not.
    pub(crate) fn note_unmanaged(&self) {
        self.unmanaged.set(self.unmanaged.get().saturating_add(1));
    }

    /// The two tallies, for folding into the renderer's diagnostics.
    pub(crate) fn tallies(&self) -> (usize, usize) {
        (self.managed.get(), self.unmanaged.get())
    }

    /// Every bridge `space` can have at `intent`, resolved once — see
    /// [`ColorBridges`] for the table. Cheap on a cache hit; a miss parses
    /// the profile once per page.
    pub(crate) fn bridges_for(
        &self,
        space: &crate::color::ColorSpace,
        intent: RenderingIntent,
    ) -> ColorBridges {
        match space {
            crate::color::ColorSpace::IccBased {
                n,
                profile: Some(src),
                ..
            } => ColorBridges {
                display: (*n == 3).then(|| self.get_srgb(src, 3, intent)).flatten(),
                ink: self.get(src, *n, intent),
                pcs: None,
            },
            crate::color::ColorSpace::Lab { .. }
            | crate::color::ColorSpace::CalRgb { .. }
            | crate::color::ColorSpace::CalGray { .. } => ColorBridges {
                display: None,
                ink: None,
                pcs: self.pcs_bridge(intent),
            },
            _ => ColorBridges::none(),
        }
    }

    /// Fetch or build the bridge for one source profile at one intent.
    pub(crate) fn get(
        &self,
        src: &std::sync::Arc<[u8]>,
        src_components: usize,
        intent: RenderingIntent,
    ) -> Option<std::sync::Arc<IccBridge>> {
        let dest = self.dest.as_ref()?;
        if let Some(hit) = self.lookup(src, src_components, intent, BridgeDest::OutputIntent) {
            return hit;
        }
        let bridge = IccBridge::build(src, src_components, dest, intent).map(std::sync::Arc::new);
        self.entries.borrow_mut().push(CacheEntry {
            src: std::sync::Arc::clone(src),
            src_components,
            intent,
            dest: BridgeDest::OutputIntent,
            bridge: bridge.clone(),
        });
        bridge
    }

    /// Fetch or build the **display** bridge for one source profile at one
    /// intent — source → iccce's constructed sRGB.
    ///
    /// Unlike [`Self::get`] this needs no `/OutputIntent`: the destination
    /// is the screen, which is known by construction. So it answers on a
    /// document that named no output device at all, which is the ordinary
    /// document. A `None` here means the profile itself would not parse or
    /// model, and the caller falls back to Table 66's reinterpretation.
    pub(crate) fn get_srgb(
        &self,
        src: &std::sync::Arc<[u8]>,
        src_components: usize,
        intent: RenderingIntent,
    ) -> Option<std::sync::Arc<IccBridge>> {
        if let Some(hit) = self.lookup(src, src_components, intent, BridgeDest::Srgb) {
            return hit;
        }
        let bridge = IccBridge::build_to_srgb(src, src_components, intent).map(std::sync::Arc::new);
        self.entries.borrow_mut().push(CacheEntry {
            src: std::sync::Arc::clone(src),
            src_components,
            intent,
            dest: BridgeDest::Srgb,
            bridge: bridge.clone(),
        });
        bridge
    }

    /// One lookup for both routes, so the two cannot come to key differently.
    ///
    /// The outer `Option` is "was there an entry"; the inner is the entry's
    /// own answer, which is `None` for a profile that was tried and would
    /// not build — cached so a broken profile is parsed once, not per paint.
    fn lookup(
        &self,
        src: &std::sync::Arc<[u8]>,
        src_components: usize,
        intent: RenderingIntent,
        dest: BridgeDest,
    ) -> Option<Option<std::sync::Arc<IccBridge>>> {
        self.entries
            .borrow()
            .iter()
            .find(|e| {
                e.src_components == src_components
                    && e.intent == intent
                    && e.dest == dest
                    && same_profile(&e.src, src)
            })
            .map(|e| e.bridge.clone())
    }
}
