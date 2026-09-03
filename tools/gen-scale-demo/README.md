# gen-scale-demo — one page, nine orders of magnitude

Generates a letter-size PDF holding a **banana at life size** and **two of
its cells at that same scale**. Nothing on the page is enlarged for
clarity, which is the entire point: the cells really are 300 µm and 60 µm
across, so at a zoom where the page fits a screen they cover about one
pixel between them.

```
python tools/gen-scale-demo/gen_banana.py <out.pdf>
```

**`banana-at-scale.pdf` is checked in beside this file**, and is attached
to the `v0.8.0` release. Regenerate it with the command above; the
generator is deterministic, so a regenerated file is byte-identical to the
committed one unless the source changed.

It is **not a fixture**: it is not in `fixtures/`, no test loads it, and
`cargo test` never runs it. It is here to be looked at — and because a
claim about rendering nine orders of magnitude on one page is worth more
as a file you can open than as a paragraph.

*(This paragraph read "Nothing ships" until `v0.8.0`, when the operator
asked for the PDF to go out with the release.)*

---

## 1. Why it exists

`Pass 74.1`/`74.2`'s deep-zoom work made region rendering flat across
magnification and pushed the numerical ceiling out past a **trillion
percent**. That is a claim about arithmetic. This file is the claim made
visible: a document you cannot read without exercising it.

It also happens to be the only artefact in this repository that a
non-programmer can look at and immediately understand what the renderer
does.

## 2. The scale chain

Every tier is roughly ten times the zoom of the one above it. Measured
with `pdfcer render-page --region`, each view a ~1600 × 1000 viewport:

| tier | true size | as points | readable from | render time |
|---|---|---|---|---|
| banana | 153 mm | 433 pt | 100 % | 1 490 ms |
| cell outlines | 300 µm | 0.85 pt | ~2 000 % | — |
| cell labels, starch grains | 30–45 µm | 0.07–0.13 pt | ~12 000 % | 1 600 ms |
| organelle labels | 8 µm | 0.023 pt | ~45 000 % | 1 420 ms |
| chloroplast grana, plasmodesmata | 1–5 µm | 0.003–0.014 pt | ~400 000 % | 120 ms |
| whole mitochondria | 1.2–3 µm | 0.003–0.009 pt | ~2 600 000 % | 243 ms |
| cristae, junctions, nucleoids | 18–210 nm | 5e-5 – 6e-4 pt | ~16 000 000 % | 182 ms |
| ATP synthase particles | 10 nm | 2.8e-5 pt | ~35 000 000 % | 190 ms |
| the molecule box | 50 nm | 1.4e-4 pt | ~1 500 000 000 % | 158 ms |
| a water molecule | 0.37 nm | 1.0e-6 pt | ~190 000 000 000 % | 60 ms |

★ **Render time still does not grow with magnification — it falls**, and
the reason is worth stating plainly. A viewport is a fixed number of
pixels whatever the page behind it is doing, so the top rows and the
bottom rows differ only in how much of the document is *in* it. The deep
tiers are the fast ones because most of the page is off-screen and now
gets skipped (§7).

★ **Every tier is ten to twenty times slower than the previous version of
this page, and that is the honest price of the detail.** The page went
from ~3 000 path operators to roughly **one million**: 342 mitochondria,
each now a full section with two membranes, 6–15 cristae, and 60–360 ATP
synthase particles. At page-fit zoom every one of those paths is
rasterised into about a hundredth of a pixel. §7 says what was fixed and
what was deliberately left alone.

## 3. The arithmetic the request imposed

The brief was specific, and two of its constraints turned out to be the
interesting ones.

- **Labels under each cell at 1/10 the height of the largest cell.**
  300 µm / 10 = **30 µm = 0.085 pt**. That is the smallest type most
  people have ever deliberately set.
- **The arrow ends two cell lengths above the cells.** 2 × 300 µm =
  **600 µm = 1.70 pt**. The arrow's point is at `y = 562.125984` and the
  cell's top edge is at `560.425197`; the difference is exactly that.
- ⇒ **and therefore the arrow could not have a conventional head.** One
  large enough to see at page scale is about 8 pt across — *nine times
  wider than the thing it points at*, which would bury the cells at the
  only zoom where they matter. It is a tapered dart with a **250 µm**
  head: an arrow from across the room, and an arrowhead under
  magnification.

## 4. Files

| file | holds |
|---|---|
| `gen_banana.py` | page layout, the banana's Béziers, scale bars, the dart, and the PDF writer |
| `cells_detail.py` | both cell interiors, authored in **micrometres** — a `Pen` converts to points at the last moment, so no drawing code ever sees a conversion factor |
| `easter_egg.py` | a stroke font and a path-to-mitochondria chainer |
| `mitochondrion.py` | **one mitochondrion, drawn to its real anatomy**, authored in **nanometres** and emitted as a shared Form XObject. Every mitochondrion on the page comes from here |
| `molecules.py` | **the ten most abundant molecules in the cells, at 1:1**, authored in **picometres** with real van der Waals radii and bond lengths, in a labelled box below the cells |

## 5. The biology, and where it is a simplification

Representative values, not measurements of a particular fruit. Both
figures are printed on the page beside the thing they describe, so the
drawing states its own assumptions.

- **Pulp** is parenchyma: thin primary wall, a vacuole filling most of the
  volume, cytoplasm squeezed to a peripheral band, the nucleus pushed
  against the wall — and the signature of an unripe banana, amyloplasts
  packed with large **concentrically layered starch grains with an
  eccentric hilum**.
- **Peel** epidermis is a brick under a waxy **cuticle**, its outer wall
  far thicker than its side walls, with **chloroplasts** (so: unripe;
  they become carotenoid chromoplasts as it ripens).
- Simplified on purpose: no ER network beyond a few strands, no cytosolic
  ribosomes, no membrane bilayers. **Organelle counts are illustrative** —
  a real section would show far more mitochondria.
- **The mitochondrion is the exception, and is not simplified.** §6.

## 6. The mitochondrion, the one thing on this page drawn in full

Every mitochondrion used to be an ellipse with a chord or three ruled
across it. That reads correctly at the zoom where the organelle is a few
pixels wide and is **wrong at every zoom below it**, because a crista is
not a chord. It is an invagination *of the inner membrane*, joined to it
through a narrow neck, and the space inside a crista is continuous with
the space between the two membranes — **not** with the matrix. Drawing
cristae as chords gets the compartment topology backwards, which is the
one thing a section view exists to show.

`mitochondrion.py` now draws, outside in:

| feature | size | first readable at |
|---|---|---|
| outer membrane | 5 nm | ~2 600 000 % |
| inner boundary membrane | 4.5 nm | ~2 600 000 % |
| crista junction (the neck) | 18 nm | ~4 000 000 % |
| intermembrane space | 24 nm | ~3 000 000 % |
| crista lumen | 26 nm | ~3 000 000 % |
| mitoribosome | 26 nm | ~3 000 000 % |
| matrix granule | 36 nm | ~2 000 000 % |
| mtDNA nucleoid | 210 nm | ~400 000 % |
| **ATP synthase F1 head** | **10 nm** | **~35 000 000 %** |

Three details are worth knowing, because they are the ones a diagram
usually gets wrong.

- **The inner membrane is ONE path.** Boundary arc, dive in to form a
  crista, round the blind end, come back out, continue along the
  boundary — a single closed curve for the whole organelle. Filling it
  with the matrix colour therefore leaves each crista lumen showing the
  intermembrane colour laid down beneath it, so the compartments come out
  right *by construction*, instead of by drawing the lumen as a separate
  object that could drift out of register with its own membrane.
- **A self-crossing at the junction is invisible until you fill it.** The
  first version dived in on the wrong lateral side, so the path crossed
  its own junction mouth twice — a bow tie. Every membrane was still
  exactly the right shape and the stroked outline looked perfect, but
  nonzero winding then filled the lumen with matrix colour: the same
  compartment error, one layer down and much harder to see. Catching it
  took a **160 000 000 %** render of a single crista, because at anything
  less the lumen is too narrow to read a colour from.
- **ATP synthase density follows the real gradient.** Dimer rows crowd the
  highly curved crista rims and faces — that curvature is largely *caused*
  by them — while the flat boundary membrane carries far fewer. Spaced
  11.5 nm on cristae and 62 nm on the boundary, so the gradient is
  visible rather than asserted.

All four populations share this module: the 14 in the pulp cytoplasm, the
3 in the skin cell, and the 325 beaded into the easter egg. Before, they
were three copy-pastes that had drifted — three cristae, one or two
cristae, and **none at all** in the skin cell. The beads in the letters
are smaller organelles now, not simpler ones.

**They are Form XObjects**, twelve of them, placed 342 times. Two reasons,
and the second is the one that made it non-optional:

1. **Size.** 342 inline copies of ~2 500 path operators would be megabytes
   of content stream on a page whose entire previous content was 53 kB.
2. **Precision.** A 10 nm feature is 2.8e-5 pt. Written as an absolute
   page coordinate near `x = 540` that needs eleven significant figures —
   past the five decimal digits ISO 32000-1 Annex C says a conforming
   reader need only honour. Inside a form it is the literal `5.000`, and
   one matrix per instance carries the magnitude.

A side effect worth having: the page is now a deep-zoom stress test of the
renderer's Form XObject path under an extreme CTM, not only of its path
rasteriser.

## 7. What this cost the renderer, and what got fixed

Building this page found a real defect in `pdfcer-render`, which is most of
the value of having built it.

**Every `Do` executed its form in full, however far off-screen it was.**
Rendering the deepest tier — a viewport 0.09 pt wide, holding parts of
maybe three organelles — decoded, parsed and rasterised all 342 of them,
then discarded ~700 000 paths against the clip. §8.10.1 makes `/BBox` a
*clip* on a form's contents, so a form whose transformed box misses the
viewport **cannot** contribute a pixel, and skipping it is exact rather
than approximate. `do_form` now culls on that and reports `forms_culled`
beside `forms` on the `render-page` metrics line.

**Where the cull sits is the whole point, and the first attempt got it
wrong.** Placed next to the `/BBox` clip — the natural-looking home — it
reported the *same counts* and bought almost nothing, because by then the
stream had already been sliced, inflated and parsed. Moved ahead of the
decode, the same tier went **802 ms → 120 ms**. A cull is worth only what
it skips, and the counter looked identical either way, which is exactly
why the wrong version was convincing.

**What was deliberately NOT done.** At page-fit zoom all 342 forms *are*
on the canvas, each about 1/70th of a pixel across, and pdfcer still
rasterises every path inside them — about 1.5 s. Skipping sub-pixel
geometry would fix that and would be **lossy**: those paths do contribute
anti-aliased coverage, and pdfcer does not silently trade fidelity for
speed. That is an operator decision, not an engineering one, and it is
left open rather than quietly taken.

## 8. The molecule box, and the units it is drawn in

Below the cells, at the end of a tapering dart of the same kind that
points at them, sits a **50 nm** box holding the ten most abundant
molecules in a banana cell — at the same 1:1 scale as everything else.

```
banana                 153 mm
water molecule           0.37 nm
ratio             412 585 829 : 1
```

`molecules.py` authors in **picometres**, because that is the unit bond
lengths and atomic radii are published in, and using the published unit
means every number in the file can be checked against a reference table
without arithmetic: O–H is `96`, C–C is `154`, oxygen's van der Waals
radius is `152`, potassium's ionic radius is `138`.

**Space-filling, not ball-and-stick, and that is a correctness call.** A
stick model of glucose is mostly empty space, and this box exists to say
how big these things are; the van der Waals envelope IS the molecule's
size. So every atom is drawn at its full radius and every label quotes
the envelope. The cost is stated rather than hidden: the rings come out
as lumpy blobs, and connectivity is sacrificed. Bonds are drawn
underneath as sticks so they still show in the gaps.

**Why water's label says 0.37 nm and not the 0.28 nm everyone quotes.**
Both are right and they measure different things. 0.28 nm is water's
*kinetic* diameter — the hole it can squeeze through, which is what
membrane and zeolite work cares about and therefore what is in every
table. 0.37 nm is how much space the molecule actually occupies. This box
draws at 1:1 and invites you to measure it, so a label quoting a number
the drawing disproves would be worse than useless.

**Every molecule's label carries its share of the fruit**, and the
ranking follows from those numbers rather than the other way round:

| | | | | |
|---|---|---|---|---|
| 1 water 74.9 % | 2 starch 5.4 % | 3 glucose 5.0 % | 4 fructose 4.9 % | 5 sucrose 2.4 % |
| 6 cellulose 1.2 % | 7 protein 1.1 % | 8 pectin 0.7 % | 9 malic acid 0.4 % | 10 potassium 0.36 % |

The ten come to **96.4 %** of a banana's fresh mass; the missing 3.6 % is
fat, ash, hemicellulose, the other organic acids and everything present in
milligrams.

★ **These are RIPE figures, and an unripe banana is a completely
different list** — green pulp is roughly 20–25 % starch and 1–2 % sugars,
so ripening drops starch by a factor of four while glucose and fructose
rise by a factor of ten. Rows 2 through 5 are the same carbon, before and
after. That leaves a seam inside this document, disclosed rather than
tidied away: the banana at the top of the page is drawn **yellow**, which
these figures match, while the two cells are drawn with green
chloroplasts and packed starch grains, which is an **unripe** section.
Both are deliberate — the starch grains are what make the pulp cell
recognisably a *banana* — and the box's subtitle names which state its
numbers describe, so a reader can see the seam instead of tripping over
it.

★ By molecule **count** the ranking is not close and not interesting —
water is about 99.5 % of them, and everything else competes for the
remaining half percent. The box says which measure it is using, because a
"top ten" with no unit is a claim with no content.

Four of the ten are polymers with no single size at all. Each is drawn as
a representative segment or cross-section and its label names the
dimension being quoted — "1.3 nm helix", "3.5 nm fibril" — rather than
pretending a chain has a diameter. Cellulose is drawn in **cross
section** because lengthwise it would run off the box, off the page, and
out of the building.

**The labels are bigger than the things they label**, by up to 22:1 —
"10 potassium ion" is 6.3 nm of text over a 0.28 nm ion, and the column
pitch of the whole box is set by the text, not the chemistry. That is the
same finding §3 records about the arrowhead, arrived at independently
four orders of magnitude further down.

## 9. How the ceiling was found — history, not current behaviour

★ **This section is the INVESTIGATION: what the limits were, how each was
measured, and how two of the three were confirmed by accident.** pdfcer no
longer behaves this way — `Pass 74.7` (2026-08-23) took these limits, and
§10 is what it does now. Nothing here is a live limitation.

The measurement table below shows before and after side by side, and a
`correct` column, because a before-figure alone says nothing about whether
it moved and because `correct` is not what anyone would guess.

---

**A note on how this section was written, kept short on purpose.**

Several rounds of correction ran against this file, and between them they
turned up wrong **values**; wrong **pointers** (*"the table above"*);
wrong **quantifiers** (*"everything in this section"*); and an
**unwritten antecedent** (*"the sentence above it"* — opening the very
paragraph that explained why such pointers fail). The later rounds
produced fresh instances of the earlier kinds, so the list is of shapes
rather than of stages.

*(No ordinals, deliberately. An earlier draft numbered the rounds and
disagreed with `ROADMAP.md`'s numbering of the same commits two sentences
later. **An ordinal is a claim; a sequence is not** — and this note's
whole subject is claims nobody checks.)*

★ **Every one of those defects lived in a sentence ABOUT a previous
correction, not in the engineering content.** The substance — the
arithmetic, the measurements, the three `f32` limits — was correct from
the start and never moved. What kept breaking was the commentary layer
each round added to explain the round before it: a running annotation of
its
own edit history, in which each new sentence was an unchecked claim about
a set, a position, or a count.

⇒ **So that layer is deleted rather than corrected again.** The previous
version of this passage carried four paragraphs of it and had three live
defects in them at the time of writing — including a table described as
having *"three post-fix columns"* when it has two, the third item
enumerated being a prose note.

**The general form, which is the only part worth carrying elsewhere:** *a
document that annotates its own corrections has made its correction
history part of its content, and that part has no tests, no measurements
and no reader who would notice.* It generates defects at a steady rate and
each repair adds another sentence to the generator. `ROADMAP.md` and
`SESSION_LOG.md` are where this project's edit history belongs; they are
append-only, dated, and read by someone looking for exactly that. This
file is for the artefact.

The substantive lessons the rounds produced are kept — in `R214`, `R215`,
and `D:/dev/rag/rust/`, where they are one claim each and get swept.

---

### The ceiling that was claimed, and the one that bit

`Pass 74.1`/`74.2` pushed the deep-zoom ceiling past a **trillion
percent**. That claim is true and it is about the **viewport**: the
region rectangle, its corners, and the base CTM are computed in `f64` and
survive being multiplied by 1e12.

**It was never a claim about page-space geometry, and this box is what
made the difference measurable.** Two `f32` limits sit under the content:

1. **Path coordinates.** A point near `x = 540` has an `f32` spacing of
   `6.1e-5 pt`, which is **21.5 µm**. Any feature smaller than that,
   written as an absolute page coordinate, is quantised away. This is why
   everything small on this page lives in a Form XObject with small local
   coordinates — the mitochondria in nanometres, the molecules in
   picometres — and it is not a workaround but the only representation
   that works.
2. **The placement matrix.** Concatenating a `cm` that carries a page
   coordinate leaves the CTM's translation as the difference of two large
   nearly-equal `f32` values. The drift is about

   ```
   page_x × scale / 16 700 000   pixels
   ```

   which is ~5 px at the mitochondrion tier (invisible), ~400 px at the
   molecule-box tier (the box lands off-centre but internally perfect,
   because every part of it drifts together), and past the viewport above
   roughly `scale = 5e6`.

Measured, on the box's own molecules, framing a 1600 px viewport on the
water molecule:

| scale | before | after | correct |
|---|---|---|---|
| 2 000 000 | 11 | 11 | 11 |
| 5 000 000 | 11 | 11 | 11 |
| 12 500 000 | 7 | **2** | **2** |
| 25 000 000 | 3 | **2** | **2** |
| 50 000 000 | 1 (the box only) | **2** | **2** |

★ **The "correct" column is not `11` all the way down, and this table
originally implied it was** — its header read *"forms rendered of 11"*,
which quietly makes 11 the target at every row. It is not. Above
`1.25e7` the viewport is small enough that only the box and **one**
molecule are in it, and `Pass 74.4`'s exact `/BBox` cull removes the other
nine *by design*. A fix that produced 11 there would have been a
regression, so the obvious-looking acceptance criterion was one that only
a broken build could meet. See standing rule `R215`.

Confirmed **not** to be the Form XObject path and **not** the `Pass 74.4`
cull: a synthetic page with the same square drawn three ways — absolute
coordinates, a `cm` translation, and a scaled `cm` — loses all three at
the same magnification, including the one that uses no `cm` and no form.

★ **Confirmed a second way, by accident, which was the more convincing of
the two.** The molecule box rendered off-centre in its frame, so the
obvious move was to nudge `--region` by the observed offset. It did
**nothing** — two successive nudges produced byte-identical framing, the
box's bounds landing on the same pixels each time.

`--region` is parsed as `f64` and stays `f64` through
`region_base_geometry_of`, so the viewport really did move. What did not
move is the *content*: its device translation is
`540 × scale` (huge, rounded to its own `f32` ulp) plus a small offset,
and at `scale = 8.1e6` that ulp is **512 px**. A 76-pixel correction is
swallowed whole by the rounding of the sum. So the content's position is
**quantised in ~500 px steps** at this magnification — which is the same
drift stated as a resolution rather than as an error, and is why the
saved render of the box sits up and to the right in its frame instead of
centred. That framing is evidence, not sloppiness, and it is left as it
came out.

## 10. Where the ceiling is now

★ **This section is what pdfcer does NOW.** `Pass 74.7` (2026-08-23) took
the limits §9 measured; `Pass 74.10` closed the second rendering path
behind it. Where a before-figure appears here it is labelled as one, since
a current number alone says nothing about what changed.

**The nudged coordinates are gone.** §9's framing came from a region
hand-corrected twice against the drift; the box now lands within **one
pixel** of where the arithmetic says, so the region can just be computed:

```
pdfcer render-page banana-at-scale.pdf --page 1 --scale 13749133 \
  --region "539.9999200,558.8519152,540.0000800,558.8520218" -o box.png
```

Measured on exactly that command: the box's centre falls at pixel
`(1100, 732)` of a 2200 × 1467 raster, against a geometric centre of
`(1100, 733)`. Before the fix, the same computed region put it **76 px out
horizontally and 288 px vertically** — which is where the "just nudge it"
instinct came from, and why the nudge not working was the clue.

**What changed.** The CTM is carried in `f64` through content-stream
composition and narrowed only at the leaf, which fixes limit 2. Limit 1 is
fixed separately and only where it bites: past a magnitude threshold the
interpreter builds each path **relative to its own first point**,
differencing in `f64`, so a page coordinate never has to survive being
narrowed. Ordinary rendering takes neither route and pays nothing.

**A single water molecule now renders sharply at a scale of `1.6e9` —
about 190 billion percent.**

★★ **The count in §9's measurement table goes to `2`, not to `11`** — and
`11` is worth stating, because it is the number this Pass was originally
given as its acceptance criterion.

**Two** is correct from `1.25e7` upward: the box, plus the one molecule
actually in frame. The other nine are removed by `Pass 74.4`'s exact
`/BBox` cull, which is not a defect and does not go away. So *"11 of 11"*
was never achievable, and a change that produced it would have been a bug.

That criterion was written by reading the broken system's output and
assuming the whole of it was the defect. Standing rule `R215`.

★ **The part nobody predicted: the same change made deep zoom 23× faster.**
A stroke-heavy CAD region at 100 000× went from **31 s to 1.3 s**.

The figure `93 s` appears elsewhere for the same region and is **not** the
baseline: it is the **rejected** device-space attempt, which was three
times slower than doing nothing. Quoting `93 → 1.3` gives `71.5×`, which
measures the shipped code against a discarded draft rather than against
what was there before. The honest ratio is **23.8×**.

Deep zoom was never slow *because* it was imprecise — it was slow for the
*same reason*: large magnitudes reaching a rasteriser that flattens curves
to a tolerance measured in the path's own units. One cause, two symptoms,
and only the precision one was ever attributed correctly.

## 11. The easter egg, and the arithmetic that said it would fit

Inside the pulp cell's vacuole: a heart drawn from mitochondria, with
`KEN ♥ EMILY` inside it also drawn from mitochondria — the ♥ is itself a
little heart of mitochondria — and an anniversary line beneath.

The question was whether mitochondria are too big to fit it. They are not,
and it is not close:

```
cell          300 µm            = 200 mitochondria wide
outer heart   140 × 125 µm      ≈ 190 mitochondria around its curve
capitals      12 µm tall        ≈ 8 mitochondria per vertical stroke
"KEN ♥ EMILY" 9 glyphs, 81 µm   ≈ 210 mitochondria
```

★ **The binding constraint is legibility, not space.** A 12 µm capital
built from 1.5 µm beads has about eight per stroke — enough to read, not
enough to look smooth. Below that the letters become dotted lines, which
is why the anniversary text underneath is ordinary type: at 7 µm a beaded
letter would be four mitochondria tall and stop being a letter.

The starch grains were moved into a ring around the vacuole to clear the
centre. That is the only change the egg required, and it costs nothing
biologically — grains cluster wherever they cluster.

Every bead in the heart and the letters is now a **fully detailed
mitochondrion** (§6), with its own cristae, junctions, ATP synthase and
nucleoid. Three interior variants and a mirror give six apparent
organelles before the pattern repeats, which at this bead pitch is far
enough apart that the eye reads tissue rather than a stamp.
