# NEXT_SESSION.md — engineer handoff

**Read this first on resume**, then the newest `docs/SESSION_LOG.md` entry.
Engineer-owned; replaced each session. The previous long-form handoff
(gate-sweep history, the carried OWED list, benchmark recipes) is in git:
`git show 78635459:docs/NEXT_SESSION.md`. Grep it; do not re-adopt it wholesale.

**Written:** 2026-10-16, after Pass 601.0 (G204) and the 1086th filing.

## State

- **Release:** v0.83.0 is published (tag at `5e4404ec`, GitHub 8 assets,
  OneDrive `pdfcer2`; `pdfcer1` keeps 0.82.0). **The next release writes
  `pdfcer1`.** It carries 591.0-599.0 (G196-G203), including 596.0's GUI
  API change (`BlockEditPreview::glyphs` became `runs`). Unreleased since:
  `600.0` (3D sliver rule, `08071cc2`) and `601.0` (G204,
  `edit_recognised_block_text_preview` returns `RecognisedBlockEditPreview`,
  a GUI API change; `06c13df7`). `verify-release`
  failed only "working tree clean", on the uncommitted spec-librarian
  memory files, which are deliberately left uncommitted.
- **Ledgers:** next free Pass **602.0** (589.0 is the Backlog det-normalisation
  investigation; 590.0 is G195, `docs/ocr-upscale.md`); next filing **1088th**
  once the 601.0 filing lands (check ROADMAP's newest ledger line); no decision
  added since 183. Core-api verb count **378**. G203 `599.0` and G204 `601.0`
  both replied FIXED in the channel.
- **3D small nuts (door sample, `TS-0232-16-3D.pdf`, 4 placements of
  m23/m39; second sample 5 meshes) still do not decode** — OPEN Backlog item.
  `600.0` fixed m76 (a sliver folded only when tall in its own plane,
  `MIN_PLANE_HEIGHT`). The nuts fail differently: an exactly collinear
  status-3 sliver at triangle 484 needs three coordinated flips
  (484+485->843, 484+839->1779) and the wall at 1779 is beyond the 40-step
  search window; a forced fold of 484 fails at 487. It is a search-shape
  problem, not a threshold. Door 152/156, second sample 136/141.
- **G194 shipped as `588.0`** (`475c6f4a`): four pinned OCR sets (`ocrs`,
  `ppocrv5-latin`, `ppocrv4-ch-en`, `ocrcer`), `manifest()` written on fetch,
  `fetch-ocr-models --model NAME`, paddle reads `inference.yml`'s
  `character_dict` when `dict.txt` is absent. Not pinned: `paddle-vl` (1.3 GB,
  hosting is Ken's open question) and `tesseract` (install-from-file).
- **`589.0` (Backlog):** PaddlePaddle's v5 det config specifies ImageNet
  normalisation; the engine uses RapidOCR's [-1,1]. A probe was mixed (v5
  `scan` CER 27%->12%, `scan_rotated_90` 14%->72%), so nothing changed. v5
  Latin reads `scan.pdf` at 27% CER where v4 reads 0%.
- **pdfcer-gui G187-G193 are all shipped as `581.0`-`587.0`**, each with a
  FIXED reply. `587.0` (G193) added `copy_fields`/`paste_fields` and the
  `FieldClipSet` clip. The paste is all or nothing, with one undo entry. CLI
  twins are `copy-fields` and `paste-fields`.
- **The clip readers refuse what the writer never emits**: NaN/Inf
  coordinates, and stream data beside a non-dictionary (`rebuild_stream`).
  `clipboard_payloads` found both; a 300 s rerun (3M runs) is clean. On this
  machine libFuzzer writes no crash artifact: add a temporary
  `std::fs::write` of `data` before the failing assert to capture it.
- **Build gotchas:** `fetch-ocr-models` needs `--features download`, which
  the release build lacks (use a debug build for an end-to-end fetch). Never
  wrap a cargo build in `timeout`: it killed the release build (exit 143). If
  autocfg panics "output path is not a writable directory", clear ReadOnly on
  `target/*/build/*/out`. `check-code-structure.py` counts braces naively: a
  `'{'` char literal unbalances a function; spell it `'\u{7b}'`.
- **clap gotcha:** `requires = "X"` is waived when X `conflicts_with` a present
  arg. Use a runtime check (rust RAG lesson written).
- **Inbound:** check all three channels. The open GitHub issue (text layer
  position after OCR) still awaits Ken's OK to close; Ken said the text layer
  was fixed.
- **`580.0` OCR `--magnify`** numbers and the unshipped bicubic trial are in
  the SESSION_LOG entry for the 1067th filing.

## Follow-ups worth a Pass

- **Table cell boxes are inferred** (even grid over the region; disclosed).
  A later Pass could measure them from ruling lines or the page image.
- **3D small nuts** (see *State*): widen or reshape the collinear-sliver
  search in `pdfcer-3d/src/compressed.rs`; lead: spec RAG
  `prc__8137__tess_3d_compressed.md` §2c. Re-create a local probe if needed;
  none is in the tree now.
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
- `.claude/agent-memory/pdfcer-spec-librarian/` edits (another agent's).

## How to push

1. `CARGO_BUILD_JOBS=4 bash tools/run-gates.sh > log 2>&1 < /dev/null` with
   `run_in_background` **and `timeout: 7200000`** (the 30-minute default
   killed a run); redirect to a file, never pipe (a pipe reports the
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
