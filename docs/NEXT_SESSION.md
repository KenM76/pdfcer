# NEXT_SESSION.md — engineer handoff

**Read this first on resume**, then the newest `docs/SESSION_LOG.md` entry.
Engineer-owned; replaced each session. The previous long-form handoff
(gate-sweep history, the carried OWED list, benchmark recipes) is in git:
`git show 78635459:docs/NEXT_SESSION.md`. Grep it; do not re-adopt it wholesale.

**Written:** 2026-10-20, Pass 606.0 shipped (see *Resume here*); earlier: 2026-10-19, mesh research after `v0.85.0` was published (tag
at `91f287f3`, carrying `603.0`–`605.0`; nothing shipped since — see
*Resume here*). Ken's directive 2026-10-16:
"release a new version and let pdfcer-gui know of the changes … then
continue trying to fix the issue" — releases follow the normal standing
authorisation. **Ken's only open goal is the 3D compressed-mesh failures
below; no feature-request loop and no other Passes while it is open.**

## State

- **Release:** v0.86.0 is published (tag `v0.86.0` at `bedd8329`,
  GitHub 8 assets, OneDrive `pdfcer1`; `pdfcer2` keeps 0.85.0). **The next
  release writes `pdfcer2`.** No API change in it. pdfcer-gui was told in
  `pdfce_FeatureRequests/open/notice_2026-10-20_v0.86.0_released.md` (no
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
- **Ledgers:** next free Pass **607.0**; next filing **1098th** (check
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
  §2c/§2d before trying another rule. **The m48 axis findings of
  2026-10-18 (AXAT/ZAT, "topological at 53") were measured with the probe's
  `P603_MOR` static defaulting to 0 — WITHOUT `605.0`'s rule — and are
  void.** With MOR=4 the control censuses are door 155/0/1, trailer
  127/10/4 (`results/fits_*_mor4.txt` in the backup folder; census files
  duplicate lines, `sort -u`). Spec RAG rows X58–X67 (dispatch labels X66–X75) file today: see
  *Resume here*. Known failure classes (all with the
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

## Resume here: Pass 606.0 SHIPPED (2026-10-20); three trailer meshes left

`04e769fc` (`606.0`, 1097th filing): (a) `compressed/bend.rs`, the
incremental planar-bend check (10 tol x (1 + 3 x distance / running
diameter)); (b) `search::events`, best-first, inert-pruned, last in
`best_fit` after `greedy` (window 300, budget 1M steps). Census exactly as
predicted: **door 155/1/0 (m48 F->B), trailer 127/11/3 (m63 F->B)**, no mesh
worse, slowest trailer mesh 4.6 s. Probe files are OUT of the tree again
(backups in `D:\Dev\pdfTests\prc-probe\p603\`; the trimmed census harness is
`probe_test_p606_census.rs` there: copy it in as `probe_test.rs` with
`probe.rs`, and add `mod probe; mod probe_test;` under `#[cfg(test)]` beside
`mod bend;` to run `fits`). Dumps:
`C:\Users\Ken\AppData\Local\Temp\claude\D--Dev-pdfcer\9ea26e0e-7096-4476-9831-f1c4c5306c0c\scratchpad\p603\{door,tr}`;
the PDFs are in `C:\Users\Ken\OneDrive\pdfTests\`.

**Next:** trailer m64, m95, m107 (finding 6 below: the fan-closing REF
stall class). Start with m95 against m63's fitted geometry, ti 55-67.

**Findings, all measured** (release probe binary, MOR=4; judge = per-mesh
census vs control door 155/0/1, trailer 127/10/4):

1. **The collinear-sliver fold is a coin flip over the stored data.**
   `thinfold` census over every production fit, by [x+ in-edge, x+
   beyond, x- in, x- beyond]: door folds [11,0,10,31] vs unfolds
   [24,0,12,18]; trailer [42,3,57,36] vs [94,3,57,47]. No sign rule (X32/
   X41 already said so). The fix is a search, not a rule.
2. **A stored-normal cosine gate is dead** (`P603_GEO/GEON/GEOH`): along
   TRUE fits |cos| drops to 0.27 (Normal) / 0.40 (Reused) on tall
   triangles of curved faces; a fresh Rev record is in the triangle's own
   frame and carries no placement. At 0.95: door 132 U, trailer 98 U.
3. **The incremental planar-bend gate is valid and gains m48.** LOCAL
   block in `Walk::step` (~line 843): per planar face a running centroid
   plane (`psum`, undone via `plog` on rewind); a new vertex farther than
   `P603_IBEND` tol + `P603_IDEF` × the face's running deflection fails
   "a planar face bends while rebuilding". It is `MAX_BEND` (20 tol,
   post-hoc in `flat()`) applied while walking. IBEND=10 IDEF=3: **door
   155/1/0 (m48 F→B under the unchanged production search), trailer
   127/10/4 identical per mesh.** Blind on curved faces (trailer parts are
   79% curved-face triangles; m48 is 55% planar).
4. **A wrong sliver fold dies fast.** `symptom` on m63's 21-flip fit: with
   any ONE flip removed the walk fails within +1..+7 steps for 17 of 21
   (+37/+65/+206/+248 for F47/F963/F4500/F8827; `results/symptom_m63.txt`).
   Vertex coincidence, fold-back (dot<−0.9) and same-face dihedral >60°
   all occur along the TRUE fit (counts 8/39/209) — not oracles.
5. **The event-DFS blows up on ordering and inert slivers, not depth.**
   Depth histogram (`DFS_HIST`): linear to depth 11, then ~1e5 nodes at
   depths 22–36. (a) most-recent-first picks the wrong candidate when a
   wrong default survives ~250 steps (F4500/F4501/F4504 are needed
   JOINTLY for fail@4505); (b) ~30 slivers per 300-step window are INERT
   — flipping one leaves the failure step unchanged — and both DFS and
   best-first enumerate their subsets. Uniqueness proof (MAXFITS=1) and
   window size were measured NOT to be the cause (11M steps either way;
   W=64 loses F8827). Fix = trial-walk every fold/turn candidate within
   300 steps of the failure, DROP those whose failure step does not move,
   order the rest farthest-first (`P603_ORDER`/`P603_INERT`/`P603_WIN`;
   tests `dfs`, `bestfirst`). **m63 fits in 53,668 steps / 27 expansions /
   106 ms** (was 11M steps, 17 s), same 21 flips as the 3 earlier fits:
   `F46,F47,F374,F601,F963,F1200,F2146,F2171,F2772,F3195,F3808,F4500,
   F4501,F4504,F4703,F4739,F4747,F5800,F6173,F8827,F9187`
   (`results/m63_flips.txt`). Inert pruning is incomplete in principle
   (two jointly-needed flips each inert alone) — accepted, measured.
6. **m64, m95, m107 are a different class.** Best-first + inert pruning
   EXHAUSTS (not budget-cut): m64 at step 6376 (deepest `F46,F48,F378,
   F700,F814,F815,F890,F959,F1679,F1684,F2822,F3203,F3539,F3698,F4467,
   F5566,F6330,F6341,F6351,F6362,F6365`), m107 at 3876 (`F16,F112,F116,
   F120,F867,F1108,F1198,F1551,F1660,F2072,F2741,F2834,F3843,F3864,F3865,
   T3866`), m95 at 67 (no flips; the bend gate fires — a planar face via
   vertices 19/20 bends; without the gate production dies later). Every
   fold (mode 3) or every triangle (mode 4) as a branch point: nothing in
   12M steps. All three stall where a REF closes a fan onto an early
   vertex (m64 PT raw [75,−1,0] on (3368,3352,3236); m107 REF [1812] from
   (1814,1813,2145); m95 REF [19] from (48,20,47) after a kind=Unk REF
   [19] cos=0 at ti=57, REF [20] cos 0.19 at 64, thin unsignalled 65,
   degenerate 66). Not a fold/turn. Siblings: m63≈m95 (161 faces/68
   planar; T 9973/9963) and m64≈m107 (157/67; 9369/9367) — not exact
   copies, but m63's fitted geometry is the nearest oracle for m95: start
   there, ti 55–67.

**Pass 606.0 as designed (shipped; kept for the record):** (a) make the
incremental planar-bend check permanent in `Walk::step` (allowance 10 tol
+ 3 × the face's running deflection; cite WD 7.8.9.1 planar faces; relate
to `MAX_BEND`; keep `flat()`); (b) add the windowed, inert-pruned,
farthest-first event search (fold+turn candidates within 300 steps of the
failure, trial-walked, ~1M step budget) as a stage after `unique_fit`
inside `best_fit` (`search.rs`), returning best-fit (B, disclosed) unless
proven unique. Expected: door 155/1/0 or 156/0/0; trailer 127/11/3 (m63).
Then: census on both samples (no mesh worse than control), fmt/clippy,
change-scoped gates, commit code alone, filing gates, push, librarian
(hashes + census), spec RAG rows after X67 via `pdfcer-spec-librarian` (the 606.0 census;
findings 1–6 are already rows X58–X67), release → OneDrive `pdfcer1`.

**Probe recipe:** `CARGO_BUILD_JOBS=4 cargo test -p pdfcer-3d --release
--no-run` → `target/release/deps/pdfcer_3d-<hash>.exe <test> --ignored
--nocapture` with `P603_DIR=<dumps>/tr P603_ONLY=63 P603_IBEND=10
P603_IDEF=3 P603_INERT=1 P603_WIN=300 P603_DFS=2 P603_BUDGET=12000000`.
`fits` = census; `P603_FLIPS=F46,...` replays a fit (`rawsteps` with
`P603_SHOW=a..b`); dumps are regenerated by `dump_prc` (`probe.rs`) from the two PDFs.
Never rebuild while a census runs (LNK1104).

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
