---
name: a-starved-test-run-looks-exactly-like-a-broken-one
description: Windows doctests failing en masse with 0xc0000142 / "Couldn't compile the test" is resource starvation, not a defect — and long gate sweeps get killed in the background, so run run-gates.sh in the FOREGROUND with a warm cache
metadata:
  type: feedback
---

**A `cargo test --workspace` that fails dozens-to-all doctests with
`Test executable failed (exit code: 0xc0000142)` and `Couldn't compile the
test` is a STARVED run, not a broken tree.** Confirm by running the same
command alone before diagnosing anything.

**Why:** 2026-08-29. `run-gates.sh` reported `FAILED — 1 of 27:
cargo test --workspace`, with **58 of 157** doctests failing. The decisive
counter-evidence was cheap and I did not take it first:

- **Zero** of the 24 failing source files were files I had touched. (The ten
  `edit.rs` hits were `vector/edit.rs`, not the `edit.rs` I edited — read the
  full path.)
- `cargo test -p pdfce-core --doc` alone: **157 passed, 0 failed.**

## ★ I reported a cause before I had checked it, and the next run refuted it

I told the operator the failures came from two overlapping sweeps. The next
run — which I described as solo — failed **worse**: 157 of 157. That killed the
hypothesis. Only then did I run the doctests in isolation and get a clean pass.

The "solo" run was not solo: I was concurrently running `du -sh` over a **28 GB**
tree and `git status` across **seven 4 GB worktrees**
([[reference_worktrees_are_a_grep_trap]]). Heavy I/O and process churn, just not
`cargo`.

⇒ **"Nothing else is running" is a claim about the machine, and I make it by
assumption.** Check it (`ps -ef | grep -c '[c]argo'` is not enough — my own
shell commands count).

## The other half: long sweeps get KILLED in the background

`run-gates.sh` was launched with `run_in_background: true` **three times** and
was `killed` every time, always partway through command 11
(`cargo test --workspace`), each time with **no failure recorded yet**. It is a
background-task lifetime cap, not a defect and not starvation.

**How to apply — the recipe that worked:**

1. Warm the cache first (`cargo test -p pdfce-core --doc`, or any build).
2. Run the sweep in the **FOREGROUND**: `timeout 590 bash tools/run-gates.sh`.
   With a warm cache the full 27 commands fit inside the 600 s tool limit.
3. Do **nothing else** while it runs — no `du`, no `find`, no `git status` on a
   worktree, no second cargo.

Do **not** hand-type a subset to dodge the time limit
([[feedback_a_gate_sweep_certifies_the_tree_it_ran_on]] — that is exactly how
the two filing gates got omitted and CI went red).

Related: [[feedback_a_rising_failure_count_can_mean_a_false_pass_was_removed]]
and [[feedback_a_rising_failure_count_measure_the_oracle]] — same discipline,
different subject: a failure count is a measurement of the *harness* until you
have shown otherwise.
