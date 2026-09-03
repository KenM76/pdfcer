---
name: a-plan-list-is-a-snapshot-not-the-ruling
description: When an operator ruling cites a plan ("the safeguards like we had planned"), the RULING is authoritative and the plan's enumerated list is a dated snapshot — re-derive the set from his words, then check the plan for what it adds
metadata:
  type: feedback
---

When Ken's instruction **cites** a written plan — *"make the submit and other
options that don't need javascript available for buttons **with the
safeguards like we had planned**"* — the citation resolves the *safeguards*,
not the *scope*. **His sentence defines the set; the plan's list is a dated
snapshot of somebody's earlier reading of it.**

**Why:** 2026-08-30. `Pass 183.0` shipped `/SubmitForm`, `/GoTo`, `/Named`
and `/URI` because those were the five bullets in
`docs/plan-scripting-submit-and-plugins.md` §8 Phase 1, written four days
earlier. It omitted **`/Hide`** — a show/hide button, the second-commonest
script-free button on a real form after reset, and unambiguously one of *"the
other options that don't need javascript"*. Nothing was wrong with the plan;
it simply predated the ruling and was written to answer a different question
(what is buildable in phases), not this one (what did he just ask for).

Shipping the gap and then closing it as `Pass 183.1` cost a whole second
Pass, a second spec ingestion, a second filing and a second note to the
consuming project — all of which one re-read of his sentence would have
avoided.

★ **The tell is that a cited plan feels like a specification.** It is dated,
detailed, and written by me, so its list reads as authoritative. It is
neither the operator nor the standard.

**How to apply:**
- Read the operator's sentence and enumerate the set **from it** first, on
  its own terms, before opening the plan.
- Then open the plan and take from it what it *adds*: safeguards, refusals,
  sourcing, sequencing, prior rulings. That is what a citation is for.
- Where the two disagree on **scope**, the sentence wins and the divergence
  gets stated in the commit — see [[feedback-read-the-inbound-do-not-inherit-its-summary]],
  which is the same failure against a different kind of document.
- If the standard defines a **closed set** (here: eight action types that are
  both script-free and reach-nothing), enumerate it and say which members are
  in, which are out, and why. That converts "five arbitrary picks" into a
  boundary, and it is what surfaces the omission before shipping rather than
  after.
