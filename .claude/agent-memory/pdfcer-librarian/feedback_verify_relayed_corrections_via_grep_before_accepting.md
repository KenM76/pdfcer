---
name: feedback-verify-relayed-corrections-via-grep-before-accepting
description: When the operator resolves a flagged librarian discrepancy with a measured figure, still Grep/Read the underlying files before filing the correction verbatim — it can surface a second, independent error the operator's own report didn't catch.
metadata:
  type: feedback
---

When this librarian flags an unresolved numeric discrepancy (no shell,
hard rule 8) and the operator later comes back with a directly-measured
resolution (e.g. "I ran `cargo test --workspace` twice, here is the
number, here is the `git diff`"), **don't just transcribe the operator's
figure into the correction footer — independently corroborate it with
`Grep`/`Read` on the actual files first**, even though this librarian
still has no shell.

**Why:** on 2026-08-12 (hundred-and-twenty-second filing), the operator
resolved a flagged 3,535-vs-3,534 test-count discrepancy
(`docs/ROADMAP.md`'s `Pass 67.0` phase E family) with a correct,
well-sourced measurement (3,542 passing post-fix, 7 tests added per
`git diff`). Grep-counting `#[test]` occurrences in the three touched
files as a check — not because the operator's number was doubted, but
because it was cheap and available — turned up that the ORIGINAL
flagged entry's own "Six new tests" narrative list had itself
undercounted by one: it named only 3 of `embed_font.rs`'s 4 new tests,
omitting `a_document_with_nothing_to_embed_is_told_that_rather_than_pointed_at_reasons`.
That undercount was the reason the entry's own internal gate arithmetic
(3,535 + 6 = 3,541) disagreed with the true post-fix total (3,542) —
a second, independent defect in the SAME entry that the operator's
top-line figure alone would not have explained.

**How to apply:** treat an operator-relayed measurement as strong
sourcing (better than a bare assertion — method + evidence stated), but
still spend the one `Grep` it costs to corroborate on disk before
writing the correction into `ROADMAP.md`/`SESSION_LOG.md` — per hard
rule 10's own logic, a figure that can be independently reproduced by
two different methods (operator's `git diff`, librarian's on-disk
`#[test]` count) is the one worth trusting, and the reproduction
sometimes catches something the first report didn't. See
`docs/ROADMAP.md`'s hundred-and-twenty-first/twenty-second Shipped
entries and their footers for the full worked example, and see
[[feedback_dispatch_should_carry_git_evidence_when_no_shell]] for the
related, earlier-established rule about relayed engineer evidence.
