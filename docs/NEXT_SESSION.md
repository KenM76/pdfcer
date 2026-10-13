# NEXT_SESSION.md — engineer handoff

**Read this first on resume**, then the newest `docs/SESSION_LOG.md` entry.
Engineer-owned; replaced each session. The previous long-form handoff
(gate-sweep history, the carried OWED list, benchmark recipes) is in git:
`git show 78635459:docs/NEXT_SESSION.md`. Grep it; do not re-adopt it wholesale.

**Written:** 2026-10-13, after `Pass 580.0` and the 1067th filing.

## State

- **Release:** v0.81.0 is still the latest (tag on `c0761048`). OneDrive
  `pdfcer2` = 0.81.0, so **the next release writes `pdfcer1`**. `562.0`-`580.0`
  are unreleased, including the OCR upscale option (`566.0`, Ken's request).
  `target/release/pdfcer.exe` was last built from a reverted bicubic
  experiment: rebuild before packaging.
- **Push:** `origin/main` was at `33e8620d` (the 1052nd filing) at the start
  of this handoff; everything since is local until the gates go green and it is
  pushed. Check `git log origin/main..HEAD`.
- **Ledgers:** next free Pass **581.0**; next filing **1068th**; no decision
  added since 183. Core-api verb count **362**.
- **pdfcer-gui G174-G186 are all shipped as `567.0`-`579.0`**, each with a
  FIXED reply. `579.0` (G184) added `export_history`/`import_history`: an
  undo history moves to a fresh session on the same base bytes (format
  `PDFCERHIST` v1, kinds by variant name, fuzz target `import_history`).
  The variant list in `src/edit/history_kind.rs` must gain every new
  `CommandKind` variant; the encoder's exhaustive match will not compile
  until it does.
- **`580.0` shipped** (`f191b9c0`, `ef27637c`): `ocr --magnify`, default
  `smooth` (bilinear); display keeps point-sample. One clean 50 dpi page,
  % words at `--dpi 150`, point-sample -> smooth: ocrs 42.6 -> 61.7,
  tesseract 87.2 -> 87.2, paddle 91.5 -> 100. `--dpi 50 --upscale 3` still
  beats it for ocrs (66.0) and tesseract (95.7). Bicubic was tried and NOT
  shipped: ocrs 68.1, tesseract 91.5, paddle 93.6 (worse for the default
  engine; 2-4 words on one page). A multi-page corpus would settle it.
- **Inbound:** check all three channels. The open GitHub issue (text layer
  position after OCR) still awaits Ken's OK to close.

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
