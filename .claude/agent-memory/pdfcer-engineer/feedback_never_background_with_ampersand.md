---
name: never-background-with-ampersand
description: a gate sweep started with a trailing `&` inside a foreground Bash call is untracked and stalled; always use run_in_background
metadata:
  type: feedback
---

Start `tools/run-gates.sh` (or any long job) ONLY with the Bash tool's `run_in_background: true`, never with a trailing `&` in a foreground call.

**Why:** 2026-10-04, Pass 487.0: a `bash tools/run-gates.sh > log &` appended to a diffstat command ran detached, sent no completion notification, and stalled at `check-bypass-paths.sh` with no verdict line; a hand-rolled wait loop (`pgrep` is absent in Git Bash) exited early and reported nothing. It cost a kill and a full re-run.

**How to apply:** one background call per sweep; read the log for `run-gates: PASS`. If a stray detached sweep exists, kill it before starting another (never two cargo runs at once). See [[a-starved-test-run-looks-exactly-like-a-broken-one]].
