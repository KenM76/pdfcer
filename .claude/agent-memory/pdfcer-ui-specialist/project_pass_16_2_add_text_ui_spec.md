---
name: project-pass-16-2-add-text-ui-spec
description: Pass 16.2 Add Page Text UI spec (docs/ui_specs/pass-16.2-add-text-ui.md) — CanvasTool's SECOND variant (opposite call from 15.2's sub-mode), the FreeText-label-collision finding, 16.0's already-shipped core (no §0.2 gap), 16.1's two named box-mode asks
metadata:
  type: project
---

Wrote `docs/ui_specs/pass-16.2-add-text-ui.md` (2026-08-01 session, decision
016/FF-D's final slice — on-canvas "Add Page Text" UI, i.e. genuinely NEW
page content, never the Pass-6.2 FreeText annotation). Read the ACTUAL
shipped Pass 16.0 core (`crates/pdfce-core/src/text_edit/addtext.rs` in
full, `EditSession::add_text` in `edit.rs` lines 1680-1780,
`cmd_add_text`/`AddTextArgs` in `pdfce-cli/src/main.rs`) before designing —
this mattered enormously, see finding 1 below — plus the shipped toolbar
cluster (`Markup ▾`/`Text ▾`/`Edit Text` at `main.rs` lines ~3567-3659) and
`ui_text.rs`'s existing `text_menu_tooltip()`/`edit_text_tool_tooltip()`.

**Load-bearing call #1, the opposite direction from 15.2's own precedent
(flag first if ever asked to review the implementation): Add Text is a
SECOND `CanvasTool` variant (`CanvasTool::AddText`), NOT a `TextEdit`
sub-mode.** 15.2 made reflow a `TextEdit` sub-mode because reflow targets
EXISTING recognized structure (a `Block` under the caret) using the SAME
model/caret machinery `TextEdit` already owns. Add-Text is structurally
different: it targets ARBITRARY, unstructured page-space coordinates, needs
no `EditableTextModel`/caret/hit-test at all to place a new origin, and —
most importantly — making it a `TextEdit` sub-mode would force EVERY click
while `TextEdit` is active to gain a new "did this hit existing text (edit)
or miss (start placing new text)?" disambiguation, silently repurposing the
shipped, safe "a miss just clears my caret" behavior — exactly the kind of
silent mode-shift 15.2 itself rejected for "click a block to enter reflow."
A dedicated second `CanvasTool` variant avoids this: `TextEdit`'s click
semantics are completely unchanged, and a click only ever creates new
content after the operator has explicitly, discoverably switched tools —
the same deliberate mode-switch mechanism already governing `TextEdit`
entry itself, not a riskier new pattern. `active_tool: Option<CanvasTool>`
is already a single value, so the two tools are automatically mutually
exclusive with zero new substrate plumbing (`canvas_suppresses_pan`/
`resolve_escape`/`current_gesture_interrupt` are already generic over a
plain `tool_active: bool`, not over which variant — confirmed by reading
`canvas.rs` and `main.rs`'s actual dispatch code, not assumed).

**Finding #2, the single biggest surprise of this Pass (read the real code
before assuming a gap exists): Pass 16.0 ALREADY SHIPPED both the free
function AND the session-integrated `EditSession::add_text` sibling, sharing
one `plan_add_text` planner** — confirmed by reading `edit.rs` lines
1680-1780 directly. This is the FIRST tool-bearing UI spec in this project
that did NOT have to name a "§0.2 missing EditSession sibling" P0 gap (14.3
needed `edit_text`/`format_text`; 15.2 needed `reflow_block`; 16.2 needed
nothing — it was already done). Worth remembering as a positive precedent:
core-side authors have started shipping the free-function+session-sibling
pair together from day one, closing what used to be a recurring UI-spec
finding. **Do not assume every tool-bearing Pass will have this gap going
forward — check the actual `edit.rs` first, as this Pass's own spec-authoring
did, before writing a "core needs X" ask that may already be moot.**

**Finding #3, a genuine remaining core gap, correctly narrower than first
expected: Pass 16.1 (boxed/wrap add) has NOT shipped** — `AddTextRequest`
(16.0) is point-origin only, no width/box concept at all (confirmed by
grepping for `wrap_width`/`AddTextBox` in `pdfce-core/src` — only reflow's
own 15.x wrap concept exists). This spec designs BOTH click(point)/drag(box)
placement per decision 016's own 16.2 line, but named TWO concrete asks for
16.1 to confirm, not silently guess: (a) a mutating box-add API mirroring
16.0's own free-function+session-sibling shape; (b) a NEW pure, read-only
wrap-PREVIEW function for box-mode's live per-keystroke composing feedback —
genuinely new, since 15.0's `ReflowEngine::preview` only re-plans an ALREADY-
recognized existing block, not a literal not-yet-committed string. Point-
mode (16.0) is fully buildable today with zero backend dependency; box-mode
is fully specified but correctly marked P1-BLOCKED until 16.1 lands.

**Finding #4, a presumption CORRECTED by reading the real code (worth
remembering as a "check before assuming a gap" lesson): font enumeration for
the property bar needs NO new core/render accessor.** `AddTextRequest::
base_font: Std14` is constrained to exactly the 14 canonical §9.6.2.2 names
(`pdfce_core::fontdata::Std14`, `std14_base_font_name`) — "Supplied" for
Add-Text does NOT mean "any arbitrary font," it means "the operator has a
font-folder face registered under one of these 14 exact names, so the
PREVIEW renders with its real shapes; the WRITTEN `/BaseFont` is identical
either way." `pdfce-cli`'s own shipped `cmd_add_text` already does exactly
`font_env.classify_nonembedded(base_font_name)` (the SAME hoisted function
14.3 uses) to compute this — the GUI's ComboBox reuses this ONE call per
candidate name, no new bookkeeping needed. Only a tiny, non-blocking P2
nicety remains: `Std14` has no public `ALL: [Std14; 14]` (the 14-arm list
exists only in a `#[cfg(test)]` fixture) — low-risk since the Standard-14 set
is spec-frozen (ISO 32000-1 §9.6.2.2 defines exactly 14, never revised),
unlike the `reflow_recognition_options`/`font_subset_stem` hoists which
guarded genuinely tunable values.

**Finding #5 — the label/collision problem is REAL and already live in
shipped code, not hypothetical:** `ui_text::text_menu_tooltip()` (the
Pass-6.2 FreeText/Sticky/Stamp menu's shipped tooltip) currently reads "Add
a text box, sticky note, or stamp to the current page" — almost exactly
what an operator would say Add-Text does. The spec requires BOTH updating
this existing string AND `edit_text_tool_tooltip()` (to name Add Text too)
as a required companion change, not optional — R78's disambiguation is
bidirectional now that there are three adjacent, easily-conflated controls
(`Text ▾`, Edit Text, Add Text) instead of two.

**Other key design calls:**
- Toolbar placement: a bare toggle button (`"+ Aa"`, Ctrl+Shift+E — verified
  unclaimed by grep at spec-authoring time, re-verify at implementation)
  immediately adjacent to Edit Text (`"✎ Aa"`), NOT nested in a
  split-button/dropdown and NOT moved to the Tools dock — rule-3 tension
  named and resolved explicitly: Acrobat's own complaint is about deeply
  nested chrome for RARE features, not a small number of top-level MODE
  toggles for the single most common editing surface (text).
- Degenerate box-drag falls back to POINT MODE at the drag's start position
  — a DELIBERATE divergence from Pass 6.1's own "degenerate-drag discards
  silently" precedent, reasoned explicitly: a near-zero drag here still
  names a sensible construct (a point insertion), unlike a near-zero
  square/circle which has no sensible minimal shape to keep.
- A genuine, buildable accessibility improvement over Pass 8's own
  named-but-unsolved pointer-first gap: real, Tab-focusable X/Y (and,
  16.1-gated, W/H) `DragValue` fields in the property bar let an operator
  place text by typed coordinates alone, no mouse drag required — flagged
  as a deliberate design win, not a mitigation of an unfixable gap.
- Disclosure strip must render ALL THREE possible `AddTextReport.
  disclosures` entries (font-provenance always; the §7.7.3.4
  inherited-`/Resources` note conditionally; the R73 tagged-untagged note
  conditionally) — confirmed by reading `plan_add_text`'s actual disclosure-
  building code, not just decision 016's prose (which foregrounds only the
  R73 one at a skim).
- Refusal hint table is written against the REAL, already-shipped
  `AddTextError`/`RInvTrigger` variants (unlike 15.2's provisional table for
  an unshipped error type) — a genuine strength of this Pass's timing.
- Tool STAYS active after a successful Accept (multi-add workflow, matching
  Acrobat's own precedent) rather than auto-exiting; "no separate
  added-text mode" is satisfied structurally (switching to Edit Text
  re-extracts current page content, the new run is just there) — an
  optional P1 "Edit this text now →" convenience is named but not required.

Read the full spec at `docs/ui_specs/pass-16.2-add-text-ui.md` before
reviewing or extending any part of the eventual Pass 16.2 implementation.
