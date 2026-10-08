# NEXT_SESSION.md — engineer handoff

**Read this first on resume**, then the newest `docs/SESSION_LOG.md` entry.
Engineer-owned; replaced each session. The previous long-form handoff
(gate-sweep history, the carried OWED list, benchmark recipes) is in git:
`git show 78635459:docs/NEXT_SESSION.md`. Grep it; do not re-adopt it wholesale.

**Written:** 2026-10-08, after `Pass 560.0` and the 1044th filing.

## State

- **Release:** v0.80.0 is the latest (2026-10-06). OneDrive `pdfcer1` = 0.80.0,
  `pdfcer2` = 0.79.0, so **the next release writes `pdfcer2`**. Everything
  from `Pass 532.0` to `560.0` is unreleased.
- **Ledgers:** next free Pass **561.0**; next filing **1045th**; no decision
  added since 183. Core-api verb count **355**.
- **Shipped since the last handoff** (all filed, all pushed with
  `78635459`): `538.0`–`552.0` (annotation/vector/form/image/text editing
  requests G148–G162), `553.0` (3D named views, G168), `554.0` (wrapped vs
  typed line ends, G163), `555.0` (metadata inventory/removal, G164),
  `556.0` (OCR word lists, G166), `557.0` (deskew, G165), `558.0` (PaddleOCR-VL
  layout regions via PP-DocLayoutV3, OTSL tables, VL task prompts, repetition
  stop, per-region OCR layers, `pdfcer ocr --layout --region-layers`, G167),
  `559.0` (exact wrap/break marks in pdfcer-written blocks: `pdfc_TextBlock`
  / `pdfc_Break` `MP` points, `LineEndSource`; G163 reply updated), `560.0` (`RenderOptions::invisible_text`:
  OCR/mode-3/7 text painted in a chosen colour, `only` = that layer alone on
  a transparent page; `render-page --invisible-text`; G169, pushed `ea888d12`).
- **Next up is empty of open Passes.** Take the operator's ordered plan below.
- **Inbound:** G142–G169 all have FIXED replies. Older requests still sit in
  the `open/` folder (`check-requests-scoped.py` is green, so each is scoped
  or answered). The one open GitHub issue (text layer position after OCR)
  awaits Ken's OK to close. Check all three channels every session.

## Follow-ups worth a Pass

- **Table cell boxes are inferred** (even grid over the region; disclosed).
  A later Pass could measure them from ruling lines or the page image.
- **3D missing-parts research on the second assembly sample** is paused.
  Leads: spec RAG `prc__8137__tess_3d_compressed.md`; the local-only probe
  module in `pdfcer-3d` (never commit it).
- **Operator's ordered plan, still queued:** `Pass 142.0` (embedded-donor
  `format-text --set-font`), resize-page-contents (Acrobat librarian first),
  `Pass 259.0` (core-api line citations), `Pass 10.11` (B-T timestamps).
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
