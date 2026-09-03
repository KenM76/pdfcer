---
name: keep-the-error-catching
description: Ken declines changes that reduce the ability to catch errors, even when they'd buy speed or convenience — decided 2026-08-05 on the parallel-sessions question
metadata:
  type: feedback
---

When a proposed change trades error-detection for throughput or convenience,
Ken takes the error detection. State that tradeoff explicitly when it exists —
he will decide on it, and he decides consistently.

**Why:** on 2026-08-05 he proposed splitting pdfce into three parallel
sessions, one per crate (core / cli / gui), to keep a usable GUI while
features were built. Shown that the crate boundary is a *dependency*
direction rather than a work boundary — the substantive feature Passes are
cross-crate, and the session's three most valuable findings were all
*between* crates — he answered: *"we'll just keep things as they are. I don't
want to do anything that reduces our capabilities of catching errors."*

He gave up a real convenience (a stable GUI to use during development) rather
than accept a structure that would have made each session locally correct and
collectively blind.

**How to apply:**

- Surface the detection cost of a proposal *before* its benefits. He weights
  it heavily enough that it usually decides the question.
- Don't quietly drop a verification step to move faster. Say it is being
  dropped and why — the honest "I did not verify X" is what he wants, not a
  smoothed-over claim (see [[engineer-does-the-observing]]).
- This is consistent with the project's standing discipline — differential
  testing, five gates, adversarial verification — and is a good tiebreaker
  when a rule is ambiguous: choose the reading that catches more.
- The corollary is that he *will* accept slower progress. Don't optimize for
  apparent velocity on his behalf.

Related: the better answer to his stated goal was a release build to its own
folder (decoupling "usable app" from session count entirely, via the
single-folder-portable packaging the project already targets). He declined for
now — it remains available and is worth re-offering when the GUI churn starts
costing him.
