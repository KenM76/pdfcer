---
name: project-gui-polish-pass-audit
description: Key findings from the presentability/polish audit of the shipped GUI (Passes 0-8.0), written to docs/ui_specs/gui-polish-current-featureset.md, 2026-07-31
metadata:
  type: project
---

Authored a full P0/P1/P2 polish change-list at
`docs/ui_specs/gui-polish-current-featureset.md`, on dispatch from the engineer
ahead of the operator's first hands-on use of pdfce. Scope was explicitly
polish/cohesion/discoverability only — no new features, no canvas-interaction
design. Read `main.rs`/`ui_text.rs`/`viewer.rs`/`raster.rs` in full plus
`ARCHITECTURE.md` §12 continuation-20/23/25 (the settled five-way placement
taxonomy this agent itself delivered in an earlier session).

**Confirmed several earlier review findings (Pass 3.1) are already fixed in
code** — non-atomic save write, missing rotate shortcut, panel-wide (not
per-field) lossy-properties marking, toolbar status-summary not right-aligned,
undo/redo tooltips not naming the operation. Don't re-flag these; verified
resolved as of this session. Recorded in the new doc's §0 so a future review
doesn't waste time re-checking them.

**Genuinely new findings this session, highest-value ones:**

1. **Cross-document stale-state bugs, not just cosmetic ones.** `open_path()`
   only resets `save_result`; it does NOT reset `edit_note`/`copy_result`/
   `pending_text_kind`/`text_input` when a new file is opened mid-session, so
   a previous document's narration (e.g. "2 pages deleted…") can appear
   attached to a just-opened, unrelated document. Separately, opening a new
   file while the Properties panel is already open leaves it rendering an
   EMPTY grid (new `OpenDoc` starts with `properties_draft: Vec::new()`,
   only seeded on toggle-open) until closed/reopened — confirmed NOT a
   data-overwrite risk (Apply/Revert correctly no-op on the empty draft) but
   a real "looks broken" bug on the very likely first-session workflow of
   opening more than one file. **Important self-correction during this
   review:** I initially suspected this was a silent-overwrite/data-integrity
   hazard and it is not — always re-verify the actual `dirty` check logic
   before asserting a correctness claim, not just the state-staleness shape.

2. **Window title never reflects the open file** — fixed once at
   `ViewportBuilder::with_title("pdfce")` in `main()`, never updated via
   `ViewportCommand::Title`. Zero drag-and-drop support anywhere (confirmed
   by grep — no `dropped_files`/`hovered_files` handling exists). Both are
   genuine "looks unfinished on first contact" gaps despite the shipped
   feature set itself being solid.

3. **Status bar has no height cap.** `egui::Panel::bottom("status")` with an
   unbounded body that, BY DESIGN (every line is a mandatory disclosure -
   R20/R43/R50/R51/R52/redaction-pending), can legitimately stack 8+
   simultaneous lines and crowd the canvas. Fix recommended: wrap the
   existing body in a height-capped `ScrollArea` — nothing hidden, just
   scrollable past a cap. This is the kind of finding that only shows up by
   tracing which status-bar contributors can be non-empty SIMULTANEOUSLY,
   not by reading any one of them in isolation.

4. **One inconsistent click-target:** every icon-only toolbar button except
   the Pass 6.0 annotation-visibility toggle is wrapped in
   `add_sized(ICON_BUTTON_SIZE, Button::new(...))`; the annotations toggle
   alone uses a bare `selectable_label` with no sizing, breaking the app's
   own documented minimum click-target rule. Easy to miss because it's a
   ONE-CONTROL exception in an otherwise totally consistent pattern — worth
   remembering to check "is this control following the same wrapper pattern
   as its neighbors" specifically, not just "does it have a tooltip."

5. **Rule-6 (colour never sole signal) gap found in `selectable_label` usage**
   specifically: Fit Page/Width, Properties toggle, and the annotations
   toggle all signal "active" via background fill ALONE, no glyph/weight
   change — inconsistent with the rest of the app's careful colour+glyph+text
   discipline everywhere else (every `colored_label` pairs colour with ⚠/✖/✔).
   `selectable_label` as an egui pattern is apparently the recurring rule-6
   blind spot — check for it by name in future reviews of any egui-based
   pdfce panel.

6. **Silent per-annotation-subtype colour inconsistency**: the Markup menu's
   colour picker (`self.markup_color`, session state) is read by FreeText
   authoring but NOT by Sticky/Stamp (hardcoded yellow/red) — plausibly
   correct behavior (PDF convention: sticky=yellow, Draft stamp=red) but
   completely undisclosed, and the picker itself only lives in the Markup
   menu so an operator authoring FreeText from the separate Text menu never
   sees where the colour came from. General pattern worth remembering: when
   one shared piece of session state is read by some but not all sibling
   authoring paths, that asymmetry needs an explicit disclosure even if the
   asymmetry itself is the right design choice.

7. **Same-location stacking**: repeated Markup/Text authoring clicks land at
   an identically-computed centred rect every time (no per-invocation
   offset) — since the canvas has no drag-to-reposition yet, this makes a
   second click look like "nothing happened." Recommended a small
   deterministic per-page-annotation-count jitter, NOT a canvas-interaction
   feature — stayed carefully inside the "no new features" boundary the task
   set.

**Confirmed the Pass 7 (forms) and Pass 8 (redaction) GUI surfaces are
exactly as minimal as their specs' P0 called for** — grepped main.rs for
Field/AcroForm/flatten/widget/checkbox and found NO interactive form-fill
code at all (only read-only annotation-appearance painting of widget fields,
counted in `annotations_painted_summary`), and NO redaction mark/apply GUI
beyond the one mandatory status-bar disclosure line. This matches both prior
specs' P0-is-CLI-first recommendation exactly — good confirmation that those
specs' P0 was actually followed, worth checking again after any future
Pass 7.x/8.x GUI work lands.
