# NEXT_SESSION.md — engineer handoff

**Read this first on resume**, then the newest `docs/SESSION_LOG.md` entry.
Engineer-owned; replaced each session. The previous long-form handoff
(gate-sweep history, the carried OWED list, benchmark recipes) is in git:
`git show 78635459:docs/NEXT_SESSION.md`. Grep it; do not re-adopt it wholesale.

**Written:** 2026-10-18, after `v0.85.0` was published (tag at
`91f287f3`, carrying `603.0`–`605.0`). Ken's directive 2026-10-16:
"release a new version and let pdfcer-gui know of the changes … then
continue trying to fix the issue" — releases follow the normal standing
authorisation. **Ken's only open goal is the 3D compressed-mesh failures
below; no feature-request loop and no other Passes while it is open.**

## State

- **Release:** v0.85.0 is published (tag object `f5dedeb9` at `91f287f3`,
  GitHub 8 assets, OneDrive `pdfcer2`; `pdfcer1` keeps 0.84.0). **The next
  release writes `pdfcer1`.** No API change in it. pdfcer-gui was told in
  `pdfce_FeatureRequests/open/notice_2026-10-18_v0.85.0_released.md` (no
  reply owed; the 0.84.0 ask to expose `mesh_fit` still stands). Local
  release gates are **change-scoped** (Ken, 2026-10-16): test the crates
  that changed since the last tag + the filing gates + the fresh-folder
  smoke; `run-gates.sh` is CI's job. `verify-release` fails only "working
  tree clean", on the uncommitted spec-librarian memory files (deliberate).
  Release recipe that worked: bump `Cargo.toml` + `cargo update -w` (root
  and `fuzz/`), librarian files the bump, tag the FILING commit, push
  both, `package-portable.py --note`, zip the build folder with Python
  (`Compress-Archive` through bash eats the `\$` paths), `sha256sum`,
  `gh-release.py`, `deploy-onedrive.py --build`, `verify-release.py`.
  Delete untracked `.rs` probe files before packaging or the build is
  marked `-dirty`. `render-page` takes `--scale`, not `--width`; the OCR
  fixtures are `fixtures/synthetic/ocr/{printed,scan}.pdf`.
- **Ledgers:** next free Pass **606.0**; next filing **1096th** (check
  ROADMAP's newest ledger line); no decision added since 183. Core-api verb
  count **378**.
- **3D compressed meshes after `605.0`:** door `TS-0232-16-3D.pdf` 155/156
  unique (**m48** fails); trailer `TR-0522.pdf` 127 unique / 10 best-fit /
  4 failed (failed **m63 m64 m95 m107**; m39 is now a best-fit; after
  `604.0` it was 125/11/5, after `603.0` 123/12/6, 117/19/5 at v0.84.0).
  `605.0` (`28e9a53f`) = at a degenerate apex frame take only `Z` from
  `MakeOrthoRep(X)` and rebuild `Y = unitize(Z × X)` from the frame's OWN
  `X` (MakeOrthoRep re-unitizes its copy of `X`; `unitize` is only ~1e-8
  accurate in length, and the walk amplifies 1e-13 tol of noise to ~5e-4
  tol by triangle 450, then a degenerate frame flips the `along` sign).
  Sibling-instance oracle (m23↔m45, m39↔m153) found it: m23@1216 went
  13.9 tol → 0.82. Spec RAG X60–X65. `603.0` = the blind-normal rule (`BLIND_NORMAL`
  0.01: a record normal in the triangle's plane makes its reversed bit
  noise; default fold, unsignalled, sliver retry) + `thin` narrowed to
  weak AND collinear. **The spec RAG `threed/prc__8137__tess_3d_compressed.md`
  is the ledger of every hypothesis: R17, X42–X49; X38/X41 closed** — read
  §2c/§2d before trying another rule. Known failure classes (all with the
  chain of flips a greedy search tried): door m23 degenerate frame after
  collinear 484; m39 `Turn(589)`; m48 a chain of degenerate-frame apexes
  (51/109/121/turn 87, stuck 157). Trailer m39: triangle 702, a
  reused-normal reference apex, wrong under every rule; m52/m107 chains
  from collinear thin 43/16; m115: triangle 1008 at |cos| 0.02 (not blind
  at 0.01), stuck 5276; m63/m64/m95 unexamined. **First thing to examine:
  m108** (T=3916, same size as m53/m58 — the trailer nut family): it fit
  at v0.84.0 and fails now, so either a weak non-collinear triangle there
  must NOT trust its record (a counterexample to the `thin` narrowing) or
  it is a second blind-ish case above 0.01 like m115's 1008. Also: m52
  (66 inverted/59 strong) and m96 (62/56) "best fits" are mirrored wrong
  fits; m110 hits the search budget. Threshold lesson: 0.2 regressed m90
  by stage-deepening ambiguity (fit pushed from depth 4 to 5 into a run of
  unsignalled reused-normal weak triangles 3512..3633), not by a wrong
  fold — judge any rule by per-mesh U/B/F, not by the fold it changes.
  Research negative: no public non-GPL description of the encoder's
  sliver-fold rule; patent US 8,207,965 B2 reads "Expired - Fee Related"
  on Google Patents only (not USPTO-verified — surfaced to Ken).
  Baseline recipe: the previous release binary under `D:/builds/` and
  `3d-render`'s note lines ("N rebuilt / M best-fit / K left out") are the
  cheapest regression check; use a release build (debug takes >20 min on
  the trailer). `602.0`'s `--mesh-fit partial|continue` floor is unchanged.
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

## Shipped this session: Passes 604.0 and 605.0

- `28e9a53f` (`605.0`): see *State*. Rejected by census: skipping
  MakeOrthoRep's re-unitize entirely (fixes m23, breaks door m117 + 11
  trailer meshes); re-unitizing `X` for the apex's own `d.x` (breaks
  everything). The LOCAL `compressed.rs` with the `P603_MOR` /
  `P603_NOISE` / `P603_FLIPS` knobs, `frame.py`, `sib2.py`, `growth.py`
  (noise-growth vs a sibling) are in `D:\Dev\pdfTests\prc-probe\p603\`.
- `be4bbe81` (`604.0`): after a failed walk the search tries the last 40 unsignalled
  folds + turns (`PRIMARIES`) and at most 20 signalled weak folds
  (`SECONDARIES`) among them, instead of the last 40 of everything. m108's
  wrong fold (unsignalled sliver 2989) was hidden behind 136 signalled weak
  folds in the old window; a wrong fold consumes every stored number so
  the fault surfaced only at 3149. Measured (release `3d-render` notes):
  door 153/0/3 unchanged, trailer 123/12/6 -> **125 unique / 11 best-fit /
  5 failed** (m108 F->U, m53 B->U). Still failed: door m23/m39/m48, trailer
  m39 (1351) m63 (1034) m64 (53) m95 (122) m107 (294).
- **Ken's observation, confirmed (not shipped):** faces before a reported
  failure are already wrong. A fold-back census (new triangle anti-parallel
  to a non-sliver neighbour across a shared edge, dot < -0.995, slivers
  sine < 1e-4 excluded) finds fold-backs in all 12 trailer best-fits, 27 of
  123 trailer uniques (the nut family, first at ~74% through), 10/153 door.
  "Unique" is not "right". Rejecting fold-backs in the walk fails every
  such mesh (trailer 96/2/43, door 143/0/13): no fold/turn flip, no apex
  mirror rescues them; the nut-family fold-back apexes are REUSED vertices,
  so the error is upstream traversal, not placement. m44 (thin plate)
  folds back from triangle 2 as a unique fit, so the detector is a
  diagnostic, not a rule. Spec RAG X50/X51.
- **Next lens:** the nut family's reused-vertex fold-back at ~74% — what
  the walk did one step before the first fold-back (the stack/next choice,
  not the apex). The probe modules that find it (`probe108.rs`:
  `calib`, `fbdetail`, `fbgeom`, `automirror`, `brute`, `firstbad`) and
  the LOCAL `search.rs`/`compressed.rs` with the detector are in
  `D:\Dev\pdfTests\prc-probe\p603\`; re-apply by hand, never commit.
  pdfcer-gui has no per-mesh render; ask Ken to eyeball a nut vs a plate.

## Follow-ups worth a Pass

- **Table cell boxes are inferred** (even grid over the region; disclosed).
  A later Pass could measure them from ruling lines or the page image.
- **3D compressed meshes** (see *State* and *Shipped this session*): five
  meshes still fail (door m48; trailer m63 m64 m95 m107) and trailer m39
  is a best-fit. Same lens as `605.0`: rare-branch arithmetic that differs
  from the encoder by ~1e-8 (every affected mesh fails right after a thin
  triangle / degenerate frame); use a sibling instance as the oracle where
  one exists, then the nut family's fold-back. Never read
  nanoPRC/libPRC/Asymptote/MuPDF/Ghostscript/Poppler source (licence rule,
  decision 169). The probe modules (`probe.rs`, `probe_test.rs`: `fits`,
  `audit`, `chain`, `detail`, `blindcensus`, driven by `P603_DIR` pointing
  at per-mesh array dumps) are backed up in the session scratchpad
  `D:\Dev\pdfTests\prc-probe\p603\`; re-create from there, never commit them. pdfcer-gui
  does not yet consume `MeshFit::Partial/Continue`.
- **`continue` on the trailer sample is untimed**: the debug exe took >20 min
  per `3d-render` of it; use a release build for any measurement there.
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
