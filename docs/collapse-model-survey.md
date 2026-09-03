# The N-plane → sRGB collapse: sourcing record

> ## ★★ AMENDMENT 2026-08-19 — THE CENTRAL CLAIM IS EDITION-SCOPED AND THIS SURVEY DOES NOT SAY SO
>
> This survey concludes that **"there is no consensus formula to find"** and
> that *"match Acrobat" is not an available specification*. Both statements
> are **true of the thirteen engines it measured** and the engine findings
> stand unamended.
>
> **What does not follow, and what this survey was read as licensing, is that
> no formula is SPECIFIED.** ISO 32000-2:2020 **§10.8.3 "Separation
> simulation"** specifies one — convert each separation to "flat XYZ" (no
> gamma) against an all-white background matte, combine with a multiply blend
> — and **§12.11.2 Table 275 NOTE 5** names it: *"This is sometimes referred
> to as "Overprint Preview"."*
>
> ★ **The reason this was invisible is worth more than the correction:
> pdfcer's spec corpus held NO CLAUSE 10 AT ALL** until 2026-08-19, and clause
> 10 is the colour-conversion chapter. The gap was found from **outside** the
> corpus, by a sibling project that tried to cross-reference us and could
> not. A survey of implementations cannot detect a specification its own
> reference corpus is missing — it will conclude "unspecified" from silence
> that is the library's, not the standard's.
>
> **Read `iso32000__s__10.8.md` before citing this file.** §10.8.3 is a
> `should` on the OUTCOME with three registered ambiguities of its own
> (`SEP-A2` a mis-citation, `SEP-A3` "flat XYZ" undefined, `SEP-A4` the
> ink→XYZ map unspecified), so it is a constrained target rather than a
> complete recipe — but it is a target, and this survey concluded there was
> none. The consequences for `Pass 97.2` are worked through in
> `docs/compositor-plan.md` Stage C.


**Written 2026-08-18**, engineer-owned. Commissioned research pass, all claims
URL-cited below. Companion to `docs/overprint-architecture-survey.md` (which
established *why* the N-plane buffer is required) and `docs/compositor-plan.md`
§4 Stage C (which is what this decides).

**The question this answers.** Once pdfcer composites overprint in an N-plane
colorant buffer (`Pass 97.1`), the buffer must be collapsed to sRGB for screen
display — "overprint preview". `docs/overprint-architecture-survey.md` §6
recorded that this step is unstandardised and that vendors disagree. This file
is the evidence for that claim and the decision it licenses.

---

## §0 — The one sentence

**There is no specified, published, or consensus collapse.** The N-plane
buffer → per-colorant tint transform → single ICC hop to sRGB is the *settled
architecture*; the **blend arithmetic inside it is a genuine free parameter
that two products from the same vendor answer differently**. It must therefore
be a documented **setting**, not a hard-coded choice — the standing rule
(never hard-code what the standard leaves open) applies squarely.

---

## §1 — PRIMARY: the absence is documented, not merely unfound

This matters more than any single vendor's method. **Hans Bärfuss** — founder
and CEO of PDF Tools AG, ISO TC171 participant — went looking through ISO and
reported the gap in print:

> *"It doesn't say, however, anything about how to compute a resulting color
> in a preview function."*
>
> *"…developers are trying to refer to the Adobe Acrobat implementation.
> However, **the method Acrobat uses is not publicly documented**."*

He obtained a functional diagram only by raising it in an ISO working group
(not public), then matched Acrobat **by eye** — *"the result looks similar to
Acrobat."*

<https://blog.pdf-tools.com/2014/07/how-to-preview-overprinting.html> ·
<https://www.pdf-tools.com/pdf-knowledge/how-preview-overprinting/>

**And Acrobat is explicitly not ground truth**, per the ICC itself:

> *"NOTE that many widely used PDF viewers, **including Adobe Acrobat**, do not
> support these spot inks and so should not be used as a guide."*

<https://www.color.org/cxf_test/>

Adobe's own wording concedes approximation: Output Preview is *"an onscreen
simulation that **approximates** blending and overprinting."*
<https://helpx.adobe.com/acrobat/using/previewing-output-acrobat-pro.html>

⇒ **"Match Acrobat" is not an available specification.** Any pdfcer decision
here is a product decision, and should be recorded as one.

---

## §2 — Per-engine landscape

**⚠ CLEAN-ROOM NOTE.** Ghostscript, MuPDF and Poppler are **AGPL/GPL**. The
rows below are **behavioural descriptions** — what those engines produce, for
divergence testing. **No implementation guidance, no code, is taken from
them.** `docs/LEGAL.md` §6.1: an MIT project cannot link them, and R61 makes
them behavioural references only.

| Engine | Model | Detail | Type |
|---|---|---|---|
| **Ghostscript** ≥9.54 | tint-equivalent + accumulate, **no n-channel ICC** | `pdf14` compositor holds CMYK-or-CMYK+spots; `-dOverprint=/simulate` shows overprint on Gray/RGB devices too. Spot equivalents from the alternate tint transform. `-dMaxSpots` default **64**. | PRIMARY |
| **Ghostscript** `tiffsep`/`psdcmyk` | **n-channel ICC**, optional | `-sICCOutputColors="Cyan,Magenta,…,Orange"` binds an NCLR profile. **Only** those devices. **15-colorant ceiling.** | PRIMARY |
| **Ghostscript** `tiffsep` composite | tint-equivalent | Documented as possibly wrong: *"may not produce an accurate preview, if the job uses overprinting."* **Do not copy it.** | PRIMARY |
| **Adobe Acrobat** | **undocumented** | Simulation Profile defaults to the **OutputIntent**; the working-CMYK fallback is **SECONDARY in every source found**. No algorithm ever published. **★ This is the WEAKEST row in the table and is graded accordingly:** `helpx.adobe.com` was unreachable across 8 URLs and 4 locales, `web.archive.org` blocked, so the Acrobat claims rest on an Adobe **community-expert post**, not Adobe primary. **That Acrobat resolves spot appearance via the tint transform is INFERENCE** — Adobe never states it; it merely follows that nothing else is machine-readable absent a named-colour library. | **SECONDARY** (PRIMARY only for the absence) |
| **Adobe APPE 6** | spectral-adjacent, **print only** | *"…uses modern perceptual-based color management to mix the color levels"* + a CxF spectral-spot ingest API. Marketing wording; no algorithm. | PRIMARY (non-technical) |
| **Poppler** (splash) | **per-colorant** tint transform + **hard-coded table** | `splashModeDeviceN8`: 4 process + `SPOT_NCOMPS` (**default 4**) spots. **Decomposes a multi-component `DeviceN` at mapping time**, pulling the matching single-colorant `/Separation` out of the DeviceN's own sub-colourspace list — so the plane list is always one-colorant Separations and the **collective DeviceN transform is never consulted** for a multi-ink overprint's screen colour. At readout each spot plane goes through **its own** tint transform into its own alternate, summed into CMYK, each channel clipped independently — then a **16-corner multilinear CMYK→RGB interpolation with fixed xpdf-heritage constants**, which **no** profile reaches (not `-displayprofile`, not `-defaultcmykprofile`, not the OutputIntent). `overprintMask` is a **32-bit colorant bitfield** (bits 0–3 CMYK, 4+ spots); **two write modes** — replace (default) and **additive, selected for spot spaces so two real spot inks accumulate instead of the second erasing the first**. | PRIMARY |
| **Poppler** (cairo) | **none** | `pdftocairo`, Evince, poppler-glib render knockout only. | PRIMARY |
| **MuPDF** ≥1.12 | **ICC per role**, OutputIntent load-bearing | Overprint applies **only to a subtractive destination**, so for an RGB target it pushes an internal group whose process space is **OutputIntent > proof > DeviceCMYK**, composites there, collapses on pop. `mudraw` **forces overprint simulation on when the intent's channel count differs from the target**, precisely so the intent gets simulated. Process channels go through the **ICC** converter; directly-mappable colorants copy plane-to-plane unconverted; **unmapped colorants take a per-pixel converter lookup** then add-and-clamp (subtractive dst) or subtract-the-complement (additive dst). Loader takes `/OutputIntents[0]` and **ignores `/S`**. **A faster orthogonal path — convert each equivalent once, add at the end — exists in the same file and is COMPILED OFF, with the orthogonality assumption named in the source comment as the thing being traded away. That disabled path is roughly what Poppler does today.** Entirely undocumented. | PRIMARY |
| **Harlequin RIP** | **architectural**; `max()` fallback | Collapse answer is **architectural, not arithmetic**: the N-channel buffer is a **virtual device** whose `/VirtualDeviceSpace` may be `/DeviceCMYK` (default), **`/DeviceRGB`** or `/DeviceGray` — spots render unconverted, and you collapse to RGB by *setting the virtual device's process space to RGB*. With `OverprintPreview=false`: *"emulated using a technique similar to the **Darken PDF blend mode** (that is, for each colorant… use the **darkest of the foreground and background**)"* — per-colorant `max()`. A Named Colour Database match **replaces** the job's alternate space and tint transform, first match wins. N-colour ICC is the **output/DeviceLink** path, explicitly **not** the preview path. Sourced from Global Graphics' own `documentation.hybridhelix.com`, **not** the vaguer Xitron OEM doc. | PRIMARY |
| **Mako** (Global Graphics) | **multiply-of-complements** — recipe published | One framebuffer per colorant, then merge scanline-by-scanline: `newVal = 1 − ((1 − eqCMYK[c]·spotVal)·(1 − currentVal))`. Merged CMYK → RGB via ICC. ⭐ | PRIMARY |
| **callas pdfToolbox** | **= Adobe's** | Built on the **Adobe PDF Library** (not APPE), so its overprint preview *is* Adobe's undisclosed model — **not an independent data point**. Plumbing: `simulationprofile`, `nosimulateoverprint` (simulation **on** by default), `colorspace Multichannel`; precedence sim profile → OutputIntent → sRGB / ISO Coated v2. **★ Its entire CxF surface is embed / extract / ANALYZE — no rendering, no appearance computation, no overprint prediction from spectral data.** The vendor most invested in CxF tooling still treats it as **validation metadata**, not a rendering input. | PRIMARY |
| **Enfocus PitStop** | **= Adobe's** | *"Overprint preview is an Adobe Acrobat function."* | PRIMARY |
| **PDFium / Chrome** | **none** | Tint transform only. *(pdfcer's render-parity oracle — see §5.)* | PRIMARY |
| **pdf.js** | **none** | Issue #7360, 2016, closed unimplemented. | PRIMARY |
| **ColorLogic ZePrA** | **spectral** | *"an intelligent spectral color mixing model"*; auto-uses embedded CxF/X-4; *"automatically considers the **opacity and the printing sequence** of spot colors."* | PRIMARY |

---

## §3 — The divergences, concretely

1. **★ One company, two products, two different collapse maths.** Harlequin
   documents per-colorant **`max()`/Darken**; Mako documents
   **multiply-of-complements**. Global Graphics has no house standard.
   **Anyone claiming an industry consensus formula is wrong.**
2. **Ghostscript's own three-way split, stated by Artifex:** *"it is possible
   to get **three different appearances for the same input file** using the
   `tiff24nc` (RGB), `tiff32nc` (CMYK), and `tiffsep` (CMYK plus spot colors)
   devices"* — and they add it is *"part of the PostScript and PDF
   specifications"*, i.e. **by design, not a bug**.
3. **Ghostscript regressed itself:** 9.16 simulated spot overprint on
   non-separation devices; **9.19 removed it** as *"not adequate for every
   case"*; **9.54** restored it via `pdf14`. Stated driver: *"several customers
   wanted Ghostscript to do the same as Adobe's Acrobat."* The de-facto target
   is undocumented **and moving**.
4. **Acrobat changed behaviour undocumented, ~Jan 2025:** switching the
   Simulation Profile used to change appearance at constant ink values; it now
   changes the **ink percentages** to preserve appearance. (SECONDARY, but
   well-specified in-thread; Adobe staff replied without addressing it.)
5. **Poppler silently destroys separations** past a 5th distinct spot name
   (compile-time `SPOT_NCOMPS = 4`), **and** when two spaces claim one
   colorant name with differing tint-transform results: it warns,
   tint-transforms that ink immediately via the **collective** transform,
   **and sets its overprint mask to all colorants**, losing overprint
   isolation entirely.
   **Version/tooling corrections, both PRIMARY:** the `SPOT_NCOMPS != 4`
   handling is Poppler **0.66.0 (2018-06-19)**, not 23.08.0 — 23.08.0 is
   *"Fix PCS 19.2 — DeviceN Overprint (White)"*. And the
   `-processcolorformat CMYK8` requirement is **`pdftops` only**:
   `pdftoppm` has its own independent `-overprint` flag (default false)
   that switches the render to DeviceN8, **present in the source and absent
   from every manpage**, unconditional since 0.81.0 removed the
   `SPLASH_CMYK` gate. ⇒ **"Poppler on this machine" is not a stable
   comparison baseline without checking its build flags.**
6. **OutputIntent selection is itself ambiguous and resolved differently.**
   MuPDF takes `/OutputIntents[0]` and **ignores `/S`** (so `GTS_PDFA1` and
   `GTS_PDFX` are indistinguishable to it); Poppler acts **only when the array
   has exactly one element**. ISO 32000-1 §14.11.5 permits several with
   different subtypes. **⇒ a second settings-shaped ambiguity.**
7. **No Acrobat-vs-Poppler or Acrobat-vs-MuPDF comparison exists anywhere.**
   Both projects validate against **the print-conformance suite** and **ECI
   Altona Test Suite 2**, not against Acrobat. If pdfcer wants that delta it
   must measure it — which is what `tools/suite-check.py` plus the Acrobat
   reference strips already do.
8. **★ THE LOAD-BEARING INVARIANT, stated by two vendors independently.**
   Harlequin: *"if spot colorants are converted to process colors,
   overprinting of these objects is disabled by default."* Ghostscript:
   *"Overprinting with spot colors is not allowed if the tint transform
   function is being used to convert spot colors."*
   ⇒ **Tint-transforming early and overprinting correctly are mutually
   exclusive.** This is a third independent confirmation of the N-plane
   architecture, arriving from vendor documentation rather than from pdfcer's
   own ablation (`ac15158`) or from the Artifex paper.

---

## §4 — Spot equivalent colour: what everyone actually does

**The tint transform is the universal answer at collapse time**, and PDF Tools
AG states both halves explicitly:

> *"If a rendering engine supports overprinting, then Separation, DeviceN and
> NChannel colors are rendered into separate channels **without transforming
> them**. At the very end when the final image is produced the channels are
> blended to receive the final color."*
>
> *"**The tint transform of the DeviceN colour space should only be used if
> the tint transforms of the individual colorants are not available.**"*

<http://blog.pdf-tools.com/2014/08/ambiguous-color-representation-of.html>

⇒ **Actionable rule: prefer the per-colorant `/Separation` tint transform over
the DeviceN collective one.** The difference is visible in real documents.

**Spectral / Neugebauer modelling: real science, ships only in prepress,
never in a viewer.** Deshpande & Green (CIC18) measured YNSN best at ~ΔE00 2.0
and Kubelka-Munk worst. Compute is *not* the blocker — real-time
Kubelka-Munk ships (Mixbox, SIGGRAPH Asia 2021). **Data is.** Spectral
Neugebauer needs 2^k measured primaries (CMYK + 3 spots = 128 patches on the
actual substrate), and a PDF carries no substrate reflectance, no solidity, no
tint ramp. A Yule-Nielsen `n` models optical dot gain **in a halftone**, and a
screen preview has no halftone. **⇒ out of scope, permanently, absent CxF.**

**Ink opacity and laydown order: measured, standardised, universally ignored.**
Hersch & Crété (IS&T/SPIE EI 2005) showed superposition-dependent dot gain is
real and **process-dependent in direction** (yellow gains *more* over cyan in
offset; magenta gains *less* over C+Y in thermal transfer), cutting mean ΔE
1.87→1.00 (offset) and 3.08→0.91 (inkjet). **So overprint is not commutative
in reality.**

The tractable middle road, and its formula is in the wild (Ricoh US 9,052,851
/ 9,665,324):

```
Xn = min(Xmax, max(X0, X1) + (1 − M) · min(X0, X1))
```

**Note it generalises the whole disagreement:** `M = 1` (transparent)
degenerates to Harlequin's `max()`; `M = 0` (opaque) to additive; and it sits
between `max()` and Mako's multiply.

**★ PDF has carried the inputs since 1.6 and nobody reads them:** DeviceN
`/Attributes → /MixingHints` with `/Solidities`, `/PrintingOrder`,
`/DotGain`. Essentially unpopulated in the wild and unconsumed by every engine
surveyed except ColorLogic ZePrA.

---

## §5 — Standards status

| Standard | Verdict |
|---|---|
| **ISO 32000-1/-2** | Defines **which colorants get marked**. Says nothing about computing a preview pixel. |
| **ICC nCLR (`2CLR`…`FCLR`) / `ncl2`** | Model a **fixed, measured ink set**. `ncl2` is a solid-tone LUT (one Lab per name at 100% — no opacity, no overprint model). **You cannot build one for an ad-hoc spot**: it needs a printed and measured target (CMYK 928–1,617 patches; 7-colour from ~1,260, growing combinatorially). Useless for a spot appearing in one job. |
| **iccMAX / ISO 20677** | Has a spectral PCS. **ICC itself says there is *"no drive to adopt"*** and still recommends v4. Its "Spot Colour Overprint Simulation" ICS deliberately standardises **the container, not the model**. |
| **CxF/X-4 (ISO 17972-4)** | Real, and **PDF 2.0 carries it natively**: OutputIntent `/SpectralData` (incl. ink opacity) + `/MixingHints`. Supported by ColorLogic, callas, GMG, Esko, X-Rite — **not** Acrobat/InDesign/Photoshop. Near-universally absent in the wild. |
| **ISO 12647** | Press process control. Off-topic. |
| **Pantone LIVE** | Cloud, licensed, gated. **Unusable by an MIT renderer.** |

**One free, standards-sourced accuracy win**, from the ICC's own ICS page:

> *"the interpolated 50% Lab tint will NOT correspond well to the interpolated
> 50% spectral tint. However, there should be good correlation between
> linearly interpolated **XYZ** values and spectral values."*

⇒ **Interpolate spot tint ramps in XYZ, never in Lab.** Costs nothing.

---

## §6 — What pdfcer should do

### Default pipeline (confidence: HIGH)

1. **Composite in the N-plane buffer. Never tint-transform on the paint path.**
   §3 item 8 — every vendor states that violating this disables overprint by
   construction.
2. **At collapse, resolve each spot plane via its own `/Separation` tint
   transform**, preferring per-colorant over the DeviceN collective one (§4).
   Accumulate with **multiply-of-complements**,
   `new = 1 − (1 − eq·tint)(1 − cur)` — Mako's documented formula. Preferred
   over Harlequin's `max()` because `max()` is Harlequin's own *degraded* path
   (`OverprintPreview=false`), and multiply is the subtractive-density model
   matching transparent-ink physics.
3. **One ICC transform, colorant space → sRGB, through the OutputIntent when
   present.** This is exactly where Poppler is weakest (a hard-coded xpdf
   table, never colour-managed) — a free place to **exceed the reference**.

### Settings to expose (confidence: HIGH — textbook "spec leaves it open")

| Setting | Values | Why |
|---|---|---|
| **Collapse blend** | `multiply` (default) / `max` (Harlequin-compatible) / `additive` | Demonstrably no consensus (§3 item 1); users matching a specific RIP need the lever |
| **Spot equivalent source** | `separation-tint-transform` (default) / `devicen-collective` / `lab-alternate` | Acrobat and InDesign already surface an equivalent lever ("Use Standard Lab Values For Spots") — parity, not invention |
| **Destination profile** | `output-intent` (default) / explicit simulation profile / working CMYK | Mirrors callas' documented precedence chain |
| **OutputIntent selection when ≥2 entries** | pick one, document it | MuPDF takes `[0]` ignoring `/S`; Poppler refuses. §3 item 6 |
| **Overprint mode** | `off` / `enable` / `simulate` | Ghostscript's tri-state is the right shape **and the right vocabulary**. Acrobat's own preference is four-valued and defaults **off**; recommend pdfcer default to *simulate-when-the-page-uses-overprint* and **disclose it off-canvas per rule 4** |

### Cheap wins

- **Interpolate spot tint ramps in XYZ, not Lab** (§5). Free. Confidence HIGH.
- **Read `DeviceN`'s `/MixingHints` (`/Solidities`, `/PrintingOrder`) when
  present** — §8.6.6.5 Table 73, PDF 1.6, in the **attributes** dictionary —
  and switch to `Xn = min(Xmax, max(X0,X1) + (1−M)·min(X0,X1))`, which makes
  overprint correctly **non-commutative**. Ignored by every surveyed engine
  but ColorLogic. **★ But see §6.5: `DN-N1` records that the standard defines
  NO algorithm to consume these — they are "inputs to an unspecified algorithm
  by design", marked PERMANENT.** So this is pdfcer **inventing** a model, not
  implementing one: it is an "exceed the parity reference" item that must be
  **disclosed as pdfcer's own** and must be a setting. Confidence MEDIUM (data
  is rare; code path is small).
- **Detect PDF 2.0 OutputIntent `/SpectralData` (CxF/X-4) and disclose it.**
  No open-source engine consumes it and per ICC neither does Acrobat. Full
  spectral mixing is a later Pass; recognising and reporting it is cheap.

### Do NOT pursue

**N-channel ICC / DeviceLink as the primary path.** Confidence HIGH.
Ghostscript supports it only for `tiffsep`/`psdcmyk` with a hand-supplied
profile and a **15-colorant ceiling**; neither Poppler nor MuPDF does it at
all; and **you cannot construct such a profile for an ad-hoc spot without
printing and measuring it.** Keep it as an optional advanced input if an
operator supplies an NCLR profile.

---

## §6.5 — ★ WHAT pdfcer'S OWN SPEC CORPUS CORRECTS, AND IT CORRECTS BOTH PASSES

A **second pass** from the same researcher converged on every headline above —
same Bärfuss source, same per-colorant-over-collective rule, same Mako formula,
same Ghostscript three-appearances statement, same `moxcms`/`lcms2` verdict,
same "don't build spectral". On its own that is a **repeated measurement, not
two independent ones** (R188), and it was recorded as such.

**★ THEN THE INDEPENDENT PASSES ARRIVED, AND THE ROUTING IS ITSELF THE
FINDING.** Two peer researchers (Poppler/MuPDF; Acrobat/commercial RIPs) had
been dispatched by the lead and **their results never reached it** — their
`SendMessage` replies failed with *"No agent named 'general-purpose' is
reachable"*, so both delivered **to the orchestrator instead**. The lead
therefore closed out stating, correctly and unprompted, that its briefing
*"should be filed as single-sourced by me, not as a merged multi-agent
result"* — while the orchestrator was holding both peer reports.

Two consequences worth separating:

- **The convergence IS independent after all**, for the claims the peers
  covered — and they did not merely agree, they **corrected the lead** on
  Poppler's version history, the `pdftops`-vs-`pdftoppm` flag distinction,
  Harlequin's sourcing, and MuPDF's OutputIntent handling. All of those
  corrections are folded into §2 and §3 above **in place**, not appended.
- **A harness fact worth carrying:** a subagent's *grandchildren* report to
  the **orchestrator**, not to the parent that spawned them. A parent can
  therefore truthfully report "my peers never returned" while their output is
  sitting in the orchestrator's queue. **Neither party is wrong and neither
  can see it** — only the orchestrator can join them.

**And a self-correction from the lead, unprompted and worth the same weight as
its findings:** it had earlier stated mid-run that the Poppler/MuPDF research
"is in and is very detailed" when no agent had reported at that point. It
flagged and withdrew that itself. Same shape as this session's own repeated
defect — a confident statement about work that had not been checked.

Checking both against `D:\Dev\Rag-Specialized\PDF_Spec\` — which had
already registered these as ambiguities on 2026-08-08, **before either
research pass ran** — corrected three things:

| Claim | Corpus says |
|---|---|
| Pass 2: *"`SpectralData` **and** `MixingHints` in the OutputIntent dictionary"* | **`/MixingHints` is in the `DeviceN` ATTRIBUTES dictionary** — §8.6.6.5, Table 73, PDF **1.6**, under the `NChannel` subtype, alongside `/Colorants` and `/Process` (`color__devicen.md`). Pass 1 had this right. PDF 2.0's OutputIntent `/SpectralData` is a *separate* key; do not conflate them. |
| Both passes: *"read `/MixingHints` and switch to the opacity blend"* | **`DN-N1`, marked PERMANENT: "No blending algorithm is defined to consume `/MixingHints`; it is inputs to an unspecified algorithm by design."** So consuming it is **pdfcer inventing a model**, not implementing one. Permitted (exceed-the-reference), but it must be disclosed as pdfcer's own and is settings-shaped by construction. |
| Both passes: *"destination profile = the OutputIntent"* | **`OI-N2`: "the standard states NO relationship between output intents and overprint, trapping, or `Separation` alternate spaces."** Every vendor does it anyway. That makes it the right **default** and a **product decision**, not a conformance requirement — record it as such. |

**And one independent rediscovery worth noting**, because it is the strongest
form of confirmation available here: the corpus had already measured that ISO
32000-1 *"**never** describes overprint preview/simulation"* — `overprint
preview` **0 hits**, `separation preview` **0**, `ink manager` **0** across the
756-page source — and concluded that *"Acrobat's three simulation toggles have
no normative counterpart and their precedence is not derivable from the spec."*
Two web research passes with no access to that file reached the same
conclusion from vendor documentation. **The gap is real and it is now
established from two directions.**

---

## §7 — ★ THE ICC HOP IS `iccce`'S, AND THIS SECTION WAS WRONG WHEN FIRST WRITTEN

**CORRECTED 2026-08-18, after the operator said "iccce has been updated, use
the latest version."** This section originally recommended **`moxcms`**
(BSD-3-Clause OR Apache-2.0, pure Rust, ≤16 inks) as *"the candidate of
record"* for the final ICC hop, and the librarian filed a matching
`docs/PRIOR_ART.md` entry. **Both were wrong, and not marginally.**

**`ARCHITECTURE.md` decision 064 (2026-08-17) already assigns colour
CONVERSION to `iccce`** — the operator's own from-scratch MIT colour-management
project at `D:\Dev\iccce\`, whose README names pdfcer as its first consumer:

> pdfcer owns **COMPOSITING** (overprint, blend modes, transparency groups, and
> what a PDF's colour components mean); `iccce` owns **CONVERSION** (profile
> parsing, transform construction, rendering intents, ΔE).

A third-party CMM crate is not a gap to fill — **the boundary is decided, and
recommending a competitor to it was a decision made without reading the
record.** Root cause on the engineer's side: `CLAUDE.md` requires reading
`docs/ARCHITECTURE.md` every session; it was not read this session, and the
research brief that produced the `moxcms` recommendation never mentioned
`iccce`, so the researcher could not have known.

### What `iccce` already provides — verified by reading its source, not its prose

`crates/iccce-cmm/src/transform.rs`:

```rust
Chain::with_destination(&src, Destination::None, intent)   // line 670
pub enum Destination<'a>                                    // line 133
pub enum DestinationProvenance { BuiltInSrgb, .. }          // lines 168–172
pub fn destination_provenance(&self) -> DestinationProvenance   // line 877
```

Tests present: `builtin_srgb_destination.rs`, **`builtin_srgb_from_cmyk.rs`**
(a LUT-based CMYK press profile into the constructed destination — the PCS
handed over is **Lab, not XYZ**, so it genuinely exercises unification),
`builtin_srgb_a2b_only_source.rs`.

**The built-in sRGB destination is constructed from published constants** —
ITU-R BT.709-6 primaries, W3C transfer-function constants, Bradford-adapted
to the D50 PCS per ICC.1:2022 Annex E.3. **There is no shipped `.icc`**, so
`tools/check-shipped-assets.py` has nothing to refuse and `LEGAL.md` §6.1's
profile-redistribution objection does not arise.

### ★★ THE TRAP, and it is rule-4-shaped

`Destination::None` is **not** `Option::None`. It is a caller **assertion**
that you looked and there genuinely is no destination. The chain then records
`DestinationProvenance::BuiltInSrgb`, whose own disclosure reads:

> *"…This is NOT the document's declared output intent — if one was declared,
> it was not passed to iccce."*

**A destination that WAS declared and failed to parse must never reach
`Destination::None`.** That is a named refusal to propagate, not a fallback to
take — a PDF/X page rendered to a substituted sRGB while its declared print
condition was silently dropped would look completely normal. Surface
`destination_provenance()` in the export log and the substitution can never be
invisible.

### The cost, which is a Stage C design input and not a footnote

iccce measures its grid evaluation at **~1.4 Mpix/s**, and states plainly that
a narrower `u8` buffer surface would fix the **memory** (268 MB → 67 MB in,
201 MB → 25 MB out at 300 DPI) and **do nothing for the time**, because the
cost is per pixel regardless of how the pixel arrived.

⇒ **~6 s/page against pdfcer's ~0.6 s render — roughly 10×.** So the collapse
cannot be an unconditional per-frame step in an interactive viewer. Options,
none chosen yet: collapse only on export; cache the converted page; or engage
it only for pages that actually use overprint or a declared output intent
(§3.4's scope guard, which already exists for a different reason). iccce has
committed to answering *"is the grid evaluation inherently this cost?"* **with
a measurement** when its Pass 6 optimisation work reopens.

### `lcms2` is still rejected, for the same reason as before

C bindings; cannot cross the **wasm32** gate `pdfcer-core`/`pdfcer-render` are
held to. **`iccce` gates `wasm32` in its own CI as of 2026-08-17** — build over
all four library crates, *plus* a separate job asserting every crate in
`cargo tree` is one of theirs. (Worth stealing: their first version matched
`^iccce-` and an injection test found a registry crate named `iccce-evil`
would have passed; the pattern now requires the parenthesised path `cargo tree`
renders for a path dependency. **Name-based trust is what a typosquat
exploits.**)

**Nothing is added to any `Cargo.toml` yet, and that is deliberate** — the
compositor (`Pass 97.0`/`97.1`) has to exist before there is anything to
convert. A dependency with no caller is the `R151` shape.

## §8 — Validation corpus

The print-conformance suite's `SPOT` and `CMYK` categories, plus
**ECI Altona Test Suite 2**. This is what Poppler itself validates against and
it is the only rights-clean overprint corpus. pdfcer already runs the
print-conformance half (`tools/suite-check.py`); **Altona is not on this
machine and is not currently used.**
