---
name: run-the-projects-own-gates
description: fmt + clippy + tests are NOT the full gate set — pdfce has tools/check-ui-strings.sh and tools/check-ledger-numbers.py, and skipping them silently broke R1 across eight commits
metadata:
  type: feedback
---

Before declaring any Pass done, run **all four**, not the three that are
muscle memory:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
bash tools/check-ui-strings.sh          # R1 operator-string catalog
python tools/check-ledger-numbers.py --stats   # Pass/rule/decision numbers
```

**Why:** on 2026-08-04 I shipped eight commits running only fmt/clippy/tests.
`check-ui-strings.sh` had been failing the whole time with 11 violations —
every one a `diag::trace` format string I had added — and I never saw it
because it was not in my checklist. The librarian caught it only by refusing
to record the gate as green when it had not been reported, which is the
second time that "record what was actually measured" habit has caught
something real.

`check-ledger-numbers.py` matters for a different reason: it is what stops a
Pass ID / standing rule / decision number being minted twice. Two collisions
have already happened this project (Pass 19.4, then Pass 24 claimed
simultaneously by a decision record and by me). Run it **before** assigning a
number, not after.

**How to apply:**
- The project's own `tools/` gates are part of "green", not extras. A commit
  message claiming a Pass is done should be able to name all five results.
- When fixing a checker violation, ask whether it is a CATEGORY or an
  instance. Eleven `// ui-text-exempt:` comments would have been eleven
  chances to forget; one narrow exclusion for `diag::trace` bodies is
  self-maintaining.
- **Differentially test any exclusion you add to a checker.** Plant a real
  violation where the exclusion ends and confirm it still fires. Otherwise
  "the gate passes now" may only mean the gate stopped looking — the same
  trap as R96's unreachable guard.

Related: [[reference_gui_diag_harness]] (the diag traces that broke it),
[[feedback_librarian_needs_exact_hashes]] (the librarian's insistence on
measured facts is what surfaced this).
