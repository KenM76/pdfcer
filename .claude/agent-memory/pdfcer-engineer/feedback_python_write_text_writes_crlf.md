---
name: python-write-text-writes-crlf
description: Python Path.write_text on Windows converts every \n to CRLF — it silently flipped edit.rs (67k lines) and 7 other files; cargo fmt --check did not flag it
metadata:
  type: feedback
---

Edit repo files from Python with `read_bytes`/`write_bytes` (or `open(p, "w", newline="\n")`), never `Path.write_text`.

**Why:** 2026-09-30, Pass 10.18: two helper scripts used `write_text`; on Windows that translates `\n` to `\r\n`, so edit.rs, cli.rs, dispatch.rs, listing.rs, sign.rs and three core-api docs became CRLF. `cargo fmt --check` passed and the diff stat looked normal; the only signal was git's "CRLF will be replaced by LF" warning. `grep -q $'\r'` in Git Bash ALSO missed it — a Python byte count found it.

**How to apply:** use byte I/O in every scratch edit script; if git prints the CRLF warning, run a Python `b.count(b"\r\n")` over `git diff --name-only` and rewrite before committing. Related: [[windows-paths-need-literal-edits]].
