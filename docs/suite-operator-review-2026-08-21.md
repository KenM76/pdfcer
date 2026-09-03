# The print-conformance suite — operator review, 2026-08-21, and what it says about the harness

**The operator read the annotated render cell by cell and found defects
`tools/suite-check.py` structurally cannot see.** This file records his
observations verbatim-in-substance, my verification of each, and the two
instrument faults they expose. It exists because these are the only
independent judgements of pdfcer's suite output that have ever been taken —
every other number in this project came from the harness scoring itself.

---

## ★★ AMENDED 2026-08-24 BY `Pass 122.2` — READ THIS BEFORE ACTING ON §2 OR §3

**The operator's cell-by-cell readings below still stand. Two of MY analyses
of them do not**, and both were wrong in the direction that would have made
someone implement the wrong thing confidently. The original wording is left
in place below rather than edited, because this document's value is that it
is dated; the corrections are here, at the top, where they cannot be missed.

**1. §2's "seven of the 51 patches use the positive criterion" is FOUR.**
`PCS150`, `PCS151` and `PCS152` are not among them. Those three state on
their own face *"If a X can be seen, Optional Content is not handled right"*
— the NEGATIVE criterion, which the harness already implements. Their
ReadMes mention a check mark only while describing what the failure cross is
drawn **out of** (*"a cross consisting of 2 check marks"*).

⇢ The list was built by grepping the ReadMes for the phrase *"check mark"*
and was then read as if it had been built by reading the rule. **A grep for a
phrase finds a mention, not a criterion.** The harness now reads each patch's
own extracted text at runtime instead of carrying a list.

**2. §3's diagnosis — "the contrast floor has no area term" — is WRONG, and
the fix it recommends would have changed nothing.** §3 says box 3's crosses
are *"roughly three times the linear size"* of the calibration patch's, so
the threshold should become a function of mark size. Measured at the scale
the harness actually renders: **every trap on `PCS 1.0` is 36–38 px square,
and the `PCS 16.0` calibration traps are 38 px square. They are the same
size.**

The real fault was simpler and worse: `CONTRAST_MIN = 12.0` had been
calibrated against **one** population — the sub-perceptual differences
Acrobat leaves in a visually-clean render — and never against genuine traps
of moderate contrast, because none had been measured. §1's rows are that
missing population, and they separate cleanly:

| contrast | operator's reading |
|---:|---|
| 10.7, 10.2, 7.8, 7.4 | the four **clear fails** (cells i, d, j, e) |
| 4.1, 4.1 | the two **"faint outline only"** (cells b, g) |

An empty interval from 4.1 to 7.4. The floor is now **6.0**, inside it.

⇢ ★ **Had the area term been built as recommended, `PCS 1.0` would have gone
on reporting `clean` — with a fix in place and a plausible reason to stop
looking.** A fix aimed at a misdiagnosed cause is more dangerous than no fix,
because it consumes the suspicion.

**3. §1 row 4 (`PCS 1.1`, fail → pass) is NOT REPRODUCIBLE, and is left open
rather than overruled.** The harness still reports one mark at contrast
**17.4**; cropped and magnified 6×, it is a solid, filled, high-contrast teal
X on a blue swatch, not an outline. But the operator's own wording names a
**different artefact** — *"the **combined** render shows no cross"* — and the
harness scores **individual patch** renders. Both observations may be true.
⇢ If the combined and individual renders genuinely differ, **that is a defect
in its own right and nobody has looked**; it is filed as `Pass 122.4`.

**4. §5 owed item 4 is DISCHARGED.** *"Check the three optional-content
patches, which no one has looked at."* Looked at, side by side against
Acrobat renders: all three show one check mark and *"Default View"*, no
cross, in both engines. `PCS 15.0`, `15.1`, `15.2` **genuinely pass**. The
only difference from Acrobat is the swatch's cyan — the known CMYK
colorimetry gap, not a trap.

**5. §5's "26 pass at minimum" is now 25.** It reached 26 by flipping
`PCS 1.1`, which correction 3 above does not support. Current harness board:
**11 FAIL / 24 pass / 16 UNRESOLVED**, plus `PCS 5.0` whose mark is present
but which the harness cannot adjudicate ⇒ **25**.

**One thing §5 item 1 should have said and could not.** A first attempt at
the check-mark detector keyed on the mark's **colour**, measured from
`PCS 8.2` (olive). Run against `PCS 8.01` it reported the mark PRESENT — by
matching the green end of that patch's spot-colour gradient bar — while
**both real marks were absent**. ⇒ **The mark's colour is not a constant of
the criterion**; `PCS 8.2`'s marks are olive and `PCS 8.01`'s are dark green.
Any detector must key on presence relative to a reference render, not a hue.
No detector shipped in `122.2` for exactly this reason.

Full derivation and the measured populations: `ROADMAP.md`'s `Pass 122.2`
Shipped entry, and the docstring of `tools/suite-check.py`.

---

★ **Treat this as an ORACLE, not as feedback.** The harness had no
independent check before today; its thresholds were calibrated against one
patch (`PCS 16.0`, 2026-08-17) whose answer was known from a code change.
That is self-consistency, not ground truth. These rows are ground truth.

---

## 1. What he found, and what I confirmed

| # | PCS | harness said | operator says | I verified | verdict |
|---|---|---|---|---|---|
| 3 | 1.0 CMYK Overprint Test | **pass** | cells `d`,`e`,`i`,`j` are **clear fails**; `b`,`g` are faint outlines only | yes — the mask and shading cells carry large, obvious crosses | **FAIL** |
| 4 | 1.1 CMYK Overprint Mode | **FAIL, 1 cross** | **pass** except a faint outline | the combined render shows no cross at any contrast | **PASS** |
| 5 | 19.0 DeviceN Overprint (Black) | FAIL, 1 cross | cell `d` fails; `b` is a faint outline, so the operation is right and only the edge rounding differs | yes | FAIL (1 cell, `d`) |
| 6 | 19.1 DeviceN Overprint (Yellow) | FAIL, 4 crosses | `a`,`c` pass; `b`,`d` fail | yes — 4 was an over-count | FAIL (2 cells) |
| 8 | 8.2 DeviceN Support (4 colours) | **pass** | **fail — the two green check marks are missing** | yes, at 3× zoom: no check mark in either image's upper-right corner | **FAIL** |
| 20 | 5.0 Font Substitution | pass | — | check mark present and correct | PASS |
| 30 | 8.1 DeviceN Support (5 colours) | **pass** | — | **no green check marks** | **FAIL** |
| 31 | 8.01 DeviceN Support (6 colours) | **pass** | — | **no green check marks** | **FAIL** |

**Net effect on the headline: 29 pass becomes 26.** Four patches move
pass → fail (3, 8, 30, 31), one moves fail → pass (4). And three more
(`15.0`, `15.1`, `15.2`, optional content) use the same criterion as 8, 30
and 31 and are **not yet checked** — they are not on the combined pages, so
the operator could not see them either.

## 2. ★★ INSTRUMENT FAULT 1 — the harness implements ONE of the suite's TWO pass criteria

The suite marks a failure two ways, and its own artwork says so:

- **negative marker** — a cross that a correct renderer makes vanish.
  `suite-check.py` implements this, thoroughly.
- **positive marker** — *"If a check mark is visible in the upper right
  corner then DeviceN is respected (= GOOD). If no check mark appears then
  DeviceN color was transformed to CMYK (= ERROR)."*
  **The harness cannot see this at all.** It hunts for a mark that should
  not be there; it has no notion of a mark that should be there and is not.

**Seven of the 51 patches use the positive criterion** — grep the ReadMes
for "check mark": `PCS050`, `PCS080`, `PCS081`, `PCS082`, `PCS150`,
`PCS151`, `PCS152`. Every one of them has been reported `clean` for the
harness's entire life, and at least three of them are failures.

⇢ **The failure mode is exactly the one this project keeps re-learning: an
absence is invisible to a detector built to find a presence.** A gate that
looks for the wrong thing does not report "I cannot tell" — it reports
"clean", which is indistinguishable from a pass.

## 3. ★ INSTRUMENT FAULT 2 — the contrast floor has no area term

`CONTRAST_MIN = 12.0` (8-bit levels) implements the suite's *"a **clear** X
… judged by a human at 0.5 m"*. It is a fixed number regardless of how big
the mark is.

Box 3's cells `d` and `i` are crosses roughly **three times the linear size**
of the calibration patch's, at a measured contrast of **9.8** — below the
floor, and unmistakable to the eye at normal viewing distance. Meanwhile
box 11's cells sit at 1.3–3.1 and genuinely are invisible.

⇢ Perceptibility scales with area as well as contrast. A floor with no area
term is calibrated for one mark size and wrong for every other. The fix is
not to lower the number — that would drag box 11's population in with it —
but to make the threshold a function of the mark's size.

## 4. What this does NOT change

**Nothing about the compositing work.** Patches 9, 10, 11 (`16.0`, `16.1`,
`16.2`), 36 and 37 are cross-criterion patches and the operator confirmed
the first three read correctly. The blending-colour-space census
(107 wrong → 0) counts blend operations, not traps, and is unaffected by
either fault above.

**What it changes is the scoreboard**, and specifically any sentence of the
form *"N of 51 suite patches pass"*. Every such sentence in `ROADMAP.md`,
`FEATURES.md` and `SESSION_LOG.md` — including numbers filed earlier today —
is an over-count by the size of the check-mark family.

## 5. Owed work

1. **Teach `suite-check.py` the positive criterion.** Per-patch, from the
   ReadMe: which marker, where, what colour. Report `MISSING-MARK` as its
   own verdict rather than folding it into `X`.
2. **Give the contrast floor an area term**, calibrated against the rows in
   §1 — which is the first independent calibration set this harness has
   ever had.
3. **Re-measure and re-file.** The corrected suite standing is **26 pass**
   at minimum, pending the three unchecked optional-content patches.
4. **Check the three optional-content patches** (`15.0`, `15.1`, `15.2`),
   which no one has looked at.

## 6. The reason this document exists rather than a commit message

Because the operator's cell-level judgements are the calibration set for
items 1 and 2, and a calibration set that lives only in a commit message
cannot be re-read by the person doing the calibration. He also gave a
*mechanism* twice — "just an issue with the layer edge", "the math for the
edges of the x differs slightly with rounding" — which is a distinction the
harness has no way to express and which item 2 will need: a mark that is
present in outline only is a different fact from a mark that is present in
fill.
