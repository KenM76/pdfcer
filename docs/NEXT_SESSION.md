# NEXT_SESSION.md — engineer handoff

**Read this first on resume**, then the newest `docs/SESSION_LOG.md` entry.
Engineer-owned; replaced each session. The previous long-form handoff (mesh
failure classes, probe recipes, release recipe detail) is in git:
`git show 40e7d1d7:docs/NEXT_SESSION.md`. Grep it; do not re-adopt it wholesale.

**Written:** 2026-10-20, after Pass 614.0. Ken's directive 2026-10-16 ("only
goal is the 3D compressed-mesh failures") is **met on the census**: no door
or trailer mesh fails.

## State

- **Release:** v0.88.0 is published (OneDrive `pdfcer1`). **The next release
  writes `pdfcer2`.** Release gates are change-scoped (Ken, 2026-10-16): test
  the crates changed since the last tag + the filing gates + the fresh-folder
  smoke; `run-gates.sh` is CI's job. `verify-release` fails only "working tree
  clean", on the uncommitted spec-librarian memory files (deliberate).
  Recipe: bump `Cargo.toml` + `cargo update -w` (root and `fuzz/`), librarian
  files the bump, tag the FILING commit, push both, `package-portable.py
  --note`, zip with Python, `sha256sum`, `gh-release.py`, `deploy-onedrive.py
  --build`, `verify-release.py`, notify pdfcer-gui. Move untracked probe `.rs`
  files out first or the build is `-dirty`.
- **3D compressed meshes:** door `TS-0232-16-3D.pdf` 155 unique / 1 best-fit /
  0 failed; trailer `TR-0522.pdf` 127 / 14 / 0. `613.0` (`ebc02bc6`): a
  borrowing planar face signals only its tall triangles (m64). `614.0`
  (`f71554e1`): the event search may add a choice up to `EVENT_BACK` 8 before
  a set's last; budget 5M steps (m95, 6.9 s, the slowest mesh). Ledger: spec
  RAG `threed/prc__8137__tess_3d_compressed.md` (X76–X78).
- **Open on meshes, not failures:** "best-fit" is not "right" — m52/m96 are
  known mirrored best fits, and fold-backs exist even in unique fits (X50/X51).
  pdfcer-gui consumes `MeshFit` (it corrected an earlier handoff that said
  otherwise); no `mesh_fit` ask is outstanding.
- **Probe kit (LOCAL):** `probe.rs`, `probe_trace.rs` (`fits` census),
  `probe_ev.rs` (`ev_set/ev_scan/ev_swap/ev_why/ev_conflict/ev_search`) are in
  this session's scratchpad `p613/hold/`; mod lines go in `compressed.rs` and
  `search.rs` with `// P603 LOCAL`. Dumps: scratchpad `p603/{door,tr}` of
  session `9ea26e0e-...` (`P603_DIR` with forward slashes).

## Next

1. Release v0.89.0 (carries 613.0 + 614.0) to `pdfcer2`; notify pdfcer-gui.
2. Check the three inbound channels (GitHub issues, both FeatureRequests
   folders) — the mesh goal no longer blocks the feature-request loop.

## Follow-ups worth a Pass

- **Table cell boxes are inferred** (even grid over the region; disclosed).
  A later Pass could measure them from ruling lines or the page image.
- **Librarian-flagged stale wording** in the decision log (ARCHITECTURE
  around lines 12983 and 13008) about whole-page-only VL reading.

## Local-only material (never commit)

- Model weights: `D:/models/vl-src`, `D:/models/addons/paddle-vl` (incl.
  `layout.onnx`), any `inference.onnx` in the scratchpad.
- The operator's real PDFs and their dumps; the reader-field, vl-spike and
  prc-probe folders under `D:\Dev\pdfTests\`; the `p602/` and `p603/`
  scratchpads (oracles, `probe_src/`, per-mesh dumps `door/` `tr/`, the
  `fits_*`/`audit_*`/`chain*.txt` results, renders). Any `// P603 LOCAL`
  line is probe scaffolding and never ships.
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
