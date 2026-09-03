# pdfcer-core consumer API — Part 3: the feature capabilities

> **★ `crates/pdfce-gui/...` citations.** That crate was removed from this
> workspace in `Pass 247.0`. Every `pdfce@cce414e:crates/pdfce-gui/...`
> reference below is a *reference implementation* frozen at the last commit
> that carried it — read it with
> `git -C D:\Dev\pdfce show cce414e:crates/pdfce-gui/src/<file>` (the
> untouched backup repository) or on GitHub at `KenM76/pdfce` (archived). The shipping
> GUI is the separate `pdfcer-gui` project.

**Audience.** An engineer or agent building a new GUI shell at
`D:\dev\pdfcer-gui` against this workspace's crates, in a different session,
with no way to ask questions here. This is not rustdoc. It answers one
question per capability: *I want to build the panel for feature X — what do
I call, in what order, what must I SHOW the operator, and what will bite me?*

**Date:** 2026-08-13 · **Verified against commit:** `5c37c7c` (`main`).
Every symbol below was read out of the tree at that commit and is cited
`path:line`. Anything that could not be verified is marked
`UNVERIFIED — <what to check>`; treat those as questions, not facts.

**What this part covers**

| § | Capability | crate / module |
|---|---|---|
| 1 | **ce dimensions** — groups, scale/units, the `Pass 69.0` style cascade, `Pass 69.1` tolerance, authoring, the `/PieceInfo` sidecar | `pdfcer_core::dimension`, verbs on `EditSession` |
| 2 | **Forms (AcroForm)** — read, fill, flatten, author, data interchange, script census | `pdfcer_core::{forms, forms_author, fdf, formcsv, form_script, richtext, vartext}` |
| 3 | **Annotations & markup** | `pdfcer_core::{annot, annot_author}` |
| 4 | **Redaction** | `pdfcer_core::redact` |
| 5 | **OCR substrate** (no engine wired) | `pdfcer_core::ocr` |
| 6 | **Print & imposition** | `pdfcer-print` |
| 7 | **Rasterising a page for display** | `pdfcer-render` |

**What this part does NOT cover — go to the sibling documents**

- Opening a file, the object model (`Object`, `Dict`, `ObjId`), the page
  tree, xref recovery, `DocumentView` → **`01-reading-and-model.md`**.
- `EditSession` mechanics, the command log, undo/redo, incremental vs.
  full-rewrite save, `SaveOptions` → **`02-editing-and-saving.md`**.

Those two are referenced by name throughout and are never re-explained here.
Every mutating call below goes through an `EditSession` and produces exactly
one undo entry unless stated otherwise.

**Terminology (project `CLAUDE.md` rule 15) — binding on any shell that
renders these words.**

- **ce dimension** — a dimension **pdfcer authors**: a `/Line` +
  `/IT /LineDimension` annotation with a baked `/AP`, its group, its
  `/Measure` dict and its `/PieceInfo` sidecar record. Authored, editable,
  deletable, re-measurable. Everything in §1.
- **pdf dimension** — a dimension already in the file, exported by CAD or
  another authoring tool. Ordinary page content. pdfcer reads it and measures
  against it and **must not silently alter it**.

Never write a bare "dimension" in the shell's own UI strings, error text,
logs or docs. The distinction is **provenance**, not representation.

**The rule that shapes this document — project rule 4, "fuzzy, never
sneaky".** Anything pdfcer **inferred** — a best-fit circle and its residual,
a snapped point, a near-parallel classification, an auto-detected form field,
an OCR word and its confidence, a substituted font, a reflow that overflows —
must be **disclosed** rather than applied silently. Each capability below
carries a mandatory **"★ What the UI must disclose"** section. A shell that
skips one ships a rule-4 violation and will not know it has.

> **What rule 4 does and does not demand (decision 059).** It is a rule about
> **disclosure**. It is not a rule about widgets, and it is emphatically not a
> licence to mark up the page.
>
> - **The commit point is SAVE.** Undo rejects, Save commits. Nothing in an
>   open `EditSession` is document state, so an inference already applied in
>   the session needs nothing in front of it. The session **is** the preview.
>   (`ARCHITECTURE.md` §11.1 is what makes this safe rather than merely
>   convenient: the dirty set is a save-time **diff against the base
>   revision**, so an undone inference cannot reach the saved bytes.)
> - **Applied content renders exactly as saved content will render.** Typing
>   reflows live and looks normal; OCR looks normal the moment the command
>   completes. **No badge, tint, red flag, dashed outline or "provisional"
>   layer drawn into the page view.** Operator, 2026-08-13: *"the nagging and
>   red flagging in the original GUI made for a lot of extra bugs in the
>   visibility when editing"* — provisional styling is a **second rendering
>   path for the same content**, and two paths drift.
> - **Disclosure lives off-canvas**: a status line, a results panel, a
>   post-command report, a properties field. Never blocking, never requiring
>   acknowledgement, never positioned relative to the document (that was the
>   earlier 2026-08-05 complaint, decision 024 §4.4).
> - **The distinction that keeps this workable:** a **pre-commit affordance**
>   is not content marking. A snap indicator, a hover highlight, a rubber-band,
>   a selection handle — these are the *cursor*, they describe what is about to
>   happen, and they are welcome. What is forbidden is styling content that has
>   **already been applied** as though it were pending.
> - **The half that survives, and it is the point of the rule:** inferences
>   the operator **cannot see** still owe an off-canvas report — invisible OCR
>   text at render mode 3, a plausible-looking font substitution, a best-fit
>   residual, an over-eager snap. **Render normally; report separately. Both.**
>
> One-line test: *would a screenshot of the editing canvas differ from a
> screenshot of the same document saved and reopened?* If yes, and the
> difference is pdfcer marking its own uncertainty, that is the defect.

**Reading the core/cli/gui state.** Taken from `docs/FEATURES.md` at this
commit. `gui [ ]` is **not** a missing feature — it is a capability whose
model and CLI are finished and whose *only* missing piece is the surface the
new shell exists to build. Those rows are flagged as **opportunities**.

---

## 1. ce dimensions

`core [x] · cli [x] · gui [x]` for authoring, groups, scale, radius/diameter
toggle, reposition, layer toggle, delete, and the two-line gesture.
**`core [x] · cli [x] · gui [ ]`** for the **style cascade** and for
**tolerance** — `FEATURES.md:103-104`. Those two rows are the single largest
ready-made opportunity in this document: the model and the CLI do all of it,
and `docs/ui_specs/tool-options-dock-and-ce-dimension-properties.md`
**Amendment B (2026-08-13)** is a written, current design for the missing
panel. Read it before designing anything; do not re-derive it.

**ui_specs to read**

| file | what it settles |
|---|---|
| `docs/ui_specs/pass-12.M2-dimension-tools.md` | the original tool interaction spec: scale entry, the snap surface, the fit-residual disclosure |
| `docs/ui_specs/tool-options-dock-and-ce-dimension-properties.md` | where the property surface lives; §C.11.1 the group-vs-per-ce-dimension panel; **Amendment B** = the shipped cascade and what the GUI still owes |
| `docs/ui_specs/pass-68.0-two-line-ce-dimension-gesture.md` | the two-line pick gesture, including the parallel override and the virtual apex |

### 1.1 What it can do today

- Author a ce dimension of three kinds: **linear** (with an axis
  constraint), **circular** (best-fit circle, displayed as radius *or*
  diameter — one geometry, a display flag), and **angular** (`Pass 68.0`).
  `crates/pdfcer-core/src/dimension/group.rs:153`.
- Named **groups** carrying scale, number format (decimal / fraction /
  feet-inches), decimal marker, drafting standard (ANSI/ISO), an OCG layer,
  and a group-tier style. `group.rs:50`.
- A **tri-state scale**: never-set / explicit 1:1 / calibrated
  (`units.rs:423`). Never-set means measurements display raw page units *and
  say so*.
- The **style cascade** — factory → group → ce dimension, **per property**
  (`style.rs`), with provenance readable per property.
- **Tolerance** — symmetric, deviation, limit, basic (boxed), min, max — as
  the tenth and eleventh properties of that same cascade (`tolerance.rs`).
- Author from **two picked lines**: parallel ⇒ linear, angled ⇒ angular,
  collinear ⇒ refused by name (`two_lines.rs`).
- Reposition (drag or numeric), toggle the group's layer, delete
  (annotation + `/AP` + sidecar record together).

### 1.2 What it cannot do today

- **No per-ce-dimension scale.** Deliberate, and a refusal rather than an
  omission — `style.rs:67-72`. Scale lives on the group.
- **The ISO 286 fit classes are not implemented** —
  `swTolFIT`/`swTolFITWITHTOL`/`swTolFITTOLONLY`, plus block and general
  tolerance. Reason stated at `tolerance.rs:36-41`: the reference RAG flags
  its own class list as `UNVERIFIED`, and a wrong `H7/g6` deviation is a
  manufacturing defect. `FEATURES.md:215`.
- **Re-measure a placed ce dimension** (change what it measures, keep id /
  group / placement) — planned, not built. `FEATURES.md:218`.
- **Drag a ce dimension's extension lines** — planned. `FEATURES.md:217`.
- **Select/delete a ce dimension from the canvas** — planned;
  `EditSession::dimension_rects` gives you the hit rectangles today but the
  selection model is not there. `FEATURES.md:214`.

### 1.3 Entry points and the types that flow

**Read (no mutation).**

| call | returns | `file:line` |
|---|---|---|
| `EditSession::dimension_model(&self) -> DimensionModel` | the whole model, cloned out of the `/PieceInfo` sidecar | `crates/pdfcer-core/src/edit.rs:15361` |
| `EditSession::dimension_rects(&self, page_index) -> Vec<(DimensionId, [f64;4])>` | hit rectangles for the canvas, filtered by the annotation's own `/P` | `edit.rs:15645` |
| `EditSession::dimension_groups_on_page(&self, page_index) -> Vec<GroupId>` | which groups have members on this page | `edit.rs:15725` |

`DimensionModel` (`dimension/group.rs:586`) is the authoritative model:
`groups()` / `dimensions()` / `group(id)` / `dimension(id)` /
`members(group)` / `member_count(group)` / `display(id)`
(`group.rs:630-780`). It is a **snapshot**; mutating it does not touch the
document. Every persistent change goes through an `EditSession` verb below.

**Mutate — every one of these is one undo entry.**

| call | returns | `file:line` |
|---|---|---|
| `add_dimension(page_index, group: GroupId, kind: DimensionKind)` | `Result<(ObjId, DimensionId), EditError>` | `edit.rs:15380` |
| `add_dimension_group(name: &str, unit: Unit)` | `Result<GroupId, EditError>` | `edit.rs:15523` |
| `set_group_scale(group, scale: ScaleState, format: NumberFormat)` | `Result<usize, EditError>` — members **regenerated** | `edit.rs:15549` |
| `set_group_standard(group, …)` | `Result<usize, EditError>` | `edit.rs:15983` |
| `set_group_style(group, style: GroupStyle)` | `Result<usize, EditError>` — members **regenerated** | `edit.rs:16052` |
| `set_dimension_style(dimension, style: StyleOverrides)` | `Result<usize, EditError>` — always this one member | `edit.rs:16115` |
| `set_dimension_display(dimension, show_diameter: bool)` | `Result<(), EditError>` — circular only | `edit.rs:15921` |
| `place_dimension(dimension, offset: f64, text_along: f64)` | `Result<(), EditError>` | `edit.rs:15804` |
| `move_dimension(dimension, dx, dy)` | `Result<(), EditError>` | `edit.rs:16779` |
| `toggle_dimension_layer(group, visible: bool)` | `Result<bool, EditError>` | `edit.rs:15600` |
| `delete_dimension(dimension)` | `Result<(), EditError>` — annotation + `/AP` + sidecar together | `edit.rs:16178` |

**Pure helpers a panel calls without an `EditSession`** (all in
`pdfcer_core::dimension`, re-exported at `dimension/mod.rs:77-97`):

- `resolve_style(&Group, &StyleOverrides) -> DimensionStyle` — `style.rs:479`
- `style_provenance(&Group, &StyleOverrides) -> StyleProvenance` — `style.rs:526`
- `preview_group_scale(ScaleEntry) -> Option<ScalePreview>` — `units.rs:660`
- `format_measurement(points, ScaleState, NumberFormat) -> MeasurementDisplay` — `units.rs:499`
- `format_angle_degrees(degrees, NumberFormat) -> String` — `units.rs:558`
- `parse_length(&str, default_unit: Unit) -> Result<ParsedLength, LengthParseError>` — `length_parse.rs:139`
- `fit_circle_taubin(&[Point]) -> Option<FitCircle>` / `fit_circle_taubin_refined` — `fit.rs:109`, `fit.rs:131`
- `author_from_two_lines(&PickedLine, &PickedLine, ParallelPolicy, TwoLinePlacement) -> Result<TwoLineAuthoring, TwoLineRefusal>` — `two_lines.rs:248`
- `author_dimension(&DimensionKind, DimensionStyle) -> AuthoredDimension` — `author.rs:279`; the `/AP` baker. `AuthoredDimension::label` (`author.rs:240`) is **the exact string baked into the page** — see trap (b).

Geometry the canvas needs comes from `pdfcer_core::vector`:
`decompose_page(&DocumentView, &Page, Matrix) -> Result<PageObjects, ContentError>`
(`vector/decompose.rs:1293`), then
`snap_candidates(Point, &SnapConfig, &PageObjects) -> Vec<SnapCandidate>`
(`vector/snap.rs:449`) and
`linepick::pick_line_in_page(&PageObjects, Point, tolerance) -> Option<PickedLine>`
(`vector/linepick.rs`).

**★ The signature is unchanged but `PickedLine` IS NOT**, as of `Pass 138.0`:
its first field went from `object_index: usize` to `target: HitTarget`,
because the picker now searches form-XObject contents as well as the page's
own objects and a `usize` cannot name a leaf. Full account, including why the
migration helper returns an `Option` rather than a sentinel, in
`01-reading-and-model.md` §10.5. **This is the one breaking change in that
Pass; everything else is additive.**

### 1.4 Minimal worked sequences

**(a) Calibrate a group, then author a linear ce dimension.** This is the
shape the integration test uses —
`crates/pdfcer-core/tests/dimension_roundtrip.rs:113-126`.

```rust
use pdfcer_core::dimension::{
    DEFAULT_GROUP_ID, DimensionKind, NumberFormat, ScaleEntry, ScaleState, Unit,
    preview_group_scale,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::{AxisConstraint, Point};
use pdfcer_core::writer::SaveOptions;

fn calibrate_and_dimension(bytes: Vec<u8>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut session = EditSession::new(Document::from_bytes(bytes)?);

    // 1. PREVIEW the scale. Pure arithmetic, no mutation — this is what the
    //    operator sees before he accepts (ui-spec §4.2/§4.5).
    let preview = preview_group_scale(ScaleEntry::RealLength {
        drawn_pdf_length: 42.3,
        real_length: 25.0,
        unit: Unit::DecimalFeet,
    })
    .ok_or("degenerate scale entry — show nothing to accept")?;
    // preview.scale, preview.unit, preview.ratio_label are the disclosure.

    // 2. Commit it. Every wired member's baked /AP is regenerated.
    let _regenerated = session.set_group_scale(
        DEFAULT_GROUP_ID,
        ScaleState::Calibrated { scale: preview.scale },
        NumberFormat::decimal(preview.unit, 2),
    )?;

    // 3. Author. Additive: /Line + /IT /LineDimension + baked /AP + /OC to the
    //    group's OCG + /Measure mirror + /PieceInfo sidecar, ONE undo entry.
    let (_annot_id, _dim_id) = session.add_dimension(
        0,
        DEFAULT_GROUP_ID,
        DimensionKind::Linear {
            a: Point::new(100.0, 200.0),
            b: Point::new(300.0, 200.0),
            constraint: AxisConstraint::Horizontal,
            offset: 0.0,
            text_along: 0.0,
        },
    )?;

    Ok(session.to_incremental_bytes(&SaveOptions::identity())?.0)
}
```

**(b) Author from two picked lines, with the disclosure the gesture owes.**
Mirrors `pdfce@cce414e:crates/pdfce-gui/src/measure_tool.rs:459-469`.

```rust
use pdfcer_core::dimension::{
    DEFAULT_GROUP_ID, TwoLinePlacement, TwoLineRefusal, author_from_two_lines,
};
use pdfcer_core::vector::linepick::{ParallelPolicy, PickedLine, measured_angle_degrees};

struct Disclosure {
    measured_angle_degrees: Option<f64>,
    forced_parallel: bool,
    apex_is_real: Option<bool>,
    is_linear: bool,
}

fn two_line_preview(
    a: &PickedLine,
    b: &PickedLine,
    epsilon_degrees: f64, // Settings::parallel_epsilon_degrees — NEVER a literal
    operator_ticked_treat_as_parallel: bool,
) -> Result<(pdfcer_core::dimension::DimensionKind, Disclosure), TwoLineRefusal> {
    let mut policy = ParallelPolicy::from_setting(epsilon_degrees);
    if operator_ticked_treat_as_parallel {
        policy = policy.forcing_parallel();
    }
    let authored = author_from_two_lines(a, b, policy, TwoLinePlacement::default())?;

    // Everything the shell must SHOW comes back with the result. Do not
    // re-measure in the shell: a shell-side re-measure is how a disclosure
    // comes to contradict the ce dimension it describes (two_lines.rs:44-53).
    let d = Disclosure {
        measured_angle_degrees: authored.measured_angle_degrees,
        forced_parallel: authored.forced_parallel,
        apex_is_real: authored.apex_is_real(),
        is_linear: authored.is_linear(),
    };
    let _ = measured_angle_degrees; // available separately, for the pre-pick hint
    Ok((authored.kind, d))
}
// …then, only after the operator commits:
//   session.add_dimension(page_index, DEFAULT_GROUP_ID, kind)?;
```

**(c) Render the inherited-vs-overridden state of one ce dimension's style
— the `gui [ ]` panel.**

```rust
use pdfcer_core::dimension::{StyleOverrides, StyleSource, resolve_style, style_provenance};

fn style_rows(session: &pdfcer_core::edit::EditSession, dim: pdfcer_core::dimension::DimensionId) {
    let model = session.dimension_model();
    let record = model.dimension(dim).expect("live id");
    let group = model.group(record.group).expect("every record has a group");

    let resolved = resolve_style(group, &record.style);   // the VALUES the /AP is drawn with
    let prov = style_provenance(group, &record.style);    // WHICH TIER supplied each

    for (name, source) in prov.each() {                   // fixed-size [_; 11]
        let checkbox_ticked = source == StyleSource::Dimension;
        let will_move_on_a_group_edit = source.follows_group(); // TRUE for Factory too
        let _ = (name, checkbox_ticked, will_move_on_a_group_edit, &resolved);
    }
}

// Clearing an override restores INHERITANCE, not the frozen value:
fn clear_arrow_form_override(over: &mut StyleOverrides) { over.arrow_form = None; }
```

`set_group_style` / `set_dimension_style` are **read-modify-write**: take
the current struct, change one field, pass the whole thing back
(`edit.rs:16030-16034` explains why there is no per-property setter).

### 1.5 ★ What the UI must disclose

**Everything a ce dimension shows is derived** — from geometry pdfcer fitted,
a scale the operator calibrated, and a cascade pdfcer resolved. Six concrete
obligations:

1. **The best-fit circle's residual.** `FitCircle::residual`
   (`fit.rs:64`) is the RMS distance of the picked points to the fitted
   circle, in page units. `fit.rs:56-65` and decision 011 §2.3 require it be
   **surfaced always, never on request** — it is the number that says whether
   the fit is trustworthy. Show it with the centre and radius *before*
   `add_dimension` is called. A circular ce dimension placed without the
   residual on screen is a rule-4 violation.

2. **Raw page units, verbatim.** When the group's scale is
   `ScaleState::NeverSet`, `format_measurement` returns
   `MeasurementDisplay { raw_page_units: true, .. }` (`units.rs:462`) and the
   shell must render the constant `NO_SCALE_DISCLOSURE` —
   `"no scale set — showing raw page units"` (`units.rs:471`) — **verbatim,
   not paraphrased**. The string lives in core precisely so shells cannot
   invent their own wording.

3. **The scale preview before the commit.** `preview_group_scale` is the
   pure sibling of `set_group_scale` (`units.rs:660`). Show
   `ScalePreview::scale`, `unit` and `ratio_label` before mutating; a
   calibration accepted unseen silently rescales every existing member of the
   group.

4. **The two-line classification, including the angle it overrode.**
   `TwoLineAuthoring::measured_angle_degrees` is populated **even when
   `forced_parallel` is true** — `two_lines.rs:139-147` states this is
   deliberate: *"a checkbox that hides the number it is overriding is
   withholding the fact that makes the decision a decision."* So the panel
   must read, in words, something like *"0.8° apart — read as parallel
   because you asked"*. And `apex_is_real() == Some(false)` means the two
   lines only meet if extended — ordinary in CAD, **not refused**, but a fact
   the operator may not have noticed (`two_lines.rs:158-171`). Say it.

5. **A derived snap point is a guess; the rest are facts.**
   `SnapKind::is_derived()` (`vector/snap.rs:234`) is `true` for exactly one
   variant, `DerivedCenterline` — the inferred midline of a filled thin quad.
   Render it with a distinct glyph and gate it behind an extra confirm.
   Every other `SnapKind` is a deterministic fact about geometry already on
   the page and needs no confirm. `snap_candidates` returns the list sorted
   by (priority, distance) so index 0 is the default and the shell offers a
   cycle (Tab) through the rest — returning only a winner would make the
   override impossible (`snap.rs:248-258`).

6. **Which tier supplied each style value, and what a group edit will move.**
   See trap (a) below. This is the whole point of the `gui [ ]` panel.

**Refusals must be surfaced by name, never swallowed.**
`TwoLineRefusal::Collinear` and `::Degenerate` carry `thiserror` messages
written for an operator (`two_lines.rs:112-122`). `ToleranceError`'s
`Display` likewise (`tolerance.rs:133-144`) — e.g.
*"a symmetric tolerance's magnitude must not be negative (write ±0.1, not
±-0.1)"*. Nothing is clamped, swapped or absolutised;
`Tolerance::validate` (`tolerance.rs:180`) refuses and says why, because *"a
corrected value the operator never saw is exactly the sneaky case"*
(`tolerance.rs:179`).

### 1.6 Traps

**★ Trap (a) — the style cascade is per-property, and `follows_group()` is
`true` for `Factory`.**

The cascade is factory → group → ce dimension, **independently for each of
eleven properties** (`style.rs:1-16`). An `Option<T>` per property *is* the
operator's override checkbox: `None` = clear = inherit, `Some(v)` = ticked.

`style_provenance()` (`style.rs:526`) exists so a panel can **render** the
inherited-vs-overridden state instead of recomputing it, and
`StyleSource::follows_group()` (`style.rs:378`) is the predicate the panel
actually wants — *"will a group edit move this?"*

```rust
pub const fn follows_group(self) -> bool {
    matches!(self, Self::Factory | Self::Group)
}
```

**`Factory` counts as following the group.** A property nobody has set yet
*will* move when the group sets one — the group simply has not spoken. A
panel that derives the predicate by hand and tests only for `Group` will grey
out rows that are about to change, and the test that pins this exists:
`style.rs:643` `factory_sourced_properties_still_follow_a_group_edit`.

Four further points a panel gets wrong if it guesses:

- **Four properties can never report `Factory`.** `unit`, `fraction`,
  `decimal_marker` and `standard` have a *concrete* field on `Group`, not an
  `Option`, so their provenance is `Group` or `Dimension` and nothing else
  (`style.rs:527-539`). Saying "factory" for them "would be a lie an operator
  could act on."
- **`StyleProvenance::each()` returns a fixed-size `[(&'static str,
  StyleSource); 11]`** (`style.rs:423`) so a loop gets a **compile error**,
  not a silently short list, when a twelfth property lands.
  ⚠️ `docs/ui_specs/…-ce-dimension-properties.md` **Amendment B §B.2 says
  "nine"** — that text predates `Pass 69.1` adding `tolerance` and
  `tolerance_places`. The code at `style.rs:423` is authoritative: **eleven**.
  Same for §B.4's "the same nine properties".
- **Clearing an override restores inheritance**, live and in both
  directions — deliberately unlike the reference tool, whose `DeleteStyle`
  leaves the attributes frozen into the annotation (`style.rs:269-278`).
- **`Some(Tolerance::None)` ≠ `None`.** The first overrides a group default
  *with* "no tolerance"; the second inherits the group's. A group that
  tolerances everything and one feature that must not be toleranced is a real
  drawing and cannot be expressed if the two collapse (`style.rs:299-306`).

**★ Trap (b) — a limit tolerance SUPPRESSES the nominal; it does not print
beside it.**

`Tolerance::suppresses_nominal()` (`tolerance.rs:169`) is `true` for exactly
`Tolerance::Limit { upper, lower }`. The `/AP` baker branches on it
(`author.rs:297-309`):

```rust
let label = if style.tolerance.suppresses_nominal() {
    style.tolerance.caption(format, tol_places)          // "50.20/49.90" — nominal GONE
} else {
    format!("{}{}{}", kind.caption_prefix(), display.text,
            style.tolerance.caption(format, tol_places)) // "⌀50.00 ±0.10"
};
```

A panel that previews *"nominal + tolerance"* by concatenation **will
disagree with the bytes in the page** for every limit tolerance. Two more
consequences of the same branch:

- `Tolerance::Basic.caption()` returns the **empty string** — a Basic
  tolerance prints no text at all; the **box** is the notation, drawn by the
  baker at `author.rs:447`. `is_boxed()` (`tolerance.rs:161`) is the named
  predicate; do not re-derive it with `matches!`.
- `Tolerance::caption` emits **no unit suffix** in any branch
  (`tolerance.rs:258-260`): a tolerance is read in the nominal's unit, and
  `"50.00 mm ±0.10 mm"` is not how a drawing is written.

**The safe way to preview a label is not to build one.** `author_dimension`
returns `AuthoredDimension::label` (`author.rs:240`) — the exact string it
baked. `Pass 68.0` shipped a defect whose entire cause was two independent
derivations of a display value: the properties pane read `77.5°` while the
`/AP` in the document read `77.47 pt` (`author.rs:288-293`,
`group.rs:~265`). One producer, always.

**Other traps**

- **An angle does not scale.** `DimensionKind::Angular`'s
  `measured_points()` returns **degrees**, and `display_with()`
  (`group.rs:288`) branches to `format_angle_degrees` and never applies the
  group scale. 30° at 1:50 is 30°, not 1500 of anything. Never feed an
  angular kind to `format_measurement` — it will produce a plausible, wrong
  number with `raw_page_units: false`, i.e. wrong *and* undisclosed.
  Always route through `display_with`.
- **`set_group_style`'s return value is NOT the count you should disclose.**
  It is the number of members **regenerated** — which is *every* wired
  member, including those that override the changed property, because
  regenerating an overrider is byte-identical and free in the diff
  (`edit.rs:16036-16044`). The number that will visibly **move** is the count
  of members whose `StyleProvenance` for the edited property reports
  `follows_group() == true`, and **it must be computed before the edit if it
  is to be disclosed before the edit**. Stated verbatim in ui-spec
  Amendment B §B.4.
- **`EditError::SidecarWrittenByNewerBuild`** (`edit.rs:16930`) — every
  ce-dimension mutation first calls `check_dimension_sidecar()`
  (`edit.rs:16917`) and refuses if the document's `/PieceInfo` sidecar
  version exceeds `SIDECAR_VERSION` (read the constant — `grep 'pub const
  SIDECAR_VERSION' crates/pdfcer-core/src/dimension/sidecar.rs`; it was `2` when
  this line was written, `3` when it was next read and `4` now, and the line
  number moved every time). Surface
  it as *"this file's ce dimensions were written by a newer pdfcer"*, not as a
  generic failure.
- **Kind-mismatch refusals.** `set_dimension_display` refuses a
  non-circular target with `EditError::NotACircularDimension`
  (`edit.rs:15939`); there is a matching `NotALinearDimension`. Both refuse
  **before** mutating — "a refusal never leaves a half-written model behind"
  (`edit.rs:16063`, `edit.rs:16134`).
- **`DimensionModel` is a snapshot.** `dimension_model()` clones out of the
  sidecar. Mutating the returned model changes nothing in the document; a
  panel that edits it and expects a save is editing a copy.
- **`StyleDefaults::FACTORY` is a compatibility surface, not a preferences
  file** (`style.rs:164-176`, values at `style.rs:205`: text 10.0, line 0.75,
  arrow 7.0, filled, black, no tolerance). Changing a number there silently
  redraws every existing document on its next regeneration. A shell must not
  offer "change the factory defaults" — offer group defaults instead.
- **Scale is group-only, by refusal.** `StyleOverrides` has no scale field
  and this is asserted structurally (`style.rs:650`). Do not add a
  per-ce-dimension scale control to the panel.
- **The parallel epsilon is a setting, never a literal.**
  `ParallelPolicy::from_setting(Settings::parallel_epsilon_degrees)`
  (`linepick.rs:169`), default 0.5° (`linepick.rs:155-160`). A shell that
  hard-codes it re-creates the CLI/GUI disagreement that centralising it
  prevented (`two_lines.rs:35-39`).
- **`force_parallel` does not move the lines or fake the measurement**
  (`linepick.rs:144-150`). The reported distance is the real perpendicular
  distance *at the pick point*. For genuinely diverging lines that is the
  distance where the operator pointed — which is exactly why obligation 4
  above is not optional.

---

## 2. Forms (AcroForm)

**Structural fact to absorb before reading anything else.** The modules
named `forms`, `forms_author`, `fdf`, `formcsv`, `form_script`, `richtext`
and `vartext` are **read / parse / serialise only**. *Every* mutating verb —
fill, flatten, create, delete, rename, move, reset, import — is a method on
**`EditSession`** in `crates/pdfcer-core/src/edit.rs`. A shell that goes
looking for `forms::fill(…)` will not find it.

Nothing is re-exported at the crate root (`lib.rs:84-123` are plain
`pub mod`), so spell the full path: `pdfcer_core::forms::parse_acroform`.

### 2.1 State, and where the opportunities are

| capability | core | cli | gui | note |
|---|:--:|:--:|:--:|---|
| Fill text / check box / radio / choice | x | x | x | rich text `/RV` is read + exported, replaceable only by a **disclosed downgrade** |
| **Flatten to static page content** | x | x | **[ ]** | **opportunity** — `FEATURES.md:122` |
| Import/export FDF, XFDF, two-column CSV | x | x | x | |
| Create a field | x | x | ◐ | **the GUI cannot create a push button** — `FEATURES.md:124` |
| Delete field / widget / grouping subtree | x | x | x | |
| Rename a field | x | x | x | |
| **Move a widget** (carrying its artwork) | x | x | **[ ]** | **opportunity** — `FEATURES.md:127` |
| Reset to defaults | x | x | x | `/V` is *removed* where no `/DV` exists, never blanked |
| **Script census + native recompute** | x | x | **[ ]** | **opportunity**; `FEATURES.md:129` — **no script is ever executed** |
| Read `/I`/`/TI` and `/MK /CA` | x | x | — | other `/MK` keys are not read |
| Detect XFA + warn the half goes stale | x | x | x | the XFA half is **never** written |

**Cannot today:** wide/batch CSV (one row per document) `FEATURES.md:222`;
reading/filling the static-XFA half `FEATURES.md:223`; barcode fields (never);
executing embedded JavaScript (never — standing rule).

**ui_specs to read:** `docs/ui_specs/forms-panel.md` — **this supersedes**
`docs/ui_specs/pass-7-form-fill.md` for placement and interaction shape;
pass-7 remains correct for its §3 per-type behaviour, its §4 two mandatory
disclosures and its §6.2 certification-gate finding, and forms-panel reuses
those by reference.

### 2.2 Entry points and the types that flow

**Read the tree.**

```rust
pdfcer_core::forms::parse_acroform<G: ObjectGraph + ?Sized>(graph: &G) -> Option<AcroForm>
```
`forms.rs:955`. Generic over `ObjectGraph`, so it runs over a `Document`
**and** over a live edit overlay (`&session.graph()`) — `forms.rs:945-948`.
`None` means no `/AcroForm`. It never panics; malformed shapes are tolerated
(`forms.rs:950-952`).

`AcroForm` (`forms.rs:745`) carries `fields`, `groups`, `need_appearances`,
`sig_flags`, `signatures_exist`, `append_only`, `calc_order`, `xfa`,
`inline_field_roots`, `default_appearance`, `quadding`. Navigation:
`fillable_fields()` `forms.rs:840`, `field_by_name(fqn)` `:851`,
`fields_named(fqn)` `:857`, `descendants_of(fqn)` `:901`.

`Field` (`forms.rs:446`) → `fully_qualified_name`, `partial_name`,
`alternate_name` (`/TU`), `mapping_name` (`/TM`), `rich_value` (`/RV`),
`default_style` (`/DS`), `value: FieldValue`, `selected_indices`,
`widgets: Vec<Widget>`, `has_additional_actions`, `shares_parent_name`,
`parent`. Predicates: `is_fillable()` `:596`, `is_rich_text()` `:639`,
`radios_in_unison()` `:651`, `has_appearance()` `:661`.

`Widget` (`forms.rs:388`) → `id`, `rect`, `appearance_state`, `on_states`,
`has_off_appearance`, `page`, `caption` (`/MK /CA`), **`border`**,
**`visibility`**, **`annot_flags`**, `has_normal_appearance`, `merged`.

#### ★★ `border` and `visibility` — read this before wiring a properties control (`Pass 146.0`)

**`border: Option<BorderSpec>`. `None` means THE FILE STATES NO BORDER. It is
not `BorderSpec::default()`, and substituting one is the defect this field was
added to prevent.**

`BorderSpec::default()` is *solid, one point* — Table 166's own defaults,
chosen so that **authoring** a widget without specifying a border reproduces
the bytes pdfcer has always written. That is correct for a writer and a lie
from a reader. A properties control seeded from it would display *Solid, 1 pt*
over a widget whose file says nothing, and **the operator's first press would
write that invention into their document**, silently replacing a border they
never looked at.

`pdfcer-gui` refused to ship the control rather than do that, and cited pdfcer's
own precedent: the text-colour swatch shows *a sentence* rather than a
nearest-RGB approximation for a run painted in DeviceCMYK, because a swatch
showing ink as RGB would write that RGB back on the next press. Same failure,
same refusal. ⇒ **`None` is a fact to display, not a value to substitute.**

Both spellings are read. `/BS` (Table 166) wins when present, per §12.5.4;
otherwise `/Border` (Table 164) yields the width, and the style is `Dashed`
when a **non-empty** dash array is present and `Solid` otherwise — a faithful
reading of an array that has no style key, not an inference. A `/BS` present
but missing `/W` takes Table 166's default of **1**: the file has committed to
having a border, so filling in the width the standard specifies is reading.
A width of **0** is a value ("no border", Table 166), never an absence.

**`visibility: Option<Visibility>` is exact-or-`None`, and `annot_flags`
carries the raw `/F` beside it.** `Visibility` is deliberately the four
combinations pdfcer can *set*, out of a flag word that admits dozens. That
makes it a good authoring type and an incomplete reading one: a file may
legitimately carry `Print | NoZoom`. Collapsing such a widget onto the nearest
of the four would be the border defect wearing a different hat, so the mapping
refuses and `annot_flags` lets a control say *"these flags are not something
pdfcer can set"* instead of showing nothing or showing a lie.

`/F` absent is `0` per Table 164, which **is** one of the four (`ScreenOnly`) —
so `None` always means *present and unmappable*, never *absent*.

**Why this is on the parsed model rather than a separate query.** `caption` is
already modelled here for the same reason, in its own words: modelling it is
what lets a caption be listed, copied and compared. A border is the same kind
of fact, and a second locator would be a second way to reach it.

CLI: `pdfcer list-fields --widgets` prints one line per widget with rect,
border, visibility, raw flags and appearance state. Per **widget**, not per
field, because a field may own several widgets with different ones — a single
field-level column would be a lie the moment a field has two.

Guards: `MAX_FORM_FIELDS = 500_000` `forms.rs:77`,
`MAX_FIELD_TREE_DEPTH = 64` `forms.rs:84`.

**Fill and value.** All on `EditSession`, all one undo entry.

| call | `edit.rs` |
|---|---|
| `fill_text_field(fqn, text) -> Result<FillOutcome, EditError>` | `12340` |
| `fill_text_field_downgrading_rich_text(fqn, text) -> Result<FillOutcome, EditError>` | `12384` |
| `set_button_state(fqn, on_state) -> Result<(), EditError>` | `12570` |
| `set_choice_value(fqn, &[&str]) -> Result<FillOutcome, EditError>` | `13138` |
| `regenerate_appearances() -> Result<RegenOutcome, EditError>` | `13600` |
| `fill_refusal(&self) -> Option<EditError>` — **pre-flight**, grey the control instead of failing late | `12200` |
| `rename_refusal(&self) -> Option<EditError>` — same | `12268` |

**Reset.** `reset_preview(&self, only: Option<&[String]>) -> Vec<ResetPreviewRow>`
`edit.rs:12755` (**non-mutating**), then
`reset_form(&mut self, only) -> Result<ResetOutcome, EditError>` `edit.rs:12884`.

**Interchange.** `export_form_data(&self) -> Option<fdf::FormData>`
`edit.rs:13446` → `FormData::to_fdf(source)` `fdf.rs:208` /
`to_xfdf(href)` `fdf.rs:256` / `formcsv::to_csv(&data) -> CsvExport`
`formcsv.rs:111`. Inbound: `FormData::parse_fdf` `fdf.rs:305` /
`parse_xfdf` `fdf.rs:335` / `formcsv::parse_csv` `formcsv.rs:206`, then
`import_form_data(&data) -> Result<ImportOutcome, EditError>`
`edit.rs:13471`.

**Flatten.** `flatten_fields(names: Option<&[&str]>) -> Result<FlattenOutcome, EditError>`
`edit.rs:13730`. `None` = the whole form.

**Create.** `add_text_field` `edit.rs:7087`, `add_check_box` `:8043`,
`add_radio_button` `:8253`, `add_push_button` `:9414`, `add_choice_field`
`:9633` — each takes a `&New*` spec (`NewTextField` `edit.rs:882`,
`NewCheckBox` `:1410`, `NewRadioButton` `:1485`, `NewChoiceField` `:1854`,
`NewPushButton` `:2135`) and returns `FieldAuthorOutcome` (`edit.rs:990`)
carrying `FieldAuthorDisclosures` (`edit.rs:1006`).
`field_defaults(&self, source) -> Result<FieldDefaults, EditError>`
`edit.rs:9211` implements "copy settings from an existing field".

**Structure.** `delete_field` `:8464`, `field_group_deletion_preview`
`:8535`, `delete_field_group` `:8574`, `delete_widget(fqn, index)` `:8764`,
`rename_field(fqn, new_partial)` `:8889`, `move_widget(fqn, index, dx, dy)`
`:9032`.

**Scripts (census + native recompute).** These take a `DocumentView`, not an
`EditSession`:
`form_script::inventory::inventory(&DocumentView) -> ScriptInventory`
(`form_script/inventory.rs:166`) and
`form_script::recompute::plan(&DocumentView, CommaPolicy) -> RecomputePlan`
(`form_script/recompute.rs:263`).
Document-wide counters come from `forms::scan_javascript(graph) -> FormJavaScript`
(`forms.rs:1813`).

### 2.3 Minimal worked sequences

**(a) Open → read the tree → fill → save.** Read idiom from
`crates/pdfcer-core/tests/form_field_authoring.rs:49-54`.

```rust
use pdfcer_core::{document::Document, edit::EditSession, forms, writer::SaveOptions};

fn fill(bytes: Vec<u8>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut session = EditSession::new(Document::from_bytes(bytes)?);

    // Read over the LIVE overlay, so the tree reflects edits already made.
    let form = forms::parse_acroform(&session.graph()).ok_or("no AcroForm")?;
    for f in form.fillable_fields() {
        let _ = (&f.fully_qualified_name, f.is_rich_text(), &f.widgets);
    }

    // Pre-flight: grey the control rather than failing after the click.
    if let Some(why) = session.fill_refusal() {
        return Err(Box::new(why));
    }

    let outcome = session.fill_text_field("Customer", "Ken Mantle")?;
    // MANDATORY disclosures — see §2.5:
    let _ = (
        outcome.xfa_may_disagree,
        outcome.applied_autosize,
        outcome.unencodable_chars,
        outcome.need_appearances_cleared,
    );

    Ok(session.to_incremental_bytes(&SaveOptions::identity())?.0)
}
```

**(b) Selective flatten — the `gui [ ]` opportunity.** From
`crates/pdfcer-core/tests/form_field_hierarchy.rs:466-474` and `:246-259`.

```rust
// Appearances first: flatten burns the artwork, so there must be artwork.
session.regenerate_appearances()?;
let out = session.flatten_fields(Some(&["Personal.Address.Zip", "Personal.Address.City"]))?;
// out.fields_flattened / widgets_burned / pages_touched — all disclosable.
// Whole form: session.flatten_fields(None)?  — afterwards parse_acroform()
// returns a form with no fields; flatten REMOVES what it burned.
```

**(c) Script recompute — plan, show, then apply.** The borrow shape is
load-bearing; copied from `pdfce@cce414e:crates/pdfce-gui/src/main.rs:8014-8021` and
`crates/pdfcer-cli/src/main.rs:9712-9715`.

```rust
use pdfcer_core::form_script::{calc::CommaPolicy, recompute};

// `session.view()` borrows immutably — the plan MUST be produced in a scope
// that ends before any &mut fill.
let plan = {
    let view = session.view();
    recompute::plan(&view, CommaPolicy::default())
};

// Show the plan. Nothing has changed yet. Skipped rows FIRST (see §2.5).
for s in &plan.skipped { let _ = (&s.field, &s.reason); }
for c in &plan.changes { let _ = (&c.field, &c.previous, &c.proposed, &c.disclosure); }
let _order_is_a_guess = plan.order_source.is_pdfcer_choice();
let _zeros = plan.coerced_operands();

// Only after the operator accepts. There is NO EditSession::apply_recompute —
// applying a plan is a loop the shell writes itself.
for c in plan.changes {
    session.fill_text_field(&c.field, &c.proposed)?;
}
```

### 2.4 ★ What the UI must disclose

Forms are the densest disclosure surface in the crate. Every row below is a
value pdfcer **inferred, substituted or declined** and each has a field on an
outcome struct precisely so the shell can render it — they exist to be shown,
not to be logged.

**On every fill** (`FillOutcome`, `edit.rs:5756`):

1. **`xfa_may_disagree`** (`edit.rs:5769-5791`). The document also carries an
   XFA packet; pdfcer filled the AcroForm half and **cannot write the XFA
   half**, so an XFA-aware viewer may show the old value. *"Nothing about the
   saved file looks wrong"* — which is exactly why the shell must say it. Note
   the deliberate asymmetry: **filling succeeds and discloses; field
   *creation* is refused outright** with `EditError::FieldAuthoringRefusedXfa`
   (`edit.rs:2492-2502`), because a one-sided add makes two viewers disagree
   about how many fields the document has.
2. **`applied_autosize`** (`edit.rs:5828`, `vartext.rs:228-232`) — pdfcer
   *chose* the point size by its own heuristic; auto-size is not specified.
   That is an inference. Show the number it picked.
3. **`unencodable_chars`** (`edit.rs:5830`, `vartext.rs:233-235`) — characters
   replaced with `?` because WinAnsi could not encode them. Silent character
   loss is the worst kind.
4. **`need_appearances_cleared`** (`edit.rs:5826`) — pdfcer cleared the
   producer's "appearances are stale" flag on output.

**On rich text.** `fill_text_field` **refuses** a rich-text field outright
rather than writing half the `/DS` + `/RV` pair (`forms.rs:475-481`). The
only way through is `fill_text_field_downgrading_rich_text`, and the shell
must present it *as a downgrade*: it clears `/Ff` bit 26, **deletes** `/RV`
(not empties it — §12.7.3.4 gives an empty one no meaning), and regenerates a
plain appearance (`edit.rs:12492-12502`). Formatting is lost. Also:
`import_form_data` **skips and counts** rich-text targets rather than failing
the import (`edit.rs:13534-13538`), so `ImportOutcome::skipped` must be shown
— an import that reports "done" while silently dropping entries is the sneaky
case. And rendering rich text at all *"is policy, not conformance"*
(`richtext.rs:75-90`): pdfcer's choice, disclosed as pdfcer's choice.

**On the script census / recompute** — the whole feature is a disclosure
feature:

5. **pdfcer never runs the script.** Every branch of `Disclosure::message()`
   says so, repetitively and on purpose (`form_script/disclose.rs:92-97`) —
   it is *"the single fact most likely to be assumed away by a reader who has
   used Acrobat, and the one whose absence would turn a careful disclosure
   into a false claim of authority."* Do not deduplicate that sentence out of
   the UI.
6. **The calculation order may be pdfcer's guess.** `RecomputePlan::order_source`
   with `is_pdfcer_choice()` (`recompute.rs:95-124`) plus
   `unlisted_calculations`. The existing GUI renders it in the warning colour
   (`pdfce@cce414e:crates/pdfce-gui/src/main.rs:7952-7957`).
7. **Blank and non-numeric operands were counted as ZERO**, not skipped —
   `RecomputePlan::coerced_operands()` (`recompute.rs:243-254`). `calc.rs:499-503`
   calls this *"the single most important behaviour in this module, because the
   intuitive reimplementation ('skip what isn't a number') is wrong."*
8. **List the skipped fields BEFORE the proposed changes.** The existing GUI
   does (`pdfce@cce414e:crates/pdfce-gui/src/main.rs:7957-7968`) and it is the right order:
   what pdfcer declined to compute matters more than what it did.
9. **Never auto-run recompute on open** (`pdfce@cce414e:crates/pdfce-gui/src/main.rs:7930-7933`).
10. **`Field::has_additional_actions`** (`forms.rs:544-548`) — the value shown
    is *as stored*, possibly stale, because pdfcer does not recompute `/CO`
    automatically. Badge the field.

**On CSV export.** `CsvExport::neutralised`, `.neutralised_fields` and
`.message()` (`formcsv.rs:89-106`, `:303`). Values a spreadsheet would
evaluate as formulae get a `'` prefix. The PDF is unchanged; the change is
**visible, not silent**; and it is **reversible on import** (`parse_csv`
strips it). `formcsv.rs:30-38` names the hazard, including
`=WEBSERVICE(…)` reaching the network — a capability pdfcer refuses itself.

**On authoring a field** (`FieldAuthorDisclosures`, `edit.rs:1006-1035`):

11. `tagged_document` — the new field is **untagged** in a Tagged PDF.
12. `structure_tab_order` — under `/Tabs /S` the new field has **no tab
    position at all**.
13. `tooltip_declined` — the accessibility name was declined. Note
    `TooltipChoice::Undecided` **refuses** creation
    (`EditError::TooltipDecisionRequired`, `edit.rs:930-950`): `Option<String>`
    cannot distinguish *"chose not to"* from *"nobody thought about it"*.
14. `FieldAuthorOutcome::merged` (`edit.rs:994-999`) — the create **merged**
    into an existing same-named field, so the new widget **shares a value**
    with one already on the form. That is not what "add a field" sounds like.
15. A choice field created with an empty `/Opt` is **unfillable**; a push
    button with an empty `/MK /CA` is **blank** (`edit.rs:1033-1035`, `:1098-1100`).

**On structural edits:**

16. `WidgetMove::siblings_left_behind` (`edit.rs:5852-5861`) — *"moving one
    and silently leaving two behind is the kind of partial result that reads
    as a bug later."*
17. `FieldRename::descendants_renamed` (`edit.rs:6786-6791`) — descendants'
    FQNs changed **without any of their own objects being written**, so every
    FDF and JavaScript reference naming them now points at nothing
    (`edit.rs:6774-6777`). ⚠️ **Not "submit mapping" any more**: since
    `Pass 184.0` a rename REPAIRS the button actions naming the field, and
    reports how many in `FieldRename::action_targets_retargeted`. A deletion
    does not repair, and reports `action_targets_orphaned` instead.
18. `FieldDeletion::selection_cleared` (`edit.rs:6058-6064`) — *"silently
    leaving the dangling `/V` would be sneaky; silently clearing it would also
    be."*
19. `FieldGroupDeletion::node_names` (`edit.rs:6727-6743`) — the shell must
    invalidate its own per-FQN state (open rename drafts, selections) or a
    stale draft resurfaces pre-filled with an old value.

**On reset:** `ResetPreviewRow::would_remove` (`edit.rs:5686-5690`) —
*"an absent key and an empty string are different bytes, and a shell that
showed both as `""` would be describing the wrong edit."* Rows are returned
for **every** field in scope including ineligible ones and ones already at
their default (`edit.rs:12751-12754`), and the three ineligibility reasons
(`PushButton`, `Signature`, `ReadOnly` — `edit.rs:5653-5661`) stay distinct
because *"a shell that can only say '3 skipped' cannot tell the operator
which of those happened."*

**On flatten:** it is destructive but **revision-recoverable** under the
default incremental save — the pre-flatten values survive in the earlier
revision until a full rewrite (`edit.rs:13720-13726`). That is an R48
destructive-disclosure the caller must surface, and the honest wording is
*"recoverable from the previous revision until you do a full rewrite"*.

**On the document as a whole:** `AcroForm::need_appearances`
(`forms.rs:765-767`) — the producer's assertion that appearances are stale,
*"disclosed, never a silent on-load regenerate"*; and
`AcroForm::inline_field_roots` (`forms.rs:825-838`) — malformed direct dicts
in `/Fields` that were skipped, meaning `fields.len()` understates the true
count by exactly that many.

### 2.5 Traps

- **`/Ff` bit 26 is overloaded** — rich text on `/Tx`, radios-in-unison on
  `/Btn`. *"A caller holding only the flag word cannot decode it correctly
  even in principle"* (`forms.rs:620-632`). Use `Field::is_rich_text()`
  (`:639`) and `radios_in_unison()` (`:651`); never test the raw bit.
- **Do not derive grouping nodes by splitting an FQN.**
  `Personal.Address.Zip` *looks* like it yields `Personal` and
  `Personal.Address` for free. *"It does not, and the failure is silent"*
  (`forms.rs:718-721`). Use `AcroForm::groups` — and note it is
  **deepest-first (post-order)** (`forms.rs:750-757`), so a breadcrumb built
  in iteration order renders backwards.
- **`resolve_field_path`** (`forms_author.rs:308`) is *"the ONLY entry point
  through which an authoring write may learn what a name currently denotes"*
  (R100, `forms_author.rs:274-276`). It retains grouping nodes that
  `parse_acroform` discards. Do not resolve names any other way.
- **Same-name create MERGES; same-name rename REFUSES.** Deliberate
  asymmetry (`forms_author.rs:176-193`): a create with an existing name
  attaches another widget to one field; a rename names an existing field
  *and* a new name, and silently fusing them *"would destroy an identity they
  never offered up."* Surface `FieldTypeCollision`, `NameIsGroupingNode`,
  `FieldPathCrossesTerminal`, `RenameCollision`, `PeriodInPartialName`,
  `EmptyName`, `PathTooDeep` (`forms_author.rs:145`) as operator-actionable
  refusals — each names the field and what is in the way.
- ★★ **A dotted path may not nest under an existing TERMINAL field**, and this
  is the refusal to wire if you are wiring only one. `FieldPathCrossesTerminal
  { fqn, terminal }` (`Pass 174.8`) is the **mirror** of `NameIsGroupingNode`
  and the destructive half of the pair: that one is *"you asked for a terminal
  and the name is a group"*, this one is *"you asked for a child and the
  ancestor is a terminal"*. Creating `Text.2` while `Text` is a terminal gives
  `Text` field-kids, which makes it non-terminal (§12.7.3.1) — and Table 220
  gives a non-terminal no type of its own, so `Text`'s `/FT` and `/V` become
  *inheritable defaults for its kids* rather than its own. Until `Pass 174.8`
  that happened **silently**, reporting success.
  ★ **It is a demotion, not a deletion, and an orphaned-widget census will not
  find it.** Measured on a damaged file: object 6 is still the sole `/Fields`
  entry, still in `/Annots`, and still carries `/V (K. Mantle)` — while every
  reader correctly projects one field named `Text.2` and the operator's value
  is reachable by no form verb, no FDF export and no script.
  `fqn` is the name the operator typed (`Text.2`), not the unmatched tail.
  **There is no repair verb and none is planned** — not because there is
  nothing to put back (there is), but because the operator's document would
  already be wrong in a way they cannot see, and refusing the operation is a
  better answer than repairing it afterwards.
  A *deliberate* promotion (`Text` → a group, the original demoted to `Text.0`
  keeping its value) would be a different verb with its own confirmation,
  because it **renames an existing field**, and a field's name is its identity
  to every script, calculation order and FDF mapping that refers to it. It is
  not built.
- **A widget written with a `/T` is not a second view of a field — it is a
  second field underneath it**, silently composing `Ref.Ref`
  (`forms_author.rs:528-535`, `FIELD_ONLY_KEYS`).
- **`move_widget` translates only.** `/Rect`'s extent is unchanged, so
  §12.5.5's matrix degenerates to a pure translation and the existing artwork
  is carried at its original size by the algorithm every conforming reader
  already runs (`edit.rs:8993-9002`). **Resize is deliberately not this
  method** — the same algorithm applies a non-uniform stretch, normatively, so
  a resized check box gets a distorted tick (`edit.rs:9004-9012`). Move takes
  the **strict** certification gate (`edit.rs:9026-9031`); fill takes the
  `/P >= 2` gate (`edit.rs:13728-13730`).
- **`FormData` omits any field with no value** (`fdf.rs:126-131`), so an
  import **never clears a field the file did not name**. A shell offering
  "import" must not describe it as "replace the form's data".
- **Import is document-gated once, up front** (`edit.rs:13478-13485`) —
  *"discovering that on entry seventeen, after sixteen have already been
  committed, is both late and destructive."*
- **A CSV row missing a column is an error, not a best-effort skip**
  (`formcsv.rs:452-456`) — otherwise a spreadsheet that lost its value column
  imports as names with empty values and blanks the whole form. And a value
  that genuinely begins with `=`/`+`/`-`/`@` **cannot survive the round trip**
  (`formcsv.rs:196-199`); that trade is deliberate.
- **Format helpers are display-only.** *"Nothing here produces a value a
  caller could store"* (`form_script/format.rs:962-966`); storing formatted
  output destroys `/V`. A helper on the wrong trigger deliberately does not
  classify (`form_script/mod.rs:599-602`).
- **Percent multiplies by 100** — writing `8.5` into such a field makes it
  read `850%` (`format.rs:743-747`). **Ambiguous dates decline rather than
  guess** — `03/04/2026` (`format.rs:902-905`, `datetime.rs:527-530`).
  **`CommaPolicy::default()` refuses to guess** the decimal separator,
  because pdf.js's first-comma rewrite turns `1,234` into `1.234` — a
  thousand-fold error (`calc.rs:594-598`).
- **A script pdfcer could not read is always `Custom`** by construction, never
  optimistically a known built-in (`inventory.rs:56-63`); a `/JS` carried as
  a **stream** is still classified, because *"a form whose scripts all live in
  streams would otherwise be reported script-free"* (`inventory.rs:321-324`).
- **`Style` fields in `richtext.rs` are `Option`, and `None` means
  *unspecified*, not *default*** (`richtext.rs:153-160`). Unknown markup keeps
  its text: *"a missed style is a cosmetic difference the operator can see; a
  dropped run is text that vanishes"* (`richtext.rs:60-73`). CSS `justify` is
  **not** an accepted `Align` (`richtext.rs:114-118`). `/DA` vs `/DS`
  precedence is **undefined by the standard** and is a setting, resolved
  elsewhere — this module never reads `/DA` (`richtext.rs:93-102`).
- **No font substitution in variable text.** `VarTextError::FontUnresolved`
  refuses rather than substituting, *"because a substituted font changes glyph
  advances, so the operator would see different metrics than every other
  reader"* (`vartext.rs:196-203`).
- **Creation refusals to surface by name:** comb combined with
  `/MaxLen`/multiline/password/file-select (`edit.rs:914-921`); a duplicate
  choice export, because the fill verb resolves to the first match
  (`edit.rs:9642-9648`); `Off` as an on-state (`edit.rs:8258-8262`); an
  editable list box (`ChoiceEditRequiresCombo`, `edit.rs:9637-9639`).
  Setting the `/Opt`-sort flag alone does nothing visible — readers display
  `/Opt` order regardless (`edit.rs:1876-1880`).

---

## 3. Annotations & markup

`core [x] · cli [x] · gui [x]` across the board (`FEATURES.md:110-115`), so
this is a *rebuild*, not a gap — but two named limits shape what the shell
can offer, and they are limits of the **model**, not of the old GUI.

**ui_spec to read:** `docs/ui_specs/pass-6.1-markup-tools.md` — the tool-mode
state machine, the gesture shapes, and the placement rules. Pass 8's
redaction spec explicitly reuses its Mark phase.

### 3.1 What it can do, and the two limits

Author: **Ink, Square, Circle, Line, Polygon, PolyLine**, the four text
markups (**Highlight, Underline, StrikeOut, Squiggly**), and the text-bearing
**FreeText, Sticky (`/Text`) and Stamp**. Read note text (`/Contents`),
author (`/T`) and modification date (`/M`). Delete, with a preview. Honour
the annotation-flag set and annotation `/OC`.

**Limit 1 — RETIRED 2026-08-28. Geometric markup CAN carry note text, by
two routes.** `MarkupOptions::note` at author time (`Pass 150.0`) and
`EditSession::set_markup_note` / `clear_markup_note` afterwards
(`Pass 154.0`). Both are CLI-wired.

> ### ★★★ What this paragraph used to say, and why it is struck rather than deleted
>
> It read: *"geometric markup cannot carry note text … a Comments panel will
> therefore show 'no note text' on every shape pdfcer itself drew — **expected,
> not missing data**, and the shell must say so in those words."*
>
> ★ Those last words are the problem. This was not merely a stale statement of
> fact — **it was an instruction to a consuming project to tell its operator
> something untrue**, and it kept that instruction for the nine hours between
> `Pass 150.0` shipping and this correction. It also called note authoring
> *"unbuilt in every shell"* on a day it shipped twice.
>
> **And the commit that falsified it edited this very file**, at a hunk whose
> first context line is this paragraph's own closing sentence — correcting
> *Limit 2* while reading past *Limit 1* one paragraph up.
>
> ⇒ **A reported-survivor list is a worklist, not a scope.** Fixing the
> survivors somebody handed you is not the same as sweeping for the claim, and
> the two feel identical while you are doing them.

**Limit 2 — RETIRED 2026-08-28: a placed markup can be moved, resized AND rotated.**

★★ **THIS PARAGRAPH SAID THE OPPOSITE, AND ITS LAST SENTENCE TOLD A SHELL NOT
TO BUILD SOMETHING THAT NOW EXISTS.** It read, in full:

> ~~"There is no `move_annotation` / `resize_annotation` / `set_annot_rect`
> anywhere in `pdfcer-core` (verified absent). Geometry is fixed at gesture
> end; repositioning is *"Discard-and-replace"*. Only deletion is available
> afterwards. **Do not design a shell around dragging a placed markup; the
> verb does not exist.**"~~

Every clause was true when written and the last one is now actively harmful:
it is an instruction, in the document a consuming project builds against, not
to design the feature that shipped as `Pass 149.0`.

⇒ Worth noting *why no gate caught it*: `check-core-api-verbs.py` detects a
verb **missing** from these documents. **A claim of NON-coverage is invisible
to a coverage check** — nothing can tell "there is no such verb" from prose.
That class needs a reader, and it got one.

**What is true now.** `EditSession::move_annotation(annot_id, dx, dy)`
(`Pass 149.0`) translates `/Rect` **and every geometry key** — see
`02-editing-and-saving.md`. CLI: `pdfcer move-annotation`.

**Both are now built.** `resize_annotation` (`Pass 151.0`) and
`rotate_annotation` (`Pass 155.0`), for markup, redaction marks and links
alike, both CLI-wired. `move_dimension` and `move_widget` remain the verbs
for ce dimensions and widgets respectively, and all three transform verbs
refuse both by name.

> ### ★★ This paragraph read *"still unbuilt, and these are real"* until 2026-08-28
>
> It named the reason resize was left out of `Pass 149.0` — §12.5.5 scales the
> existing artwork anisotropically, so a per-subtype decision was owed rather
> than guessed at. That reasoning was **correct and was acted on**:
> `ResizeOptions` is the decision, taken as an operator ruling.
>
> ★ Rotation turned out to be the *easy* one, which is the opposite of what
> this paragraph's ordering implies. §12.5.5 step (a) transforms the appearance
> `BBox` through its **own `/Matrix`**, so a rotation is composed into that
> matrix and **nothing is redrawn** — a foreign producer's artwork rotates
> correctly, and being an isometry it cannot distort a stroke. Resize needed
> an options type and a refusal; rotate needed neither.
>
> Worth keeping because the pairing was intuitive and wrong: *resize* sounds
> tamer than *rotate*, and the spec makes it the other way round.

### 3.2 Entry points

**Read.** `annot::page_annotations(graph, page_id) -> Vec<Annotation>`
(`annot.rs:531`) and `page_annotations_with(graph, page_id, missing_as)`
(`annot.rs:567`). Both generic over `ObjectGraph` — pass `&document` **or**
`&session.graph()` (`annot.rs:513-516`). `page_id` comes from
`page_tree::Page::id`. `/Annots` is **not inheritable**, so there is no
page-tree walk (`annot.rs:509-511`). Bounded by
`MAX_ANNOTS_PER_PAGE = 1_000_000` (`annot.rs:117`).

`Annotation` (`annot.rs:290`): `id`, `subtype`, `rect`, `flags: AnnotFlags`,
`appearance: Appearance`, `is_popup`, `contents` (`:336`), `title` (`:348`),
`mod_date` (`:360`, **raw and unparsed**), `oc`, `popup`, `in_reply_to`,
`reply_type`. Methods: `is_widget()` `:450`, `is_group_subordinate()` `:468`,
`effective_reply_type()` `:485`, `subtype_label()` `:495`.

`AnnotFlags` (`annot.rs:132`) — `hidden()` `:184`, `no_view()` `:190`,
`print()` `:197`, `invisible()` `:203`, `no_zoom()` `:209`, `no_rotate()`
`:215`, `locked()` `:226`, `suppressed_on_screen()` `:239`.

`Appearance` (`annot.rs:253`) — `Normal { stream_id }` / `None` /
`StateUnresolved`.

Optional content: `optional_content_default_off(graph) -> BTreeSet<ObjId>`
(`annot.rs:701`), `oc_is_hidden(graph, oc, &off)` (`annot.rs:994`),
`apply_view_usage(graph, &mut off, magnification) -> UsageNotes`
(`annot.rs:1268`).

**Pure builders** (no document, no allocation — R47,
`annot_author.rs:353-354`):
`build_appearance(&MarkupSpec) -> AuthoredAppearance` `annot_author.rs:356`;
`build_redact_mark(&RedactSpec) -> AuthoredAppearance` `:930`;
`build_text_annotation(&TextAnnotSpec) -> Result<AuthoredTextAnnot, VarTextError>` `:1238`.
Use these to draw a **live preview** on the canvas without touching the
document — that is what makes rule-4 disclosure cheap here.

**Session verbs** (`&mut EditSession`, one undo entry each):

| call | `edit.rs` |
|---|---|
| `add_markup(page_index, &MarkupSpec) -> Result<ObjId, EditError>` | `9986` |
| `add_text_annotation(page_index, &TextAnnotSpec) -> Result<ObjId, EditError>` | `12034` |
| `delete_annotation(annot_id) -> Result<AnnotationDeletion, EditError>` | `10847` |
| `annotation_deletion_preview(&self, annot_id) -> Result<AnnotationDeletion, EditError>` — **non-mutating** | `11316` |
| `annotation_deletion_refusal(&self) -> Option<EditError>` — pre-flight | `11492` |

`MarkupSpec` (`annot_author.rs:215`, `#[non_exhaustive]`) — `Square`,
`Circle`, `Line`, `Ink`, `Polygon`, `PolyLine`, `TextMarkup`. `TextAnnotSpec`
(`:1144`, `#[non_exhaustive]`) — `FreeText`, `Sticky`, `Stamp`. Supporting:
`Color` `:85`, `Quad` `:129` + `Quad::from_rect(rect)` `:145`,
`TextMarkupKind` `:179`, `LineEnding` `:299`, `StickyIcon` `:1022`,
`StampName` `:1060` (14 names).

`AnnotationDeletion` (`edit.rs:5936`) reports `subtype`, `route`
(`AnnotationDeletionRoute::{General, RedactionMark, Dimension}`,
`edit.rs:5908`), `popup_removed`, `parent_popup_cleared`,
`replies_orphaned`, `group_members_promoted`, `appearance_streams_removed`.

**Shared guards** on all three authoring verbs (`edit.rs:9958-9985`,
`:10039-10048`, `:12039-12048`): `DocumentEncrypted` → certification →
hidden-object refusal → `EmptyGeometry`; plus `PageOutOfRange`, `PageTree`,
`ObjectNumbersExhausted`, `AnnotsNotAnArray`, `NotADictionary`, and
`VariableText` for the text ones. The **certification gate** (`edit.rs:11449`)
refuses at `/P` 1 and 2 and **permits at `/P` 3** (§12.8.2.2 Table 254); `/P`
**defaults to 2 when absent**, so an absent `/P` is a refusal.

### 3.3 Minimal worked sequence

From `crates/pdfcer-core/src/edit.rs:18980-19022` and its round-trip sibling
at `:19029-19060`.

```rust
use pdfcer_core::annot_author::{Color, MarkupSpec, build_appearance};
use pdfcer_core::{annot, edit::EditSession, object::Rect};

fn draw_a_square(session: &mut EditSession) -> Result<(), Box<dyn std::error::Error>> {
    let spec = MarkupSpec::Square {
        rect: Rect { llx: 20.0, lly: 20.0, urx: 120.0, ury: 70.0 },
        border: Some(Color::Rgb(1.0, 0.0, 0.0)),
        interior: None,
        border_width: 2.0,
    };

    // PREVIEW on the canvas, document untouched — pure, no allocation.
    let preview = build_appearance(&spec);
    let _ = (&preview.ap_content, preview.rect);

    // Pre-flight so the control is greyed rather than failing after the click.
    if let Some(why) = session.annotation_deletion_refusal() { /* related gate */ let _ = why; }

    // COMMIT. One undo entry; a full /AP is baked (R44); /P back-references the page.
    let _annot_id = session.add_markup(0, &spec)?;
    Ok(())
}

// Enumerate for a Comments panel — over the LIVE overlay, not the base revision.
fn comments(session: &EditSession, page_id: pdfcer_core::object::ObjId) {
    for a in annot::page_annotations(&session.graph(), page_id) {
        let _ = (a.subtype_label(), a.contents.as_deref(), a.title.as_deref(),
                 a.mod_date.as_deref(), a.flags.suppressed_on_screen());
    }
}
```

### 3.4 ★ What the UI must disclose

1. **"Delete is not redaction."** `delete_annotation` *"does not remove
   content from the file … **Deleting a comment is not redacting it**, and a
   caller whose operator might believe otherwise must say so"*
   (`edit.rs:10770-10778`). This is mandatory copy, not a nicety — the
   existing shell carries it at `ui_text.rs:1700`, `:6046-6052`, `:6093-6094`,
   `:6145-6146`. Undo labels must distinguish the two as well
   (`ui_text.rs:4825-4834`: *"remove a redaction mark"*, deliberately never
   *"undo redaction"*).
2. **Shapes pdfcer drew carry no note text — say it is expected.** See limit 1.
   A bare "No note text" column reads as data loss.
3. **The collateral of a deletion, before it happens.**
   `annotation_deletion_preview` (`edit.rs:11316`) returns the same
   `AnnotationDeletion` struct the real call does: how many replies get
   **orphaned**, whether the popup goes, how many group members get
   **promoted**. Show it before the delete. ⚠️ But see the preview's own
   limits in §3.5 — it is not a perfect oracle.
4. **`Appearance::StateUnresolved`** (`annot.rs:271-278`) — pdfcer *"displays
   nothing and does not guess a first / `On` / `Off` key."* A blank annotation
   with no explanation looks like a rendering bug; say the appearance state
   could not be resolved. The governing setting is
   `MissingAppearanceState`, default `PaintNothing`, and it is explicitly
   **evidence tier (d), a reasoned guess** (`annot.rs:551-556`) — i.e. an
   inference, i.e. rule 4 applies.
5. **Suppressed annotations.** `AnnotFlags::suppressed_on_screen()`
   (`annot.rs:239`) — an annotation the file says not to show. A Comments
   panel that silently omits it is hiding document content; list it and mark
   it hidden.

### 3.5 Traps

- **★ `apply_view_usage` must never be reachable from a print or export
  path** (`annot.rs:1209`) — §8.11.4.5 is a `shall not`.
  `optional_content_default_off` *"is the complete and correct answer for
  printing and for aggregation, and calling this on the way to a printed page
  would violate the standard"* (`annot.rs:1219-1224`). They are two functions
  *"so that the print path cannot acquire this one by accident."*
- **`/Contents` is dual-purpose** — *"a UI labelling this 'comment' is right
  for markup and wrong for a Link"* (`annot.rs:315-324`). And the §12.5.6.2
  group-attribute inheritance rule is **deliberately not applied**; `contents`
  is the raw dictionary value (`annot.rs:326-335`).
- **`title` is `/T` from Table 170, not 164.** `None` on a Link means *"this
  subtype has no such concept"*, not *"anonymous"* (`annot.rs:340-347`).
  `mod_date` is stored **raw** because §12.5.2 requires accepting a string in
  any format (`annot.rs:352-359`) — do not assume it parses.
- **`reply_type: None` is NOT `ReplyType::Reply`.** Use
  `effective_reply_type()` (`annot.rs:415-420`, `:485`).
- **The deletion preview is not a perfect oracle** (`edit.rs:11293-11308`):
  `appearance_streams_removed` is reported as **0, not computed**; a
  *delegated* target comes back with zeroed counts; and **the preview does not
  run the destination verb's certification gate**, so *"a preview can say
  'this would work' where the real call refuses."* Handle the refusal at the
  real call anyway.
- **`delete_annotation` does not chase `/S /Hide` actions and does not
  update `/StructParent` / `/OBJR`** (`edit.rs:10779-10789`). Named gaps, not
  bugs.
- **Widget annotations are not canvas-selectable** and cannot become so until
  that work lands (`pdfce@cce414e:crates/pdfce-gui/src/main.rs:801`).

---

## 3.6 Restyling existing text — **the capability-shaped answer**

**Added 2026-08-27 because it was missing, and its absence cost a consuming
project a filed request.** `EditSession::format_text` shipped 2026-08-20 and
is fully documented in
[`02-editing-and-saving.md`](02-editing-and-saving.md) — under *text editing
mechanics*. A shell asking **"how do I make this selection bold"** did not
find it there, concluded no such verb existed, and filed
`request_restyle_an_existing_text_run.md` with a table whose
"on EXISTING text" column read *"none available"* across the board.

Nothing was wrong in either document. `tools/check-core-api-verbs.py` was
green, because it catches a verb that is **absent** — and this one was
present, correct, and **unfindable by the question a reader actually has**.
That is a distinct failure and this section is the repair for it.

### What works on existing text, today

| the operator's button | field on `FormatRequest` | works? |
|---|---|---|
| **size** | `set_size: Option<f64>` | ✅ always. Changes only the `Tf` operand; the line is relaid out and `advance_delta` is reported |
| **colour** | `set_fill: Option<NewFill>` | ✅ always. **Stores the chosen device space** (`rgb:`→`rg`, `cmyk:`→`k`, `gray:`→`g`) — pdfcer does *not* force-convert to DeviceRGB the way Acrobat does. A run originally painted in a non-device space is disclosed as a narrowing conversion |
| **face** | `set_font: Option<FontSelector>` | ⚠️ **only to a font that is ALREADY a resource on the page** — see below |
| **bold** | `set_synthetic` (`StyleSynthesis`) | ✅ **on any page**, as a disclosed *synthetic* weight — see §3.6.1 |
| **italic** | `set_synthetic` | ✅ as a disclosed *synthetic* slant, except where a `Td`/`TD`/`T*` follows the run in the same text object — refused by name, §3.6.1 |
| character spacing | `set_char_spacing` (`Tc`, §9.3.2) | ✅ |
| word spacing | `set_word_spacing` (`Tw`, §9.3.3) | ✅ simple fonts; **refused by name on a composite run** (`Tw` is spec-void for multi-byte codes, so emitting it would do nothing) |
| horizontal scale | `set_h_scale` (`Tz`, §9.3.4) | ✅ |
| super/subscript | `set_script` | ✅ |
| free-form baseline | `set_rise` (`Ts`, §9.3.7) | ✅ — an exceed over Acrobat, which dropped it |
| **alignment, leading** | — | ❌ not a run-level property; those live on `reflow_block` |

Targeting is by `find` text or by `pinned_span` — the same
`GlyphProvenance::operator_span` pin `edit_text` takes. **With a pin and no
find, the whole pinned show operator is the target**, spelled
`FormatRequest::whole_operator(page, span)` and
`EditRequest::whole_operator(page, span, replacement)` respectively. Same
`EditTarget` selector, so a shell that has decided which stream a caret is in
does not translate that decision between the two verbs. Form XObject content
is reachable (`Pass 119.2`).

CLI: `pdfcer format-text --set-size / --set-color / --set-font / …`.

### ★ The one real limit, and it is a property of the DOCUMENT

```
$ pdfcer format-text runs-two-explicit.pdf --find ALPHA       --set-font Helvetica-Bold --output bold.pdf
pdfcer: format-text refused: the target font "Helvetica-Bold" is not an
existing font resource on this page; adding a new font resource / embedding
a new face is deferred (FF-C)
```

**`set_font` selects; it does not create.** The target must already be in
`/Resources /Font`, located by resource key or by `/BaseFont` (subset tag
stripped per §9.6.4). `FF-C` is the tracked identifier for *add a font
resource / embed a new face*; `add_text` already does it for Standard-14 and
(via `--embed-font`) for donor faces, but it is not wired into `format_text`,
whose plan currently produces a content buffer and no new objects. Filed as
Backlog `Pass 142.0`, with the missing pre-flight as `142.1`.

#### ★★ 3.6.1 But bold and italic ARE reachable — `set_synthetic`

**This paragraph replaces a wrong one. The first draft of §3.6 said bold and
italic were unavailable on existing text, and that claim was sent to a
consuming project before it was checked.** It is corrected here rather than
quietly deleted, because the *shape* of the error is worth more than the fact:
**I measured `set_font`'s refusal, found it real, and inferred a capability
gap from a single verb — without asking whether a second verb reached the
same operator goal.** An absence claim about pdfcer is a claim about *all*
routes, and it was checked against one.

`FormatRequest::set_synthetic` shipped in **`Pass 19.2`** (`ebe35d8`,
2026-08-03, decision 019 §3.6, `R90`). CLI: `--bold-synthetic`,
`--italic-synthetic`.

Measured on §3.6's own worked example — the *same* Helvetica-only page whose
`--set-font Helvetica-Bold` refusal is quoted above:

```
$ pdfcer format-text runs-two-explicit.pdf --find ALPHA       --bold-synthetic --output bold.pdf
    - synthetic bold: the run is painted in text rendering mode 2 (fill,
      THEN stroke — §9.3.6 Table 106) with a stroke width of 0.22 …
```

**★★ `gate_synthesis` ANSWERS whether a real face is available; what happens
next is the POSTURE's** (`Pass 179.2`, decision 106). *Available* still means
`set_font` would ACTUALLY ACCEPT it for this run, and the survey is unchanged.

| `style_policy` | a real face IS available | none is |
|---|---|---|
| `auto` *(default)* | synthesis is **applied**, and the face is named in `FormatReport::real_face_passed_over` | synthesis applies |
| `warn` | the same, and the shell should surface it prominently | synthesis applies |
| `refuse` | **refused**, quoting the exact selector to retry with — pdfcer's behaviour before this was a choice | synthesis applies |

This paragraph previously said, flatly, that synthesis *"is refused"*. That was
true of every build up to `0.16.0` and is now true only of `refuse`.

⚠️ **A shell must read the posture to know what pressing the button does.**
`StyleOutcome::RealFaceResolves` answers *"is a real face available"* — which
is stable — and NOT *"will this be refused"*, which is not.

> #### ★★ CORRECTION, 2026-08-27 — this paragraph carried a false universal
>
> It used to end: ~~"So between the two verbs, **every page is covered**: where
> a real Bold exists, `set_font` uses it and synthesis is refused; where none
> exists, synthesis applies and `set_font` refuses."~~ **That was not true**,
> and it was sent to `pdfcer-gui` in that form.
>
> `gate_synthesis` decided *"a real face is available"* from two string tests
> on `/BaseFont` — a family-stem comparison and a search for `bold`/`black`/
> `heavy`/`semib` — and asked nothing about whether that face could show the
> run's characters. On pdfcer's own fixture
> `fixtures/synthetic/textedit/format_family.pdf` it named `/F3`
> (`Times-Bold`), whose `/Encoding /Differences` reassigns the code for `o`;
> `--set-font Times-Bold` then refused for coverage, and `/F2` — a bold face on
> the same page that **does** cover the run — was named by neither refusal.
> **Both verbs refused. Bold was unreachable on that page** for anyone who did
> not already know to try a resource pdfcer never mentioned.
>
> Fixed in `Pass 144.0`: the gate now asks `set_font`'s own acceptance test
> (`R221` — ask the accepting code, never restate its conditions), and offers a
> usable face from another family when none of the run's own family can show
> the run, saying so in the message. **`R90` is not weakened**: synthesis is
> still a fallback for when no real face *resolves*; "resolves" simply now
> means what a caller assumed it meant.
>
> Note the shape rather than only the fix. *"Every page is covered"* is a
> claim quantified over all cases and verified on the cases that came to mind —
> the same failure as *"no such verb exists"*, wearing the opposite sign
> (`R220` clause (d)).
>
> **The guidance below — *"do not grey out a bold button"* — was and remains
> TRUE**, and was deliberately not touched by this correction.

~~`R90` is why it is never silent: synthesis is applied **only when asked for
explicitly**, never as a preference…~~ — **struck, and false twice over since
decision 106.** Under `auto` synthesis IS applied as a preference (a persisted
one, `style_policy`), and *"never silent"* is the exact claim `Pass 179.2`
deleted from the disclosure string itself because the ruling falsified it.

What survives, and it is the half that matters: **synthesis is still never an
alternative to a real face** — the ladder always prefers a genuine one, in
every posture, and the report still says in the operator's words *"a FALLBACK,
not an alternative to a real face […] the letterforms are the regular face's,
thickened."* `R90`'s guarantee is now **reachable rather than mandatory**: set
`style_policy = refuse` and you have `R90`'s original gate back, unchanged.

**Nothing is undisclosed.** Removing the gate did not remove the disclosure —
`auto` reports the passed-over face on stdout, `warn` on stderr.

**The one real refusal, and it is narrow.** Synthetic *italic* premultiplies
a shear into the run's text matrix, which is **not** text state and so is not
covered by the restore ladder. It brackets the run with two absolute `Tm`
operators — and a `Tm` sets the text **line** matrix too (§9.4.2 Table 108),
so a following `Td`/`TD`/`T*` would derive its line from pdfcer's matrix
instead of the producer's origin and land shifted by this run's advance.
Refused by name rather than mis-positioned. Bold has no equivalent limit: it
is `Tr` + `w` + a stroking colour, all restored by value.

⇒ **What FF-C actually blocks is a *real* bold or italic FACE — a
typographic-quality gap, not a capability gap.** For a UI: **do not grey out
a bold button.** Offer it, and surface the synthesis disclosure when it fires.

#### ★★ Deciding where the Bold button routes, and the one way to ask wrong (`Pass 148.0`)

Two queries answer *"real face or synthesis?"* before the click, and **both
refuse an empty `find` that carries no pin**:

- `EditSession::preview_style_resolution(page, find, pinned_span, want)` —
  what `set_synthetic` **would do**: `RealFaceResolves { real_font, resource,
  selector }` ⇒ route to `set_font(selector)`; `WouldSynthesize` ⇒ route to
  `set_synthetic`.
- `EditSession::preview_font_resources(page, find, pinned_span)` —
  `real_bold()` / `real_italic()`, the same decision from the font side.

★ **An empty `find` is not a wildcard.** With a `pinned_span` it means *the
whole pinned operator*; without one it is **refused**. That refusal is a
`Pass 148.0` behaviour change and it replaced something worse: the query used
to answer about the page's **first** show operator, and on
`fixtures/synthetic/textedit/format_family.pdf` it named `Times-Bold` — the
one face there that cannot show the run. A Bold button routed from that answer
called `set_font("Times-Bold")` and got a refusal, so there was **no bold by
either route**. That is the `§3.6` defect corrected above, reached through a
different door.

⇒ The guidance is unchanged and now has an instrument behind it: **offer the
button, ask one of these two which way it goes, and never pass an empty
`find` without a pin.**

#### The `set_font` limit, stated for what it is

For a UI this is worse than a run-level limit would be: the predicate is a
property of the **page**, not of the selection, so the same button on
identical-looking text behaves differently in two files. It is a **named
refusal** (`FormatError::TargetFontMissing`), never a silent no-op, so a
shell can drive its control from the error — but driving a control from an
error means offering it, having it pressed, and then apologising.

**`Pass 142.1` shipped the pre-flight that removes that.**
`EditSession::preview_font_resources(page_index, find, pinned_span)` answers,
for one located run, which of the page's `/Font` resources `set_font` would
accept — by calling the accepting code, not by describing it. Per entry it
gives the **selector that reaches that resource** (the resource key, not the
`/BaseFont`, when the page carries two subsets of one face — the common case),
the acceptance verdict with `set_font`'s own refusal message verbatim, and
whether a real Bold or Italic of that family **would be accepted**. See
`02-editing-and-saving.md` §1 for the full contract. `pdfcer font-preflight`
is the same answer from a shell.

### What is refused, and it is never a silent substitution

- **coverage failure** — a target face that cannot show every character in
  the run. Never `.notdef`;
- **an outlined/vector run** — there is no font to swap;
- **an embedded-subset target** not already carrying the resulting code
  (also FF-C).

### ★ What the UI must disclose

- a **narrowing colour conversion**, when the run was painted in a non-device
  space;
- the **relayout**: the line was shifted by `advance_delta` and *may* now
  overflow its original right margin. Reflow is the default; pinning the tail
  is the alternative;
- a formatting change inside a **tagged** run preserves its BDC/EMC+MCID
  wrapper and discloses that the structure tree went stale — pdfcer does not
  reproduce Acrobat's tag-corruption defect (`R72`);
- changing a run's width inside a **justified** line invalidates that line's
  slack; pdfcer discloses that and offers re-justification rather than leaving
  it wrong.

### Trap

`writer::content::set_font` is **not** this. Its name makes it look like the
answer and it is a low-level content-stream emitter, not a session verb. The
requesting shell found it, correctly concluded it was not the answer, and
then had nothing else to find.

---

## 4. Redaction

`core [x] · cli [x] · gui [x]` for mark and apply; **`gui [ ]`** for the
unencrypted-wrapper warning (`FEATURES.md:137-139`).

**ui_spec to read:** `docs/ui_specs/pass-8-redaction.md` — the
**Mark → Review → Apply** three-phase model. That phase split is the
feature's safety property, not a UI preference.

### 4.1 ★★ The single most important fact for a new shell

**The "runtime-verified true-removal proof" is NOT in `pdfcer-core`.** Core
provides `redact::apply_redactions` and *keeps the material* for a proof —
`RedactionReport::redacted_text` exists *"for the absence-proof gate to
grep"* (`redact.rs:317-319`). The proof itself is implemented in the **GUI
crate**:

`pdfce@cce414e:crates/pdfce-gui/src/redact_apply.rs`

| item | line |
|---|---|
| `pub fn prepare_redaction_apply(session: &EditSession) -> Result<PreparedRedaction, RedactApplyRefusal>` | `269` |
| `PreparedRedaction { bytes, report, verification, promoted_by_materialisation }` | `234` |
| `AbsenceVerification { strings_checked, strings_too_short_for_raw_check, raw_byte_residuals }` | `197` |
| `AbsenceVerification::is_clean()` | `219` |
| `RedactApplyRefusal { NothingToApply, FullRewriteUnavailable, MaterialisedDocumentUnreadable, CoreRefused, VerificationFailed { survivors } }` | `141` |
| `MIN_VERIFIABLE_LEN: usize = 4` | `127` |
| `leaked_in_decoded_streams` / `verify_absence` (private) | `338` / `361` |

A shell that calls `redact::apply_redactions` directly and writes the bytes
**ships an unverified redaction** and may legitimately not know it. Either
port `redact_apply.rs` into the new shell, or lift it into `pdfcer-core` — but
do not skip it. **UNVERIFIED — whether `prepare_redaction_apply` should be
promoted into `pdfcer-core` is an open engineering question; no decision
record was found. Raise it with the operator before duplicating the file.**

The three-way verdict it implements (`redact_apply.rs:80-84`):

| redacted text found… | verdict |
|---|---|
| in a **decoded stream** of the output | **REFUSE — write nothing.** *"Its survival is a real leak, not a coincidence, and no acknowledgement checkbox makes it acceptable."* |
| in the **raw bytes only** | **DISCLOSE** as a residual needing the operator's explicit acknowledgement — pdfcer cannot tell an un-recognised carrier from a coincidence |
| nowhere | **verified** — *"this is what licenses §5.1's wording contract to use the word 'verified' at all"* |

Two further constraints from the same file: there is **deliberately no
`to_incremental_bytes` call anywhere in it, and no fallback that could
introduce one** (`redact_apply.rs:33-55`); and it must be handed the
**session**, not `session.document()` — passing the base revision would apply
**zero** marks placed this session *"and report success … not a disclosure
that stayed silent, but an apply that removed nothing while saying it had"*
(`redact_apply.rs:59-67`). The same bug class was already fixed once in core
(`redact.rs:1892-1901`).

### 4.2 Entry points

**Mark (non-destructive, `&mut EditSession`, one undo entry each).**

| call | `edit.rs` | notes |
|---|---|---|
| `add_redaction(page_index, &RedactSpec) -> Result<ObjId, EditError>` | `10480` | hand-drawn / named region |
| `mark_redactions_by_search(query, case_insensitive) -> Result<Vec<ObjId>, EditError>` | `11512` | **literal** — `#`/`?` are ordinary characters |
| `mark_redactions_by_search_with(query, &TextSearchOptions)` | `11556` | adds whole-word |
| `mark_redactions_by_pattern(pattern, case_insensitive)` | `11584` | `#` = ASCII digit, `?` = any char, rest literal — e.g. `###-##-####` |
| `delete_redaction_mark(annot_id) -> Result<(), EditError>` | `10617` | refuses a non-`/Redact` subtype by name |

Empty query or pattern returns `Ok(vec![])`, not an error.
`RedactSpec` is `annot_author.rs:908`: `quads: Vec<Quad>`,
`fill: Option<Color>`, `overlay_text: Option<String>`, `quadding: Quadding`.
A "whole page" mark is just `Quad::from_rect(page.crop_box)` — **CropBox, not
MediaBox**, deliberately (`pdfce@cce414e:crates/pdfce-gui/src/main.rs:9881-9892`).

**Census (read-only, generic over `ObjectGraph`).**
`redact::redaction_marks(graph) -> Vec<RedactionMark>` (`redact.rs:1822`) and
`count_redaction_marks(graph) -> usize` (`redact.rs:1921`).
`RedactionMark { page_index, annot_id, rect }` (`redact.rs:1792`).
**Pass `&session.graph()`, never `&document`** — see §4.4.

**Apply (destructive).**
`redact::apply_redactions(doc: &Document, options: &SaveOptions) -> Result<(Vec<u8>, RedactionReport), RedactError>`
(`redact.rs:1079`). Takes a **`&Document`, not an `EditSession`**, and returns
**new bytes**; it mutates nothing in place and **forces a full rewrite**
internally (`redact.rs:1220-1224`).

`RedactionReport` (`redact.rs:290`): `pages_redacted`, `marks_applied`,
`glyphs_removed`, `show_operators_edited`, `content_streams_rewritten`,
`annotations_removed`, `containers_decomposed`, `objects_promoted`,
`info_strings_scrubbed`, `estimated_width_fonts`, `overlay_text_burned`,
`overlay_ro_not_drawn`, `overlay_transparent`, **`images_cleared`,
`images_removed`, `images_cloned_shared`, `images_overcovered`,
`vector_paths_intersecting`, `marks_retained`** (all `Pass 245.0`),
**`vector_paths_cut`, `vector_paths_dropped`, `vector_clips_kept`**
(`Pass 246.0`), **`shadings_intersecting`** (`Pass 246.1`),
`carriers: Vec<CarrierStatus>`, `redacted_text`, `notes`; plus
`has_disclosed_residuals()` (`redact.rs:343`).
`CarrierStatus { carrier, present, action }` (`redact.rs:242`) with
`CarrierAction::{Absent, Scrubbed, DroppedByRewrite, DisclosedNotScrubbed}`
(`redact.rs:256`) and `as_str()` (`:274`) yielding
`"DISCLOSED_NOT_SCRUBBED"`. The thirteen carriers: `info`, `xmp`, **`images`**,
`xfa`, `struct_tree`, `attachments`, `ocg`, `thumbnails`, `object_streams`,
`prior_revisions`, `overlapping_annotations`, **`vector_paths`**,
**`shadings`** (`sh` paints whose clip box meets a region — a shading fills
its whole clip (§8.7.4.5.1) and is not cut this build, so each is a
`DisclosedNotScrubbed` residual; `Pass 246.1`).

**Images (`Pass 245.0`, `redact_image.rs`).** A raster image a region
touches — image XObject or inline, any codec pdfcer decodes (raw, DCT, CCITT,
JBIG2, JPX) — has the covered sample cells **destroyed** (overwritten, then
re-encoded losslessly as `FlateDecode`; its `/SMask` and stencil `/Mask` are
cleared over the same cells because a soft mask's alpha is a shape). A
placement one region contains entirely is **removed** from the page (the
`Do`/`BI…EI` is deleted). An image also painted elsewhere — another page, a
form, an appearance stream, an unmarked placement on the same page — is
**copied-on-write**: the marked placement is rebound to a fresh
`/XObject` name (`/pdfceRd<obj>_<n>`) holding the cleared clone, the
original survives for its other placements, and `images_cloned_shared` plus
a `SHARED` note say so. When every use of the original was marked, the
original object is **tombstoned** in place (a 1×1 paper-sample image under the
same object number, so no resource dictionary dangles). Every placement gets
one note naming its page, position, size and fate.

**A mark can be RETAINED.** When an image's samples cannot be decoded (a
codec feature pdfcer lacks, a corrupt codestream, a bit depth Flate cannot
carry), every mark touching that placement is **left in the output as an
unapplied `/Redact` annotation** — nothing removed under it, no box drawn
over it — and counted in `marks_retained`, with a note naming the placement
and the reason; the `images` carrier reads `DisclosedNotScrubbed`, so
`has_disclosed_residuals()` is `true`. **A shell must read `marks_retained`
before presenting the output as redacted**: a non-zero value means the
document still carries marks. Only when *no* mark at all can be applied does
apply refuse, with `ImageUndestroyable`.

**Vector paths are CUT (`Pass 246.0`, `redact_vector.rs`).** A painted
path object (`S`/`s`/`f`/`F`/`f*`/`B`/`B*`/`b`/`b*`; never `n`) whose
geometry — not merely its box — meets a region is rewritten in its own
coordinates so nothing it paints lies inside: strokes are cut against the
region expanded by one stroke width (lines by Liang–Barsky, cubics by
subdivision, kept as curves where whole), fills are clipped to the region's
complement as up to four strip objects (Sutherland–Hodgman, winding
preserved for nonzero and even-odd alike), and a path wholly inside is
deleted. `vector_paths_cut` / `vector_paths_dropped` count them. A
clip-marked object (`W`/`W*` before the paint) is emitted as the cut paint
followed by the ORIGINAL geometry as `W n` — §8.5.4 applies the clip after
painting and shrinking it would hide later, unmarked content — counted in
`vector_clips_kept` and noted, because the kept geometry is a shape in the
file even though it paints nothing. `vector_paths_intersecting` is now the
RESIDUAL: a malformed path object with a foreign operator inside it (§8.2
forbids that) cannot be replaced as a unit and is disclosed through the
`vector_paths` carrier as `DisclosedNotScrubbed`; on every well-formed page
it is zero and the carrier reads `Scrubbed`. Measured on the operator's
drawings: a whole-page mark drops 780 and 1,089 path objects respectively,
a corner mark cuts 25, and `pdfcer-render`'s
`redaction_leaves_no_ink.rs` proves by pixels that the region renders white.

`RedactError` (`redact.rs:195`): `PageTree`, `NothingToApply`,
**`ImageUndestroyable { page, reason }`** (replaces `ImageRegion { page }`,
which is gone as of `Pass 245.0`), `Content { page, source }`, `Encrypted`,
`Write`.

### 4.3 Minimal worked sequence

Mark-then-apply, from `crates/pdfcer-core/src/redact.rs:2006-2014` and
`:2182-2214`.

```rust
use pdfcer_core::{document::Document, edit::EditSession, redact, writer::SaveOptions};

// 1. MARK. Non-destructive. The marks live in the session.
let mut session = EditSession::new(Document::from_bytes(bytes)?);
let ids = session.mark_redactions_by_search("SECRET", false)?;

// 2. REVIEW. Census over the SESSION graph so unsaved marks are counted.
let marks = redact::redaction_marks(&session.graph());
let pending = redact::count_redaction_marks(&session.graph());
let _ = (ids, marks, pending); // ← the operator reviews and may delete_redaction_mark

// 3. APPLY. In the real shell this goes through the verified wrapper of §4.1.
//    The raw core call, for reference:
let (out, report) = redact::apply_redactions(&session.document(), &SaveOptions::identity())?;
//    ⚠ WRONG for a session with unsaved marks — see §4.1. Materialise first.

if report.has_disclosed_residuals() { /* the operator MUST acknowledge */ }
for c in &report.carriers { let _ = (c.carrier, c.present, c.action.as_str()); }
let _ = out;
```

The `pdfcer` consumer path is a clean non-GUI reference:
`crates/pdfcer-cli/src/main.rs:13470` `cmd_redact_apply` → `apply_redactions`
(`:13491`) → per-carrier report (`:13527-13534`) → the acknowledgement gate
(`:13544-13552`) → exit code `exit::REDACTION_RESIDUALS` (`:13553`).

### 4.4 ★ What the UI must disclose

Redaction is the capability where a missed disclosure is not a usability
defect but a **security failure**. The cardinal rule, verbatim
(`redact.rs:12-18`):

> **pdfcer must NEVER claim content is redacted when it is not.**
> Under-redaction that is *disclosed* or *refused* is acceptable; silent
> under-redaction is a catastrophic failure.

1. **A mark is a RED OUTLINE, never a solid fill.**
   `annot_author.rs:903-906`: *"so a marked-but-unapplied region can never be
   mistaken for a completed redaction (the #1 real-world redaction failure is
   saving a marked doc believing it is done)."* Implemented at
   `annot_author.rs:955-967` — stroke RGB(1,0,0), width 1, `Paint::Stroke`.
   `RedactSpec::fill` is the **apply-time** `/IC`, **not** the preview colour
   (`annot_author.rs:911-913`). A shell that fills the mark black has built
   the exact trap the feature exists to prevent.
2. **A persistent, unmissable pending-marks warning.** Existing copy,
   `pdfce@cce414e:crates/pdfce-gui/src/ui_text.rs:109-113`:
   *"⚠ {count} UNAPPLIED redaction mark(s) — this document is NOT redacted;
   its marked content is still present until you apply the redactions"*; and
   `:6927-6936` *"{count} pending redaction mark(s) — the content underneath
   them is STILL IN THIS DOCUMENT."* Also the negative case, said plainly:
   *"No redaction marks in this document. Nothing is marked, and nothing has
   been removed."*
3. **Apply is irreversible and writes a NEW file.** First line of the modal,
   in the warning colour (`ui_text.rs:7159-7164`): *"Applying writes a NEW
   file with the marked content permanently removed. It is a full rewrite,
   not an edit: nothing in that file can bring the removed content back — not
   Undo, not a previous revision, not any recovery tool."*
4. **The three-rule wording contract** (`ui_text.rs:6879-6896`) — binding on
   any shell:
   1. never say *"removed"* unqualified when anything was left; the residual
      goes in the **same sentence**;
   2. never say *"verified"* unless a verification actually ran, and only
      from a clean `AbsenceVerification`;
   3. **never put the word "Undo" anywhere near a post-apply state.**
5. **The scanned-page caveat is mandatory, not decorative**
   (`ui_text.rs:7010-7020`, `:7053-7063`): *"It can only find text pdfcer can
   extract — on a scanned page with no text layer it will find nothing, which
   is not the same as there being nothing sensitive there."* This is the
   single most consequential sentence in the whole redaction UI.
6. **Zero matches is never silent** (`ui_text.rs:7102-7108`), and the search
   result line must say nothing has been removed yet
   (`ui_text.rs:7094-7098`).
7. **Per-carrier residuals, by name.** Heading (`ui_text.rs:7248-7250`):
   *"⚠ pdfcer could NOT remove the following — read this before continuing:"*,
   then one line per `CarrierStatus` with `action.as_str()`. The verification
   limit line too (`ui_text.rs:7239-7245`): strings under
   `MIN_VERIFIABLE_LEN` (4 chars) *"were too short for a whole-file byte
   search to say anything useful, so those were checked against the decoded
   page content only."*
8. **The removal summary is a measurement, not a prediction**
   (`ui_text.rs:7202-7207`) — the apply already ran in memory before the modal
   opened. Word it in the past tense. And `annotations_removed` is a
   **total, not an overlap count** (`ui_text.rs:7212-7226`); an earlier draft
   mis-attributed it, and the correction is recorded as *"overstating
   collateral damage is a smaller sin than understating it, and still a lie."*
9. **Friction on the confirm, deliberately** (`pdfce@cce414e:crates/pdfce-gui/src/main.rs:10146-10180`):
   **no default-button binding (Enter cannot confirm), no keyboard shortcut to
   open or confirm**, the absence is stated on screen, and the confirm button
   is gated one frame behind the acknowledgement checkboxes.
10. **Removing a mark needs no confirmation**, and the asymmetry is the point
    (`edit.rs:10603-10608`): *"It changes nothing about the page's content. A
    mark that was never applied never removed anything … the reason this
    method is safe to offer with no confirmation while `apply_redactions` is
    not."*

### 4.5 Traps

- **Redaction is the one deliberate exception to round-trip / minimal-diff**
  (`redact.rs:3-8`, R35, `ARCHITECTURE.md` §5 corollary) — *"correctness IS
  security."* It **forces a full rewrite, never incremental**
  (`redact.rs:1066-1070`), because an incremental save *"structurally
  preserves superseded content … the 'removed' text would sit in the saved
  file one `startxref` hop away, trivially recoverable by any parser that
  walks `/Prev`"* (`pdfce@cce414e:crates/pdfce-gui/src/redact_apply.rs:22-28`).
- **An image intersecting a region is DESTROYED, never clipped — and a mark
  over an undecodable image is RETAINED, not applied.** (`Pass 245.0`;
  before it, `RedactError::ImageRegion` refused the whole apply, and a shell
  written against that variant will not compile.) Read `marks_retained`
  after every apply: an output with retained marks is **not redacted**, and
  `list-redactions` on it says so. `ImageUndestroyable` is raised only when
  nothing at all could be applied. A shared image's original survives for
  its unmarked placements by design — the `SHARED` note is the disclosure,
  not a defect.
- **A cut path is several path objects.** A dashed stroke restarts its dash
  phase at every cut, and a `B` becomes an `f` followed by an `S`. Cosmetic,
  disclosed in the notes, never a leak. A `W`-marked object keeps its
  original geometry as the clip (see §4.2) — say so if the clip's shape
  could itself be sensitive.
- **Destroyed image cells are PAPER, not black.** The colour space's no-ink
  sample (white for grey/RGB, zero ink for CMYK, unpainted for a mask,
  `/Decode`-aware), and a soft mask goes transparent over the same cells —
  so a mark with no `/IC` leaves the image region looking like the page
  behind it, consistent with Table 192. A shell that expected a black block
  will not see one unless the mark carries an `/IC`.
- **`RedactionMark::rect` is display information only.**
  *"This must never be used to decide what gets removed, only to describe a
  mark to a human"* (`redact.rs:1786-1790`) — apply uses `/QuadPoints`.
- **Never cache the mark list.** It is produced fresh on every call —
  *"never a cached list a UI keeps and patches incrementally"*
  (`redact.rs:1776-1783`).
- **Census must read the session graph, not the base document.** The
  `&Document`-only version of `count_redaction_marks` meant *"a `/Redact` mark
  the operator placed during this session was not counted"*
  (`redact.rs:1892-1901`). The same class of error, one layer up, is §4.1's
  base-revision trap.
- **Search-marking reads the session view, not `self.document()`** — the
  pre-fix behaviour placed marks *"on a different page than the one holding
  the matched text — silently, with correct-looking geometry"*
  (`edit.rs:11610-11631`).
- **Find and Redact must use the same options.** `mark_redactions_by_search_with`
  exists because a Find bar with whole-word on and a redact verb without it
  means *"the mark set is a superset of what you were shown"*, and that *"is
  not a cosmetic mismatch"* (`edit.rs:11529-11538`). There is deliberately no
  `_with` variant for the pattern form (`edit.rs:11593-11598`).
- **A redaction mark carries no `/F Print`** — it is *"transient review
  state, not page content — it must not print"* (`edit.rs:10540-10541`).
- **`delete_redaction_mark` is deliberately not a general
  `delete_annotation`** and refuses any non-`/Redact` subtype by name
  (`EditError::NotARedactionMark`, `edit.rs:10582-10588`).
- **Width estimation is cosmetic, never a security regression** — the
  security guarantee is independent of width accuracy
  (`redact.rs:66-71`); `estimated_width_fonts` is disclosed as a layout
  caveat, not a redaction caveat.
- **Unencrypted wrapper (§7.6.7) — `gui [ ]` opportunity.** Core and CLI
  detect a wrapper document and warn that *"the visible page is a cover, not
  the document"*; no GUI surfaces it (`FEATURES.md:139`). A new shell showing
  a wrapper's cover page without that warning is showing the wrong document
  and saying nothing.
  The entry point is `pdfcer_core::wrapper::detect(graph) -> WrapperInfo`
  (`wrapper.rs:90`), generic over `ObjectGraph`. `WrapperInfo`
  (`wrapper.rs:66`) carries `is_wrapper`, `payload_name: Option<String>` and
  `payload_count: usize`. It is *"cheap: one catalog lookup and a walk of a
  normally-empty array. **Safe to call on every document open, which is the
  point — a detector an operator has to remember to run is a detector that
  does not fire on the day it matters**"* (`wrapper.rs:83-87`). Call it on
  open, unconditionally, and **name the payload** — *"'this document wraps an
  encrypted payload' is a weaker statement than naming it"* (`wrapper.rs:69-72`).
  `payload_count > 1` is reported rather than collapsed and should be shown.

---

## 5. OCR — substrate, writer, and engine

`core [x] · cli [x] · gui [x]` — **OCR works end to end**, and as of
`Pass 129.0` (`181d9bd`, 2026-08-25) that is a measured statement rather than
an architectural one.

**This section has now been rewritten three times and every prior version was
wrong in a different direction**, which is worth knowing if you are holding a
cached copy:

| version | said | why it was wrong |
|---|---|---|
| pre-2026-08-13 | no code writes a text layer; no engine exists | true when written; `Pass 71.0` slices 2 and 3 landed |
| 2026-08-13 → 2026-08-25 | *"the last piece — the model weights — is not in the repository yet"* | the weights landed the same day the operator answered the licence question `YES` |
| — | *(implicitly)* that shipping the weights meant OCR worked | **it did not.** See the banner below |

### ★★★ OCR PRODUCED GARBAGE ON EVERY PAGE UNTIL 2026-08-25

Read this before trusting any OCR number you find in any document, including
your own.

The bundled **detection** model — the network that finds *where* words are —
did not work with `ocrs` 0.12.2. On a clean 150 dpi render of a page of 12 pt
Helvetica it returned sixteen fragments (`"1"`, `"?"`, `"E"`) at the right page
margin plus one "word" whose bounding box was the entire page. Noise, not
degraded output. The **recognition** model was fine throughout; the fault was
isolated by swapping one file at a time, and the working detection build lives
on a different channel from the recognition one. See
`crates/pdfcer-core/assets/models/ocrs/PROVENANCE.md` for the four-row table and
the instruction not to "tidy" the two files back onto one channel.

★ **The consequence for any consumer: every accuracy figure measured before
`181d9bd` is a measurement of noise.** If you derived a DPI curve, a target
pixel count, or a pinned test from OCR output, re-measure. Check which
detection model you have first — 2,510,284 B / `f15cfb56…` works, 2,523,564 B
/ `614aafab…` does not — or you will measure the same thing twice.

**What it does now**, on a synthetic 200 dpi scan with blur, 0.35° skew, sensor
noise and grey paper: **47 of 47 words**, with the invisible layer landing a
median **0.90 pt** from the ink on the clean control (≈ 1/80 inch).
`tools/ocr-accuracy.py` is the scorer; it measures **content and position
separately**, because a mirrored or transposed text layer scores 100 % on
content and is useless.

### 5.1 The four layers, and exactly where the boundary sits today

OCR in `pdfcer-core` is four separable pieces, and knowing which one you are
missing is the difference between a two-hour job and a wrong architecture.

| # | piece | module | state |
|---|---|---|---|
| 1 | **Types + coordinates** — `RecognizedWord`, `OcrPage`, the `OcrEngine` trait, the single y-flip | `ocr` (always compiled) | **done** |
| 2 | **The sandwich writer** — words → an invisible, selectable text layer in a real PDF | `ocr::layer` (always compiled) | **done**, 21 tests |
| 3 | **A recogniser** — pixels → words | `ocr::engine_ocrs`, behind the `ocrs` Cargo feature | **done**, code only |
| 4 | **Model weights** — the ~12 MB of `.rten` files piece 3 loads | not in the repo | **THE GAP** |

**So: everything is wired except the weights.** `OcrsEngine::from_model_dir`
compiles, runs, and returns a named `ModelMissing` error today. Nothing is
stubbed, nothing is faked, and the failure is a clean refusal that tells the
operator which file to put where.

**What this means for you.** You can build the entire OCR panel now — the
command, the progress, the report, the error path — against the real API, and
it will work end to end the moment weights are present. You do **not** need to
design around a placeholder. The one thing you cannot do is ship a build that
recognises text, and that is a licensing/packaging step on the pdfcer side, not
an API gap.

### 5.2 Public surface

**Piece 1 — types and coordinates** (`crates/pdfcer-core/src/ocr/mod.rs`)

| item | `file:line` |
|---|---|
| `RecognizedWord { text, rect, confidence: Option<f32> }` | `ocr/mod.rs:87` |
| `OcrPage { words, confidence_available: bool }` | `ocr/mod.rs:108` |
| `OcrPage::mean_confidence() -> Option<f32>` | `ocr/mod.rs:132` |
| `OcrPage::words_needing_review(threshold) -> Vec<&RecognizedWord>` | `ocr/mod.rs:148` |
| `trait OcrEngine { recognize(w, h, pixels); reports_confidence() }` | `ocr/mod.rs:163` |
| `words_to_page_space(words, img_w, img_h, page_rect)` — the y-flip, **`/Rotate 0` ONLY** | `ocr/mod.rs:211` |
| ★ `words_to_page_space_on(words, img_w, img_h, PagePlacement)` — **use this one** | `ocr/mod.rs` |
| `PagePlacement::new(rect, rotate)` / `PagePlacement::upright(rect)` | `ocr/mod.rs` |

**Piece 2 — the writer** (`crates/pdfcer-core/src/ocr/layer.rs`)

| item | `file:line` |
|---|---|
| `add_ocr_layer(&doc, page_index, &OcrPage, &opts) -> Result<OcrLayerOutcome, OcrLayerError>` | `ocr/layer.rs:603` |
| `build_layer_content(&OcrPage, font_name, &opts) -> (Vec<u8>, OcrLayerReport)` — **pure**, no `Document`, no I/O | `ocr/layer.rs:496` |
| `OcrLayerOptions::new()` / `.with_font(Std14)` | `ocr/layer.rs:216`, `:238`, `:244` |
| `OcrLayerReport` — see §5.4, every field is a disclosure | `ocr/layer.rs:259` |
| `OcrLayerReport::disclosures() -> Vec<String>` — **ready-to-show lines** | `ocr/layer.rs:313` |
| `OcrLayerError` (`PageIndex`, `Encrypted`, `NothingToWrite`, `Unsupported`, `PageTree`, `ObjectNumbersExhausted`, `Write`) | `ocr/layer.rs:365` |
| `OcrLayerOutcome { bytes, report }` | `ocr/layer.rs:397` |
| `HELVETICA_ASCENT_FRAC` 0.718 · `HELVETICA_DESCENT_FRAC` 0.207 · `MIN_TZ` 1.0 · `MAX_TZ` 10 000.0 | `ocr/layer.rs:182`, `:190`, `:199`, `:207` |

**Piece 3 — the engine** (`crates/pdfcer-core/src/ocr/engine_ocrs.rs`, feature `ocrs`, **on by default**)

| item | `file:line` |
|---|---|
| `OcrsEngine::from_model_dir(&Path)` | `engine_ocrs.rs:184` |
| `OcrsEngine::from_model_files(&Path, &Path)` | `engine_ocrs.rs:193` |
| `MODEL_DIR` `"ocrs"` · `DETECTION_MODEL` · `RECOGNITION_MODEL` | `engine_ocrs.rs:86`, `:89`, `:97` |
| `OcrsEngineError` (`ModelMissing`, `ModelLoad`, `ImageSize`, `Image`, `Recognition`) | `engine_ocrs.rs:108` |

**Piece 4 — finding the weights on disk** (`crates/pdfcer-core/src/ocr/models.rs`)

| item | `file:line` |
|---|---|
| `resolve_model_dir(engine, explicit, exe_dir, user_data) -> Result<ModelSource, ModelsNotFound>` | `models.rs` |
| ★ `resolve_model_dir_with(…, required: &[&str])` — a dir only counts if it CONTAINS the files | `models.rs` |
| `ModelSource` (`OperatorSupplied` / `BesideExecutable` / `UserData`), `.path()` | `models.rs:84`, `:100` |
| `ModelsNotFound { engine, searched }` — **carries every path tried** | `models.rs:126` |

### 5.3 Worked sequence, end to end

```rust
use pdfcer_core::document::Document;
use pdfcer_core::ocr::{OcrEngine as _, OcrPage, PagePlacement, layer, models,
                      words_to_page_space_on};
use pdfcer_core::ocr::engine_ocrs::{MODEL_DIR, OcrsEngine};

// 1. Find the weights. An operator-named path that does not exist is REPORTED,
//    never silently replaced by a bundled copy (models.rs:164).
// `_with`, naming the engine's two files. An EMPTY `models/ocrs` otherwise
// resolves AND SHADOWS a good directory further down the search order, and
// the failure then arrives later wearing the engine's vocabulary instead of
// the resolver's.
let src = models::resolve_model_dir_with(
    MODEL_DIR, explicit, exe_dir, user_data,
    &[engine_ocrs::DETECTION_MODEL, engine_ocrs::RECOGNITION_MODEL],
)?;

// 2. Load the engine once, not per page. Both models load eagerly, so a bad
//    install fails here rather than on page 340 of a batch.
let engine = OcrsEngine::from_model_dir(src.path())?;

// 3. Rasterise the page yourself (pdfcer-render) and give it 8-bit greyscale.
//    Returned rects are IMAGE PIXELS, y-DOWN.
let words = engine.recognize(img_w, img_h, &grey)?;

// 4. Flip to page space. This is the ONLY place a flip may happen.
let page_rect = /* the page's crop box, or the region the image covers */;
let page = OcrPage {
    // ★★ THE ROTATION TRAP. `pdfcer-render` HONOURS `/Rotate` -
    // `page_device_geometry` swaps the raster's axes at 90 and 270 - and
    // `words_to_page_space` does not. A rotation-aware rasteriser feeding a
    // rotation-blind mapping yields an invisible layer TRANSPOSED relative to
    // the ink; the page still looks perfect and the only symptom is that
    // selecting a word gets a different one. Scanner drivers write `/Rotate`
    // rather than re-imaging, so this is the NORM in the population OCR
    // exists for.
    //
    // `page_rect` must be the CROP box, which is what the rasteriser drew.
    words: words_to_page_space_on(
        &words, img_w, img_h,
        PagePlacement::new(page.crop_box, i32::from(page.rotate)),
    ),
    confidence_available: engine.reports_confidence(),
};

// 5. Write the layer. Additive: one content stream, one font dict, one page
//    dict. The scan is never decoded or re-encoded.
let out = layer::add_ocr_layer(&doc, page_index, &page, &layer::OcrLayerOptions::new())?;
std::fs::write(path, &out.bytes)?;

// 6. Disclose — off-canvas. See §5.4.
for line in out.report.disclosures() { /* status line / results panel */ }
```

**Step 4 is the one to get right.** `confidence_available` must come from
`engine.reports_confidence()`, not from "did any word have a score" — those
differ, and the difference is the whole point (§5.4).

### 5.4 ★ What the UI must disclose

OCR is **the single largest inference pdfcer makes** — every word is a guess.
Rule 4 as amended by **decision 059** says precisely what to do about that, and
for OCR the amendment does more work than anywhere else in this document:

**Render normally. The page must look untouched — because it is.**
The layer is written at text rendering mode 3 (ISO 32000-1 §9.3.6 Table 106,
*"neither fill nor stroke text (invisible)"*), and
`crates/pdfcer-render/tests/ocr_layer_is_invisible.rs` asserts **not one pixel
of 7,755,264 changes**. The operator asked for exactly this: *"I want OCRed
stuff to look normal when the command is executed too."*

**So: never draw a confidence tint, badge, dashed outline or highlight into the
page view.** That is not a style preference — provisional marking is a second
rendering path for the same content, and two paths drift. It is the bug class
decision 059 exists to delete.

**Disclose off-canvas**, from `OcrLayerReport`:

| field | what it must say, and the trap |
|---|---|
| `words_written` | how much text was added. |
| `confidence_available` | **`false` is a fact, and must be stated as one.** `disclosures()` says *"this engine reports NO per-word confidence, so no word here has been scored either way — that is not the same as a high score."* **`ocrs` — the only engine currently wired — reports no confidence at all**, so this is the live case, not a hypothetical. |
| `mean_confidence: Option<f32>` | `None` means *nothing was scored*. **Never render it as 0%** — that is a specific, alarming, false claim about text nobody scored. |
| `words_substituted` | characters with no WinAnsi code, written as `?`. A **high count means the page is in a script a Standard-14 face cannot represent** (CJK, Cyrillic, Greek, Arabic) — a real limit, surfaced here rather than discovered as a page of question marks. |
| `words_skipped` | empty text or a degenerate box. A large number means the engine and the page geometry disagree — a genuine diagnosis. |
| `words_scale_clamped` | `Tz` hit `MIN_TZ`/`MAX_TZ`; selection there will not track the ink. |

`OcrLayerReport::disclosures()` builds all of these as finished strings and
**says nothing when there is nothing to say** — a report that always emits a
paragraph trains the reader to skip it. Use it rather than composing your own,
so the CLI and the GUI cannot disagree about what was disclosed.

**`OcrPage::words_needing_review(threshold)` counts unscored words as needing
review.** That is deliberate: excluding them would let an engine that reports
nothing produce an empty needs-review list and look *more* trustworthy than one
that reports honestly.

### 5.5 Traps

1. **The y-flip belongs to `words_to_page_space` and nowhere else.** Engines
   report y-DOWN image pixels; PDF is y-UP. `ocr::engine_ocrs` deliberately
   does **not** flip. A "helpful" flip in a second place produces a layer that
   is mirrored *twice* — i.e. correct — for one engine and mirrored once for
   the next, and that defect gets blamed on the wrong module for a long time.
   The symptom is silent: the page looks perfect until someone selects a line
   and gets a different one.
2. **`add_ocr_layer` refuses `NothingToWrite` rather than writing an empty
   layer.** Do not treat it as a failure to report loudly — on a blank or
   image-free page it is the correct answer, and writing a stream plus a font
   for zero words would grow the file and change its bytes to accomplish
   nothing.
3. **`OcrsEngine::recognize` validates the buffer length and refuses a
   mismatch.** `ocrs`'s own `ImageSource::from_bytes` infers a channel count
   from `len / (w × h)`, so a buffer twice the expected size is silently taken
   as a 2-channel image and "works", recognising noise. Pass exactly
   `width × height` bytes of 8-bit greyscale.
4. **Load the engine once.** Both models load eagerly in the constructor. Doing
   it per page re-reads ~12 MB and re-initialises the runtime every time.
5. **`ASCENT_FRAC` is ambiguous in this crate; the OCR ones carry the face.**
   `text_edit::addtext` and `text_edit::reflow` hold `ASCENT_FRAC`/`DESCENT_FRAC`
   = **0.75/0.25** (the block model's nominal figures, shared so a run's box and
   a reflowed line's box agree). `ocr::layer` holds
   `HELVETICA_ASCENT_FRAC`/`HELVETICA_DESCENT_FRAC` = **0.718/0.207** (the real
   AFM metrics). **Both are correct.** They differ by 0.043 em, which is small
   enough to look like a rounding artefact rather than a different quantity —
   so if you compare authored geometry against extracted geometry you will see a
   constant sub-point offset (0.558 pt at 13 pt) that is **not a bug**.
6. **A `Tz` error is invisible to an origin check.** If you write your own
   geometry assertions, assert on the **extent**, not just the position: `Tm`
   sets the left edge correctly no matter how wrong the horizontal scaling is.
   This cost pdfcer a test that named the defect in its own failure message and
   could not detect it.

### 5.6 What is still owed on the pdfcer side

- **The model weights** (~12 MB, two `.rten` files). The licence question is
  settled — the operator answered **yes** on 2026-08-13 to a CC-BY-SA-4.0 model
  file shipping inside pdfcer's MIT portable folder — but the files are not
  committed, and committing ~12 MB of binary into a public repository's history
  is permanent, so it is a deliberate step rather than a routine one.
- **A `pdfcer ocr` subcommand.** Not yet present; `grep -rn "ocr" crates/pdfcer-cli/src/`
  returns zero hits at this commit.
- **A second engine.** The operator's decision (2026-08-12) was *"just build
  for both"*. `ocrs` is the first; the feature is named after the crate rather
  than the capability precisely so the second can land without a rename.


## 6. Print & imposition (`pdfcer-print`)

`Print`: `core [x] · cli [x] · gui [x]` (`FEATURES.md:166`).
**`Imposition`: `core — · cli [x] · gui [ ]`** (`FEATURES.md:167`) — *"N-up,
booklet, poster; mutually exclusive, refused in combination. **No GUI surface
at all.**"* `FEATURES.md:210` names the planned work: *"needs the sheet
composition extracted into `pdfcer-print` so both shells share one
implementation."*

**This is the largest greenfield opportunity in the document.**
`grep -rn "imposition::" crates/` finds hits **only** in
`crates/pdfcer-cli/src/main.rs`. The planners are built, tested and unused by
any GUI.

**Crate posture:** *"core rasterises, the shell spools"* (`lib.rs:16`).
`pdfcer-print` *"does device setup, placement and blitting, and knows nothing
about PDF — which is why it does not depend on `pdfcer-render`"*
(`lib.rs:1893-1896`). **The caller rasterises.**
Every entry point returns `Err(PrintError::Unsupported)` on non-Windows
(`lib.rs:1848`, `:1858`, `:1874`, `:2323`).

### 6.1 Entry points

**Device.** `list_printers() -> Result<Vec<Printer>, PrintError>`
`lib.rs:260`; `printer_caps(name) -> Result<PrinterCaps, PrintError>`
`lib.rs:432`; `device_features(printer) -> Result<DeviceFeatures, PrintError>`
`lib.rs:1820` (`supports_duplex`, `max_copies`).

**Planning.** `JobSpec` `lib.rs:1179` (`pages`, `mode: ScaleMode`, `max_dpi`,
`subset: PageSubset`, `reverse`, `copies`, `collate`) →
`JobSpec::sequence()` `lib.rs:1263`, `first_page_pt(&page_sizes)` `:1339`.
`DeviceGeometry::from_caps(&caps, requested_orientation, first_page_pt)`
`lib.rs:1779` — **the only route from `PrinterCaps` to `DeviceGeometry`**.
`job_resolution(&device, &spec) -> JobResolution` `lib.rs:1464`;
`plan_job(&device, &page_sizes, &spec) -> Vec<PagePlan>` `lib.rs:1486`.
`PagePlan { index, placement, render_scale }` `lib.rs:1357`.
`place_page(page, printable, ScaleMode) -> Placement` `lib.rs:544`;
`ScaleMode { Fit, ActualSize, ShrinkOversized, Custom(f64) }` `lib.rs:496`.

**Spool.** `PageBitmap { width, height, rgba, placement, page_pt }`
`lib.rs:1898` — **RGBA8, row-major, top row first, i.e. `pixmap.data().to_vec()`
handed over unchanged** (`lib.rs:1903-1905`).
`spool(printer, &[PageBitmap], DryRun, output, DeviceSettings, first_page_pt) -> Result<SpoolReport, PrintError>`
`lib.rs:1979`. `SpoolReport { pages, printed, dpi, clipped_pages, job_id }`
`lib.rs:1943`.

**Imposition** (`pdfcer_print::imposition`, `lib.rs:104`). The planners take
**only** `(f64, f64)` printable areas and page-size slices — never a driver
type (`imposition.rs:31-34`) — and return rectangles you composite into one
pixmap per **sheet**:

| planner | `imposition.rs` | returns |
|---|---|---|
| `plan_n_up(printable_pt, &page_sizes, &NUpSpec)` | `644` | `Result<NUpLayout, ImpositionError>` |
| `plan_booklet(printable_pt, &page_sizes, &BookletSpec)` | `1145` | `Result<BookletLayout, ImpositionError>` |
| `booklet_pairing(page_count, Binding)` | `1025` | `Result<Vec<BookletSheet>, ImpositionError>` |
| `plan_poster(printable_pt, page_pt, &PosterSpec)` | `1435` | `Result<PosterLayout, ImpositionError>` |
| `fit_into_cell(page, cell, auto_rotate)` | `269` | `CellFit { rect, scale, rotated }` |

Specs: `NUpSpec { grid: NUpGrid, order: PageOrder, border, auto_rotate }`
`:557`; `BookletSpec { binding, subset, sheets: Option<(usize,usize)>, auto_rotate }`
`:911`; `PosterSpec { tile_scale, overlap_pt, cut_marks, labels, tile_only_large_pages, max_tiles }`
`:1243`. Limits: `MAX_CELLS_PER_SHEET = 1024` `:132`,
`DEFAULT_MAX_TILES = 400` `:142`, `MAX_BOOKLET_SHEETS = 100_000` `:150`.

### 6.2 Minimal worked sequences

**(a) The canonical print pipeline.** Both existing shells follow it
(`pdfce@cce414e:crates/pdfce-gui/src/print_flow.rs:637-702`, `:1793-1852`).

```rust
use pdfcer_print::{
    DeviceGeometry, DeviceSettings, DryRun, JobSpec, PageBitmap, ScaleMode,
    device_features, job_resolution, list_printers, plan_job, printer_caps, spool,
};

let printers = list_printers()?;                       // 1
let caps = printer_caps(&printers[0].name)?;           // 2
let features = device_features(&printers[0].name)?;    //   consult BEFORE offering duplex (R83)

let spec = JobSpec { mode: ScaleMode::Fit, ..job_spec_from_dialog() };   // 3
let settings = DeviceSettings::default();
let device = DeviceGeometry::from_caps(&caps, settings.orientation,      // 4
                                       spec.first_page_pt(&page_sizes));
let res = job_resolution(&device, &spec);              // 5  res.capped → disclose
let plans = plan_job(&device, &page_sizes, &spec);

let mut bitmaps = Vec::new();                          // 6  YOU rasterise
for plan in &plans {
    let rendered = pdfcer_render::render_page_with_view(
        &session.view(), &pages[plan.index], plan.render_scale as f32, &options)?;
    bitmaps.push(PageBitmap {
        width: rendered.pixmap.width(), height: rendered.pixmap.height(),
        rgba: rendered.pixmap.data().to_vec(),         // premultiplied, unchanged
        placement: plan.placement, page_pt: page_sizes[plan.index],
    });
}
let first_page_pt = bitmaps.first().map_or((612.0, 792.0), |b| b.page_pt); // ← NOT pages[0]
let report = spool(&printers[0].name, &bitmaps, DryRun::No, None, settings, first_page_pt)?;
let _ = (features, res, report.clipped_pages);
```

**(b) N-up imposition — the `gui [ ]` opportunity.** From
`crates/pdfcer-cli/src/main.rs:8612-8660`.

```rust
use pdfcer_print::imposition::{NUpGrid, NUpSpec, PageOrder, plan_n_up};

let nup = NUpSpec {
    grid: NUpGrid::Count(4),
    order: PageOrder::Horizontal,
    border: false,
    auto_rotate: true,
};
let sequence = spec.sequence();
let ordered_sizes: Vec<(f64, f64)> =
    sequence.iter().filter_map(|&i| page_sizes.get(i).copied()).collect();
let layout = plan_n_up(device.printable_pt, &ordered_sizes, &nup)?;

// One pixmap PER SHEET, at device DPI, filled white; then blit each slot.
for sheet_index in 0..layout.sheets {
    // let mut sheet = tiny_skia::Pixmap::new(w, h).unwrap(); sheet.fill(WHITE);
    for slot in layout.slots.iter().filter(|s| s.sheet == sheet_index) {
        let scale = (dpi / 72.0) * slot.fit.scale;
        let _rendered = pdfcer_render::render_page_with_view(
            &session.view(), &pages[sequence[slot.source]], scale as f32, &options)?;
        // blit into slot.fit.rect (y-DOWN, top-left origin — see §6.4)
        let _ = slot.border;
    }
    // push one PageBitmap per physical sheet, then spool once.
}
```

### 6.3 ★ What the UI must disclose

1. **★ Spooling is an irreversible outward-facing act** (`lib.rs:58-71`):
   *"Printing consumes paper, occupies a device other people may share, and
   cannot be undone. Nothing in this crate starts a job as a side effect of
   anything else: `spool` is the only function that reaches `StartDoc`, and it
   is reached only from a control an operator deliberately clicked."* A shell
   must never spool as a consequence of anything but a deliberate click.
2. **Clipping is REPORTED, not refused** — `Placement::clipped`
   (`lib.rs:529`) and `SpoolReport::clipped_pages` (`lib.rs:1950-1955`).
   *"Acrobat's documented behaviour here is to clip SILENTLY … pdfcer reports
   it instead"* (`lib.rs:522-528`). Show it **before** the job goes out; it is
   pdfcer's inference that the page will not fit.
3. **Resolution capping.** `JobResolution { dpi, device_dpi, capped }`
   (`lib.rs:1381`) — when `capped`, pdfcer is printing at less than the device
   can do, by pdfcer's own memory judgement. Say so; `uncapped_page_mb()`
   (`lib.rs:1400`) is the number that justifies it.
4. **Duplex is driver-gated, never simulated** (`lib.rs:1533-1543`): *"A
   printer that cannot do it will not be made to by reordering pages and
   asking the operator to reinsert the stack."* Consult
   `DeviceFeatures::supports_duplex` **before offering the control at all**
   (R83). A duplex setting the driver declines produces *"a job that silently
   comes out single-sided"* (`lib.rs:1560-1566`) — which is exactly why
   `DeviceSettings` is kept separate from `JobSpec`, so a shell can say it.
5. **`DryRun::Yes` is the development mode, not a test convenience**
   (`lib.rs:1915-1932`) — it runs every step except `StartDoc`/`StartPage`/
   `EndPage`/`EndDoc` and the blit. Offer it.
6. **Imposition: blanks and padding.** `BookletLayout::padded_pages` and
   `blank_positions` (`imposition.rs:1116-1120`) — pdfcer **added blank pages**
   to reach a multiple of four. That is an inference about intent; show the
   count. Likewise `PosterLayout::rows × columns` and `tiles` — how many
   sheets of paper the operator is about to consume.
7. **`CellFit::rotated`** (`imposition.rs:242`) — pdfcer **turned the page**
   to make it fit. Visible in the preview, and stated.

### 6.4 Traps

- **★ Imposition modes are mutually exclusive, and the guard is CLI-local.**
  `crates/pdfcer-cli/src/main.rs:8565-8596`: *"N-up, booklet and poster each
  REMAP the job rather than scale it, and no two of them compose. Before this
  guard existed the three branches ran in sequence and the last one to fire
  silently overwrote the others' work: `--poster --booklet` composed nine
  poster tiles, threw them away, and printed a booklet. The operator got a
  plausible job that was not the one they asked for, with no indication
  anything had been discarded."* And: *"Refusing is right rather than picking
  a precedence. There is no reading of `--poster --booklet` that is obviously
  intended, so any precedence pdfcer chose would be a guess presented as a
  result."*
  **`pdfcer-print` will not stop you. A new GUI shell must re-implement this
  guard.** Message shape: *"{modes} cannot be combined — each one changes the
  shape of the job, and no two of them compose. Pick one."*
- **★ Imposition `Rect` is y-DOWN, origin top-left — deliberately NOT PDF's
  convention** (`imposition.rs:36-53`): *"Introducing a second, y-up
  convention here would mean a flip on every hand-off, and a flip that is
  applied twice — or zero times — prints upside down, which is obvious on
  paper and invisible in every test that does not print."*
- **★ Booklet `sheets` counts PHYSICAL SHEETS, not document pages**
  (`imposition.rs:920-927`): *"'Sheets 1 to 1' of a 40-page booklet prints the
  outermost sheet, which carries document pages 40, 1, 2 and 39 … exactly what
  a document-page reading would get wrong while looking right."* An
  overrunning end is **clamped**; a range starting past the end is **refused**
  (`:1140-1144`).
- **★ Auto-rotate is always CLOCKWISE** (`imposition.rs:230-239`) — the
  direction is unsourced, and *"a sheet with one page turned clockwise and its
  neighbour turned counter-clockwise is unreadable at any head angle."* And
  `CellFit::rect` **already accounts for the rotation** (`:216-221`): placing
  the page by its unrotated size *"would produce a sideways page hanging out
  of its cell."*
- **★ `tiles_page` is measured AFTER the tile scale** (`imposition.rs:1302-1310`)
  — *"a 200-point page at 800% is a 1600-point poster … measuring the unscaled
  page instead would pass it through untiled and print the top-left corner of
  a poster eight times too big, silently."*
- **★ Odd/even is by DOCUMENT page number** (`lib.rs:1217-1224`): *"an
  operator printing '2-9, odd' means document pages 3, 5, 7, 9 — the numbers
  printed on the paper."* Order of operations is **subset → reverse → copies**
  (`lib.rs:1246-1261`).
- **★ The orientation page is the first page SENT, not `pages[0]`**
  (`lib.rs:1130`). Take `first_page_pt` from `bitmaps.first()`.
- **★ An asymmetric device renders at its SMALLER axis** (`lib.rs:758`,
  `:1466-1471`).
- **`Fit` ≠ `ShrinkOversized`** (`lib.rs:490-494`) — *"treating them as one,
  which is the natural simplification, silently blows a business card up to
  A4."*
- **`plan_n_up` degrades on a malformed page; `plan_poster` refuses.**
  Asymmetric on purpose: *"refusing a whole 40-page job over one malformed
  MediaBox would lose 39 good pages to save one bad one"* (`imposition.rs:1430-1434`)
  versus `DegeneratePage` (`:1444-1447`).
- **Every planner refuses rather than clamps** (`imposition.rs:55-60`):
  *"the output of this module becomes **paper**."* `ImpositionError`
  (`:313`) has twelve named variants — `NoPages`, `EmptySheet`,
  `DegeneratePage`, `ZeroCells`, `TooManyCells`, `SheetRangeEmpty`,
  `SheetRangeBeyondBooklet`, `BookletTooLarge`, `InvalidTileScale`,
  `NegativeOverlap`, `OverlapExceedsSheet`, `TooManyTiles` — surface each by
  name. It is deliberately **not `Eq`** (several variants carry `f64`).
- **Non-Windows `list_printers` returns `Err(Unsupported)`, not an empty
  `Vec`** (`lib.rs:1859-1866`) — *"reporting the same value for 'this platform
  cannot enumerate printers at all' would collapse two different facts into
  one and send a caller looking for hardware."*

---

## 7. Rasterising a page for display (`pdfcer-render`)

`core [x] · gui [x] · cli —` (`FEATURES.md:145-146`). A new shell rebuilds
the *worker*, not the rasteriser.

**Zero GUI/windowing dependency, verified by `cargo tree` in CI**
(`lib.rs:13-19`) — a CPU rasteriser is fine, a windowing toolkit is not. And
*"`pdfcer-render` itself never enumerates, opens, or reads a font (rule R19),
which is what makes the same document render to the same pixels on a CI
runner, a developer laptop, and the WASM fork"* (`lib.rs:224-228`).

### 7.1 The one call

```rust
pdfcer_render::render_page_with_view(
    view: &DocumentView<'_>, page: &Page, scale: f32, options: &RenderOptions
) -> Result<RenderedPage, RenderError>
```
`crates/pdfcer-render/src/lib.rs:235` — **the real implementation**;
`render_page` (`:165`), `render_page_with` (`:176`) and `render_page_view`
(`:211`) are wrappers.

- `scale` is **device pixels per user-space unit** = `dpi / 72.0`;
  `1.0 ≈ 72 DPI` (`lib.rs:154-155`).
- **`view` must be `session.view()`, not `session.document()`.** Decision 018,
  `lib.rs:188-199`: *"Until Pass 17.0 this crate only knew how to render a
  `&Document`, and the GUI could only give it `EditSession::document()` — the
  BASE revision. Every editing feature from Pass 3.1 to Pass 16.2 therefore
  authored correctly and displayed not at all."* This is the same base-revision
  trap as §4.1, in a different subsystem. `DocumentView` is re-exported at
  `lib.rs:81`.
- `page: &Page` from `pdfcer_core::page_tree::pages(&doc)`.
- Returns `RenderedPage { pixmap, diagnostics }` (`lib.rs:147`).

**Pixel format:** `tiny_skia::Pixmap` — white-filled (`lib.rs:249`), RGBA8,
**PREMULTIPLIED**, row-major, top row first. `tiny_skia` is re-exported at
`lib.rs:102` so a shell need not depend on it directly.

**Geometry:** `page_device_geometry(page, scale) -> (u32, u32, Transform)`
`lib.rs:344`. Composition (`lib.rs:21-33`): translate the resolved **CropBox**
to the origin → flip y and scale → apply `/Rotate` **clockwise** (the
opposite sense to §8.3.3's CCW matrices), swapping width/height for 90°/270°.
The GUI uses it for hit-testing (`pdfce@cce414e:crates/pdfce-gui/src/object_provider.rs:48`).

**Size guard:** `MAX_PIXMAP_EDGE = 16384` (`lib.rs:115`); zero or oversized
gives `RenderError::BadRasterSize { width, height }` (`lib.rs:137`).

### 7.2 `RenderOptions` — the knobs

`crates/pdfcer-render/src/font/mod.rs:428`, `#[non_exhaustive]` (so **use the
builders**, `:684-690`): `fonts: FontEnvironment` `:432`, `annotations: bool`
`:470`, `annotation_scope: AnnotationScope` `:484`, `cancel: Option<RenderCancel>`
`:494`, `layers: Option<LayerVisibility>` `:507`, `view_magnification: Option<f32>`
`:532`, `cmyk_intent` `:597`, `mask_resample` `:640`, `image_minify` `:648`,
`cmyk_jpeg_polarity` `:655`, `missing_as` `:664`. Builders:
`with_annotations` `:805`, `with_annotation_scope` `:821`, `with_cmyk_intent`
`:878`, `with_cancel` `:972`, `with_mask_resample` `:996`, `with_image_minify`
`:1004`, `with_cmyk_jpeg_polarity` `:1012`, `with_layers` `:1028`,
`with_view_magnification` `:1038`, `with_missing_as` `:1043`,
`with_max_cmyk_buffer_bytes` (`Pass 132.0` — see **§7.3a**, which is the one
knob whose value you should COMPUTE rather than pick), `with_ink_probe`
(`Pass 174.0` — see **§7.3b**; it is the only one of these that ANSWERS a
question rather than changing what is drawn), and **`with_backdrop`**
(`Pass 248.0` — `backdrop: PageBackdrop`, `White` by default; `Transparent`
keeps the page group's own alpha instead of compositing it onto paper. See
**§7.7**; a canvas should never set it, an *export* is what it is for).

`AnnotationScope` (`annot.rs:348`) is the comments-and-forms filter:

| Acrobat's name | variant | paints |
|---|---|---|
| Document | `Document` `:376` | page content + non-markup annotations |
| Document and Markups | `DocumentAndMarkups` `:385` (**`RenderOptions` default**) | page content + every annotation |
| Document and Stamps | `DocumentAndStamps` `:388` | page content + non-markup + `/Stamp` **only** |
| Form fields only | `FormFieldsOnly` `:403` | `/Widget` appearances, **no page content at all** |
| (pdfcer's own) | `ContentOnly` `:360` | page content, no annotations |

### 7.3a The CMYK compositing ceiling — READ it before you size a raster

**Added `Pass 132.0`, at the request of the shell that hit it.** This is the
one render input whose correct value is a function of *your* viewport and
*your* memory tolerance, and it is the reason a page can come back with
different colours at different zooms.

**The mechanism.** A page whose group declares a subtractive blending space
(§11.4.7 — `/Group /CS /DeviceCMYK`, `Separation`, `DeviceN`, or a
four-component `ICCBased`) is composited in a four-colorant buffer at
**`CMYK_BYTES_PER_PIXEL` = 20 bytes per pixel**. Above a ceiling the page
composites in sRGB instead and **says so** (`cmyk_buffer_refused`). Both
rasters are of the same page; only one of them ran §11.3.4's complement, and
the difference on transparency patches measured up to **16 levels of 255**.

**Four public items, and you want the third:**

| item | answers |
|---|---|
| `CMYK_BYTES_PER_PIXEL` | the cost per pixel (20) |
| `DEFAULT_MAX_CMYK_BUFFER_BYTES` | the built-in ceiling (256 MiB) |
| `max_cmyk_composite_pixels(max_bytes: Option<usize>) -> u64` | how many pixels fit |
| **`will_composite_in_cmyk(w: u32, h: u32, max_bytes: Option<usize>) -> bool`** | **"will this raster have exact colours?"** |

`max_bytes: None` means the default, so all four take
`Settings::max_cmyk_buffer_bytes` **verbatim** — there is nothing for a
caller to resolve.

**Do not hardcode 13,421,772.** The predicate exists so the 20-B/px
arithmetic stays on this side of the crate boundary; a copy of a measured
limit is a copy that rots the next time the buffer's element type changes.
`the_operator_s_ceiling_decides_the_path_and_the_predicate_agrees`
(`crates/pdfcer-render/tests/transparency_is_disclosed.rs`) is what pins the
predicate to the renderer.

**★ Your render tier should end at this ceiling, not at `MAX_PIXMAP_EDGE`.**
Those are two different bounds and the gap between them is very nearly a
factor of four on A4 (3.76×) — a whole-page raster is *permitted* up to
**1946 %** zoom (where
`MAX_PIXMAP_EDGE` bites on an 842 pt edge) and stops compositing in ink at
about **518 %**. Every raster in between comes back with approximate
colours.

**★ And moving the switch down is NOT sufficient on a large display.** With
50 % overscan a region raster needs ~110 MB at 1600×900, **~281 MB at
2560×1440** and **~633 MB at 3840×2160** — so on a 1440p monitor the region
path already exceeds the default ceiling. The ceiling has to grow with the
display *as well*.

**SETTING it.** `RenderOptions::with_max_cmyk_buffer_bytes(Option<usize>)`,
whose value should come from `pdfcer_core::settings::Settings::max_cmyk_buffer_bytes`
— it rides the existing preset machinery and the settings file, and
`parse_byte_size` / `format_byte_size` (both `pub` in
`pdfcer_core::settings`) are the **same** vocabulary the file uses, so a
settings window and `settings.txt` accept and show identical strings
(`default`, `256mib`, `1.5gb`, a bare byte count; `mb` and `mib` both mean
1,048,576 here).

**It is deliberately UNCAPPED**, on the operator's own ruling — the same one
that governs `max_zoom_percent`. `ARCHITECTURE.md` §10's no-unbounded-
allocation rule is about **untrusted input**, and a page's dimensions are
untrusted input while a number the operator typed is not. A ceiling the
machine cannot honour is **not a crash**: the allocation is attempted
fallibly and refuses down the same disclosed path. So a settings window
offers it with **no guard and no preflight** — state the cost, do not
prevent the choice.

**The cost, measured**, for whatever your settings UI says: 20 B/px, and
compositing in ink ran roughly **50 % slower** than compositing on screen at
the same pixel count (1.47 s / 1.39 s against 1.04 s / 0.85 s at the
boundary). Whole-page A4 (595 × 842 pt) wants **~641 MB at 800 %**, **~1.44 GB
at 1200 %** and **~3.8 GB** at the end of the `MAX_PIXMAP_EDGE` tier; a square
16,384² raster, **~5.4 GB**.

*(Your request's table gave 576 MB / 1.26 GB / 3.77 GB, the ceiling at 534 %
and the tier's end at 2071 %. Those are correct for the **596 × 791 pt** page
you bisected on, which was labelled A4 and is not — and ours repeated the
error for a day, which is how it propagated. The figures above are real A4.)*

**★ AND THE CEILING BOUNDS ONE BUFFER, NOT THE RENDER.** Every buffer on a
page is page-sized, and a page can hold several at once: the page buffer,
plus a transparency group's child, plus the retained spare a sibling group
reuses, plus — for a **knockout** group — a full copy of its initial
backdrop. So peak resident memory is a small multiple of the ceiling, up to
about **4×** on a page with a knockout group, and a ceiling that admits one
large page is not a ceiling that admits one large *allocation*. Size a
settings UI's advice accordingly: the honest sentence is *"up to about four
times this on a page with nested transparency"*, not *"this much."*

**Key on `cmyk_buffer_refused`, not `blends_in_wrong_space`.** The second is
zero on a page whose transparency happens to fall outside the rendered
region, so a status line keyed on it goes quiet exactly where the operator
scrolled away from the affected patch. The first says *the correct buffer was
not available*, which is a property of the raster.

### 7.3b The ink probe — what is in the colorant buffer, before it stops existing

**Added `Pass 174.0`.** `RenderOptions::with_ink_probe(x, y)` (device pixels,
origin **top-left**) fills `Diagnostics::ink_probe: Option<InkProbe>`. It
changes no pixel of the output and costs nothing when unset.

```rust
use pdfcer_render::{InkProbeSource, RenderOptions};

let options = RenderOptions::default().with_ink_probe(612, 440);
let page = pdfcer_render::render_page_with_view(&doc.view(), &page, 2.0, &options)?;
if let Some(p) = page.diagnostics.ink_probe {
    match p.source {
        InkProbeSource::CmykBuffer => println!("ink {:?} alpha {:?}", p.cmyk, p.alpha),
        InkProbeSource::ScreenSrgb => println!("no ink: composited on screen"),
        InkProbeSource::OutOfRange => println!("outside the raster"),
        _ => {}
    }
}
```

**What it answers that a PNG cannot.** A raster is sRGB — the *output* of the
colour pipeline — so every question about what happened *inside* the pipeline
is unanswerable from it, and two very different colorant states flatten to the
same triple. A page destined for ink is composited in a four-colorant buffer
and converted to sRGB at the very end; the probe reads that buffer
**immediately before the conversion**, which splits a colour error into the
half that happened while compositing and the half that happened while
converting.

**★ The property that makes it an oracle:** for a **single opaque paint over
an empty page** a correct colorant composite is the **identity on its
operand** — transparent backdrop, alpha 1, Normal blend, nothing to blend
with. So an operand that arrives unchanged and a colour that is still wrong
**convicts the conversion and acquits the compositor**. That is the whole
content of the probe, and `crates/pdfcer-render/tests/ink_probe.rs` pins it.

**When there are no colorant numbers.** `InkProbeSource::ScreenSrgb` — the
page's blending space was additive, or the buffer exceeded
`max_cmyk_buffer_bytes` (§7.3a). `cmyk` and `alpha` are `None`, deliberately:
running the sRGB result backwards through `rgb_to_cmyk` would fill the fields
convincingly with a **different quantity**, a max-GCR reconstruction of the
output rather than a reading of a composite that never happened. Every field
is present in the CLI's line with `-` where there is no value, so *"this page
was never composited in ink"* and *"this pixel has no ink on it"* cannot be
confused.

**Out of range is a report, not a refusal.** The raster's size depends on the
scale, the region and the page's own box, so a coordinate cannot be judged
when it is parsed — and a diagnostic must not destroy the output it was asked
about.

**In the CLI:** `pdfcer render-page --probe-ink X,Y`, which prints one
extra line beside the stable metrics line:

```text
ink-probe: x=200 y=200 source=cmyk-buffer c=0.750 m=0.000 y=1.000 k=0.000 alpha=1.000 srgb=47,181,73
```

It is a **second line, not more keys on the first one**: the metrics line is
`key=<integer>` pairs in a published fixed order, and this payload is four
floats plus a classification that is absent unless asked for.

**★ The same page composited ON SCREEN gives `srgb=47,180,73` for that
operand — one count of blue apart.** That is a property of the compositing
path, not of the conversion: one path converts an 8-bit paint colour, the
other converts `f32` colorants at the very end. **Every `DeviceCMYK` colour
carries that ±1 blue**, so do not read a one-count difference between two
probes, or between pdfcer and another engine, as a disagreement.

*(This block's example read `srgb=24,140,108` until `Pass 174.5`. That is the
**pre-`Pass 165.0` defect value** — so this document, which a consuming
project builds against, was restating the very claim `Pass 174.0` measured
away. Recorded rather than silently corrected: a worked example is a claim,
and a stale one is a wrong claim that reads as an illustration.)*

### 7.3 Cancellation — the off-thread contract

`RenderCancel` (`crates/pdfcer-render/src/cancel.rs:85`) is a plain
`Arc<AtomicBool>` — *"no windowing, runtime or executor dependency, and works
identically under wasm"* (`cancel.rs:3-8`). `new()` `:90`, `cancel()` `:100`
(idempotent, any thread, returns immediately without waiting), `is_cancelled()`
`:109`. Wire via `RenderOptions::with_cancel`.

- **It stops the work, not just the result** (`cancel.rs:32-37`): *"Dropping a
  receiver would discard the result while the worker carried on painting for
  another 58 seconds — still occupying a core."*
- **Granularity is one operator** (`cancel.rs:52-58`) — worst-case latency
  *"a third of a millisecond, not the whole render."*
- **`RenderError::Cancelled` is NOT a failure** (`lib.rs:124-132`): *"It means
  the answer stopped being wanted … a caller should discard it silently rather
  than surfacing it."* It is an error variant only because a cancelled render
  has no pixmap, *"and inventing a half-painted one would be worse than saying
  so."*
- **The flag is checked AFTER the work** (`lib.rs:317-332`): *"a render
  cancelled on its last operator is still cancelled, and a partial raster is
  exactly what must not escape."*
- **Default is `None`, deliberately** (`font/mod.rs:496-501`) — existing
  callers cannot acquire a new failure mode; only a caller that opts in can be
  cancelled.

**Thread-safety:** `pdfcer-render` declares no `Send`/`Sync` bounds and
contains no threading. `RenderCancel` is asserted `Send + Sync`
(`cancel.rs:155-158`). **The threading model is entirely the shell's** —
`pdfce@cce414e:crates/pdfce-gui/src/render_worker.rs` is the reference implementation:
`RenderWorker` `:172`, `RenderRequest` `:199` (holds `Arc<EditSession>`),
`spawn()` `:242`, `poll()` `:323` (never blocks), `cancel_and_wait()` `:387`,
`Drop` cancels `:416`.

Why (`cancel.rs:10-30`): *"On a real CAD sheet that is ~10 s at 1× and ~58 s
at 2× (measured 2026-08-07) … The operator's report was 'it took minutes to
try and update the view and hung the entire gui.'"* The worker holds an
`Arc<EditSession>` while rendering, so `Arc::get_mut` fails and an edit
arriving mid-render cannot mutate the session; making the edit wait
re-creates the freeze; so **the edit path cancels the in-flight render and
proceeds**, which is only viable because cancellation is fast.

### 7.4 Minimal worked sequences

**(a) Synchronous** — `crates/pdfcer-render/tests/cmyk_intent.rs:102-107`.

```rust
use pdfcer_core::{document::Document, page_tree, settings::CmykIntent};
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

let doc = Document::from_bytes(bytes)?;
let page = page_tree::pages(&doc)?.remove(0);
let options = RenderOptions::default().with_cmyk_intent(CmykIntent::NeutralBlack);
// NeutralBlack, not Calibrated: `Calibrated` IS the shipped default as of
// Pass 153.0, so passing it would make this example a no-op that reads like
// a demonstration. An example that sets a value the default already returns
// cannot show a reader that the builder does anything.

let rendered: RenderedPage = render_page_with(&doc, &page, 1.0, &options)?;
```

**(b) Cancellable, off-thread** — the idiom from
`pdfce@cce414e:crates/pdfce-gui/src/render_worker.rs:425-467`.

```rust
use pdfcer_render::{RenderOptions, cancel::RenderCancel, render_page_with_view};

fn render_on_worker(session: &pdfcer_core::edit::EditSession,
                    page: &pdfcer_core::page_tree::Page,
                    scale: f32, cancel: &RenderCancel) {
    let mut options = RenderOptions::default().with_annotations(true);
    options.cancel = Some(cancel.clone());

    // session.view(), NOT session.document() — decision 018.
    let view = session.view();
    match render_page_with_view(&view, page, scale, &options) {
        Ok(rendered) => { let _ = (rendered.pixmap, rendered.diagnostics); }
        // ★ Check the TOKEN, do not match RenderError::Cancelled — this stays
        //   correct if the render gains other early-exit paths.
        Err(e) if cancel.is_cancelled() => { let _ = e; /* discard silently */ }
        Err(e) => { let _ = e.to_string(); }
    }
}
```

### 7.5 ★ What the UI must disclose

**Rendering is best-effort by design, and every shortfall is COUNTED**
(`lib.rs:35-50`): *"every shortfall is COUNTED in `Diagnostics` and returned
with the pixels — the caller can always tell a faithful raster from a partial
one ('fuzzy, never sneaky')."* And, explicitly: *"A shell that renders pages
is expected to surface these; **they are not decoration**."*

1. **`glyphs_substituted` + `substituted_fonts`** — *"are these the
   document's own letterforms?"* A substituted font is pdfcer's guess at what a
   missing face should look like. It is the canonical rule-4 case in rendering.
2. **`glyphs_notdef`** — *"is anything missing?"*
3. **`fonts_unsupported`** — *"was any text skipped outright?"*
4. **`contents_streams_unresolved`** (`lib.rs:270-275`) — copied from the page
   because the interpreter cannot observe it: *"without this the raster of a
   page with a dangling `/Contents` would be silently blank."* A blank page
   that is blank for a reason must say so.
5. **`page_content_suppressed`** — set under `AnnotationScope::FormFieldsOnly`
   (`annot.rs:392-402`). The page looks empty on purpose.
6. **`MissingAppearanceState`** — see §3.4 item 4; the default is a **reasoned
   guess**, i.e. an inference.
7. **A cancelled render is discarded silently** — never surface it as an
   error (§7.3).
8. **`cmyk_buffer_refused`** — *"are these the exact print colours, or the
   approximation?"* (§7.3a). Off-canvas, like everything else here: the
   inferred/approximate content still renders **normally** (`CLAUDE.md`
   rule 4, narrowed 2026-08-13), and what is disclosed is that pdfcer could
   not blend in ink at this size — plus, now, that the ceiling is the
   operator's to raise.

### 7.6 Traps

- **★ Premultiplied alpha.** `pdfce@cce414e:crates/pdfce-gui/src/raster.rs:1418-1424`:
  *"`tiny-skia` stores pixels PREMULTIPLIED, both egui constructors accept the
  bytes without complaint, and the wrong one silently darkens every antialiased
  glyph edge."* Use the premultiply-correct upload path, or demultiply
  (`crates/pdfcer-render/tests/cmyk_intent.rs:110-114`). For `pdfcer-print`, hand
  `pixmap.data().to_vec()` over **unchanged**.
- **★ Check the cancel token, don't match the error variant**
  (`render_worker.rs:461-466`).
- **★ `RenderOptions::layers` REPLACES the document's configuration; it does
  not merge** (`lib.rs:2732`). Related pinned behaviours: *"a hidden section's
  CLIP still applies to what follows"* (`lib.rs:2873`) and *"an image XObject
  inside a hidden section is not drawn"* (`lib.rs:3000`).
- **★ "Document and Stamps" is not a synonym for "and Markups."**
  `annot.rs:294-301`: *"It admits `/Stamp` and no other markup type … a
  two-option implementation that collapsed the two would over-include every
  non-stamp markup, and would do it silently, since the result still looks
  like 'a page with annotations on it'."*
- **Scope and §12.5.3's flags compose as AND, never OR** (`annot.rs:331-340`)
  — neither mechanism can override the other, and both are counted.
  `annotations: bool` is a **master gate that can only subtract**
  (`font/mod.rs:483-501`); read the composition **only** through
  `effective_annotation_scope()` (`font/mod.rs:749`), never either field
  directly.
- **`FormFieldsOnly` never decodes content streams** (`lib.rs:278-291`), so
  that branch **cannot** return `RenderError::Content`, and
  `contents_streams_unresolved` stays 0 *"because pdfcer did not look.
  Reporting a page-level incompleteness it never measured would be an invented
  fact."*
- **★ Annotation flags come from Table 169, not §12.5.6.2's prose — the prose
  is wrong** (erratum T169-E1, `annot.rs:114`).
- **The default `AnnotationScope` differs between layers.**
  `RenderOptions::default()` is `DocumentAndMarkups` (`font/mod.rs:640-676`);
  the GUI's print dialog defaults to `Document` — Reader's default,
  deliberately narrower (`pdfce@cce414e:crates/pdfce-gui/src/print_flow.rs:570`,
  `annot.rs:303-316`). Pick deliberately; do not inherit by accident.
- **`RenderPolicy` is `PartialEq` but deliberately not `Eq`**
  (`font/mod.rs:605-609`) — `view_magnification` is an `f32`, and *"claiming
  `Eq` for a type that can hold a NaN would be a lie the compiler happens to
  allow via the other fields."*
- **Render mode 7 is a future trap** (`interpret.rs:1828-1832`): modes 3 and
  7 both paint nothing, and skipping the outline lookup is safe **only**
  because text clipping is unimplemented — when modes 4–7 land, mode 7 must
  still compute outlines.

### 7.7 Exporting a page as a PNG or JPEG file (`Pass 248.0`)

`core [x] · cli [x] · gui [ ]`. The operator's request (2026-09-03): *"export
page(es) to png, jpg, svg … full support (including transparency where
supported!)"*. SVG is `Pass 248.1`; this section is the raster half.

**I want to… → call this**

| I want to… | call |
|---|---|
| a PNG that is see-through where nothing was painted | render with `RenderOptions::default().with_backdrop(PageBackdrop::Transparent)`, then `pdfcer_render::export::encode_png(&rendered.pixmap, Some(dpi))` |
| the ordinary white-backed PNG, but with a `pHYs` DPI so Word/PowerPoint place it at physical size | render with the default backdrop, `encode_png(&pixmap, Some(dpi))` |
| a JPEG | render (either backdrop — the encoder flattens), `encode_jpeg(&pixmap, &JpegOptions { quality, background, dpi })` — `JpegOptions` is `#[non_exhaustive]`: `let mut o = JpegOptions::default(); o.quality = 92;` |
| flatten a transparent raster onto a colour myself | `export::flatten_over(&pixmap, Rgb { r, g, b })` — returns `Cow::Borrowed` when every pixel was already opaque |
| parse an operator's `#rrggbb` | `export::Rgb::parse_hex` (six digits only — no `#abc` shorthand, deliberately) |

**Where the transparency comes from, so you do not build a second source of
it.** ISO 32000-1 §11.4.7 makes the page an *isolated* transparency group
composited onto "a backdrop colour appropriate for the medium … nominally
white". `pdfcer-render` has performed exactly that since 2026-08-17
(`lib.rs`, the comment block above `flatten_page_group_over_white`): the
buffer starts transparent and the white is added **once, at the end**.
`PageBackdrop::Transparent` declines that last step. On the subtractive
(`DeviceCMYK` page group) path the collapse is a *separate function*
(`CmykBuffer::to_srgb_transparent`, a sibling of `to_srgb_over_white` with the
`1 − a` term absent) — which is why the render test covers both paths: a
flag that worked on the additive path alone would silently flatten every
CMYK page.

**The two facts a file carries that a `Pixmap` cannot**, and why the
encoders exist when `Pixmap::encode_png` already does: (1) **physical
resolution** — a PNG without `pHYs` pastes into Word at 96 DPI, four times
too large for a 300 DPI export; (2) for JPEG, **the colour the transparency
was flattened onto**. Resolution is *metadata, never a resample*: render at
`scale = dpi / 72` and pass the same `dpi` to the encoder; they are two
claims, and the file can only carry the second.

**★ What the UI must disclose** (rule 4, both directions):

1. **A transparent export IS transparent** — say so beside the file, because
   a viewer drawing the alpha over a checkerboard and one drawing it over
   white show two different pictures of the same bytes. The CLI prints
   `transparent=1 background=none` on the stable line and a stderr note.
2. **JPEG cannot carry alpha — refuse `transparent` for it by name.** Never
   flatten silently: a white-backed JPEG looks exactly like the export
   succeeding. Offer the background colour instead.
3. **Every `Diagnostics` counter `render-page` discloses applies unchanged**
   (§7.5) — an export is a render with a different last step. The CLI prints
   the identical counter set after its own prefix (`render_counters_line`,
   one `format!` for both verbs).

**Traps**

- **Premultiplied in, straight out.** `tiny_skia` stores premultiplied RGBA;
  PNG wants straight. Both encoders demultiply through
  `PremultipliedColorU8::demultiply` — the library's own — and the test
  asserts a 50 %-alpha red decodes as `(255, 0, 0, 128)`, not
  `(128, 0, 0, 128)`. A hand-rolled division here ships dark fringes on every
  anti-aliased edge and no error.
- **`replay_region` (the display-list cache) still flattens over white.** The
  shell's cached panning route is untouched by `backdrop`; a transparent
  export must go through a direct render.
- **JPEG's 16-bit dimensions.** `encode_jpeg` refuses a side over 65 535 px
  (`ExportError::TooLargeForJpeg`); PNG has no such limit but
  `MAX_PIXMAP_EDGE` still bounds the render.
- **`--background` on a PNG is not the renderer's white.** A non-white
  background is composited by `flatten_over` on a *transparent* render; the
  renderer's own white path is used only when no colour was asked for, so an
  opaque default export is byte-identical to `render-page`'s.

### 7.8 Exporting a page as SVG (`Pass 248.1`)

`core [x] · cli [x] · gui [ ]`. The vector half of the operator's export
request, and the format that pastes into Inkscape, Word/PowerPoint/Excel
(M365) and browsers as **editable vectors** (`docs/clipboard-interop-survey.md`
§7: `"image/svg+xml"` is the clipboard format every one of them reads).

**I want to… → call this**

| I want to… | call |
|---|---|
| a page as SVG | `pdfcer_render::svg::export_svg(&doc, &page, &RenderOptions, &SvgOptions) -> Result<SvgExport, RenderError>`; `export_svg_view` for an edit session's live `DocumentView` |
| choose the resolution of anything that must be raster inside it | `SvgOptions::default().with_raster_dpi(300.0)` — also the scale every coordinate is written at (vectors are exact at any value) |
| an opaque background | `.with_background(Some(Rgb {..}))`; the default is transparent, natively |
| know what is raster or approximated inside the file | `SvgExport::outcome.tally: ExportTally` — `shadings_rasterised`, `soft_masks_kept`, `overprint_approximated`, `nonseparable_approximated`, `non_isolated_groups_isolated`, `colorant_buffer_on_screen`; `tally.is_exact()` when the whole page went out as geometry |
| the rest of the disclosure | `outcome.ops`, `images_embedded`, `dashed_strokes_pre_applied`, `blend_modes_used`, `width_pt`/`height_pt`/`scale`, and the render's own `diagnostics` |

**Where the geometry comes from, and why that is the whole design.** The SVG
is written from the renderer's **display-list recording** (`display_list.rs`,
`Pass 75.0`) — one interpreter, the same device space and scale as the
raster, no second reading of the content stream. The cache recorder refuses
by name every operator that reads the destination back (`PoisonReason`,
`R211`); the export recorder (`ExportState`, decision 132) is the same walk
with every refusal replaced by a **rasterise-into-a-scratch-and-harvest**
fallback (shadings), a kept mask (soft masks — as `<mask>` with a grey PNG,
`color-interpolation:sRGB`), or the counted approximation (overprint,
per-paint non-separable blends, non-isolated groups, a subtractive page
group). It never refuses, and every fallback is a number in the tally.

**The oracle.** `crates/pdfcer-render/tests/export_svg.rs` rasterises each
export with **resvg** and compares it pixel-by-pixel against
`render_page_with(.., PageBackdrop::Transparent)` at the same scale; and,
when Inkscape is installed, through Inkscape itself (the paste target).
Measured 2026-09-03 on the synthetic fixtures: Inkscape's render of the
export differs from pdfcer's own PNG by anti-aliasing only — exact on the
soft-mask and mesh-shading fixtures.

**★ What the UI must disclose** (rule 4):

1. **Text is glyph outlines**, not `<text>`. Editable as shapes in
   Inkscape, not as words. Say so once per export (the CLI does).
2. **Every non-zero tally field, by name** — "2 shadings are embedded as
   raster at 300 DPI", "1 soft mask carried as a mask image", "3 overprinted
   paints drawn Normal". The picture is right; what the format cannot hold is
   what changed.
3. **`mix-blend-mode` does not survive Word's importer** (shown `Normal`);
   Inkscape and browsers honour it. `outcome.blend_modes_used` says whether
   the file relies on it.
4. **A dashed stroke is pre-dashed geometry** — exact, but the pattern is no
   longer an editable attribute (`dashed_strokes_pre_applied`).

**Traps**

- **`clip-path` on an element that also carries `transform` is evaluated
  post-transform.** A device-space clip on a transformed `<image>` excludes
  everything; the writer puts every clip on a wrapping `<g>`. Found by
  rendering the first shading export in Inkscape: two rasters, both
  invisible.
- **Do not build the SVG from `vector::PageObjects`.** It carries no images,
  clips, transparency, blend modes or Type 3 glyphs, and a writer over it is
  a second interpreter.
- **The cache recorder now REFUSES an elementary object under `gs /SMask`**
  (`PoisonReason::SoftMask`). Before `Pass 248.1` it silently dropped the
  mask and a cached replay painted the object unmasked. A shell that caches
  display lists will see a few more `PageNotRecordable` fallbacks; they are
  correct.
- **Recording scale = raster DPI.** An SVG recorded at 72 DPI has 72 DPI
  shadings inside it forever; the default is 300 for that reason, and the
  CLI's `--dpi` is the same knob.

---

## Appendix — capability → primary module → `FEATURES.md` state

| capability | primary path | core | cli | gui |
|---|---|:--:|:--:|:--:|
| ce dimensions (author, groups, scale, two-line) | `pdfcer_core::dimension` + `EditSession` | x | x | x |
| ce-dimension **style cascade** | `dimension::{resolve_style, style_provenance}` | x | x | **[ ]** |
| ce-dimension **tolerance** | `dimension::tolerance` | x | x | **[ ]** |
| Forms — fill / import / export / create / delete / rename / reset | `forms`, `forms_author`, `fdf`, `formcsv` + `EditSession` | x | x | x / ◐ |
| Forms — **flatten** | `EditSession::flatten_fields` | x | x | **[ ]** |
| Forms — **move a widget** | `EditSession::move_widget` | x | x | **[ ]** |
| Forms — **script census** | `form_script::{inventory, recompute}` | x | x | **[ ]** |
| Annotations & markup | `annot`, `annot_author` + `EditSession` | x | x | x |
| Redaction — mark & apply | `redact` + `EditSession`; **proof in `pdfce-gui`** | x | x | x |
| Redaction — **unencrypted-wrapper warning** | `wrapper` | x | x | **[ ]** |
| **OCR substrate** | `pdfcer_core::ocr` | **[ ]** | **[ ]** | **[ ]** |
| Print | `pdfcer-print` | x | x | x |
| **Imposition** (N-up / booklet / poster) | `pdfcer_print::imposition` | — | x | **[ ]** |
| Rasterise a page | `pdfcer-render` | x | — | x |
