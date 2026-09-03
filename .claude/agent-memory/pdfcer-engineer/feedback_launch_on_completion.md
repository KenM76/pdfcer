---
name: launch-on-completion
description: Always launch the app when a work unit completes — Ken wants to try the result immediately, not just read a report
metadata:
  type: feedback
---

When a work unit completes (a Pass ships, or a session's engineering goal is reached), **always launch the project** so Ken can immediately try it — don't stop at "tests green" and a text summary. For pdfce that means running the built `pdfce-gui` binary (and/or demonstrating the relevant `pdfce-cli` subcommand on a fixture) at the end of the completed work.

**Why:** Ken's instruction 2026-07-30: "remember to always launch the project when completed." The deliverable is a working program he can see running, not a report about one.

**How to apply:** After the ship-checklist (tests, clippy/fmt, invariants, librarian dispatch), build release or dev as appropriate and launch the GUI (`cargo run -p pdfce-gui` or the packaged exe). The `/run` skill can drive this. A GUI window opening on his desktop is the completion signal. For CLI-only work units, run the new subcommand on a fixture and show the output instead.
