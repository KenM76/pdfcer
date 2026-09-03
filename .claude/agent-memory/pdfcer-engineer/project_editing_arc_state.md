---
name: editing-arc-state
description: Where text/vector/node editing stands as of 2026-08-05 — what shipped, what is CLI-only, and the two capability gaps a new session should not re-derive
metadata:
  type: project
---

State of the editing arc at the end of the 2026-08-05 session (HEAD `7c45bf8`).

**Why:** an entire session was spent removing editing limitations, and several
of the fixes were *not* where the problem appeared to be. Re-deriving that
costs hours; the shape of each fix is the reusable part.

**How to apply:** read this before touching text_edit, vector, or the canvas
ladder. Verify against `docs/ROADMAP.md` — this is a snapshot, the roadmap is
authoritative.

## Shipped (limitations removed)

- **Composite (Type 0 / CIDFont) text now edits** (Pass 29.0). Was refused by
  name, which blocked every text edit on Ken's SolidWorks drawing — its font
  is a subset CenturyGothic Type 0.
- **Anchors with no operand of their own now edit** (Pass 30.0): `re`
  rectangle corners, and the inherited start of a subpath reopened after `h`.
  Both fixed by *materializing the missing operand*, not by special-casing.
- **Bézier handles** — a curve's shape is editable (Pass 30.1 core+CLI,
  Pass 26.1 GUI).
- **The Node rung** exists in the GUI (Pass 26.0): double-click descends
  Object → Subpath → Node.

## Still open, with the shape of the fix already known

- **Pass 32.0 per-run text deletion.** One text object holds all 237 dimension
  labels on Ken's drawing, so deleting "a label" deletes them all. It is the
  *same shape as Pass 28.0's subpath work*: needs per-run token spans on
  `TextObject` (which today carries only `runs: Vec<Bounds>` for hit-testing),
  plus a guard for runs whose position is inherited from the previous run's
  advance rather than set by an explicit `Tm`/`Td` — the exact analogue of
  `starts_implicitly`.
- **Pass 33.0 reflow auto-width.** Four options recorded, none chosen.
- **Decision 028 items 10/12/13**: keyboard node navigation, and item 12's
  *navigation* half. ~~item 9, subpath-move as a canvas gesture~~ **SHIPPED
  Pass 36.0** — see [[rung-ladder-state]]. Item 12 is **half done**: Pass 36.2
  discharged its *disclosure* obligation (the rung is named, a failed descent
  says so) but built no clickable breadcrumb, so Escape-one-rung-at-a-time is
  still the only way up.
- **Clip-gate Tier 2 routing** — blocked on operator question (av), not on
  engineering.

## Two traps that cost real time

**A refusal can become self-justifying.** The composite-text refusal cited the
encoding path being single-byte; the three narrowings that made it single-byte
each cited the refusal. A prior session's survey estimated four coupled pieces
of work — re-surveying, most was already built and the encoder existed, tested,
called by nothing. Re-verify a refusal's stated reason before letting it scope
work (R143).

**Removing a refusal can remove an unrelated protection** (R144) — and it fired
*twice on the same Pass*. Pass 30.0's clip-path case was caught; the second,
that the same refusal was the only thing gating an ungated GUI drag gesture,
was not, because the reasoning stayed inside core. R147 is the companion:
audit the CALLERS, not just the module.

See [[gui-diag-harness]] for how GUI behaviour was verified without taking
Ken's screen, and [[run-the-projects-own-gates]] for the full gate set.
