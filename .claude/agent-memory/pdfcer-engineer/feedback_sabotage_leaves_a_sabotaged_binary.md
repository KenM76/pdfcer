---
name: sabotage-leaves-a-sabotaged-binary
description: a sabotage run's `cargo test` rebuilds target/debug/pdfcer.exe; restoring the source does NOT restore the binary, so manual probes afterwards test the sabotage
metadata:
  type: feedback
---

After a sabotage check (byte-swap source → `cargo test` → restore source), `target/debug/pdfcer.exe` is still the SABOTAGED build until something rebuilds it. On 2026-10-04 I probed `fill-field --set Agree=on` by hand after a sabotage that made the alias resolve to `Off`, and spent several steps chasing a "bug" that was my own sabotage.

**Why:** `cargo test` builds the bin target for `CARGO_BIN_EXE_pdfcer`; the restore is a file copy that cargo only notices at the next build.

**How to apply:** run `cargo build -p pdfcer-cli` immediately after every sabotage restore, before any manual CLI probe. If a manual probe contradicts a passing test, suspect a stale binary first. Related: [[sabotage-catches-false-comments]].
