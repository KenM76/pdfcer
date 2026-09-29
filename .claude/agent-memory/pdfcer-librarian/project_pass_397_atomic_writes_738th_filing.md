---
name: project-pass-397-atomic-writes-738th-filing
description: Pass 397.0 (CLI atomic output writes) shipped 2026-09-29, 738th filing — partly closes the 279th-filing --in-place Backlog item
metadata:
  type: project
---

**Pass 397.0** (`ed93dd50`, 2026-09-29, 738th filing) — `edit_common::write_output`
makes all 45 non-test `std::fs::write` sites in `crates/pdfcer-cli/src` atomic
(temp file + `sync_all` + rename). Fixes silent corruption when `-o` names the
command's own input and the write fails part-way. CLI-only, no `Cargo.toml`
change.

**Why this matters for future filings:** it **partly** closes the Backlog item
filed 2026-08-27 (279th filing), "`--in-place` owed on the other `pdfce-cli`
editing subcommands" — `docs/ROADMAP.md` line ~22999. Do not treat that item as
fully closed: `-o <input>` is now safe everywhere, but the explicit
`--in-place` flag (ocr's mutually-exclusive-with-`--output` shape) is still
absent from the ~108 other `save_edited`-routed subcommands. If a future
session ships that flag, the Backlog entry's heading (currently "PARTLY
ADDRESSED 2026-09-29, `Pass 397.0`") needs its own dated closure, not a second
independent note.

**FEATURES.md placement:** no dedicated row exists for CLI output-write safety
as a standalone capability — it was folded into the general "A first-class
scriptable CLI over the capabilities above" row (`docs/FEATURES.md` line 460)
rather than inventing a new row for an infrastructure fix. Check that row
first before minting a new one for any future CLI-output-plumbing change.
