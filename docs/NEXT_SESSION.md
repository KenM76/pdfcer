# NEXT_SESSION.md — engineer handoff

**Read this first on resume**, then the newest `docs/SESSION_LOG.md` entry.
Engineer-owned; replaced each session. The previous long-form handoff
(gate-sweep history, the carried OWED list, benchmark recipes) is in git:
`git show 78635459:docs/NEXT_SESSION.md`. Grep it; do not re-adopt it wholesale.

**Written:** 2026-10-14, after `Pass 587.0` and the 1071st filing.

## State

- **Release:** v0.82.0 is being cut from this state into OneDrive `pdfcer1`
  (v0.81.0 is in `pdfcer2`). If `git tag` shows v0.82.0, the next release
  writes `pdfcer2`.
- **Ledgers:** next free Pass **588.0**; next filing **1072nd**; no decision
  added since 183. Core-api verb count **370**; EditError has 183 variants.
- **pdfcer-gui G187-G193 are all shipped as `581.0`-`587.0`**, each with a
  FIXED reply. `587.0` (G193) added `copy_fields`/`paste_fields` and the
  `FieldClipSet` clip. The paste is all or nothing, with one undo entry. CLI
  twins are `copy-fields` and `paste-fields`.
- **The clip readers refuse what the writer never emits**: NaN/Inf
  coordinates, and stream data beside a non-dictionary (`rebuild_stream`).
  `clipboard_payloads` found both; a 300 s rerun (3M runs) is clean. On this
  machine libFuzzer writes no crash artifact: add a temporary
  `std::fs::write` of `data` before the failing assert to capture it.
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
