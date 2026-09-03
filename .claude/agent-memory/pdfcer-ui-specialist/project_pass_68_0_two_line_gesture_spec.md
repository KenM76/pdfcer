---
name: project_pass_68_0_two_line_gesture_spec
description: Pass 68.0 GUI-half spec (docs/ui_specs/pass-68.0-two-line-ce-dimension-gesture.md) — the two-line-pick ce-dimension gesture (parallel→Linear, angled→Angular). Pick MODE of MeasureLinear, not a new CanvasTool; own state, not `pending`; explicit-Accept-only per decision 031; a real code-verified drag gap found in run_dimension_drag.
metadata:
  type: project
---

Wrote `docs/ui_specs/pass-68.0-two-line-ce-dimension-gesture.md` (2026-08-12).
Core (`e931836`, `905791f`) and CLI (`bc13a86`) were already shipped and
tested; the GUI had ZERO path to this capability (no canvas gesture calls
`pick_line`/`author_from_two_lines` anywhere) — this spec is the entire gap.

**Corrected the ROADMAP's own framing before designing against it.**
`ROADMAP.md`'s `bc13a86` entry describes an OLDER, now-superseded inline CLI
implementation (`classify_two_lines` called directly with hand-rolled
`PickedLine` construction). The actual current, canonical surface is
`crates/pdfce-core/src/dimension/two_lines.rs`'s `author_from_two_lines` —
a newer consolidation with its own `TwoLinePlacement`/`TwoLineAuthoring`/
`TwoLineRefusal` types, whose OWN module docs already state the "one
function, not one per shell" principle in near-standing-rule prose. Found
by reading the actual crate tree, not trusting ROADMAP prose — the same
"always grep the real code" lesson this agent's [[project_pass_12_M2_dimension_tools_spec]]
memory already names, recurring on a fresh Pass.

**Central design call: Two-Lines is a PICK MODE of `CanvasTool::MeasureLinear`
(a `LinearPickMode` toggle), not a new CanvasTool variant, and not a silent
sub-state.** Argued explicitly against BOTH nearest precedents rather than
picking one by default: Pass 16.2's `AddText`-vs-`TextEdit`-submode (separate
tool, because click MEANING silently changes) and Pass 46's `MarkupKind`
(one tool, explicit kind selector, even though gesture SHAPE differs by
kind). Two-Lines is the `MarkupKind` case — the operator explicitly declares
the mode via a segmented control before any click's meaning changes, so
nothing is silently repurposed. Textual evidence cited too: the operator's
own wording ("the dimensioning tool should allow...") names "the" existing
tool, not a new one.

**Load-bearing finding, the spec's most important one: the two-line
CLASSIFICATION must NOT be stored in `MeasureState::pending`, even though it
looks like the obvious home.** Read `PdfceApp::committable_gesture` directly
and found it already has a Pass-34.0/decision-031 rule: `pending.is_some()`
on the Linear tool IS commit-on-interrupt-eligible (safe, because an
ordinary two-point pick carries no inference); Circular's best-fit is
EXPLICITLY EXCLUDED from that same path in the function's own comment
("Circular = inferred, deliberately excluded"). A two-line classification
(parallel-vs-angled, which-of-four-angles, virtual-apex) is an inference in
exactly Circular's sense. If it were stored in `pending`, the EXISTING,
already-tested `is_some()` check would silently make it interrupt-committable
too — reopening the exact hazard decision 031 closed for Circular, on a
brand-new gesture, by construction rather than by anyone choosing it. Fix:
a sibling `TwoLinePick` struct (mirrors `CircularPick`'s shape/reasoning
exactly), never touching `pending` — this makes `committable_gesture`'s
existing exclusion correct for free, no new `if` needed. **This is the kind
of gotcha that only surfaces by reading the actual interrupt-commit code
before designing state shape — a generalizable lesson for any future
"where does this new inferred-value gesture's state live" question in this
project: check `committable_gesture`/`current_gesture_interrupt` FIRST for
what any existing `Option` field's presence already implies.**

**Also corrected mid-design: `GestureInterrupt::Commit` is NOT still unused.**
This agent's own [[project_gesture_commit_and_shell_audit]] memory says it
"has sat unused since Pass 12.0" — true as of 2026-08-04, but Pass 34.0/
decision 031 wired it for the Linear `pending` case specifically (confirmed
live in `current_gesture_interrupt`/`committable_gesture`, both real,
tested code). Flagged this explicitly in the new spec rather than citing
the stale memory as current fact — a reminder that this agent's own memory
entries are snapshots and must be re-verified against live code before
being cited as present-tense truth, same discipline as the "memory is a
claim, not a fact" rule this agent operates under generally.

**A second, independently valuable code-verified finding: `run_dimension_drag`
(the existing drag-to-reposition gesture for an authored ce dimension)
filters to `DimensionKind::Linear` only** —
`.filter(|k| matches!(k, DimensionKind::Linear { .. }))`, predating
`Angular`'s existence by construction. An Angular ce dimension CAN be
click-selected (selection isn't kind-filtered) but a drag attempt silently
does nothing (`current`/`placed` both resolve to `None`). Named as a real,
concrete P1 gap the two-line feature surfaces but does not require —
`TwoLinePlacement::default()`'s already-shipped derived-arc-radius fallback
(half the shorter arm, floored at 20pt) means Accept produces a usable,
visible result with NO placement gesture needed for P0, so this gap doesn't
block shipping. The eventual fix needs a genuinely new apex-relative
drag-decomposition (radial component → arc radius, tangential component in
degrees → text_along), not a one-line filter change — named as a "contract,
not implementation" ask per this project's standing convention.

**The "editing an already-authored dimension" half of the operator's
mid-build request is answered as a genuine, data-model-grounded LIMIT, not
papered over.** `DimensionKind::Angular`'s own doc comment states the reason
directly: the two source `PickedLine`s are never retained after authoring
(deliberately — so scale re-derivation can't silently reinterpret a
committed dimension). `author_from_two_lines` requires exactly those two
`PickedLine`s. So "editing" is satisfiable only for the PRE-ACCEPT review
window (the `force_parallel` checkbox is a live, re-run-on-every-toggle
control while both lines are still held in `TwoLinePick`) — retroactively
reclassifying an already-committed dimension is structurally impossible
without a NEW core capability with different semantics (projecting two
direction vectors instead of two finite segments), named as an explicitly
open, not-yet-scoped question for the librarian rather than silently
decided either way.

**Composed-vs-verbatim disclosure distinction made precise, extending a
rule this agent has cited loosely before.** `TwoLineRefusal`'s two
`thiserror` variants ARE rendered verbatim (`refusal_line(&err.to_string())`,
reusing Circular/Scale's own existing `Err` arm pattern exactly). The
POSITIVE verdict sentence ("reading these as PARALLEL...") is NOT a core
string — core only returns structured `Ok` fields
(`measured_angle_degrees`/`relation`/`forced_parallel`), so the GUI composes
the sentence, the same way `measure_length_readout`/`best_fit_circle_
disclosure` already compose from structured fields rather than render a
core-owned string. Worth remembering as the general test for any future
"is this disclosure verbatim or composed" question in this project: verbatim
applies to `thiserror`/error-type strings core already wrote for an operator
audience; composed applies whenever core hands back plain data and no ready
sentence.

**Palette-role reasoning, reusing UI_PREFERENCES.md's own vocabulary rather
than inventing new tokens:** hover-highlight of a pickable (but not yet
picked) line = `theme.palette.node_mark` (blue, "editable/selectable,"
generalized from point to line); an already-picked line awaiting its pair =
`theme.palette.preview` (orange, "part of my in-progress gesture"); virtual-
apex extension lines = `theme.palette.guide` (weaker relative of preview);
virtual-apex disclosure text = `theme.palette.notice` (not `danger` —
"worth knowing, nothing broken," matching `theme.rs`'s own doc comment,
deliberately distinct from the Collinear/Degenerate refusal's `danger`
coloring). No new `Palette` field needed — full reuse of the already-shipped
role vocabulary from the (separately memoried) icon/toolbar and shell-redesign
work.

Read the full spec at `docs/ui_specs/pass-68.0-two-line-ce-dimension-
gesture.md` before implementing or reviewing any part of Pass 68.0's GUI
half — §19 consolidates the engineer's change-list, §17 the P0/P1 priority
table, §18 the three items to file with the librarian at Pass completion.
