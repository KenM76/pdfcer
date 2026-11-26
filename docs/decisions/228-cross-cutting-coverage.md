# 228 — Cross-cutting coverage is enforced by the compiler and a gate test

Consultant: autonomous-builder. Operator ask, 2026-11-26: every feature must
be fully supported by every other feature (copy/paste is the example)
without his having to ask.

## Decision

A `pdfcer-core::xcut` module with four parts, each closing a different hole:

1. **One `ObjectKind` enum, matched exhaustively once per operation.** `pub`,
   deliberately not `#[non_exhaustive]`: a new kind breaks the build in every
   operation's policy table, in the fixture builder and in pdfcer-gui's own
   exhaustive match.
2. **Clone-then-patch is the only legal carry.** Copying an object deep-clones
   its COS dictionary and closure, then strips or patches a declared key list.
   Rebuilding through a model, or from an allowlist of keys to carry, is
   forbidden.
3. **A table-driven property-equality gate test.** Each kind has a fixture with
   every property non-default plus a sentinel unknown key. Each operation's
   output is compared as a canonical property dump; only the operation's
   typed `Change` list may differ, and every dropped key must be disclosed.
4. **A generated matrix doc plus a check script** (`docs/cross-cutting.md`,
   `tools/check-cross-cutting.py`) tying `FEATURES.md` rows and Passes to
   kinds.

## Why

- **No unifying kind exists.** Kinds live in at least six types:
  `VectorObject`, `HitTarget`, `AnnotKind` (22 variants, `#[non_exhaustive]`,
  an undo label only), `ClipAnnotation {Markup, Dimension, Raw, Unsupported}`,
  `ClipItem.kind: &'static str`, and six clip types (`ObjectClip`,
  `FieldClip`, `FieldClipSet`, `OutlineClip`, `AttachmentClip`, `PageClip`).
  Adding a kind fails nothing, so each operation silently doesn't know it.
- **Properties are carried field by field in three places:**
  - `ClipAnnotation::Markup` re-authors from `MarkupSpec` + `MarkupCarry`,
    which hand-carries five keys (dash, `/CA`, `/Contents`, `/T`, `/BM`);
    everything else drops by construction.
  - `ClipAnnotation::Dimension` rebuilds from the model; `scale` and
    `label_override` were each added after a report.
  - `formclip::FIELD_CARRIED_KEYS` is a 17-key allowlist.
- **The good pattern already exists:** `RawAnnotation` and the widget half of
  `FieldClip` (`WIDGET_SOURCE_BOUND_KEYS`) deep-clone and strip a denylist.
- **Undo is already preserving by construction** (whole-object snapshots in
  `Command`); clone-then-patch generalises that.

## Artifacts

```
crates/pdfcer-core/src/xcut.rs              module doc: contract, links this decision
crates/pdfcer-core/src/xcut/kind.rs         ObjectKind, ALL, id(), classify_annot()
crates/pdfcer-core/src/xcut/op.rs           Op, ALL
crates/pdfcer-core/src/xcut/expect.rs       Expect, Change, Disclosure
crates/pdfcer-core/src/xcut/policy.rs       fn expect(op, kind) -> Expect (exhaustive over Op)
crates/pdfcer-core/src/xcut/policy/*.rs     one exhaustive match over ObjectKind per Op group
crates/pdfcer-core/src/xcut/dump.rs         PropertyDump (canonical, renumber-proof)
crates/pdfcer-core/src/xcut/carry.rs        clone_then_patch(): the only carry primitive
crates/pdfcer-core/src/xcut/tests/*.rs      fixture, runner, matrix_doc, negative (#[cfg(test)])
pdfcer-model feature `read-probe`           dev-dep only; thread-local key-read log
docs/cross-cutting.md                       GENERATED golden; PDFCER_BLESS=1 rewrites
tools/check-cross-cutting.py                CI step; run-gates.sh via check-ci-parity
```

**`ObjectKind`**, from one macro that defines the enum, `ALL` and stable string
IDs together:

- content: `Path`, `Text`, `ImageXObject`, `InlineImage`, `FormXObject`
- annotations: one per subtype pdfcer distinguishes (Text, FreeText, callout,
  Line, CeDimension, Square, Circle, Polygon, PolyLine, the four text markups,
  Caret, ReplaceText, Stamp, Ink, Popup, FileAttachment, Sound, Screen, 3D,
  Link, Redact, Watermark) plus an explicit, decided `UnknownAnnot` (Raw
  route), which is not a wildcard
- forms: `Widget`, `Field`
- document: `Page`, `OutlineItem`, `Attachment`, `Layer`, `NamedDest`

`ClipItem.kind` becomes `ObjectKind`. `AnnotKind` gains
`fn object_kind(self) -> ObjectKind` (a sibling type, not a widened enum).

**`Op`**: Copy, Cut, Paste, Duplicate, Undo, Redo, SaveIncremental, SaveFull,
ReloadRoundTrip, Move, Transform, AlignDistribute, Restack, Group, Ungroup,
Delete, LayerAssign, PropertiesRead, PropertiesWrite, PageInsert, PageExtract,
PageMerge, PageRotate, PageReorder, Flatten, Redact, Sanitize, ExportSvg,
ExportDocx, ExportImage, Print, PdfaConvert, StructureTags, Search, Compare,
HitTest, CliVerb, CoreApiDoc.

**`Expect`**:

- `Preserve(&[Change])`: dump after equals dump before, except these changes.
- `Remove(&[Key])`: redact or sanitize; the sentinel value is absent from
  every byte.
- `Consume { render_equal }`: flatten; the object is gone and the page
  render is unchanged.
- `Represent`: the kind appears in the output (export, print, hit-test,
  search).
- `Refused(cause)`: the operation returns a named error; a silent no-op fails.
- `NotApplicable(reason)`: the reason cites a spec clause or a decision.
- `Pending(pass_id)`: a filed Pass, which the check script validates.

**`Change`**:

- `Translated(key)`: an exact offset only.
- `Rebound(key)`: `/P` or `/Parent`, which must resolve in the destination.
- `Regenerated(key)`: `/NM`, which must be unique.
- `Dropped { key, disclosed_by }`: passes only if the operation's report
  counter for that key is above zero. This makes rule 4 a test assertion.
- `Rebaked { key, only_when }`: `/AP` re-baked without its cause fails.
- `Renumbered`: object numbers; the dump is reference-blind.

**Carry rule.** `clone_then_patch(src, root, strip, patch) -> Carried`. Every
copy, cut, paste, duplicate, page-carry, merge and import path calls it. No
other core function assembles a carried object's dictionary from parts.
Violations are caught in two ways:

- the sentinel test, which catches them by behaviour;
- the check script's grep stage, which fails on a new `*_CARRIED_KEYS` or
  `*_CARRY_KEYS` const, or on a new `struct *Carry`, measured against a
  shrink-only baseline (`MarkupCarry`, `FIELD_CARRIED_KEYS`).

**Canonical dump.**

- It is a `BTreeMap<path, Canon>` over the object's closure.
- References are replaced by their structural path.
- Streams are recorded as their dictionary plus a SHA-256 of the decoded data.
- Numbers are compared exactly, except `Translated`, which allows 1e-9.
- Content kinds are destructured exhaustively, with no `..`, and include
  their resource closure and marked-content chain (`/OC`, `BDC` property
  lists).

**Gate test.**

1. `fixture(kind)` matches exhaustively.
   - It destructures every model struct with no `..` and asserts each field
     is non-default.
   - It adds a third-class sentinel key holding a string and a stream
     reference. Confirm the name form against Annex E in the spec RAG.
   - Content kinds put the probe in their `ExtGState` and in a `BDC`
     property list.
2. The read-probe asserts that every key a reader or author touches is set
   non-default in the fixture.
3. The runner iterates `Op::ALL × ObjectKind::ALL`.
   - It prints `xcut cells=N run=… preserve=… remove=… consume=… refused=…
     na=… pending=…`.
   - It asserts `run == N − na − pending`.
4. Negative controls: a paste that drops the sentinel and a redact that leaves
   it must both fail the comparator.
5. `Remove`: the sentinel bytes are absent from the raw file, every decoded
   stream, and every object-stream member.
6. `matrix_doc` renders `docs/cross-cutting.md` and compares it with the file
   on disk.

**Matrix cells:** `ok[:changes]`, `remove`, `consume`, `represent`,
`refused:<cause>`, `n/a:"<reason>"` or `pass:<id>`. They are generated from
`Expect`, so a blank cell cannot occur. GUI cells live in pdfcer-gui, keyed
by `ObjectKind::id()`.

**`check-cross-cutting.py` stages:**

1. **Freshness:** the kinds and ops in the macros must match the doc's rows
   and columns.
2. **Cell grammar:** every cell parses, and `n/a` and `refused` carry a
   reason.
3. **Pending cells:** every `pass:` ID exists in `ROADMAP.md`; a Pass that
   has shipped while still named in a cell fails.
4. **`FEATURES.md` tags:** in the object-bearing sections, a row changed
   since the pinned `XCUT_BASE` must end with `<!-- xc: id, … -->` or
   `<!-- xc: none "reason" -->`.
   - Every tagged ID must be a real kind.
   - Every kind must be tagged at least once.
   - Untagged legacy rows are shrinking debt; never re-pin the base.
5. **Carry lint:** the shrink-only baseline.
6. **`--self-test`:** synthetic bad inputs must each fail.
7. **Output:** counts are always printed.

## Pass split

| Pass | Scope | Acceptance |
|---|---|---|
| 781.0 | `xcut` foundation. Clipboard columns (Copy, Cut, Paste, Duplicate, ReloadRoundTrip) driven; other columns `pass:782.x`. | No wildcard in `xcut/policy/*`. Metrics line printed. Both negative controls fail. Every clipboard cell `ok` or `pass:781.n`. Structure gate clean. |
| 781.1 | Process gate: check script and CI step, `XCUT_BASE`, carry-lint baseline, engineer agent text, CLAUDE.md rule 16, channel note to pdfcer-gui. | `--self-test` green; the script is listed by `check-ci-parity.py`; an untagged new `FEATURES.md` row fails locally. |
| 781.2 | Markup paste becomes Raw plus patches; `/AP` re-baked only under a non-translation transform; `MarkupCarry` deleted. | All markup kinds green with the sentinel; `MarkupCarry` removed from the baseline; CLI `paste-clipboard` output byte-identical apart from declared changes. |
| 781.3 | `FIELD_CARRIED_KEYS` inverted into a denylist of identity keys, for `FieldClip` and `FieldClipSet`. | Field and widget green on both halves; the constant removed from the baseline. |
| 781.4 | ce dimension clone-then-patch: re-register the sidecar and map the group; re-bake `/AP` only when the destination's scale or format differs. | `annot.ce_dimension` green; a same-group paste leaves `/AP` bytes equal. |
| 781.5 | Content kinds: the ExtGState probe, `BDC` and `/OC` chain carried; `ClipItem.kind` becomes `ObjectKind`. | All `content.*` green, including leaves of a form XObject. |
| 781.6 | Document kinds (outline, attachment, page, layer, named destination). | Green with the sentinel. |
| 781.7… | The audit's gap list, one row each, a day or less each. | Clipboard columns show `pending=0`. |
| 782.0 | Undo, Redo, SaveIncremental, SaveFull. | Undo gives the before-dump and redo the after-dump; untouched objects byte-identical (§5). |
| 782.1 | Move, Transform, AlignDistribute, Restack, Group, Ungroup, LayerAssign. | `Preserve` with `Translated` and matrix changes only. |
| 782.2 | Delete and every page route. | Every kind survives every page route. |
| 782.3 | Redact and Sanitize in `Remove` mode. | Sentinel bytes absent from the saved file for every kind in the region. |
| 782.4 | Flatten, Print, ExportImage, ExportSvg, ExportDocx, PdfaConvert. | Page render unchanged after flatten; every visible kind in each export, or `n/a` with a reason. |
| 782.5 | `read-probe`. | A test that reads a new key fails until the fixture sets it. |
| 782.6 | StructureTags, Search, Compare, HitTest, PropertiesRead, PropertiesWrite, CliVerb, CoreApiDoc. | Every column driven; matrix `pending=0`; untagged legacy row count reported in `NEXT_SESSION.md`. |

A gap the harness finds is fixed in the same session. A `pass:` cell is
allowed only for the next Pass in the arc.

## Rejected

- **A per-kind trait with a method per operation.** A new variant forces no
  new impl, and default methods become silent wildcards.
- **A hand-maintained checklist.** It rots; the operator's complaint is the
  evidence.
- **Adding `MarkupCarry` fields as bugs arrive.** That is the current failure
  mode.
- **Allowlists of keys to carry.** Future keys drop silently.
- **A property × operation doc matrix.** Property coverage lives in the test.
- **Reflection through serde or a proc-macro.** COS keys are not Rust fields.
- **Tagging all `FEATURES.md` rows now.** Mass churn; rows are tagged when
  touched.
- **`#[non_exhaustive]` on `ObjectKind`.** It would let pdfcer-gui write
  `_ =>`.
- **A git hook.** CI plus `check-ci-parity.py` is the existing mechanism.

## Known limits

- **The sentinel proves unknown keys travel, not that they travel
  correctly.** A vendor key holding coordinates is carried verbatim; the
  paste report discloses the count of unrecognised keys carried.
- **`NotApplicable` and `Refused` are judgments.** They are visible in the
  golden diff and cite a clause or decision, but are not machine-verified.
- **The read-probe sees only `Dict::get` and its relatives.** Direct
  iteration bypasses it, so the sentinel and the destructure cover that gap.
- **pdfcer CI cannot verify the GUI column.** pdfcer-gui's own exhaustive
  match and gate do.
- **The gate must be in-crate unit tests.** `#[non_exhaustive]` model
  structs force `..` in integration tests.
- **Each new kind is a semver break for pdfcer-gui.** This is intended:
  pdfcer is pre-1.0 with one consumer.
- **`edit.rs` is about 70.5k lines.** New code goes in `xcut/`; dispatch
  sites move onto `ObjectKind` when touched.

## Appendix — copy/paste property audit at HEAD 735b9486

A read-only audit across pdfcer and pdfcer-gui. It is the gap list plugged
into Passes 781.2–781.7 of decision 228.

**Root cause, confirmed:** every route that deep-clones the source dictionary
carries properties by construction. Every route that rebuilds from a model
carries only what the model was taught.

### Routes

| # | Route | Carry |
|---|---|---|
| R1 | Selection copy/cut/paste: gui `canvas/clipboard.rs` → core `edit.rs` `copy_selection` / `paste_objects_on_layer` | Content: byte span + rebuilt prelude + deep resource closure. Annotations: R1a–d. |
| R1a | Markup (Square, Circle, Line, Ink, Polygon, PolyLine, the four text markups) | **Rebuild**: `MarkupSpec` + 5-field `MarkupCarry`; fresh `/AP` |
| R1b | ce dimensions (`clip_dimension`) | Rebuild from the full dimension model; complete |
| R1c | Raw (FreeText, Stamp, Link, Text, FileAttachment, 3D, RichMedia, Caret, Sound, others) | **Deep clone** minus `/P /Parent /StructParent /NM /Popup /IRT` |
| R1d | Refused | Widget, Popup, Redact, ce dimension without sidecar |
| R2 | Duplicate (gui `canvas/duplicate.rs`) | Same as R1 / R3, at an offset |
| R3 | Form XObject leaves (`edit/form_copy.rs`) | Content only, placement baked in |
| R4 | Fields (`formclip.rs`) | Field half: **allowlist rebuild** (`FIELD_CARRIED_KEYS`). Widget half: deep clone |
| R5 | Pages (`copy_pages` / `paste_pages` → `insert_pages`) | A real PDF; layers, ce dimensions and fields carried |
| R6 | Copy as picture/vector to OS clipboard (gui `clipboard/place.rs`) | `ObjectClip::to_pdf`: content only |
| R7 | Paste from another program or OS-clipboard PDF (gui `dispatch/ospaste.rs`) | Content or stamp only |
| R8 | Rich text copy (`text_edit/model/rich_copy.rs`) | HTML/RTF from derived run styles |
| R9 | Bookmarks | Model rebuild: title, destination, open, `/C`, `/F` |
| R10 | Attachments | Model rebuild: name, bytes, description |
| R11 | Region export (`pageops/region`) | Annotations burned in |

### Gaps (core unless marked gui)

**Page content (R1, R3), prelude in `vector/clip.rs`:**

- **Colour is reset.** Device colours become DeviceRGB, so CMYK turns into
  RGB. Separation, ICC and pattern paint emit nothing, so the path takes the
  destination's ambient colour.
- **ExtGState is dropped.** Dash, `/ca` `/CA`, blend mode and soft mask are
  lost on paths, text and images. Dash and alpha are already in the model;
  the comment saying they are not is stale.
- **pdfcer's own `set_object_stroke_style` prefix is lost.** It sits before
  the defining-operator span, so a dashed or translucent line pdfcer itself
  styled pastes solid and opaque. No test combines stroke style with copy.
- **Caps, joins and miter limit are dropped.** They are not in the model.
- **The clipping path is dropped.**
- **`/OC` layer membership is reset** to the GUI's current drawing layer.
  `decompose` captures `oc` and `oc_chain` but nothing uses them.
- **Structure `/MCID` is dropped.**

**Annotations:**

- **Markup (R1a):**
  - Circle `/BE` is dropped.
  - `/F` is reset to Print, losing Hidden, Locked, NoView and ReadOnly.
  - `/M`, `/CreationDate`, `/Subj`, `/RC`, `/DS`, `/ExData` and `/Lang` are
    dropped.
  - A custom `/AP` is re-baked.
  - `/OC` is reset.
  - Popup and `/IRT` are dropped, even when the parent was copied.
  - `/StructParent` and alt text are dropped.
- **Measurement Line, Polygon and PolyLine** (pdf dimensions): `/Measure`,
  `/IT`, `/LL /LLE /LLO` and `/Cap /CP /CO` are dropped, so a pdf dimension
  pastes as a plain line. **This is the largest gap.**
- **ce dimensions:** the annotation's own `/Contents` and `/T` are
  re-authored; `/F` is reset.
- **Raw (R1c):**
  - `/OC` is carried as a fresh copy of the OCG that is not registered in
    `/OCProperties`.
  - Rotation and scale are not applied (disclosed).
  - Popup and `/IRT` are dropped.

**Fields (R4):**

- Inherited `/Ff /DA /Q /V /DV`, from a parent field or the AcroForm `/DA`,
  are dropped, losing comb, multiline and alignment.
- Any key not on the allowlist is dropped.
- `/M` is dropped.
- The widget's `/OC` is an unregistered copy.

**Pages (R5):**

- `paste_pages` uses plain `insert_pages`, so bookmarks and named
  destinations are dropped. `insert_pages_with_options` exists but is unused
  here.
- Structure tree, Threads and Perms are dropped.

**Bookmarks (R9):** `/SE` and non-GoTo actions (URI, Launch, GoToR, Named)
are dropped. **Attachments (R10):** `/AFRelationship`, `/Subtype`, `/Params`
(dates, checksum), `/CI` and pointing FileAttachment annotations are dropped.

**GUI:**

- (a) Paste targets the current drawing layer rather than the source layer.
  It needs a core option to keep the source `/OC`.
- (b) OS-clipboard copy and paste (R6/R7) drop every annotation, so a copy
  between two pdfcer windows loses every comment.
- (c) Fields are copied with one `copy_field` call per field instead of the
  shared-closure `copy_fields`, so shared resources are duplicated.

### Stale docs

- `vector/clip.rs` says dash and alpha are "not in the object model".
- The `edit.rs` table and `pageops/mod.rs` say AcroForm and `/OCProperties`
  are "not merged".
- The `formclip.rs` module doc describes the widget allowlist as a denylist.
