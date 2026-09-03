---
name: project_gesture_commit_and_shell_audit
description: Accept/Reject redesign + status-bar canvas-jump fix + ribbon assessment, dispatched off operator feedback 2026-08-04 ("separate accept/reject box... doesn't match anything I've seen").
metadata:
  type: project
---

Spec delivered: `D:\Dev\pdfce\docs\ui_specs\gesture-commit-and-shell-
conventions-audit.md` (2026-08-04), answering verbatim operator feedback
(zoom-to-cursor bug already fixed and out of scope; this covers the
Accept/Reject complaint + a general "doesn't match other software" audit +
a ribbon assessment).

**Load-bearing findings for future UI work in this project:**

1. **`GestureInterrupt::Commit` has existed, unused, since Pass 12.0.**
   `canvas.rs`'s `GestureInterrupt` enum has always had `Nothing`/
   `Discard`/`Commit`; `PdfceApp::commit_active_gesture` (main.rs ~L4945)
   is STILL an empty stub `{}`. Every tool shipped since (TextEdit,
   AddText, Measure) chose `Discard` + an explicit Accept button instead.
   `current_gesture_interrupt`'s own doc comment (~L4872-4878) explicitly
   cites rule 4 ("operator-accepted, never silent") as WHY TextEdit chose
   Discard over Commit for a plain typed find/replace edit — this is the
   exact artifact of rule 4 being over-applied to deliberate authored
   content (case a) rather than scoped to algorithmic inference (case b).
   **Any future tool-commit design in this project should check whether
   `Commit` is finally wired before re-deriving a Discard+button design
   from scratch** — the spec recommends wiring it now for TextEdit's
   plain edit, AddText's authored content, and Measure's plain
   (non-derived) linear pick, while keeping explicit review for
   MeasureCircular's best-fit, the "derived-centerline confirm" (already
   a named, distinct case in code, L12000-12002, tagged "fuzzy inference"
   in-code already), and Reflow (already its own accept/reject sub-flow).

2. **Three independent per-tool floating `egui::Area` PAIRS is the actual
   shape of the "accept/reject box somewhere on screen" complaint.**
   TextEdit/AddText/Measure each have a `…-propbar` Area (top-left of
   canvas, draggable) and a SEPARATE `…-status` Area (bottom-left,
   fixed) holding Accept/Reject + disclosures — same gesture, three
   points of visual attention (gesture location, top-left, bottom-left).
   Recommended fix: merge each pair into ONE floating panel (controls on
   top, disclosures/accept-reject at bottom of the SAME panel) —
   mechanical, low-risk, ships independently of the Commit-wiring
   decision. This is the SolidWorks PropertyManager convention
   (OK/Cancel lives in the same pane as the feature's inputs), generalized.

3. **The canvas-jump-on-select defect (found same session, not
   operator-reported) has a confirmed, cheap root cause:** `egui::
   Panel::bottom("status")` has no fixed height — it auto-sizes to
   content, capped at 220pt by the EARLIER GUI-polish audit's P0-4 fix
   (a `ScrollArea::max_height`). `selection_readout` (main.rs ~L10971)
   renders NOTHING when selection is empty, so selecting an object is
   the ONE moment the status panel's height changes. `canvas()`'s
   `apply_fit` (~L7567) reads `ui.available_height()` UNCONDITIONALLY
   every frame with no "did this really change" gate — so any status-
   panel height delta immediately re-fits and visibly shrinks/re-centers
   the page. Same complaint-family as the (separately, already-fixed)
   zoom-to-cursor bug: a geometry computation reacting to a cause the
   operator didn't associate with "resize." Fix: give the status Panel a
   FIXED height (not auto/capped-but-variable) so it stops contributing
   to `CentralPanel`'s available-space computation at all; P0-4's
   internal ScrollArea already handles overflow correctly, it just isn't
   wrapped in an outer height that holds still. **This generalizes: any
   future auto-height chrome panel sitting adjacent to a live-fit canvas
   in this app should be checked for the same failure mode** — P0-4's
   own 220pt cap was NOT sufficient on its own, because "capped but still
   variable" still varies frame to frame within the cap.

4. **Ribbon assessment: yes, but scoped as "toolbar-widget replacement
   only," not a full-shell rearchitecture.** The dock (Objects/
   Properties/Batch Tools/Redact, R80/R81/R82) stays exactly where it is
   — every one of the operator's three named reference apps (Word,
   SolidWorks, Illustrator/Acrobat-family) ALSO keeps a persistent side
   dock alongside its ribbon/panel system, so this is not an either/or.
   Named a rule-12 boundary explicitly: adopting the RIBBON PARADIGM
   (tabs+groups+contextual tabs) does not breach "never copy Acrobat's
   GUI mechanics" because it's a shared, non-proprietary pattern named
   from three apps, not an Acrobat lift — but the SPECIFIC tab captions
   must stay pdfce's own designed choice, worth flagging consciously any
   time this comes up again. Tab sketch: Home / Insert / Edit
   (absorbing Batch Tools' Combine/Split/Insert-pages — a genuine
   placement improvement) / Protect (Redact's eventual real home,
   resolving the existing L6125-6170 code comment's own named tension
   between rule 3 and rule 7) / contextual Text-Format / Object /
   Dimension tabs (the last replacing the floating propbar pattern
   entirely for those three tool families). Cheaper interim named
   separately (§4.3 of the spec): group captions + splitting the
   9-control "edit" group + consistent icon-style-per-group, all
   shippable against the EXISTING flat toolbar with no new widget
   architecture — explicitly NOT sufficient to fully answer "look like a
   ribbon," but a real, cheap partial win if the full ribbon is deferred.

5. **The toolbar's "edit" group has quietly grown to 9 controls
   (3 of them menus) in one unlabeled `ui.separator()`-bounded cluster**
   (main.rs L5850-6203: rotate×2, Properties, Markup▾, Text▾, Edit Text,
   Add Text, Edit Objects, Measure▾, Redact) — each addition was
   correctly, locally justified by its own Pass's doc comment, but no
   Pass re-balanced the GROUP as a whole. This is rule 3's own "Acrobat's
   worst habit" arriving bottom-up instead of top-down — worth checking
   for recurrence any time a new toolbar control is added to an existing
   group rather than assumed impossible because "each addition was
   reviewed."

6. **`Self::commit_active_gesture` / `current_gesture_interrupt` /
   `resolve_escape` / `GestureInterrupt` / `EscapeOutcome` (canvas.rs
   ~L397-472, main.rs ~L4796-4951) are the load-bearing substrate for ANY
   future accept/reject or cancel-gesture design in this app** — read
   these before proposing a new commit/cancel mechanism for a future
   tool; the four-way Escape precedence chain and the three-way
   GestureInterrupt are already general enough to cover it.
