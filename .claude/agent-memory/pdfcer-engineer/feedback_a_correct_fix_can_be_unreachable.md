---
name: a-correct-fix-can-be-unreachable
description: A/B the counters before and after any fix aimed at a corpus failure — twice in one session a plan-endorsed "live defect" was fixed and moved nothing, because the dependency ran the other way
metadata:
  type: feedback
---

**Before spending a Pass on a defect a plan calls "measured and confirmed",
A/B the counters on a pre-fix binary and a post-fix binary over the same
corpus.** A defect can be real in the code and unreachable on the files
that motivated it.

**Why:** on 2026-08-21 this happened twice in one session, both times
against `docs/compositor-plan.md`, which had *correctly* identified both
defects.

- **`/Indexed` overprint classification.** The plan called it *"MEASURED
  AND CONFIRMED… a live defect, not a suspicion"*, present in 4 of 7
  failing suite overprint patches. It was a live defect. After the fix,
  `overprint_effective` / `overprint_composited` / `overprint_refused` /
  `overprint_pixels` were **identical to the digit** on all four patches.
  Cause, verified structurally rather than guessed: every `/Indexed`
  `/DeviceN` space in those patches is an **image** colour space, and
  `overprint::composite` has no image call site at all. The plan listed
  the `/Indexed` half first and the image half second; **the dependency
  runs the other way**.
- **Stage A of the compositor.** Scoped to deliver 7 suite patches. It
  delivered 0 patch verdicts — while genuinely fixing four blend cells,
  taking one panel from 14 wrong cells to 2, and moving three
  reference-strip correlations from 0.58/0.73/0.91 to 0.96/0.98/0.99. The
  blocker was §11.3.4's blending colour space, which Stage B owns.

**How to apply:** keep the previous binary. `git worktree add /tmp/x <sha>`
+ `cargo build --release` is four minutes and it is the only thing that
distinguishes "this fix did nothing" from "this fix did nothing *yet*".
Then say which it was, in the commit message, as the finding — a fix that
moves nothing is publishable work when you can say *why*, and a liability
when you cannot.

**The corollary that matters more:** a plan's *ordering* of two related
items is a claim, and it is the claim least likely to have been checked.
Both of these were listed as independent; both had a dependency nobody had
looked for.

See [[verify-each-instance-not-the-class]] and
[[priority-is-a-measurement]] — same family, different face.
