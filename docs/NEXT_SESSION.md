# NEXT_SESSION.md — engineer handoff

**Read this first on resume**, then the newest `docs/SESSION_LOG.md` entry.
Engineer-owned; replaced each session. The previous long-form handoff
(gate-sweep history, the carried OWED list, benchmark recipes) is in git:
`git show 78635459:docs/NEXT_SESSION.md`. Grep it; do not re-adopt it wholesale.

**Written:** 2026-10-08, after `Pass 564.0`/`563.0` and the 1050th filing.

## State

- **Release:** v0.81.0 is the latest (2026-10-08, tag on `c0761048`, 8
  assets). OneDrive `pdfcer2` = 0.81.0, `pdfcer1` = 0.80.0, so **the next
  release writes `pdfcer1`**. `562.0`–`564.0` are unreleased.
  `verify-release.py v0.81.0` passes only once `main` is pushed and CI has
  run at the tag (the spec-librarian's memory files keep "working tree
  clean" red; they are not ours to commit).
- **Ledgers:** next free Pass **565.0**; next filing **1051st**; no decision
  added since 183. Core-api verb count **356**.
- **Shipped since the last handoff:** `561.0`, `562.0`, `564.0`
  (`MetadataRemoval::needs_full_rewrite`, G172), `563.0` (deskew measured
  off the session via `deskew::page_scan_image`/`detect_image_skew` on a
  `&DocumentView`; `EditSession::coalesce_last_same`; G171). G170–G172 have
  FIXED replies.
- **Pre-push hook bites:** an unfiled code commit refuses the push of
  `main`. Never pipe the gate scripts through `tail`/`head` in a chain.
- **Next up is empty of open Passes.** Take the operator's ordered plan below.
- **Inbound:** G142–G172 all have FIXED replies. Older requests still sit in
  the `open/` folder (`check-requests-scoped.py` is green, so each is scoped
  or answered). The one open GitHub issue (text layer position after OCR)
  awaits Ken's OK to close. Check all three channels every session.

## Follow-ups worth a Pass

- **Table cell boxes are inferred** (even grid over the region; disclosed).
  A later Pass could measure them from ruling lines or the page image.
- **3D missing-parts research on the second assembly sample** is paused.
  Leads: spec RAG `prc__8137__tess_3d_compressed.md`; the local-only probe
  module in `pdfcer-3d` (never commit it).
- **The "operator's ordered plan" carried here earlier is all SHIPPED**
  (`142.0`, `364.0` resize-page-contents, `259.0`, `10.11`, all 2026-09-27/28);
  it was a stale carry. Verify any inherited queue against ROADMAP *Shipped*.
- **Librarian-flagged stale wording** in the decision log (ARCHITECTURE
  around lines 12983 and 13008) about whole-page-only VL reading — the
  librarian's to amend if it judges it wrong.

## Local-only material (never commit)

- Model weights: `D:/models/vl-src`, `D:/models/addons/paddle-vl` (incl.
  `layout.onnx`), any `inference.onnx` in the scratchpad.
- The operator's real PDFs and their dumps; the reader-field, vl-spike and
  prc-probe folders under `D:\Dev\pdfTests\`.
- `crates/pdfcer-3d/src/compressed/probe.rs` and its `mod probe;` line.
- `.claude/agent-memory/pdfcer-spec-librarian/` edits (another agent's).

## How to push

1. `CARGO_BUILD_JOBS=4 bash tools/run-gates.sh > log 2>&1 < /dev/null` with
   `run_in_background`; redirect to a file, never pipe (a pipe reports the
   pipe's exit code). No edits while it runs. Re-run after your last edit.
2. It runs `cargo doc` with broken intra-doc links denied. **A module with an
   outer doc on its `pub mod` line resolves its inner `//!` links in the
   PARENT scope** — use full `crate::...` paths there.
3. `check-commits-filed.py` fails until the librarian has filed every code
   commit; file first, then sweep.
4. Before pushing: `check-suite-name-absent.py`, `check-ledger-numbers.py`,
   `check-passes-filed.py`, `check-requests-scoped.py`; the Next-up `###`
   count stays 80.
5. Push, confirm with `git ls-remote`, read CI with `gh run list`.

## Build environment

- Whole-workspace cargo work can OOM here; when Ken is at the PC use
  `CARGO_BUILD_JOBS=4` and single-crate builds. Release-linking `pdfcer-cli`
  is the reliably heavy step.
- CLI integration tests are one binary: `cargo test -p pdfcer-cli
  --all-features --test all <filter>`.
- `target/debug/deps` grows without bound; check `du -sh` occasionally.
