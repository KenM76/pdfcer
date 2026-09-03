# The print-conformance suite — per-patch reference

**Written 2026-08-18** by the engineer, from a commissioned research pass
that extracted the suite's combined ReadMes document (82 pp) and the
combined test-page captions.

**Why this file exists.** The suite's per-patch documentation is **not on
the web** — it ships inside the suite's individual-patches download (126 MB;
force HTTP/1.1, HTTP/2 truncates), alongside a separate ReadMes bundle
(24,508,479 bytes, no login). Re-obtaining this costs a download and an
extraction pass, so the extracted substance lives here.

**Label convention, carried from the research pass:**
- **PRIMARY (suite ReadMe)** / **PRIMARY (suite caption)** — quoted suite text.
- **MEASURED** — inspection of the patch artwork itself.
- **DERIVED** — computed from ISO 32000-1 Tables 148/149. **Not published by
  anyone.** Treat as a strong prediction, not as ground truth.

**Colorant-name note.** The patches' own spot colorant is a named
`/Separation` whose name identifies the suite's publisher; referred to
throughout this document as **Suite Green** (vocabulary scrub, 2026-08-25).
This is a substitution in this document's *prose*, not a claim about the
literal bytes in any patch file.

Numbering: PCS010 = "patch 1.0", PCS192 = "patch 19.2".

---

## §0 — Two rules that change how the harness should score

**PRIMARY, stated by Stephan Jaeggi (Co-Chair, the suite's Process Control
Subcommittee):**

> **"Faint X does not indicate a failure!"**

Evaluation is explicitly perceptual and explicitly tolerant — a human at
0.5 m / 20 in, *"you will not need a loupe"*. ~~`tools/suite-check.py` may
therefore be over-counting~~; its `CONTRAST_MIN` was calibrated against
pdfcer's own output, not against the suite's stated criterion. Patches the
suite pre-declares tolerant: **all ten cells of PCS020**, and **cell d of
each DeviceN patch**.

**★ MEASURED 2026-08-18 — the over-counting suspicion is FALSE, and this
paragraph is kept rather than deleted so the question is not re-opened.**
`tools/suite-cell-probe.py` measured the actual X-versus-surround contrast on
every still-failing patch those tolerances cover:

| patch | X | surround | the suite's word for it |
|---|---|---|---|
| `PCS020` (6 of 7 cells) | `[254,254,253]` **white** | `[141,197,62]` green | *"faint … in slightly darker green"* |
| `PCS190` cell d | `[0,0,0]` **black** | `[0,158,218]` cyan | *"a **faint** cross in patch d"* |
| `PCS192` cell b | `[255,255,255]` **white** | `[239,56,62]` red | — |

A white X on green is not "slightly darker green". Every trap still firing is
at or near **maximal** contrast, so **no recalibration consistent with the
suite's own criterion moves a single verdict.** The failures are real. §10's
caveat about strict pixel-diffs still stands in principle; it just does not
apply to anything pdfcer is currently failing.

**Two tolerances that survive, because neither is about contrast:**
- **`PCS191` cell c has TWO sanctioned correct outcomes** (§9) — a cross there
  is acceptable *"if the system performs colour conversion and sets the OPM
  for this patch c to 0"*. pdfcer converts but leaves `OPM 1`, so its cross is
  a genuine failure **today**; a future Pass that takes the other route must
  teach the harness that cell c is not binary.
- **The transparency patches are STRICTER, not more tolerant** — see §11:
  *"A 100% correct rendering does expect a 100% 'X' free output."*

**The suite ships a Reference file** — a whole-suite reference render, in the
same ZIP as the patches. Its texts are in Registration (`/Separation /All`)
so they appear in every separation. **pdfcer is not currently using it as an
oracle and should** — but **checked 2026-08-18, the file is not on this
machine**; the local corpus directories were kept from the download and it
was not. Re-fetching is an operator call (large download, `LEGAL.md` §5), so
this is **owed, not merely unstarted**.

Composition of the 51: **27 CMYK-only, 8 SPOT, 16 CMS** (ICCBased /
colour-management). **So the spot axis is only 8 of 51** — that bounds the
direct conformance ROI of the n-channel work, though some CMS patches carry
spots too.

Slide decks by the suite's publisher, authored by one of its officers, exist
but are not archived here (their URLs name the suite directly, which this
document's vocabulary scrub excludes).

---

## §1 — The five rules everything derives from

From ISO 32000-1 §8.6.7 + Tables 148/149:

- **R1** — `OP`/`op` true ⇒ colorants **not specified by the source colour
  space** are left unchanged. `OP` false ⇒ they are erased (painted 0.0).
- **R2** — `OPM 1` ⇒ additionally, a source component of **0.0 leaves that
  colorant unchanged**. Applies **only** when the current space is
  `DeviceCMYK` **specified directly** — *"shall not apply to the painting of
  images or to any colours that are the result of a computation, such as
  those in a shading pattern or conversions from some other colour space."*
- **R3** — `DeviceGray` is a **process** space: converting to a CMYK device
  it specifies **all four** process colorants, and `OPM` never applies to it.
  **DeviceGray cannot overprint DeviceCMYK, at either OPM.**
- **R4** — `Separation`/`DeviceN` specifies **only its named colorants**;
  all others unchanged under `OP` true. `OPM` never applies.
- **R5** — Table 149: *"For spot colour components, the value shall always
  be c_b"* (the backdrop).

**R3 corroborated by two vendors.** Peter Kleinheider (author of
PCS041/132/133): *"DeviceGray can not overprint DeviceCMYK"*. And Heidelberg
Prinect: *"'DeviceGray' colors overprint all spot colors lying lower down.
However, contrary to expectations, **CMY separations are knocked out**"* —
with a shipped remedy, *"Turn Overprinting Device Gray into K"*, which
converts to `/Separation/Black` because *"This conversion causes CMY
separations to be overprinted."*
<https://onlinehelp.prinect-lounge.com/Prinect_PDF_Toolbox/Version2021/en/Prinect/Color_management/Color_management-9.htm>

**Why `/Separation /Black` differs from `k`:** §8.6.6.4 reserves **only**
`/All` and `/None`; colorant names are *"arbitrary"*. `/Black` is matched
against *the device's* colorant list — on a subtractive device it paints K
alone; on an **additive** device a Separation *"never applies a process
colorant directly; it always reverts to the alternate colour space."* **For
pdfcer that only works inside a simulated subtractive device** — another
route to the same n-channel conclusion.

---

## §2 — ★ PCS040 White Overprint — the patch that diagnosed pdfcer's bug

**PRIMARY (ReadMe)**, 10 Nov 2006, Peter Claes. 4.0.1 replaced a withdrawn 4.0.

**MEASURED:** `/CS1 = [/Separation /Suite Green /DeviceCMYK]`, tint transform
`C0=[0,0,0,0] → C1=[0.5, 0, 1, 0]` — **Suite Green = 50C 0M 100Y 0K at full
tint, and carries no K**. `/CS2 = [/Separation /Black]`. Six ExtGStates
spanning OPM 0/1 × op true/false.

**PRIMARY (caption)** — 12 cells; left column OPM 0 (a–f), right OPM 1 (g–l):

| | OPM 0 | | OPM 1 |
|---|---|---|---|
| a | CMYK over spot | g | CMYK over spot |
| b | Gray over spot | h | Gray over spot |
| c | Sep. black over spot | i | Sep. black over spot |
| d | CMYK over CMYK | j | CMYK over CMYK |
| e | Gray over CMYK | k | Gray over CMYK |
| f | Sep. black over CMYK | l | Sep. black over CMYK |

**Only documented expected result — PRIMARY, deliberately unenumerated:**
*"Objects that are set to 0% and are set to overprint **disappear in most
cases, but not all cases**… includes examples of cases where objects would
be expected to disappear as well as cases where the proper behavior would be
to knock out the object below."*

### DERIVED per-cell truth table (not published by the suite)

| Cell | Result |
|---|---|
| **a** CMYK 0% over spot, OPM 0 | spot survives — **object INVISIBLE** |
| **g** same, OPM 1 | **identical to a — INVISIBLE** |
| **b** Gray 0% over spot, OPM 0 | R3 writes four process 0; spot unchanged — **INVISIBLE** |
| **h** same, OPM 1 | **identical to b** |
| **c** Sep/Black 0% over spot, OPM 0 | R4: Black 0, Green unchanged — **INVISIBLE** |
| **i** same, OPM 1 | **identical to c** |
| **d** CMYK 0% over CMYK, OPM 0 | all four written 0 → **knocks out — WHITE, VISIBLE** |
| **j** CMYK 0% over CMYK, OPM 1 | all four 0 leave backdrop — **INVISIBLE** |
| **e** Gray 0% over CMYK, OPM 0 | R3 → **knocks out — WHITE, VISIBLE** |
| **k** same, OPM 1 | R3: OPM inapplicable → **still knocks out — VISIBLE** |
| **f** Sep/Black 0% over CMYK, OPM 0 | R4: K zeroed, C,M,Y survive — **PARTIAL** |
| **l** same, OPM 1 | **identical to f** |

**Invariants:** a = g = b = h = c = i (all six "over spot" identical,
invisible); d, e, k knock out to white; j overprints; f = l partial.
**d vs j** is the sharpest OPM discriminator in the suite. **j vs k** is the
"DeviceGray ≠ DeviceCMYK" discriminator — same tint, same OPM 1, opposite
result.

### ★ Why this diagnosed pdfcer

> Flattening spot through the tint transform to RGB before compositing
> destroys R1 and R5: once Suite Green is RGB there is no unspecified
> colorant left to leave unchanged, so painting 0% over it erases it. **That
> predicts a white X in exactly a, b, c (and g, h, i), and correct output in
> d, e, f where the right answer IS knockout.**

That prediction was derived independently of pdfcer's output and **matches
the observed render exactly** (2026-08-18, `ac15158`). **The fix is
colorant-level compositing, not an overprint special case.**

---

## §3 — ★ PCS030 Gray / K black Overprint

**PRIMARY (ReadMe)**, 06 Jan 2006, Peter Claes. Same 12-cell geometry as
PCS040 (a–f OPM 0, g–l OPM 1): 50% K / 50% gray / 50% sep-black, each over
spot and over CMYK.

**MEASURED:** `/CS1 = [/DeviceN [/Black /Suite Green] /DeviceCMYK]` — **the
"spot" backdrop is a two-colorant DeviceN carrying Black**, not a plain
Separation. `/CS2 = [/Separation /Black /DeviceCMYK]`. Backdrop fills
`0.5 1 scn` and `0.5 0 1 0.5 k`. Foreground fills `0 0 0 .5 k`, `.5 g`,
`/CS2 cs .5 scn`. **Eleven ExtGStates with `/OP` and `/op` set
independently** — so this patch also discriminates **stroke-vs-fill
overprint**, which no suite prose mentions.

### DERIVED per-cell truth table

| Cell | Result |
|---|---|
| **a / g** 50% K over DeviceN(Black .5, Green 1) | **Green + 50% K** |
| **b / h** 50% DeviceGray over spot | R3 → **Green + 50% K** |
| **c / i** 50% Sep/Black over spot | R4 → **Green + 50% K** |
| **d** 50% K over CMYK, OPM 0 | **knocks out — plain 50% K grey** |
| **j** same, OPM 1 | C,M,Y = 0 leave unchanged — **backdrop preserved, OVERPRINTS** |
| **e / k** 50% Gray over CMYK, both OPM | R3 → **knocks out — plain 50% K** |
| **f / l** 50% Sep/Black over CMYK | R4 → **backdrop preserved, OVERPRINTS** |

**a = b = c = g = h = i** — a strong checkable invariant: **the three
encodings of black must agree over a spot backdrop.**
**d vs f at OPM 0** is the sharpest single pair: same tint, same backdrop,
same OPM — DeviceCMYK knocks out, Separation overprints.

**★ Version warning — PRIMARY (the suite's v4 whitepaper §2.3.2):** PCS 3.0
and PCS 12.0 were **silently changed** in the suite's 4.0 revision *"to
prevent ghosting effect"*, **filenames unchanged**. **Pin fixtures to a file
hash.**

### ★★ MEASURED 2026-08-29 (`Pass 174.2`) — all three surviving traps, attributed

Recorded here because this patch had been carried as *"cause unknown, and
**not** the defect `Pass 143.0` fixed"* for a week, with a lead
(*"`.5 G` — a grey **stroke** — appears in its streams while every synthetic
fixture uses fills"*) that turns out to be **the wrong lead**. Both halves
are worth keeping: the finding, and the fact that the plausible lead was
plausible.

**The instrument** is `--probe-ink` (`Pass 174.0`), which reads the ink in
the colorant buffer rather than the sRGB it converts to, so the X and its
surround can be compared as *ink* instead of as pixels.

| trap (device px, scale 2) | X's ink | surround's ink | reference shows |
|---|---|---|---|
| `(27, 68)` | `0 0 0 0.500` | `0.443 0 0.885 0.500` | whole cell green — X invisible |
| `(28, 135)` | `0 0 0 0.500` | `0.443 0 0.885 0.500` | whole cell green — X invisible |
| `(434, 136)` | `0.500 0 1.000 0.500` | `0.029 0 0.059 0.500` | whole cell neutral grey |

**1. `overprint_zero_tint_scope` is not the cause — and this line said "the
`Pass 143.0` ambiguity" until `Pass 174.6`.** It is a **divergence** from
ISO 32000-1, not an ambiguity (`OP-A5`), and the correction shipped in the
paragraph immediately below **while the bolded lead above it kept the word**.
That is the sharpest of the six hard-rule-11 survivors the 330th filing found:
*a sweep that starts from the diff sees what changed; only a sweep that starts
from the claim sees what did not.* Rendering the same pixel
under all three `overprint_zero_tint_scope` values — `device_cmyk_only`,
`grey_as_k_only`, `all_process_spaces` — returns **bit-identical ink**. The
setting moves the page's total effective-overprint count (24 / 29 / 29) and
does not move this pixel at all.

★★ **AND THE ABLATION IS WEAKER THAN IT LOOKED, which is worth more than the
conclusion it supported.** Audited by `pdfcer-spec-librarian` the same day
(register `OP-N3`): Tables 148/149 put *"any process colour space"* × **spot
colorant** × `OP true` at `c_b` — *do not paint* — in **both** the `OPM 0`
and `OPM 1` columns. So on a **spot** backdrop all three settings agree by
construction, and a bit-identical result was **forced by the table**. It
would have come out identical on a correct implementation and on a broken
one. **The discriminating case is grey over PROCESS components**, and this
patch's failing cells are not it.

The conclusion stands — the cause is §3 below, derived from the file's own
colour spaces rather than from the ablation — but the ablation is a
consistency check, not the evidence. A measurement whose outcome is entailed
by the spec is not a measurement of the implementation.

**2. The grey-STROKE lead is refuted too, and is worth recording as a near
miss.** `.5 G` does occur in this patch — but under `/GS5`, on a *different*
cell. The two traps at `(27,68)` and `(28,135)` are `0 0 0 .5 k` and `.5 g`
**fills** under `/GS4`, and their whole 49×49 interior is wrong, not a
0.283 pt outline. A stroke defect cannot produce a solid wrong X. ★ The lead
was reasonable — every synthetic fixture pdfcer owns does use fills, and `B`
(fill **and** stroke) is genuinely unusual — but *"an untested route exists"*
is not *"the untested route is the cause"*, and here it was not.

**3. The actual cause: pdfcer has no per-spot-colorant plane.** The backdrop
is `/CS1 = [/DeviceN [/Black /Suite Green] /DeviceCMYK]`, and pdfcer flattens
Suite Green into C/M/Y — the measured `0.443 0 0.885` above **is** that
flattening. A `DeviceCMYK` source specifies C, M, Y and K, so Table 149 has
it knock those four out; but it does **not** name Suite Green, whose plane
must therefore survive. In pdfcer there is no such plane to survive, so
knocking out C/M/Y destroys the spot with them. The suite's own DERIVED
truth table above says cells a/b/c/g/h/i must all read *"Green + 50 % K"*,
which is exactly what a surviving spot plane produces.

⇒ **`PCS030` belongs to the same bucket as `PCS020`, `PCS040` and `PCS081`:
the n-channel (per-colorant) buffer.** It is not a separate unknown, and
`ROADMAP.md`'s existing "the remaining overprint/spot FAILs need a real
n-channel buffer" line already covers it. One fewer open mystery, no new
work.

**UNRESOLVED — cell e/k backdrop.** The ReadMe says *"a 50% Gray vector
object is set to overprint **a Gray object**"*; the caption says *"50% gray
over **CMYK**"*. Wording is identical in the standalone ReadMe, the combined
ReadMes, **and the earlier manual** — stable ~20 years, so not an extraction
artifact. DERIVED reading: the caption is right, because the grid is
symmetric (d/e/f all "over CMYK") and *"50% Gray over a Gray object"* would
be a **degenerate no-op** that could never show an X. Settleable in ~10
minutes by mapping rectangles to cells in the content stream.

---

## §4 — PCS010 CMYK Overprint

Five object types × two OPM. **PRIMARY**, 07 Nov 2005. Caption columns
`font | vector | image | mask | shading`; a–e OPM 0, f–j OPM 1.

**★ Polarity is INVERTED for four cells — PRIMARY, stated twice:** *"Images
or image masks in CMYK should never overprint CMYK objects. If an X shows,
it means that overprints have been **wrongly applied**."* So **c/d/h/i fail
if you DO honour overprint**; a/b/f/g fail if you don't. Spec basis is R2.
The shading cells e/j likewise.

**Two corrections to the ReadMe — both MEASURED. Use the artwork, not the prose:**

1. **`/ImageMask` occurs ZERO times in the file.** One XObject, `/Im0`,
   `Subtype /Image`, `ColorSpace [/Indexed /DeviceCMYK 0 <lookup>]`
   (hival 0), 95×95, drawn **four** times — both the "image" and "mask"
   columns draw it. **PCS010 does not test image masks**, despite the
   ReadMe and caption saying so.
2. ReadMe cells h and i say *"with op mode 0"* while sitting in the OPM 1
   block. **MEASURED:** those draws use `/GS4 = {OP:false, OPM:1, op:true}`.
   **OPM 1 is correct; the prose is a copy-paste error.**

**Consequence for pdfcer:** `/Indexed` over `/DeviceCMYK` means colorants
must be read from the **base** space (§8.6.6.3). Reading them off `/Indexed`
yields none and gets **both PCS010 and PCS031** wrong.

**Image masks + OPM is a genuine ambiguity** — §8.6.7 excludes *"the
painting of images"* without carving out image masks, but a stencil paints
with the current non-stroking colour (§8.9.6.2), satisfying §8.6.7's own
test; PDF/A-4 phrases its restriction as covering *"image masks"*; Enfocus
documents masks as OPM-**sensitive**. **The suite ships no image mask, so it
has no evidence either way.** → **settings-shaped**; default OPM-sensitive
to match Acrobat/Enfocus.

---

## §5 — PCS011 CMYK Overprint Mode

**PRIMARY**, 27 Dec 2006, Jaeggi. Two columns `OPM 0 | OPM 1`.

**Caption:** `Rect (overpr) 0/0/10/50` · `Cross 90/10/90/0` ·
`Cross (overpr) 90/10/90/0` · `Cross 0/0/10/50` · `Rect 90/10/10/50` —
*"If an X appears the Overprint Mode (OPM) is not respected."*

**Cleanest OPM statement the suite publishes:** *"The Overprint Mode
specifies if a CMYK channel with 0% does overprint an other CMYK color
underneath (OPM = 1) or does knock out (OPM = 0)."*

Both rects and crosses carry a zero in the M or C channel — that is what
makes the two modes composite differently. **Expect only ONE X on failure**
(pdfcer currently reports exactly 1 trap).

---

## §6 — PCS020 Spot and CMYK Overprint

**PRIMARY**, 27 Nov 2005, **updated 15 Jun 2015**. Spot is "Suite Green".
Top row "cmyk over spot" (a–e), bottom "spot over cmyk" (f–j); columns
`font | vector | image | mask | shading`.

**Key contrast with PCS010:** here images and image masks **are** expected
to overprint, because the spot colorant is genuinely absent from the other
object's space (R1). PCS010's image cells must knock out because OPM cannot
reach images (R2). **Together they are a clean two-sided test of whether a
renderer keys overprint off the COLOUR SPACE rather than the OBJECT TYPE.**
This is the second patch pdfcer's spot-flattening fails — in both directions.

**Documented tolerance — PRIMARY:** *"A faint 'X' in slightly darker green
may show in **all** of the tests; this is acceptable behavior in this patch."*

---

## §7 — PCS041 White Overprint Mode

The only overprint patch where the suite publishes **both** the correct
appearance **and** a per-failure-mode diagnostic. **PRIMARY**, 08 Apr 2008,
Kleinheider. Two cells. **MEASURED:** only two ExtGStates, both **OPM 1** —
this patch does not vary OPM.

- **a)** white vector in `/Separation /Suite Green`, overprint on, over CMYK.
- **b)** CMYK *"almost white (0.2% in each process color channel except black)"*, overprint on.

**Expected, stated positively:** *"If a PDF/X conforming workflow performs
the rendering, the patch to show up as **a green and a gray rectangle**."*
**The two cells have OPPOSITE correct behaviour: a must vanish, b must knock out.**

**PRIMARY failure table, verbatim:**
- white X in **a** = *"Overprint was deactivated or not honored"*
- white X in **a** (alt) = *"The spot color object was converted to CMYK, Overprint stayed on, but **OPM was not set to 1**"*
- **red** X in **b** = *"Due to **rounding errors the 0.2% colorant are treated as 0%** leading to an overprinting white element"*
- white X in **b** = *"The overprinting got deactivated or not honored"*

**UNRESOLVED — the epsilon.** Caption says `.2/.2/.2/0` and the cell says
*"0.2%"*; the Notes paragraph says *"'nearly white' (o.o2% in this
instance)"*. The whole cell tests a renderer's **rounding threshold**, so an
implementer needs the real number. For calibration, Kodak Prinergy ships
*"White is considered white when black (K) is less than 0.9%"*.

**pdfcer currently passes PCS041.**

---

## §8 — PCS120 White Overprint / Knockout

**The bidirectional patch: half the cells are authored KNOCKOUT and must
stay knockout.** **PRIMARY**, 27 Dec 2006 rev 29 Aug 2013, Jaeggi.

Rows `Overprint` / `Knockout`; sub-rows `CMYK` / `Spot`; columns
`Vector | Text big (59 pt) | Text 6pt (8 pt)`.

**PRIMARY:** *"A lot of workflows and RIPs try to 'fix' white objects by
setting white always to knockout… When a workflow or RIP **changes** the
overprint behaviour of an element an X appears."*

PCS030/040 ask *"did you honour overprint?"*; **PCS120 asks "did you honour
the AUTHORED setting, whichever it was?"** A "white always knocks out" rule
passes the bottom half and fails the top; "white always overprints" does the
reverse. **MEASURED:** three ExtGStates, clean overprint/knockout binary at
fixed OPM 1.

**pdfcer currently passes PCS120.**

---

## §9 — PCS190 / 191 / 192 DeviceN Overprint (Black / Yellow / White)

Best-documented overprint patches in the suite. **PRIMARY**, 12 Dec 2012,
Kleinheider. All three share one layout: four cells, a/c vector and b/d
image, *"OP is true for all topmost elements."*

| Patch | a + b must render | c + d must render |
|---|---|---|
| **PCS190** Black | solid **Black (100C100K)** | solid **Cyan** |
| **PCS191** Yellow | solid **Green** | solid **Cyan** |
| **PCS192** White | solid **Red** | solid **White** |

**The discriminator, identical in all three:** the **a/b** pair uses a
DeviceN whose colorant list **omits** the backdrop's colorants, so overprint
leaves them standing; the **c/d** pair uses a DeviceN that **includes** those
colorants at 0%, so they are written as zero and knock out.
`100C` vs `100C0Y0K`; `100C` vs `100C0Y`; `0K` vs `0C0M0Y0K`.
**The colorant LIST — not the tint values — decides what survives (R4).**
A renderer that flattens DeviceN to DeviceCMYK before compositing collapses
the distinction and **fails cell c specifically.**

**Shared per-cell diagnostics — PRIMARY, identical across all three:**
*"A cross in patch **c** indicates a colour conversion to DeviceCMYK prior
to rendering. The cross appears since OPM is set to 1. **However, if the
system performs colour conversion and sets the OPM for this patch c to 0,
the rendering is also fine.**"* — **cell c has TWO sanctioned correct
outcomes; do not treat it as binary.** *"A **faint** cross in patch **d**
indicates a colour conversion using inadequate ICC profiles or method"* —
faint = tolerated.

**MEASURED (PCS192):** spaces are
`[/DeviceN [/Cyan /Magenta /Yellow /Black /None] /DeviceCMYK …]` and
`[/DeviceN [/Black /None] /DeviceCMYK …]`. **The `/None` colorant is present
in both** — must mark nothing (§8.6.6.4) and **must not join the overprint
colorant set.** *(This is exactly what Ghostscript bug 709099 gets wrong —
see `docs/overprint-architecture-survey.md` §5.)*

**Acrobat-verified ground truth for PCS192 cell d** — Poppler issue #1410
(⚠️ GPL project; tracker **prose only**), closed FIXED: *"The expected
result is to have a completely white square for d. I checked with Adobe
Acrobat and it renders a white square indeed."*

---

## §10 — Harness caveats

- The test pages carry an embedded **Preflight Audit Trail** and a preflight
  signature that **invalidates on any modification** — a useful tamper check,
  but it trips if tooling rewrites the file.
- A strict pixel-diff produces **false failures** on exactly the cells the
  suite pre-declares tolerant (§0).

---

## §11 — PCS160 / PCS1_161 / PCS1_162 Transparency Basic Blend Modes (DeviceCMYK)

**PRIMARY (suite ReadMe, "Patch 16.0 – 16.2", © 2012).** Three patches, one
axis each, over the same 16-cell blend-mode grid:

| Patch | Variant |
|---|---|
| `PCS160` | *"without applying 'Knockout' or 'Isolate'"* |
| `PCS1_161` | *"with the use of the 'knockout' effect"* |
| `PCS1_162` | *"with the use of the 'Isolate' effect"* |

**Evaluation is Method 1 (trap X), and it is STRICT — this is the important
sentence and it removes a tolerance a reader might assume by analogy with the
overprint patches:**

> *"It is possible that a faint 'X' may appear, e.g. in case of 16.2 or 16.3.
> **A 100% correct rendering does expect a 100% 'X' free output though.**"*

⇒ Unlike `PCS020` (§6) and the DeviceN cell d (§9), **no X here is
pre-forgiven**. Every trap is a real failure.

**One genuine exclusion, and the harness already honours it structurally:**

> *"Only the **fill colour** of a patch element should be evaluated. Because of
> anti aliasing, it is possible to see a very thin stroke line at the edges of
> an 'X'. This does per definition **not** indicate a problem. A distinguished
> coloured **fill** colour does though."*

`suite-check.py`'s shape test requires `fill` between 0.15 and 0.60 **and**
mass on both diagonals **and** mass at the crossing centre — an anti-aliased
outline is hollow and fails all three. So the exclusion is satisfied by
construction rather than by a threshold, which is the stronger way to satisfy
it.

### MEASURED cell layout (2026-08-18, `tools/suite-cell-probe.py`)

16 cells in two rows of 8. Row 2's labels, read from the patch's own content
stream in order: `Hard Light | Difference | Exclusion | Hue | Saturation |
Color | Luminosity | Opacity (0%)`. Cells are `22.678 pt` squares on a
`31.68 pt` pitch; at the harness's default `--scale 2.0` that is a `62.5 px`
pitch, and row 2 sits at `y ≈ 106 px`.

The `/ExtGState` mapping is recoverable the same way — the object stream
carries them in a fixed order (`ColorBurn, Multiply, Darken, Lighten, Screen,
ColorDodge, Overlay, SoftLight, HardLight, Hue, Color, Luminosity, Saturation,
{CA 0 ca 0}, Difference, Exclusion`), so a `/GSnn gs` immediately before a
`/Xnn Do` names the cell's mode.

### ★ MEASURED diagnosis of pdfcer's failures

| Patch | traps | what the probe shows |
|---|---|---|
| `PCS160` | **3** of 16 | `Hue`, `Saturation`, `Color` — and **`Luminosity` passes** |
| `PCS1_161` | **14** | X emerges as the raw source primary in nearly every cell |
| `PCS1_162` | **7** | same signature, fewer cells |

**★ `PCS160` is NOT §11.3.5.3 — corrected 2026-08-18, same day.** This
paragraph originally said those three were "exactly the nonseparable modes
whose K component is taken from the backdrop" and called `Luminosity` passing
"the one-bit confirmation". Measured: every transparency patch reports
`blend_modes_applied=11, blend_modes_ignored=4`, and pdfcer **declines all four
nonseparable modes outright**, compositing them as `Normal`. They fail because
they are **not implemented**, not because they are computed with the wrong K.
`Luminosity` is declined identically and still comes out clean — so "declined"
does not imply "visibly wrong", and that clean cell is **not** evidence
`Luminosity` works.

§11.3.5.3 is still what implementing them correctly requires. It was simply
not the explanation for the failures. Cell identities are now resolved by
`tools/suite-cellmap.py` (CTM walk + `/Matrix`/`/BBox` into device space
against the governing `/ExtGState`) rather than by cell-pitch arithmetic.

**`PCS1_161`/`PCS1_162` are §11.4.7, not knockout.** A `tiny_skia::Pixmap`
starts transparent, and a transparent initial backdrop **is** isolated
semantics. pdfcer allocates a group buffer whenever the outer graphics state is
non-neutral — and every cell in these patches sets `/BM` at the `Do`. So a
**non-isolated group silently becomes an isolated one** and every interior
blend composites against nothing, which returns `cs` unchanged. That is
precisely the saturated primary the probe reads inside the X.

Full write-up and the staged fix: `docs/compositor-plan.md` §1.2, `Pass 97.0`.

---

## §12 — PCS3_161 / PCS3_164 Transparency Basic Blend Modes (ICCBased)

**PRIMARY (suite ReadMe, "Patch 16.1, 16.4", suite version 5.0).** Same
blend-mode grid, ICCBased objects. The ReadMe documents the **object stack**,
which §11's DeviceCMYK ReadMe never does and which matters for choosing the
blending colour space:

```
Upper object       ICCBased, with the transparency effect (Fill)
Lower object       ICCBased
Background object  DeviceCMYK        <-- note: CMYK even in the "ICCBased" patch
```

⇒ The backdrop under an ICCBased blend here is **DeviceCMYK**. A group
blending in its own colour space (§11.3.4) and a page compositing in sRGB are
two different spaces in the same cell, which is exactly the seam `Pass 97.0`
and `97.1` divide between them.

**PRIMARY tolerance, and it is real but narrow:**

> *"A **faint** X is due to differences in the **CMM** and does not indicate a
> failure."* … *"A **clearly visible** X indicates that this blend mode is not
> supported properly which **is** a failure."*

**MEASURED, 2026-08-18 — the tolerance does not rescue pdfcer.** `PCS3_161`
shows X `[255,0,255]` against surround `[129,45,156]` (~126 levels) and
`PCS3_164` shows X `[165,165,165]` against surround `[19,19,19]` (~146
levels). Neither is a CMM difference; both are "clearly visible" by the
suite's own wording. Consistent with §11's finding and with §0's measured
result that no contrast recalibration moves any verdict.

`PCS3_164` fails **4** cells and is the ICCBased-CMYK twin of `PCS160`'s
nonseparable-mode defect — §11.3.5.3 names *"both `DeviceCMYK` and `ICCBased`
calibrated CMYK spaces"* explicitly. `PCS3_161` fails **15** and is the
isolated-group defect.

---

## §13 — PCS166 / PCS168 / PCS169 / PCS1610 / PCS1611 Soft Masks

**PRIMARY (suite ReadMe, "Patch 16.6, 16.8, 16.9, 16.10, 16.11", © 2012).**

| Patch | Mask kind |
|---|---|
| `16.6` (`PCS166`) | **Image** soft masks — *"a Layer Mask (and transparent gradient or feather effect)"* |
| `16.8` / `16.9` (`PCS168`/`PCS169`) | **Vector** soft masks — *"Drop Shadows, Outer Glow, Inner Glow"* |
| `16.10` / `16.11` (`PCS1610`/`PCS1611`) | The same effects applied to **Text** objects |

**Evaluation is Method 2, NOT a trap X:** *"a visual comparison to a reference
**within the patch**."* This is why `suite-check.py` scores these by strip
correlation rather than adjudicating them, and why `PCS166` and `PCS3_167`
land in the UNRESOLVED bucket rather than the FAIL one.

**PRIMARY failure signatures**, worth having because a correlation number
alone does not say what went wrong:

> *"A clear black stroked fill is visible at the edge of the image instead of a
> smoothly softened edge."*
> *"**Disappearance** of the Image or Vector Soft Masks is one way of how the
> incorrect rendering can occur. A clearly **different colour** rendering can
> also be seen as a way of how the patch should not be rendered."*
> *"In some cases **all** patch elements will be rendered wrong. There are
> occasions where only just **one or two** patches are rendered differently.
> Caution may need to be taken while evaluating the rendered result."*

⇒ A partial failure is expected and normal here; do not read "most of it looks
right" as passing.

**MEASURED (2026-08-18).** Construction is correct — both the mask groups and
the folded clips were dumped to PNG and inspected as properly-placed soft
gradients. The defect is **application**: §11.4.5 applies the mask to a
transparency group's **RESULT**, and pdfcer folds it into the clip, which
applies it to each element *inside*. Correlations after the `cb20770` soft-mask
work, against the reference engine's own score on the same strip:

| Patch | pdfcer | reference engine |
|---|---|---|
| `PCS1_1610` Text part 1 | 0.575 | 0.966 |
| `PCS1_168` Vector part 1 | 0.725 | 0.981 |
| `PCS1_169` Vector part 2 | 0.905 | 0.983 |

Fix is `Pass 97.0`: there is nowhere to apply a mask to a group result until
the group result is a value pdfcer owns.
