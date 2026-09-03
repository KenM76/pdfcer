---
name: rung-ladder-state
description: 2026-08-05 — what the Obj tool's Object/Part/Point ladder can do after Passes 36.0-36.2, and what implicit commit does and does not cover after Pass 34.0
metadata:
  type: project
---

Snapshot at HEAD `2523860`. Verify against `docs/ROADMAP.md` — that is
authoritative; this is the shape, not the record.

**Why:** an operator reported three symptoms in one sentence and they had three
different causes, only one of which was the thing he named. Re-deriving that
mapping is expensive and the mapping is the reusable part.

## The three-rung ladder (Obj tool)

Object → Part (subpath) → Point (node). Double-click descends; Escape ascends
one rung. **Descending to Point requires the double-click to land within grab
range of a real anchor** — a miss stays at Part and now says so (Pass 36.2).

| verb | Object | Part | Point |
|---|---|---|---|
| drag | `move_object` | `move_subpath` (Pass 36.0) | `move_node` |
| Delete | `delete_object` | `delete_subpath` | `delete_node` (Pass 36.1) |

Before Pass 36.0, drag at the Part rung fell through to whole-object move, and
Delete at the Point rung ran `delete_subpath`. **Both were reproduced in the
running app before being touched** — do that, the traces are decisive.

## What implicit commit covers (Pass 34.0, decision 031)

Commits on interruption: TextEdit's typed replacement, AddText's **non-empty**
draft, MeasureLinear's completed pick pair.

Does NOT commit, each for its own stated reason: reflow and circular measure
(rule 4 — inferred, "reflow results" and "best-fit geometry" are named in the
rule); MeasureScale (**blast radius** — a third axis, new in decision 031, not
in rule 4; open operator question (aw)); anything incomplete or empty.

`commit_active_gesture` returns `false` on a refusal, and
`resolve_gesture_interrupt` then **abandons the interrupting action** so the
refusal cannot scroll past unread.

## Three traps worth not re-hitting

**A capability can ship with no caller.** `EditSession::move_subpath` sat
documented, tested and uncalled from Pass 28.0 to Pass 36.0 (R151). Its sibling
verb WAS wired, so the gap read as "move is broken", not "move is missing".

**A capability can be reachable and still invisible** (R152). `move_node` had a
caller the whole time; the operator still reported it missing, because the rung
announced neither its own existence nor its failures.

**Two double-clicks in immediate succession are not two double-clicks** — egui
counts one burst, so a scripted second descent reports `double=false` and a
working rung looks dead. ~45 idle frames between pairs. Filed to
`D:\dev\rag\egui\`.

See [[gui-diag-harness]] for the harness that settled all of this without
taking Ken's screen, and [[cad-export-structure]] for why the Part rung exists
at all.
