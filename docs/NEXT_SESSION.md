# NEXT_SESSION.md — engineer handoff

**Read this first on resume**, then the newest `docs/SESSION_LOG.md` entry.
Engineer-owned; replaced each session. The previous long-form handoff (mesh
failure classes, probe recipes, release recipe detail) is in git:
`git show 40e7d1d7:docs/NEXT_SESSION.md`. Grep it; do not re-adopt it wholesale.

**Written:** 2026-10-21, during the size-reduction arc (Ken 2026-10-20: "meet
or exceed" Acrobat's optimizer, with per-object control). Decision 191 governs.

## State

- **Release:** v0.89.0 is published (OneDrive `pdfcer2`). **The next release
  writes `pdfcer1`.** Release gates are change-scoped (Ken, 2026-10-16): test
  the crates changed since the last tag + the filing gates + the fresh-folder
  smoke; `run-gates.sh` is CI's job. `verify-release` fails only "working tree
  clean", on the uncommitted spec-librarian memory files (deliberate).
  Recipe: bump `Cargo.toml` + `cargo update -w` (root and `fuzz/`), librarian
  files the bump, tag the FILING commit, push both, `package-portable.py
  --note`, zip with Python, `sha256sum`, `gh-release.py`, `deploy-onedrive.py
  --build`, `verify-release.py`, notify pdfcer-gui. Move untracked probe `.rs`
  files out first or the build is `-dirty`.
- **Size-reduction arc (`616.x`):** 616.0 space audit, 616.1 optimize
  framework (presets, TOML profiles, rules, per-object overrides,
  never-larger), 616.2 structure rebuild + navigation clean-up, 616.3
  discards, 616.4 images (complete) — shipped, core + cli. 616.5 encoders:
  RunLength/LZW/CCITT G4/JBIG2/auto/version-raise and Brotli (streams +
  images, decision 194) shipped; still open only for the Acrobat Reader
  render checks (Brotli recorded as unverified). **616.6 fonts: slices 1-5
  shipped**: unembed (`--unembed-fonts`), TrueType and CFF re-subset by
  glyph zeroing (`--resubset-fonts`), merging copies of one face (on by
  default, `--no-merge-font-subsets`; TrueType only). A cut leaves
  `/CIDSet`/`/CharSet` byte-identical: an emptied glyph is still present
  (spec RAG `pdfa/pdfa__ref__font_subset_cidset_charset.md`); slice 5's
  list rewrite under-claimed and failed veraPDF PDF/A-2b 6.2.11.4.2-2, so it
  was reverted. Plan: `docs/font-resubset-plan.md`. PDF/A documents are
  now re-subset (veraPDF verdicts unchanged on every fixture). A cut that
  tags an untagged A-1 font now writes the `/CIDSet` or `/CharSet`
  it owes, listing every glyph the program holds (slice 7); a list it
  cannot write refuses as `pdfa1-list-owed`. Optimize no longer packs object
  streams in an A-1 document (19005-1 §6.1.4). read-fonts 0.39.2 charset
  lookups are unbounded; pdfcer-render bounds them (`charset_glyph`) -- any
  new charset/encoding call must go through it. **616.6 COMPLETE**: the
  Reader check passed (9 fixtures, zero page-pixel difference); CFF merge,
  Type 1 and short loca are measured Backlog residue. **616.7 (content
  streams) in progress**: design is a `content(...)` StreamAction patch
  like `image(...)` (per-object via `--object`/rules) over
  `reemit_canonical`; rounding is lossy, disclosed as maximum user-space
  displacement through `cm`/`Tm`. Parity research from the acrobat
  librarian lands in `Acrobat_Features/` -- name it here when it does.
  Then 616.8-616.10 (linearization, quality/byte-budget, compatibility).
- **Fixture generator:** `tools/gen-resubset-fixtures.py` needs fontTools
  for `symbolic.pdf`. Python string escapes typed through Bash lose their
  backslash (an octal escape ended up as a raw 0xC9 byte), so write edit
  scripts to a file first.
- **Questions for Ken, raise when the Pass needs them:** lossy JBIG2 off by
  default? a transparency flattener? JPX encoder (licence check first)?
- **G225 (text opacity reader):** shipped as Pass 617.9, and the reply is in
  `open/`. Form leaves now inherit the alpha at `Do`, with a transparency
  group reset to 1.0. Readings changed from 1.0, so suspect that first if a
  GUI opacity report about forms arrives.
- **pdfceGUI heads-ups:** `encode=auto` presets and the slice-7 re-subset
  changes are both in `open/` as notices; nothing owed.
- **CI:** 88dfe717 failed the public-fn-docs audit (`emit.rs::write`), fixed
  in 6d77fac3. Run every `check-ci-parity.py --list` audit before a push —
  a hand-typed sweep missed it.
- **3D compressed meshes:** goal met (door 155/1/0, trailer 127/14/0);
  "best-fit" is not "right" (m52/m96 mirrored, X50/X51). Ledger: spec RAG
  `threed/prc__8137__tess_3d_compressed.md`. Older detail:
  `git show 40e7d1d7:docs/NEXT_SESSION.md`.
- **Probe kit (LOCAL):** `probe.rs`, `probe_trace.rs` (`fits` census),
  `probe_ev.rs` (`ev_set/ev_scan/ev_swap/ev_why/ev_conflict/ev_search`) are in
  this session's scratchpad `p613/hold/`; mod lines go in `compressed.rs` and
  `search.rs` with `// P603 LOCAL`. Dumps: scratchpad `p603/{door,tr}` of
  session `9ea26e0e-...` (`P603_DIR` with forward slashes).

## Next

1. Pass 616.7 content-stream optimisation (see State).
2. Each loop tick: the three inbound channels (GitHub issues, both
   FeatureRequests folders).

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
   `check-passes-filed.py`, `check-requests-scoped.py`,
   `check-commits-filed.py`, `check-core-api-verbs.py` (CI runs it); the Next-up `###`
   count stays 80.
5. Push, confirm with `git ls-remote`, read CI with `gh run list`.

## Build environment

- Whole-workspace cargo work can OOM here; when Ken is at the PC use
  `CARGO_BUILD_JOBS=4` and single-crate builds. Release-linking `pdfcer-cli`
  is the reliably heavy step.
- CLI integration tests are one binary: `cargo test -p pdfcer-cli
  --all-features --test all <filter>`.
- `target/debug/deps` grows without bound; check `du -sh` occasionally.
