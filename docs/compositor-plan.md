# The colorant compositor — plan of record

**Written 2026-08-18**, engineer-owned, at `2a75be1`+`e618d67`.
Companion to `docs/overprint-architecture-survey.md` (the sourcing record for
the colorant half) and `docs/suite-patch-reference.md` (the per-patch expected
appearance for the overprint patches).

> ## ★★★ AMENDMENT 2026-08-21 — STAGE A SHIPPED, AND IT CANNOT DELIVER ITS
> ## SEVEN PATCHES. THE TRANSPARENCY PANELS ARE BLOCKED ON §11.3.4, NOT ON
> ## THE GROUP MODEL. Read this before scoping `Pass 97.1`.
>
> `Pass 97.0` shipped: `crates/pdfcer-render/src/compositor.rs` (§11.4.4's
> element formula, §11.4.8's knockout variant, §11.4.4's backdrop removal,
> Table 136's thirteen separable blend functions), non-isolated groups
> rendered over their own backdrop, and a real §11.4.6 knockout
> implementation. **Board before and after: `26 pass · 14 FAIL · 11
> UNRESOLVED` — unchanged.** Trap count across the failing patches went
> **67 → 55**.
>
> ### What moved, measured
>
> | patch | traps before | after | why |
> |---|---:|---:|---|
> | `PCS1_161` (non-isolated **knockout**, DeviceCMYK) | 14 | **2** | §11.4.6 implemented |
> | everything else | — | unchanged | — |
>
> `PCS1_161` is the headline and it is the one this plan predicted least
> confidently: twelve of sixteen cells now render correctly, and the two
> that remain sit with the residue below. **No patch regressed**, and
> `PCS2_120` — the other `/K`-named patch — still passes.
>
> ### ★★ THE FINDING THAT CHANGES THE STAGING, and it is a derivation, not
> ### a hunch
>
> §4's Stage A expects seven patches: `PCS1_161`, `PCS3_161`, `PCS1_162`
> and the four soft-mask patches. **It will not get them, and the reason is
> not that the group model is wrong.** Worked by hand on `PCS1_162`'s
> `Difference` cell, whose two operands are printed in the file:
>
> ```text
> X1  = DeviceCMYK 0 1 0 0   (magenta)      the cell's first element
> X2  = DeviceCMYK 0 0 0 1   (black)        the second, /BM /Difference
> surround (what a correct engine must produce) = RGB (0, 165, 79)
> ```
>
> §11.3.4 requires a subtractive space's components to be **complemented
> before the blend function and complemented back after**:
>
> ```text
> cb' = (1,0,1,1)   cs' = (1,1,1,0)
> |cb' − cs'|       = (0,1,0,1)
> complement back   = (1,0,1,0)  =  DeviceCMYK 1 0 1 0  =  GREEN
> ```
>
> **That is the surround, exactly.** pdfcer renders `(237, 1, 140)`; pdfium
> renders `(202, 29, 108)`; both blend in RGB and both are wrong, in
> different places. The trap is authored on the *blending colour space*,
> and no amount of group-model correctness reaches it.
>
> ⇒ **`Pass 97.1` is the unlock for the transparency panels, not only for
> overprint**, and its first deliverable is §11.3.4's complement rather
> than the spot planes. The plan already lists the complement under Stage B
> (`blend_subtractive(cb, cs) = 1 − B(1 − cb, 1 − cs)`) — what changes is
> that it is now the **leading** item and it is what four of Stage A's
> seven expected patches were actually waiting for.
>
> Note the shape: **every suite transparency patch declares
> `/Group /CS /DeviceCMYK` on the PAGE** (`PCS3_161` included, whose own
> objects are `ICCBased` RGB). So the blending space is CMYK for all of
> them regardless of what the artwork is coloured in, and pdfcer blends
> every one of them in device sRGB.
>
> ### A defect this Pass fixed, and the mechanism is worth carrying
>
> `blend_nonsep::composite` and `blend_nonsep::composite_layer` both
> substituted a **white** backdrop wherever the destination was
> transparent, then lerped from it. That is §11.4.4's formula specialised
> to `α_b = 1`, and it is wrong precisely where §11.4.7 makes transparency
> common — the page buffer starts transparent and the white medium is
> composited in once at the end, so "transparent" means *no backdrop*, not
> *paper*.
>
> `Sat(white) = 0` and `Lum(white) = 1`, so `Hue`/`Saturation`/`Color` of
> **anything** over white is white. Measured on `PCS1_162` at scale 2.0:
>
> | cell | pdfcer before | pdfcer after | pdfium |
> |---|---|---|---|
> | `Hue` | `(255,255,255)` | `(184,184,184)` | `(184,184,184)` |
> | `Saturation` | `(255,255,255)` | `(184,184,184)` | `(184,184,184)` |
> | `Color` | `(255,255,255)` | `(184,184,184)` | `(184,184,184)` |
> | `Luminosity` | `(106,106,106)` | `(255,20,159)` | `(255,22,158)` |
>
> Three cells exact against an independent renderer, the fourth within two
> levels. **The trap still fires on all four** — that is the §11.3.4 gap
> above, and it is why "matches pdfium" and "passes the suite" are
> different claims and both had to be measured.
>
> `overprint::composite` carries the **same white-backdrop convention** and
> was left alone: it is Table 149 logic, not a blend function, and changing
> it belongs with the colorant buffer that will replace its input. Recorded
> here so it is not read as an oversight.
>
> ### What Stage A actually bought
>
> 1. **pdfcer owns the compositing arithmetic.** `compositor.rs` is the
>    single place §11.4.4/§11.4.8/Table 136 live, so the three call sites
>    that need them cannot drift.
> 2. **Knockout is real** (`KnockoutTarget`), including §11.4.6 NOTE 6's
>    nesting rule — a non-isolated group inside a knockout group inherits
>    the *outer* group's initial backdrop, not the immediate one.
> 3. **Non-isolated groups see their backdrop**, via a second content walk
>    taken only when the group's interior actually blended (§11.4.4
>    NOTE 5 makes the single walk exact otherwise). Disclosed as
>    `groups_backdrop_reruns`.
> 4. `transparency_groups_knockout_approximated` **changed meaning** —
>    from "this group is an approximation" to "these elements inside it
>    were", which is zero for a knockout group pdfcer rendered exactly.
> 5. **The soft mask reaches the group's result** (§11.4.5), and
>    `outer_is_neutral` learned about soft masks — the third instance of
>    a graphics-state field being absent from the "is the state still
>    default?" predicate, which the comment above that predicate had
>    already named twice in past tense.
> 6. **Full-corpus render parity is unchanged**, 4,023 files, measured
>    twice — this branch and a worktree at `2e6bb83` — identical bucket
>    for bucket, band for band, same single unexplained file. That is the
>    intended result and it is why the *isolated* group composite still
>    goes through `tiny_skia::draw_pixmap`: pdfcer's `f32` path computes
>    the same function with different rounding, so routing the already-
>    correct case through it would move every anti-aliased edge in the
>    corpus and turn the parity gate into a rounding detector.
>
>    ⚠ **`tools/render-parity/out/summary.json` is STALE and not
>    comparable to a current run** — it records a bucket vocabulary the
>    harness no longer emits (`benign` / `known-gap` against `below-band`
>    / `disclosed-gap-small`). Comparing two current runs is the only
>    thing that means anything until it is re-based.
>
> ### Still owed from Stage A, and named rather than left implicit
>
> * ~~**Soft mask on the group RESULT** (§11.4.5) is NOT done.~~
>   **SHIPPED as `Pass 97.0d`, same session**, and it was indeed the one
>   Stage A item with no §11.3.4 dependency. Reference-strip correlation:
>
>   | patch | before | after |
>   |---|---:|---:|
>   | `PCS1_1610` Softmasks Text part1 | 0.576 | **0.962** |
>   | `PCS1_168` Softmasks Vector part1 | 0.725 | **0.978** |
>   | `PCS1_169` Softmasks Vector part2 | 0.905 | **0.986** |
>
>   `PCS1_1611` still traps once and `PCS166` / `PCS3_167` still report
>   `corr=None` (their strips could not be located at all, which is a
>   harness limit rather than a render result).
>
>   ★ **AND IT LEAVES AN INSTRUMENT PROBLEM, named rather than solved.**
>   Those three stay in the UNRESOLVED bucket because `suite-check.py` has
>   **no calibrated threshold** for reference-strip patches — its own
>   output says so. There is now a visible bimodal split to calibrate
>   against (`0.96`–`0.99` for the three above against `0.04`–`0.41` for
>   the 16-bit patches), so the calibration is finally *possible*.
>   **Deliberately not done in the same change that moved the numbers**:
>   calibrating the instrument immediately after making it report what you
>   wanted is not a measurement. It needs its own session, its own
>   justification, and preferably a patch whose expected verdict is known
>   independently — which is exactly how `suite-check`'s trap threshold was
>   calibrated in the first place (PCS 16.0, 2026-08-17).
> * **`f_g` is approximated by `α_g`** for a group used as an element of a
>   knockout group — exact whenever that group's own elements are opaque,
>   which is §11.4 corpus §7.4's stated safe-skip condition.
> * **Elements that read the destination back** (shadings, overprint,
>   per-paint non-separable blends) composite with non-knockout semantics
>   inside a knockout group, counted on
>   `transparency_groups_knockout_approximated`.
> * ~~**`PCS3_161` is unexplained**~~ **— EXPLAINED 2026-08-21, by a counter
>   rather than by an argument.** `blends_in_wrong_space` reports **15 of its
>   15** blend modes computed additively across **18 subtractive groups**. The
>   mechanism is Table 147's `/CS` row: its own objects are `ICCBased` RGB, but
>   its cell groups are **non-isolated**, so they **inherit** the page group's
>   `DeviceCMYK` and every blend on the patch is a §11.3.4 blend performed
>   additively. The §11.3.4 hypothesis below was recorded as *fitting but
>   unconfirmed*; it is confirmed, and the operand-pairing lead it recommends
>   is **not** needed.
>
>   The original reasoning is kept because the two eliminations in it are still
>   sound and still useful — they are what made the remaining hypothesis worth
>   testing:
>
> * **`PCS3_161` was unexplained**, and two cheap explanations had been
>   RULED OUT rather than left hanging. 14 traps, no knockout groups, no
>   backdrop reruns triggered (correctly — its groups' interiors are all
>   `Normal`), and its blend modes sit at the `Do` where `draw_pixmap`
>   already handles them. The §11.3.4 hypothesis fits but was **not**
>   confirmed by derivation the way `PCS1_162`'s was.
>
>   **Ruled out (1) — the suite's own CMM tolerance.** The patch's ReadMe
>   says in terms:
>   *"A faint X is due to differences in the CMM and does not indicate a
>   failure. A clearly visible X indicates that this blend mode is not
>   supported properly which is a failure."* pdfcer **has** a different CMM
>   — its `DeviceCMYK` → sRGB is the naive additive conversion, and the
>   parity harness measures `DeviceCMYK`-only pages diverging at 5.4× the
>   clean-page mean against pdfium's `AdobeCMYK_to_sRGB1` — so this was the
>   obvious candidate. **Measured and rejected:** the X-versus-surround
>   worst-channel gap across the fourteen trapping cells is
>   **39, 47, 68, 69, 77, 88, 90, 90, 101, 121, 126, 171, 176, 178**. One
>   cell is arguably faint; thirteen are not. Same instrument and same
>   argument as §7 item 1, which measured the overprint patches and reached
>   the same verdict there.
>
>   **Ruled out (2) — "the blend is not being applied at all."** pdfium's
>   value sits **between** pdfcer's and the surround on essentially every
>   cell (e.g. `(255,0,255)` pdfcer, `(34,13,226)` pdfium, `(129,45,156)`
>   surround). A blend that never ran returns `C_s` unchanged and would put
>   pdfcer **at** the source, not past pdfium in the same direction. The
>   signature is pdfcer applying the blend **harder** than it should — which
>   is what a wrong operand, not a missing operation, looks like.
>
>   ⇒ The remaining candidates, in the order they are worth testing:
>   the blending colour space (§11.3.4, as for `PCS1_162`); the `ICCBased`
>   → alternate fallback changing what `0 1 1 scn` means; or the patch's
>   three-layer structure — its ReadMe describes **"Upper object ICCbased
>   with transparency effect (Fill), Lower object ICCbased, Background
>   object DeviceCMYK"**, and pdfcer's reading of which form XObject is
>   which was **not** established. Establish that first: a wrong operand is
>   what the evidence points at, and the operand pairing is the thing
>   nobody has checked.

> ## ★★ AMENDMENT 2026-08-19 — THE DENOMINATOR MOVED AND ONE PROBED PATCH
> ## NOW PASSES. Re-derive the thesis before scoping `Pass 97.x` from it.
>
> This plan is a **live plan**, not a dated record, and `Pass 97.x` is scoped
> from it — so its arithmetic being stale is a live problem rather than a
> historical footnote.
>
> **The standing is now `26 pass · 14 FAIL · 11 UNRESOLVED`**, not the
> `25 · 18 · 8` in §1 below. Two independent things moved it:
>
> 1. **A classifier shift that predates this session.** `18+8 = 15+11 = 26`
>    and `pass` was 25 either way, so **three patches crossed the
>    FAIL/UNRESOLVED line without any patch changing outcome.** Found by
>    building the previous commit in a worktree rather than quoting the board.
>    Filed as a reading, not a verified cause.
> 2. **`PCS1_160` genuinely passes now** — the non-separable blend modes
>    (`Pass 85.4b`, `972ddbb`) shipped, and it was the patch they flipped.
>
> ★ **Point 2 is the one that touches this document's argument, not just its
> numbers.** `PCS1_160` is one of the five transparency-group failures §1.1
> probes cell by cell, and it was resolved by **Table 137 arithmetic, not by
> a compositor** — with no CMYK buffer, no non-transparent initial backdrop
> and no `Pass 97.0`. So the claim that these failures "do not decompose"
> has one counter-example, and **"16 of the 18" cannot be re-stated as
> "16 of the 14"**.
>
> **What that does NOT mean:** the compositor case is not refuted. Overprint
> still needs per-colorant planes, and the measured negative result behind
> that (`ac15158` — a spot-ink multiplier plate built, ablated and reverted)
> stands untouched.
>
> **What it does mean:** the share is unknown until someone re-derives it,
> and this plan should not be quoted for a figure until they have. The
> current fourteen, measured today:
>
> ```text
> overprint / colorant   PCS1_011  PCS1_190  PCS1_191  PCS1_192
>                        PCS2_020  PCS2_030  PCS2_040
> transparency groups    PCS1_161  PCS1_162  PCS3_161  PCS3_164
> soft masks             PCS1_1611
> shading                PCS1_060
> ICC                    PCS3_130
> ```
>
> §1's baseline block and §1.1's `PCS1_160` probe are **left as written** —
> they are the measurement this plan was built on and rewriting them would
> destroy the record of what was true at `e618d67`. Read them as dated.

This document exists to answer one question with evidence rather than
intuition: **what single build clears the largest share of the remaining
suite failures, and why is it one build rather than five?**

The answer given here, **at the time of writing and now owed a re-derivation
(see the amendment above)**, is that **16 of the then-18** failures are
downstream of the same missing thing — pdfcer has no compositor of its own. It delegates every
per-pixel blend to `tiny_skia`, which composites **8-bit premultiplied sRGB**
with **Porter-Duff over a transparent-initialised buffer**. ISO 32000-1
clause 11 requires compositing **in the group's colour space**, over a
**non-transparent initial backdrop**, with a **backdrop-removal correction**
on the way out, and — for overprint — **per-colorant planes that sRGB cannot
represent**. Every one of those four is a property of the buffer, not of the
call site. That is why the fixes do not decompose.

---

## §1 — The measurement this plan is built on

Baseline re-measured 2026-08-18 at `e618d67`:

```
25 pass · 18 FAIL · 8 UNRESOLVED (reference-strip) · 0 render errors  of 51
```

The five **transparency-group** failures were probed cell by cell with a new
diagnostic (`tools/suite-cell-probe.py`, §7 below). For each trap X it reports
three numbers: the colour pdfcer painted **inside** the X, the colour pdfcer
painted in the **surround**, and the colour **Acrobat** painted at the same
place. That triple is decisive, because the suite's trap X is drawn so that a
*correct* engine renders it **the same colour as its surround** — so a
disagreement localises to one of the two, not to "the cell".

### 1.1 What the probe found — `PCS1_160` (DeviceCMYK, non-isolated, non-knockout)

Only **3 of 16** cells fail: the ones governed by **`Hue`**, **`Saturation`**
and **`Color`**, at device x = 204, 266 and 329 on the y = 106 row. Cell
identities resolved by `tools/suite-cellmap.py` from each form XObject's
`/Matrix` and `/BBox` against its governing `/ExtGState`.

**★ CORRECTED 2026-08-18, later the same day.** This section originally read
that those three are *"exactly the nonseparable blend modes whose K component
is taken from the BACKDROP"*, that `Luminosity` passing was *"a one-bit
discriminator falling on the correct side"*, and therefore that §11.3.5.3's
K-selection rule was the cause. **That inference was wrong, and it was wrong
in the most persuasive direction — it fit.**

What the counters said at the time, measured with `pdfcer render-page` on
all four transparency patches: **`blend_modes_applied=11,
blend_modes_ignored=4`** in every one. `blend_mode_from_name` returned `None`
for all four nonseparable modes, so pdfcer **declined them outright** and
composited them as `Normal`. They never reached the applied path at all, so
nothing about them could be evidence for *how* the applied path computes.
Caught by the librarian while filing, from the code rather than from the
pixels.

> **★ PAST TENSE AS OF 2026-08-19, and the tense is the point.** The four
> nonseparable modes ship (`Pass 85.4b`, `972ddbb`) — pdfcer computes Table 137
> itself, they no longer touch `blend_modes_ignored`, and `PCS1_160` passes.
> The paragraph above is a **dated measurement**, still correct about
> `e618d67` and still load-bearing for the reasoning that follows it.
>
> Restated here rather than only in the head amendment because **a reader
> arriving by grep lands in the body, not at the top.** A correction that only
> exists in a document's preamble is a correction the person who most needs it
> will not see.

The corrected reading, and the cell identities are now **resolved** rather
than inferred — `tools/suite-cellmap.py` walks the content stream, tracks the
CTM through `q`/`Q`/`cm`, and maps each form XObject's `/Matrix` and `/BBox`
into device space against its governing `/ExtGState`:

| patch | `Hue` | `Saturation` | `Color` | `Luminosity` | applied modes |
|---|---|---|---|---|---|
| `PCS1_160` | trap | trap | trap | **clean** | all 11 clean |
| `PCS3_164` | trap | trap | trap | **clean** | **`Difference` traps** |

**Three separate facts fall out, and the first two are different bugs:**

1. **`Hue`/`Saturation`/`Color` fail because they are not implemented** —
   declined, not miscomputed. `Luminosity` is declined *identically* and still
   comes out clean, which means "declined" does not imply "visibly wrong": on
   this artwork its correct result and its `Normal` stand-in coincide closely
   enough to stay under the suite's clear-X threshold. **Do not read that
   clean cell as evidence `Luminosity` works.**
2. **`PCS3_164`'s `Difference` cell is the real §11.3.4 evidence** — an
   *applied*, separable mode, failing on ICCBased CMYK. `Difference` is
   `|cb − cs|`, the mode most sensitive to whether its operands were
   complemented first, so it is where a wrong blending space surfaces
   soonest. One cell, not a cluster — reported as such.
3. §11.3.5.3's K-selection rule is still **required to implement** the
   nonseparable modes correctly. It is just not the explanation for today's
   failures. The clause was right; the attribution was not.

**The methodological lesson, since it cost a wrong entry in three documents
and a commit message:** the original mapping came from cell-pitch arithmetic
(`22.678 pt` squares on a `31.68 pt` pitch at scale 2.0) — one method, no
cross-check — and it produced a story so clean it discouraged looking further.
The pitch arithmetic turned out to be *right about positions* and the
*causal attribution built on it* was wrong, which is the combination that
survives a sanity check.

**The clause that Stage B still owes**, quoted so the implementation has it —
this is what *implementing* the nonseparable modes requires, not a diagnosis
of why they fail today. `iso32000__s__11.3.5.md` §4.8, verbatim:

> "The formulas in this sub-clause apply to **RGB** spaces. Blending in
> **CMYK** spaces (including both `DeviceCMYK` and `ICCBased` calibrated CMYK
> spaces) **shall** be handled in the following way: the C, M and Y components
> **shall** be converted to their complementary R, G and B components in the
> usual way; the preceding formulas **shall** be applied to the RGB colour
> values; the results **shall** be converted back to C, M and Y. For the **K**
> component, the result **shall** be the K component of **Cb** for the `Hue`,
> `Saturation` and `Color` blend modes; it **shall** be the K component of
> **Cs** for the `Luminosity` blend mode."

⇒ **K is not blended, it is selected**, and the selection differs by mode. The
same RAG file anticipated the gap in writing: *"pdfcer currently composites in
device RGB. If/when a CMYK path lands, this clause is the whole rule."*

`PCS3_164` (ICCBased **CMYK**) fails **4** cells: the same three declined
nonseparable modes, **plus `Difference`** — and that fourth cell is the only
direct evidence in the corpus that an *applied* mode is computed in the wrong
space.

### 1.2 What the probe found — `PCS1_161` / `PCS3_161` / `PCS1_162` (14 / 15 / 7 cells)

Different shape entirely. In almost every failing cell, **pdfcer's surround
agrees with Acrobat** (within a few levels) while **pdfcer's X is a saturated
primary** — `[237, 1, 140]`, `[255, 0, 255]`, `[0, 0, 255]`. A saturated
primary is what a blend mode produces when it is applied against **nothing**:
`B(cb, cs)` composited over a transparent backdrop returns `cs` unchanged.

The cause is in `crates/pdfcer-render/src/interpret.rs` and is already
documented honestly in its own comment:

```rust
let outer_is_neutral = self.gs.current.blend_mode == tiny_skia::BlendMode::SourceOver
    && self.gs.current.fill_alpha >= 1.0;
let needs_buffer =
    is_transparency_group && (!outer_is_neutral || group_flag(b"I") || is_knockout);
```

A `tiny_skia::Pixmap` starts **transparent**, and transparent-initialised is
**isolated** semantics (§11.4.7). So whenever the outer graphics state is not
neutral, pdfcer allocates a buffer — and in doing so **silently converts a
NON-isolated group into an isolated one**. The comment says so: *"Buffering
unconditionally gets those wrong in the opposite direction from flattening."*

The suite transparency patches set `/BM` on the graphics state **at the `Do`**,
which makes `outer_is_neutral` false for every cell. So every cell takes the
buffered path, every cell loses the page backdrop, and every interior blend
degenerates to "paint the source". **14 of 16, 15 of 16, 7 of 16** — the
counts are what "essentially all of them" looks like once the handful of cells
whose correct answer *is* the source colour are removed.

This is not fixable by choosing the other branch. Painting inline gets
non-isolated groups right only while the outer state is neutral; a real
non-isolated group needs its buffer **initialised from the backdrop** and then
needs §11.4.4's **backdrop removal** applied to the result, or the backdrop
contributes twice.

### 1.3 Soft masks — same buffer, different clause

Diagnosed last session and unchanged: `/Alpha` and `/Luminosity` masks are
**constructed correctly** (both were dumped to PNG and inspected — correct,
properly placed soft gradients), but they are **applied by folding into the
clip**, which applies them to each element *inside* the group. §11.4.5 applies
the mask to the group's **RESULT**. Four patches (`PCS1610`, `PCS1611`,
`PCS168`, `PCS169`).

There is nowhere to apply a mask to a group result until the group result is a
thing pdfcer owns. Same build.

### 1.4 Overprint — the colorant planes

Established three independent ways last session and written up in
`docs/overprint-architecture-survey.md`: by measurement (a spot-ink multiplier
plate was built, ablated, and **reverted** — `ac15158`), by research (seven
engines converge on one architecture; Artifex's colour architect states in a
peer-reviewed paper that collapsing colour before compositing *"is not
possible"* **specifically because of overprint**), and by pdfcer's own spec RAG,
which called it *"architectural, a different project"* on 2026-08-08.

Seven patches: `PCS011`, `PCS190`, `PCS191`, `PCS192`, `PCS020`, `PCS030`,
`PCS040`.

#### ★★ AMENDMENT 2026-08-19 — a FOURTH justification, and it is the direct one

The three above are all *indirect*: an ablation, seven engines agreeing, and
a RAG judgement. They argue that colorant planes are the right architecture.
None of them measures **what the current path actually does wrong**, and that
gap mattered — the operator asked today whether targeting CMYK was practical
at all, which is a question the indirect arguments answer weakly.

Measured with `crates/pdfcer-render/examples/overprint_roundtrip_probe.rs`
(new, re-runnable). `overprint.rs` implements Table 149 **completely and
correctly** — the decision logic is not the gap. The gap is its input:
`interpret.rs:3574` calls `overprint::rgb_to_cmyk` to reconstruct the source
ink set out of the RGB framebuffer, because there is nowhere else to get it.

```text
  painted CMYK  ->  RGB (the framebuffer)  ->  reconstructed CMYK  ->  Table 149
```

That middle arrow is the conversion ISO 32000-1 §8.6.5.7 NOTE 2 names by
hand: a 4→3→4 round trip is *"unnecessary and results in a loss of fidelity
in the black component."* (Citation carried from `iccce`'s
`note_gray_black_routing_is_yours.md`, which offers it as the standard's own
warrant against round-tripping.)

**The result is worse than a fidelity loss, because Table 149 is a
per-component ZERO/NONZERO test.** Its rules select the source component for
any component whose value is nonzero and the backdrop otherwise — so an error
in the reconstruction is not a slightly-wrong colour, it is **a different
branch of the rule**.

| ink set | painted | reconstructed | rule input |
|---|---|---|---|
| pure K line art | 0/0/0/1.00 | 0/0/0/1.00 | same |
| **registration black** | **1.00/1.00/1.00/1.00** | **0/0/0/1.00** | ★ CHANGED |
| rich black | 0.60/0.40/0.40/1.00 | 1.00/0/0.07/1.00 | ★ CHANGED |
| 75 % K (PCS 23.0) | 0/0/0/0.75 | 0/0/0/0.75 | same |
| cyan solid | 1.00/0/0/0 | 1.00/0.27/0/0.06 | ★ CHANGED |
| magenta solid | 0/1.00/0/0 | 0/0.99/0.41/0.07 | ★ CHANGED |
| cyan + magenta | 1.00/1.00/0/0 | 0.68/0.67/0/0.43 | ★ CHANGED |
| 50 % cyan alone | 0.50/0/0/0 | 0.58/0.16/0/0.04 | ★ CHANGED |
| light warm grey | 0.05/0.04/0.06/0.10 | 0/0/0.01/0.14 | ★ CHANGED |
| paper white | 0/0/0/0 | 0/0/0/0 | same |

**7 of 10 realistic ink sets make Table 149 read a different row.** Worst
single-component error **1.000**.

★ **Registration black is the headline and is not a corner case.** It is
`1/1/1/1` — every plate, which is the whole point of it — and it reconstructs
as `0/0/0/1`, pure K. It is what crop marks, registration targets and trim
furniture are printed in, precisely *because* it must appear on every
separation. The current path decides its overprint behaviour as though three
of its four inks were absent.

**And four pairs collapse to the same rule input entirely:**

- pure K ↔ registration black
- registration black ↔ 75 % K
- cyan solid ↔ cyan + magenta
- cyan + magenta ↔ 50 % cyan

For those, overprint cannot distinguish the two inputs *at all* — no amount
of correctness in Table 149 can recover a distinction the buffer already
destroyed.

**What this changes about the plan:** nothing structural — it corroborates
§3.3 rather than revising it. What it changes is the *strength of the case*.
Colorant planes are not an accuracy improvement over a working overprint
implementation; the overprint implementation is **operating on the wrong ink
set most of the time**, and its own correctness is currently unobservable.

**A caution for whoever measures Stage B.** Because 7 of 10 branch
differently, some suite overprint patches may currently pass *by accident* —
a wrong branch that happens to produce the expected pixels. Stage B should
therefore expect the patch board to move in **both** directions, and a patch
that regresses from pass to fail is not automatically a Stage B defect. Check
against this probe before assuming it is.
### 1.5 The two that are NOT this build

| Patch | Cause | Where it belongs |
|---|---|---|
| `PCS1_060` | Type 6/7 mesh shadings | `Pass 85.1` — **unblocked**; `iso32000__s__8.7.4.5__mesh.md` (1,014 lines, Tables 82–86) landed 2026-08-18 |
| `PCS3_130` | ICC source profile handling | Its own Pass; see §6 on why `lcms2` is not the answer |

---

## §2 — Why one build: the four requirements are all properties of the buffer

| Requirement | Clause | What the buffer must carry |
|---|---|---|
| Blend in the group's colour space, subtractive components complemented before and after | §11.3.4 | **N colorant channels**, not 3 sRGB |
| Nonseparable modes in CMYK: RGB detour, K selected by mode | §11.3.5.3 | the **K channel**, addressable |
| Non-isolated groups: initialise from backdrop, then remove it | §11.4.4 | **un-premultiplied f32** + a second alpha `α_g` |
| Knockout groups: each element composites against the *initial* backdrop | §11.4.8 | **two** buffers per nesting level |
| Soft mask applies to the group RESULT | §11.4.5 | a group result that exists as a value |
| Overprint preserves untouched colorants | §11.7.4.4 / Table 149 | **one plane per colorant** |

`tiny_skia::Pixmap` is RGBA8 premultiplied with a single alpha. It satisfies
none of the right-hand column, and no call-site change makes it. The build is
**a compositor pdfcer owns**, with `tiny_skia` demoted from "the thing that
blends" to "the thing that scan-converts".

---

## §3 — The architecture

### 3.1 Coverage and colour are separated

`tiny_skia` remains the rasterizer. It is asked for **coverage only**:

- **Fills** — `tiny_skia::Mask::from_path(path, fill_rule, anti_alias)` gives
  an 8-bit coverage mask.
- **Strokes** — `PathStroker` converts the stroke to a fill path first; then
  as above. (pdfcer already does this in the clip path.)
- **Glyphs** — already outlines; same route as fills.
- **Images, shadings, tiling patterns** — cannot be reduced to a single colour
  plus coverage. These rasterize into a scratch `Pixmap` as today, and the
  compositor reads *both* colour and alpha from it. The colour arrives in sRGB
  and is lifted into colorant space by the same route as any other sRGB value,
  which is a documented lossy step, not a silent one (§5).

The compositor then does the per-pixel arithmetic itself. This is the
structure every surveyed engine uses and it is why "SIMD the blend loop"
(a suggestion pdfcer received from an outside model) is premature: the loop
does not exist yet, and the failures are arithmetic, not throughput.

### 3.2 The pixel

```
struct Pixel<const N: usize> {
    c:      [f32; N],   // UN-premultiplied colorant values, group colour space
    alpha:  f32,        // α  — the full alpha, including the backdrop's
    alpha_g: f32,       // αg — the group's own alpha, excluding the backdrop
}
```

**f32, not u8**, and this is load-bearing rather than fastidious: §11.4.4's
backdrop-removal correction contains a single `1/α_gn`, which amplifies
quantisation error by that factor. At `α_gn = 0.02` a half-level u8 error
becomes 25 levels — visible, and exactly the magnitude suite traps on.

**Un-premultiplied**, because the blend function `B(cb, cs)` is defined on
un-premultiplied values and premultiplying-then-blending is a different
function for every non-linear mode.

`α_i = Union(α_0, α_gi)` is derivable and `α_0` is the parent buffer's alpha,
so **one extra scalar per pixel** is the whole cost over a plain RGBA buffer.

#### ★★ AMENDMENT 2026-08-19 — §3.2 IS WRONG IN TWO WAYS. Both measured, both cheap to fix now and expensive later.

Sourced from `D:\Dev\Rag-Specialized\Compositor\` (new subject, 21 files +
an archived benchmark, deliberately outside every git repo).

> **★★ CONSUMED AND VALIDATED 2026-08-21 (`pdfcer-librarian`, 224th filing,
> `Pass 97.1e` + `Pass 97.1f`, `a277931` + `ff4b4bf`) — recorded HERE, on the
> consuming side, so the RAG's claim of usefulness is not filed only in the
> RAG.** Both amendments below are now **proved in trap marks**, not merely
> sourced:
>
> | RAG file | proved by |
> |---|---|
> | `shape_must_be_tracked_separately_from_alpha.md` | **13 trap marks** on suite `PCS1_161`. Shape `f_g` and alpha `α_g` ship on **separate** planes in `cmyk_buffer.rs`; the patch **passes the suite**. Its *"a fixture built from opaque fills cannot distinguish a correct knockout implementation from a wrong one — build it with `/ca < 1`"* rule is followed by **all three** new knockout tests, one of which asserts an ordinary group and a knockout group give **different** answers on the same two paints. |
> | `backdrop_defaults_zero_fill_inverts_masks.md` | prevented the class **exactly at the Stage A → Stage B boundary it predicted**. Related and learned the hard way the same session: handing a knockout group a **transparent** initial backdrop took `PCS1_161` from **2 traps to 15** — worse than no implementation at all. |
>
> **Two of this file's own sentences are now stale and are flagged rather
> than rewritten** (`pdfcer-librarian` does not write this document): `:56`
> and `:71` (*"…every one of them in device sRGB"*) are false on a
> subtractive page since `a277931`, and `:174`'s *"`blends_in_wrong_space`
> reports **15 of its 15**"* was true at `Pass 97.1d` and is now **0** — the
> counter was narrowed by `97.1e`. Full record: `ROADMAP.md`'s
> `a277931` + `ff4b4bf` entry.

##### 1. The pixel is one scalar short, and our own corpus said so first

`alpha` and `alpha_g` are not enough. §11.4 requires **three**: shape
`f_gn`, group alpha `α_gn`, and complete alpha `α_n`. Shape and opacity are
separate quantities and only their *product* is alpha — collapsing them is
exactly what knockout groups cannot tolerate.

**This is not new sourcing — it is in `iso32000__s__11.4.md` §6.5, written
the day before this plan**, as two numbered obligations:

| id | obligation | strength |
|---|---|---|
| `KO-S1` | a knockout group needs `f_si` and `q_si` as **separate** quantities — they appear in different places in the formulas and never only as their product | implied by the formulas |
| `KO-S2` | *"The separate shape value **shall** be computed in any group that is subsequently used as an element of a knockout group"* | **`shall`** |

That file already states the consequence in the plan's own terms — **"+1
plane"** — and it is not an edge case: **`/TK` defaults to `true`, so every
text object is an implicit knockout group.**

★ **The trap that comes with it**, quoted because a fixture built without it
proves nothing: *"A fixture built from opaque fills cannot distinguish a
correct knockout implementation from a wrong one. Build the test with
`/ca < 1`."* Shape and alpha are equal when opacity is 1, so an all-opaque
test passes under both the correct and the collapsed model.

Cost: one more f32 per pixel. §3.2's closing claim that *"one extra scalar
per pixel is the whole cost"* becomes **two**.

##### 2. Store PLANE-major. The struct as written is the slow layout.

`Pixel<const N>` is an interleaved (array-of-structs) layout. Measured
against plane-major (structure-of-arrays) on i9-10900KF / rustc 1.97.1, with
source and raw output archived in `Compositor\bench\`:

| kernel | plane-major speed-up |
|---|---|
| fill | **3.0 – 5.4×** |
| group composite | **2.6 – 3.7×** |
| whole-plane op | **3.8 – 10.3×** |

Every kernel, every N, every working-set size. **The folk rule — "per-pixel
operations want interleaved" — does not survive a runtime N**, because the
compiler cannot unroll or vectorise across a stride it does not know.

⇒ Keep `Pixel<const N>` as an **accessor view**; store `N + 3` contiguous
planes.

And a corollary that redirects effort: **compile-time N is the wrong axis.**
Specialising N ∈ {1,3,4} buys 5–18 %; the layout buys 200–300 %. Precedent
decides nothing here — Ghostscript is planar with 64 planes, MuPDF is
interleaved 8-bit — so the measurement is the only evidence available, which
is why it was taken.

##### Three more from the same survey, not amendments but constraints

- **Never `memset(0)` a subtractive buffer.** `DeviceCMYK`'s initial colour
  is `[0 0 0 1]` and soft-mask `/BC` defaults to black, so a zero fill yields
  **white** and **inverts every luminosity mask**. It is correct in sRGB and
  wrong in CMYK, so it appears precisely at the Stage A → Stage B boundary.
  Worse: §8.6.8 gives `ICCBased` spaces all-zeros, so the *same ink set* gets
  **opposite** defaults depending on `DeviceCMYK` vs `ICCBased`. A genuine
  spec contradiction needing an explicit, disclosed call.
- **ICC cannot exceed 15 colorants** — 4 bits in the pixel-format encoding,
  confirmed in three implementations. The decided collapse is immune; an
  `nCLR` shortcut is not.
- **`Mask::from_path` (§3.1) does not exist**, and the coverage-only design
  as written allocates a page-sized mask per fill — a cost pdfcer has already
  measured at **259 µs** (`clip_cache.rs`). Bbox-sized + 2 px, reused scratch.

##### Crate landscape — verified, not relayed

**No Rust crate does this.** Three named, and the licences were checked
against the crates.io API directly rather than taken from the survey,
because two of them are hard blockers and a wrong licence is not a
recoverable error:

| crate | version | licence | standing |
|---|---|---|---|
| `stet` | 0.4.1 | **Apache-2.0 OR MIT** | usable. Implemented overprint and chose 4 plates + a spot flag — read as a map of where that breaks |
| `rustybara` | 0.1.9 | **LGPL-3.0-only** | weak copyleft ⇒ operator call required (rule 13); a trap on the obvious search terms |
| `zenblend` | 0.1.3 | **AGPL-3.0-only** | categorically unusable for an MIT project |

##### What the survey could NOT establish — measure these, do not assume

- **Hand-written SIMD.** The benchmark is autovectorisation on one
  microarchitecture. A hand-vectorised interleaved N=4 might narrow the gap;
  nobody has published the comparison.
- **Const-generic vs runtime trip count in Rust** — no public A/B exists.
  The LLVM mechanism evidence predicts a *larger* win than was measured, and
  the survey records that disagreement rather than resolving it.
- **AA between two different spot colorants**, and AA edges where backdrop
  removal divides by a small `α_gn`. No treatment found anywhere.
- **Deep nesting with a page-sized group `/BBox`.** Harlequin says only that
  it *"uses different strategies to try to recover memory for such pages"* —
  a refusal to say.
- ⚠ *"RIPs disable AA for separations"* is **explicitly not established** —
  no vendor source exists. Do not repeat it.
### 3.3 N is chosen per page, not per build

Pre-scan the page's resources for `/Separation` and `/DeviceN` colorant names
and size the plane set exactly: `CMYK + one plane per distinct spot`.
Ghostscript's own documentation notes this pre-scan **is possible in PDF and
impossible in general in PostScript** — it is a structural advantage pdfcer
inherits from the format and should take.

**Cap and fall back honestly.** Beyond a configured plane ceiling, revert to
the tint transform — which is precisely pdfcer's current behaviour, so the
fallback is already written, already tested and already disclosed by the
existing counters. Rule 4: the fallback **prints what it did**; it does not
quietly produce a different picture.

### 3.4 Scope: not page-wide by default

Poppler bug #1565 (still open) is the warning: enabling overprint preview
routed the whole page through CMYK and **visibly shifted unrelated RGB raster
content**. pdfcer should engage the colorant compositor for the **object
subtrees that need it** — transparency groups, and content under an overprint
state — and leave the ordinary sRGB path alone otherwise. A patch that fixes
7 patches and shifts 25 others is not progress.

---

## §4 — Staging

The stages are ordered so each one is independently measurable and each one
ships a number. **Do not skip A to get to B**: A is where the group semantics
get right, and B is a change of pixel type on top of correct semantics. Doing
B first means debugging colorant arithmetic and backdrop removal at the same
time, on the same pixels.

### Stage A — the compositor, RGB only (proposed `Pass 97.0`)

Replace the group-buffer path with pdfcer's own f32 un-premultiplied buffer and
pdfcer's own composite/blend implementation. **N = 3, sRGB.** No colorant
planes yet.

Delivers:
- **Non-isolated groups**: buffer initialised from the backdrop; §11.4.4
  result-block backdrop removal `C = C_n + (C_n − C_0)·(α_0/α_gn − α_0)`, with
  the single division guarded.
- **Isolated groups**: `α_0 = 0`, which the same code path expresses without a
  branch on `/I`.
- **Knockout groups**: §11.4.8's `b ∈ {0, i−1}` subscript, implemented as
  **two buffers**, not per-element copies. Memory is O(nesting depth).
- **Soft mask on the group result** (§11.4.5), replacing the fold-into-clip
  approximation — including the `/TR` transfer function, which is currently
  read and counted (`soft_mask_tr_ignored`) but not evaluated. `/TR` is where
  a mask gets **inverted**, so an ignored one can leave visible exactly what
  the document meant to hide.
- **`0/0 = 0` by convention**, adopted unconditionally — a `should` in ISO
  32000-1 and a **`shall`** in ISO 32000-2 §11.3.2. Note the `shall` is on
  *robustness*: never emit NaN or Inf.

Expected: `PCS1_161`, `PCS3_161`, `PCS1_162`, `PCS1610`, `PCS1611`, `PCS168`,
`PCS169` — **7 patches**, 25 → up to 32.

### Stage B — colorant planes (proposed `Pass 97.1`)

Make the buffer N-colorant. Same compositor, different pixel.

Delivers:
- **§11.3.4** blending in the group colour space with subtractive complement
  (`blend_subtractive(cb, cs) = 1 − B(1 − cb, 1 − cs)`).
- **§11.3.5.3** nonseparable modes in CMYK: complement CMY to RGB, blend,
  complement back, **select K by mode**.
- **Table 149 overprint** — already written as pure, tested logic in
  `pdfcer_render::overprint` (12 tests, the table transcribed cell by cell,
  `bd9d5ef`). It has never had a colorant buffer to run against.
- **Keep the tint transform OUT of the paint path** for any colorant that owns
  a plane; retain it only to derive that colorant's equivalent colour for the
  final collapse.

Expected: `PCS1_160`, `PCS3_164`, and the 7 overprint patches — **9 patches**,
32 → up to 41.

### Stage C — the collapse, and its disclosure (proposed `Pass 97.2`)

Collapse N planes to sRGB **once, at the end**.

**⇢ NOW SOURCED: `docs/collapse-model-survey.md` (2026-08-18).** The
"vendors disagree" claim is no longer an assertion — it is measured against
thirteen engines, and the absence of a specification is itself documented by
an ISO TC171 participant who went looking. Headlines:

- **Harlequin uses per-colorant `max()`; Mako uses multiply-of-complements —
  two products from the same vendor.** There is no consensus formula to find.
- **Acrobat's method has never been published**, and the ICC states Acrobat
  *"should not be used as a guide"* for spot inks. "Match Acrobat" is not an
  available specification.
- **Third independent confirmation of the N-plane architecture**, this time
  from vendor docs: *"overprinting… is disabled"* / *"not allowed"* if spot
  colorants are tint-transformed first. Tint-transforming early and
  overprinting correctly are mutually exclusive.
- **Default decided:** per-colorant `/Separation` tint transform (preferred
  over the DeviceN collective one), accumulate by multiply-of-complements,
  one ICC hop to sRGB through the OutputIntent. Five settings enumerated,
  including a second ambiguity nobody had noticed — **which OutputIntent to
  use when the array has more than one entry** (MuPDF takes `[0]` ignoring
  `/S`; Poppler refuses to act at all).
- **★ THE ICC HOP IS `iccce`'S — corrected 2026-08-18.** This bullet named
  `moxcms` as the ICC candidate. **`ARCHITECTURE.md` decision 064 already
  assigned colour CONVERSION to `iccce`** (the operator's sibling MIT project,
  which names pdfcer as its first consumer), and recommending a third-party CMM
  against it was a call made without reading the record. `iccce` already
  ships what Stage C needs: `Chain::with_destination(&src, Destination::None,
  intent)`, whose built-in sRGB destination is **constructed from published
  constants** (BT.709-6, W3C transfer constants, Bradford to D50 per
  ICC.1:2022 Annex E.3) — **no shipped `.icc`, so no redistribution
  question**. Verified by reading `iccce-cmm/src/transform.rs`, not its prose.
- **Two things Stage C must carry from that**, both in
  `docs/collapse-model-survey.md` §7: `Destination::None` is an **assertion**,
  not `Option::None` — a declared-but-unparseable output intent is a
  **refusal to propagate**, never a silent fallback; and the conversion costs
  **~1.4 Mpix/s ≈ 6 s/page against pdfcer's ~0.6 s render**, so the collapse
  cannot be an unconditional per-frame step.

#### ★★ AMENDMENT 2026-08-19 — THE COLLAPSE IS SPECIFIED IN PDF 2.0. Re-read Stage C before scoping it.

Everything above rests on *"there is no consensus formula to find"*, sourced
from `docs/collapse-model-survey.md`'s thirteen-engine survey. **That is true
of the engines and false of the standard**, and the difference was invisible
because pdfcer's spec corpus held **no clause 10 at all** until today.

`iso32000__s__10.8.md` (new, `pdfcer-spec-librarian`, 2026-08-19):

- **ISO 32000-2:2020 §10.8.3 "Separation simulation"** specifies a four-step
  algorithm: convert each separation to **"flat XYZ" (no gamma)** against a
  **background matte of all white**, then combine with a **multiply blend**.
- **§12.11.2 Table 275** gives it a requirement-handler name,
  `SeparationSimulation`, whose **NOTE 5** reads verbatim: *"This is
  sometimes referred to as "Overprint Preview"."*
- `OP-N1` — the negative result this plan and `overprint.rs` both lean on —
  is **rescoped to ISO 32000-1, not retracted**. For 1.7 it is confirmed by
  measurement (`simulat*` = 7 hits, all unrelated). For 2.0 it is simply
  wrong.

**What changes, and what does not.**

It does **not** make the collapse mandatory: §10.8.3 is a `should`, so a
compositor without it stays conformant. What it changes is the *freedom*.
The survey's conclusion licensed pdfcer to pick any defensible formula because
none was specified. One is. It is a **`should` on the OUTCOME**, which means
**shipping a simulation that does not match its four-step result is a worse
position than shipping none at all** — a documented deviation rather than a
documented absence.

★ **And it constrains the architecture, not just the arithmetic.** The new
file states the consequence directly: *a compositor that implements §10.8.3
as a final RGB pass will disagree with a conforming implementation wherever a
blend mode or a transparency group is present.* Stage C is currently written
as "collapse N planes to sRGB **once, at the end**" — that phrasing is
exactly the final-pass shape being warned about. **Reconcile before
scoping**: either the collapse happens inside the group-composite step, or
Stage C must state why the end-pass is acceptable for pdfcer and disclose the
deviation.

**Do not treat §10.8.3 as a complete specification.** Three of its own
ambiguities are registered in the same file and two are load-bearing:

| id | what is unspecified |
|---|---|
| `SEP-A3` | **"flat XYZ (no gamma)" is defined nowhere** — one occurrence document-wide. The adopted reading (linear-light CIE 1931 XYZ, no transfer curve) is **DERIVED**, labelled as such |
| `SEP-A4` | **the per-separation ink→XYZ map is unspecified.** Step (b) says "convert each separation into flat XYZ" without saying by what. ⇒ pdfcer's choice, disclosed under rule 4 |
| `SEP-A2` | step (c) cites **"Table 133"** for the multiply blend, but Table 133 is the compositing-**variables** table; the blend functions are Table 134. Loose citation or erratum; **none filed.** Read as `B(cb, cs) = cb × cs` |

So Stage C now has a specified *target* with three holes in it, rather than
no target at all. That is a better position and a more constrained one, and
the settings enumerated above stay — they now record deviations from a
known outcome rather than choices among equals.

`SEP-N2` is worth carrying too: **§10.8 says nothing about spot-colourant
overprint order, ink solidity or dot gain.** Those live in `DeviceN`'s mixing
hints (`/Solidities`, `/PrintingOrder`, `/DotGain`) and §10.8 does not
reference them. The multiply in step (c) is order-independent, so
`/PrintingOrder` has **no role** in the simulation as specified — which is
itself a useful negative result for anyone tempted to honour it.

**`docs/collapse-model-survey.md` has not been amended** and still asserts
the un-scoped "no consensus formula" claim. It is a dated survey of engines,
and its engine findings stand; this amendment is the correction to what was
inferred *from* them. Amend it there before citing it again.
### Out of scope for 97.x

`Pass 85.1` mesh shadings (`PCS1_060`) and ICC source profiles (`PCS3_130`).
Both are real, both are separately scoped, neither is blocked on this build.

---

## §5 — Risks, and the honest failure modes

1. **Performance.** Every group becomes an f32 page-sized buffer. Mitigation:
   engage the compositor only for subtrees that need it (§3.4); measure with
   `tools/render-profile` before and after; the render-parity gate
   (`tools/render-parity`, 2840/49/1 buckets) is the regression net for the
   ordinary path.
2. **The render-parity gate is against pdfium**, which does not implement
   overprint and flattens transparency differently. A Stage-B improvement can
   register as a parity *regression*. Read a parity delta on transparency
   content as a question, not a verdict — and record which oracle disagreed.
3. **sRGB → colorant is lossy and ambiguous.** Images and shadings arrive as
   sRGB. Lifting them into CMYK for a CMYK group is a guess. It must be
   **counted and printed**, not silently performed. This is precisely the
   class of inference rule 4 exists for: render it normally, disclose it
   off-canvas.
4. **Scope creep into a rasterizer rewrite.** The line is: `tiny_skia` keeps
   scan conversion, path stroking, glyph outlines and image sampling. pdfcer
   takes compositing only. If a change requires re-deriving coverage, it is
   out of scope.
5. **The 8 UNRESOLVED reference-strip patches are not addressed by any of
   this** and must not be quietly counted as headroom. Two of the three cheap
   wins in §7 bear on them instead.

---

## §6 — Two external suggestions, assessed

An outside model (Gemini, via the operator, 2026-08-18) proposed a route to
the remaining patches. Recording the assessment because two of its
recommendations would be **actively wrong for pdfcer**, and a future session
that meets the same advice should not have to re-derive why.

**Correct but already held, at lower resolution:** ISO 32000-2 clause 11 as
the governing text; W3C Compositing Level 1 as a cross-check on the separable
blend formulas; isolated-vs-non-isolated as the transparency-group axis;
Porter-Duff alpha weighting; and — independently reaching pdfcer's own
conclusion — that overprint requires **preserving the underlying colorant
rather than knocking it out**. That last convergence is worth something: an
outside model with no access to this repo arrived at the same architectural
requirement as the seven-engine survey.

**Rejected — `lcms2`.** It is a binding to Little CMS, a **C** library. It
cannot cross the **wasm32 CI gate** that `pdfcer-core` and `pdfcer-render` are
held to (`.github/workflows`, `cargo check --target wasm32-unknown-unknown`),
and that gate is not negotiable machinery — it is the enforcement of the
web-fork invariant. The OCR engine decision turned on exactly this constraint.

**★ And the follow-on recommendation was ALSO wrong, which is the part worth
keeping.** This paragraph originally continued *"if ICC handling needs a
library, the candidates are pure Rust (`qcms`, `moxcms`)"*. **There is no
candidate slot to fill**: `ARCHITECTURE.md` decision 064 assigns colour
conversion to **`iccce`**, and that decision predates this document by a day.
Rejecting the wrong crate for the right reason and then proposing a
replacement is a *narrower* failure than adopting `lcms2` would have been, and
it has the same root — the record was not read. See
`docs/collapse-model-survey.md` §7.

**Rejected — `vello`.** GPU, via `wgpu`. `pdfcer-render` may not gain a
windowing or GPU surface (`ARCHITECTURE.md` §3, project rule 2). This is
categorical, not a tradeoff. `resvg` is a fair *structural* reference — it is
`tiny_skia`'s own consumer — but it is a reference, not a dependency.

**Premature — SIMD.** The failures are arithmetic correctness. There is no
compositing loop to vectorise until Stage A exists, and vectorising the wrong
formula is not a win. Revisit after §5's measurement, not before.

---

## §7 — Cheap wins that do not wait on this build

Carried forward from `NEXT_SESSION.md` §2, unchanged and still unclaimed:

1. ~~**The trap detector is probably over-counting.**~~ **MEASURED AND FALSE,
   2026-08-18. Do not spend a session on this.** The hypothesis was that
   `CONTRAST_MIN` — calibrated against pdfcer's own output rather than against
   the suite's stated *"Faint X does not indicate a failure!"* — was firing on
   marks the suite pre-declares tolerant (**all ten cells of PCS020**, **cell
   d of every DeviceN patch**). The new probe measured the actual
   X-versus-surround contrast on every currently-failing patch those
   tolerances cover:

   | patch | cell | X | surround | faint? |
   |---|---|---|---|---|
   | `PCS020` | 6 of 7 | `[254,254,253]` white | `[141,197,62]` green | **no — maximal** |
   | `PCS020` | 2 cells | `[196,197,195]` grey | `[146,197,73]` green | no |
   | `PCS190` | d | `[0,0,0]` black | `[0,158,218]` cyan | **no** |
   | `PCS191` | a,b,d | `[0,0,0]` black | green / cyan | no |
   | `PCS191` | c | `[0,240,255]` | `[0,180,241]` | no (~60 levels) |
   | `PCS192` | b | `[255,255,255]` white | `[239,56,62]` red | **no — maximal** |

   The suite's wording for `PCS020` is *"a faint 'X' in **slightly darker
   green**"*. A **white** X on green is not that mark. Every trap still
   firing on these patches is at or near maximal contrast, so **no
   recalibration consistent with the suite's criterion changes a single
   verdict**. The failures are real rendering failures, and they are the
   ones §4 Stage B addresses.

   One live nuance that survives, and is *not* about contrast: `PCS191` cell
   **c** has **two sanctioned correct outcomes** — the suite states a cross
   there is fine *"if the system performs colour conversion and sets the OPM
   for this patch c to 0"*. pdfcer converts but leaves `OPM 1`, so its cross is
   a genuine failure today. If Stage B ever makes pdfcer take the
   convert-and-set-OPM-0 route deliberately, the harness must learn that
   cell c is not binary.
2. **The suite ships its own Reference file** — a whole-suite reference
   render, in the same ZIP, with texts in Registration so they appear in
   every separation. pdfcer is not using it as an oracle and should. This is
   the one that bears on the 8 UNRESOLVED.

   **Blocked on an input, checked 2026-08-18:** the file is **not on this
   machine**. The local corpus directories hold the 51 patch PDFs and the
   extracted ReadMes, but the Reference PDF was not among what was kept from
   the 126 MB download. Re-fetching it is an
   operator call (a large download, and `LEGAL.md` §5 governs what enters the
   corpus), so this item is **owed, not merely unstarted** — it should not be
   picked up as if it were a free afternoon.
3. ~~**`/Indexed` colorants — MEASURED AND CONFIRMED, 2026-08-18. This is a live
   defect, not a suspicion.**~~ **★ FIXED 2026-08-21 — AND IT MOVED NOTHING,
   WHICH IS THE FINDING.**

   `ColorSpace::indexed_entry` (§8.6.6.3) now resolves an `Indexed` operand to
   its palette entry in the base space, `overprint::classify` recurses into
   the base, and **both** call sites — `paint_overprint` **and**
   `overprint_would_change` — resolve before they classify. The second was not
   in the original write-up and matters more than it looks: that predicate is
   what decides whether the composite is called at all, so an `/Indexed`
   source was never even *counted* as an effective overprint.

   **Measured A/B on the four patches that carry `/Indexed`, pre- and
   post-fix binaries, same corpus:** `overprint_effective`,
   `overprint_composited`, `overprint_refused` and `overprint_pixels` are
   **identical to the digit** on `PCS1_190` (5/5/0/3607), `PCS1_191`
   (2/2/0/1679), `PCS1_192` (3/3/0/2182) and `PCS2_020` (4/4/0/1654). Board
   unchanged.

   ★ **Why: every `/Indexed [/DeviceN …]` space in those patches is an IMAGE
   colour space and nothing else.** Verified structurally — `PCS1_190`'s two
   are `/XO1` and `/XO2`, both `/Subtype /Image`. So the classification fix
   is correct, cited, tested, and **inert on the whole corpus** until images
   reach overprint at all. The original write-up put the `/Indexed` half
   first and the image half second; **the dependency runs the other way**.

   The `/Indexed` finding as originally written, kept because the fix is
   real even where it is currently unreachable: Colorants must be read from the **base** space
   (§8.6.6.3). `overprint::classify` has no `Indexed` arm, so an `/Indexed`
   space falls to `_ => SourceKind::OtherProcess` and its base's colorant list
   is invisible to Table 149. Extracted from the corpus:

   ```
   PCS1_190:  /Indexed [/DeviceN [/Cyan]              /DeviceCMYK ...] 255 <lookup>
   PCS1_190:  /Indexed [/DeviceN [/Cyan /Yellow /Black] /DeviceCMYK ...] 255 <lookup>
   PCS2_020:  /Indexed /DeviceCMYK 255 <lookup>
   ```

   The first two **are** PCS190's documented discriminator — the a/b pair's
   DeviceN **omits** the backdrop's colorants and the c/d pair **includes**
   them at 0%, and *"the colorant LIST — not the tint values — decides what
   survives"*. pdfcer cannot see either list. `/Indexed` appears in **4 of the
   7 failing overprint patches** (`PCS190`, `PCS191`, `PCS192`, `PCS020`).

   Two halves to the fix, and only the first is small: `classify` must recurse
   into the base space, **and** the tints handed to `cmyk_group_rules` must be
   the palette-**looked-up** base components rather than the index. The call
   site currently receives `(space, comps)` where `comps` is the raw index.

   **A second, larger gap surfaced while measuring this**, and it is recorded
   rather than fixed because it belongs to Stage B: `overprint::composite` has
   exactly **one** call site, in the path/glyph painter. **Image XObjects do
   not reach it at all** — and `PCS190`'s only failing cell is `d`, an image.
   Per-sample overprint needs per-sample colorants, which is the colorant
   buffer. Before building it, add the counter: an image that skips overprint
   is currently not counted as `overprint_refused`, which is the same
   blind-counter shape as the glyph painter in `bf75351`.

   **★ THE COUNTER SHIPPED 2026-08-21 — `overprint_images_unsupported`**, and
   it is deliberately **not** `overprint_refused`. Those two name different
   failures: `refused` means *"the composite was offered this paint and could
   not run it"*; this means *"the composite was never offered this object
   class at all"*. Widening the old counter would have made a whole missing
   object class look like a run of ordinary failures and would have moved a
   number an operator may already be diffing between runs — the same
   meaning-change trap `transparency_groups_knockout_approximated` walked
   into this session, and the reason that one's test was updated rather than
   deleted.

   First measurement, images painted under `/OP true` with no overprint path:

   | patch | count |
   |---|---:|
   | `PCS1_190` | 2 |
   | `PCS1_191` | 2 |
   | `PCS1_192` | 2 |
   | `PCS2_020` | 4 |
   | `PCS2_031` | **1 — and this patch PASSES the suite** |

   That last row is the one to keep: a patch can pass its own trap and still
   have an object class the renderer never offered the feature to.

   **[★★ CORRECTED 2026-08-31 (pdfcer-librarian, 357th filing) — both halves
   of the `PCS2_031` row above are now known wrong.** It does not pass the
   suite (`Pass 196.0` gives it its first correct verdict, `CRIT?` — the
   already-known n-channel-buffer/missing-spot-plane gap). And the counter's
   meaning **changed at `Pass 130.2`**, narrowing it to a strictly smaller
   set; under the current definition `PCS2_031` reads **0**, not the **1**
   quoted above from before the narrowing. See `ROADMAP.md`'s `Pass 196.0`
   *Shipped* entry for the full correction. Row kept above as the original,
   dated measurement rather than rewritten — this is a plan/reference
   document, not the append-only roadmap, so the correction is made inline
   rather than as a dated footer.]**

And one new one, produced while writing this document:

4. **`tools/suite-cell-probe.py`** — the diagnostic §1 is built on. For each
   trap it prints the X colour, the surround colour and Acrobat's colour at
   the same cell. It turned "14 traps on `PCS1_161`" into "the interior blend
   is being applied against a transparent backdrop" in one run. It currently
   lives outside the repo and should be promoted into `tools/` with the cell
   index → blend-mode mapping derived from the content stream rather than from
   the pitch arithmetic in §1.1.
