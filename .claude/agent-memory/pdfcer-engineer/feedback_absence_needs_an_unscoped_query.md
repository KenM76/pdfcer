---
name: absence-needs-an-unscoped-query
description: Never conclude a file/symbol is absent from a path-scoped query returning empty — a wrong path returns nothing with exactly the confidence of a true negative
metadata:
  type: feedback
---

Before reporting that something **does not exist**, re-run the search with the
scope removed. A path-scoped query that returns empty is a fact about **the
path**, not about the repository.

**Why:** on 2026-08-11 I concluded `UI_PREFERENCES.md` "does not exist and has
never existed in git history," wrote that into memory as a *measured* fact, and
reported it to Ken. The evidence was `ls docs/UI_PREFERENCES.md` (empty) and
`git log --all -- docs/UI_PREFERENCES.md` (empty). The file was at the **repo
root**, 34 KB, git-tracked since `b0f57af`.

`--all` is what made it stick. It reads as exhaustive — *all branches, all
refs* — so an empty result feels like a strong negative. It is still
**path-scoped**, and the wrong path yields silence indistinguishable from
truth. `pdfce-ui-specialist` reached the same wrong conclusion the same day by
globbing. Two independent agents, one shared blind spot.

The compounding part: I had *just* been correcting stale claims for exactly
this reason, so the error arrived wearing the costume of diligence. Confidence
came from the act of checking, not from what was checked.

**How to apply:** for "does X exist?", the cheap unscoped forms first —
`git ls-files | grep -i name`, `git log --all -- '*name*'`,
`find . -iname '*name*'`, `rg --files | rg name`. Only after one of *those* is
empty is "absent" a claim worth making. Cost is a few seconds; the cost of the
false negative was a wrong memory, a wrong report to the operator, and a
librarian dispatch sent to investigate a non-problem.

Corollary, and the reason this is broader than one file: **when several
documents cite a stale path, the wrong conclusion becomes the easy one.** The
stale citation is the trap; the unscoped query is the escape. See
[[design-system-and-rule12-conflict]] for the specific file and the citations
still stale.

**★★ 2026-08-21 — THE SAME ERROR ARRIVING FROM OUTSIDE, WHICH IS HARDER TO
CATCH.** A sibling project's note reported *"pdfce's spec corpus holds neither
clause 10 nor §8.6.5.6 / §8.6.5.7"*, used that to justify a recommendation, and
I **relayed it into a dispatch and into a reply without grepping my own
corpus.** Clause 10 had been there since two days earlier — 42 kB of it, built
for an earlier dispatch from the *same* project.

**The new half is the provenance, not the mistake.** Every earlier instance
was me concluding absence from my own bad query. This one was somebody else's
query, and it arrived as a *finding* rather than as a claim — with clauses,
citations and a recommendation attached, which is exactly the packaging that
makes a reader stop checking. Their note was **right when written and stale
when read**, and neither side re-derived it.

**How to apply, extended:** a claim about *your own* repository or corpus,
arriving from *outside* it, is the cheapest possible thing to verify and the
easiest to forward unverified. `ls` before relaying, always — and note that
this project's own `CLAUDE.md` already carries the mirror image of this lesson
in its XFA item (*"the answer was already sourced in one document while another
still asked the question — grep the corpus before recording something as
unverified"*). Same corpus, same failure, opposite direction, four months
apart.

**And a corollary about the channel itself:** a correction sent across a
project boundary *was sent twice and did not stick either time* (iccce's `A52`
1.7 leg). Cross-project claims have a shelf life, and the receiving side is the
one that pays for the staleness. Re-derive rather than re-quote.

Related: [[gates-i-owe-myself]] — same family, verification I skip because it
feels already done.
