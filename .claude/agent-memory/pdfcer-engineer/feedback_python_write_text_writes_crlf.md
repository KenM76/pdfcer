---
name: python-write-text-writes-crlf
description: Python text-mode writes on Windows convert \n to CRLF AND encode cp1252, not UTF-8 — flipped edit.rs to CRLF once, produced invalid-UTF-8 Rust once
metadata:
  type: feedback
---

Edit repo files from Python with `read_bytes`/`write_bytes` (or `open(p, "w", encoding="utf-8", newline="\n")` — BOTH arguments), never `Path.write_text` or a bare `open(p, "w")`.

**Why:** 2026-09-30, Pass 10.18: two helper scripts used `write_text`; on Windows that translates `\n` to `\r\n`, so edit.rs, cli.rs, dispatch.rs, listing.rs, sign.rs and three core-api docs became CRLF. `cargo fmt --check` passed and the diff stat looked normal; the only signal was git's "CRLF will be replaced by LF" warning. `grep -q $'\r'` in Git Bash ALSO missed it — a Python byte count found it.

2026-10-04, Pass 493.0: a module-split script decoded UTF-8 but wrote with `open(p, 'w', newline='\n')`. Line endings were right; the encoding was the locale's cp1252, so em-dashes and section signs became invalid UTF-8 and rustc refused the file ("stream did not contain valid UTF-8").

**How to apply:** use byte I/O in every scratch edit script; if git prints the CRLF warning, run a Python `b.count(b"\r\n")` over `git diff --name-only` and rewrite before committing. Related: [[windows-paths-need-literal-edits]].
