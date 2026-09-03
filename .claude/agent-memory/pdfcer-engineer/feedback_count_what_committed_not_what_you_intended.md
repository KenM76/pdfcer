---
name: count-what-committed-not-what-you-intended
description: When folding N operations into one undo entry, read the stack's own depth — a counter of intended calls over-counts whenever a verb returns early, and the fold then silently does not happen
metadata:
  type: feedback
---

**When a verb calls N sub-verbs and then folds their commands into one undo
entry, measure the fold size from the undo stack's depth — never from a
counter you increment per intended call.**

    let depth_before = self.undo.len();
    ...call the sub-verbs...
    let committed = self.undo.len().saturating_sub(depth_before);
    self.coalesce_last(committed, kind);

**Why:** 2026-08-29, `Pass 172.0`. `paste_outline_item` incremented a counter
per intended command. `set_outline_open` **returns early without committing**
when the state already matches — so the count exceeded the commands that
actually reached the stack. `coalesce_last`'s `undo.len() < count` guard then
correctly refused to fold, and **the paste silently became three undo entries
instead of one.**

Nothing errored. The operator would have found out by pressing undo and
watching a third of their paste come back. A test caught it; no gate could
have. I then found the same latent shape in `cut_selection` and fixed it
pre-emptively rather than waiting for it to fire.

The general form: **an intention is not an observation.** Same family as
[[feedback_a_claim_about_callers_is_a_measurement]] and
[[feedback_an_extrapolation_reported_as_a_measurement]] — a number derived
from what the code *means to do* diverges from the system the moment any
callee has an early return, and early returns are exactly what well-behaved
idempotent verbs have.

**How to apply:** any time a count feeds a mechanism that acts on real state
(folding undo entries, trimming a buffer, seeking a stack), derive it **from
that state**. And when writing the fold primitive itself, make the
under-count case *refuse loudly rather than silently* — `coalesce_last`
returning `false` and being `let _ =`'d is how this stayed quiet for a run.
