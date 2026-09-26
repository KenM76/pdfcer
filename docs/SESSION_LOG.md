# pdfce — Session log

Append-only. One section per session date. Never overwrite or reorder
a prior entry; corrections get a dated amendment footer appended to
the affected entry. Maintained by `pdfce-librarian`.

## 2026-09-25 (594th filing) — `Pass 325.0` step 5 SHIPPED (`0fe973a2`): the 46k-line `pdfcer-cli` `main.rs` split into 28 subcommand modules; a certification-check gap the split exposed is fixed

**Shipped:**
- `Pass 325.0` step 5 (`0fe973a267ed02e811ef305e5c81b57706722ace`) — `crates/pdfcer-cli/src/main.rs` cut from ~46,000 to 917 lines; 28 new modules (`cli.rs`, `dispatch.rs` + 26 feature modules). All 320 `--help` pages byte-identical to pre-split; clippy and the Linux cross-target check clean. `Pass 325.0` stays **IN PROGRESS** — steps 1–4 and 6–7 (the model-crate extraction, leaf feature crates, the `pdfcer-core` facade, the `#[cfg(test)]`-to-`tests/` move, housekeeping) remain open. Also landed, routine, no Pass: `ccf8daecb0788eaefc161bb3d4d721574bc08c6b` — OCRcer re-synced to local HEAD `926e315e51ab` (decision 160's standing rule; upstream added `feature::extract_with_grid`, additive).

**Decisions made this session:** none new — the split executes the plan `Pass 325.0` already scoped at the 586th filing; no new crate-boundary or invariant decision was made this step.

**Findings + decisions:**
- The split exposed a real defect predating it: `check-bypass-paths.sh` truncated each file at its first `#[cfg(test)]`, so it never read the second half of the old `main.rs` — `import-structure` wrote through `save_full`/`save_incremental` with no DocMDP certification check. Fixed in the same commit: `cmd_import_structure` (now `structure.rs`) honours `EditSession`'s certification refusal (exit 9, ISO 32000-1 12.8.4) on a non-empty import against a certified document; the remaining object-level write is `// bypass-exempt:`-flagged (qpdf QDF parity, one-shot, prints every changed object id). New test `crates/pdfcer-cli/tests/import_structure_certified.rs` (2 tests).
- `check-public-fns-documented.py` had an independent false-positive: a bare `//` line (an `#[allow]` justification) between a doc comment and its item detached the doc, reported as undocumented. Fixed; 16 pre-existing welded doc blocks the split exposed were returned to their own items; 6 stale baseline rows deleted.
- Generalisable finding: a gate scoped by a fixed marker or "read the whole file" stops covering the file once the file outgrows the assumption that made that scoping look complete — a large-file split is itself an audit of every scanning gate that reads it. Written to `D:\dev\rag\rust\a_gate_scoped_by_a_fixed_marker_or_file_size_stops_covering_the_file_as_it_grows.md` and filed as a dated instance of standing rule `R227` (two gates, not a new rule number — R227's own wording already covers both).
- Gates repointed to the new module layout: `check-clap-help.py`, `check-cli-help-leads.py`, `check-metrics-line-contract.py`, `tests/font_licence_notice.rs`.
- `tools/run-gates.sh`: 35/36 green — the one red, `check-commits-filed.py`, is this filing's own unfiled state, closed by this filing. ~9,888 tests passed across the workspace (all binaries + doctests). `cargo tree -p pdfcer-core`/`-p pdfcer-render` unchanged — no manifest touched.
- `docs/core-api/01`–`03`'s `main.rs:NNNN` pointers replaced by symbol names + module (already committed in `0fe973a2`, engineer-owned). `ARCHITECTURE.md` §3 gained a body note on the new module layout; §12's live invariant-table row (verb save-mode citation) repointed from `main.rs:10733/10735/10743` to `save_edited`/`cmd_import_structure` by their new module paths — historical decision-log `main.rs:NNNN` citations were left untouched, per convention.
- `docs/FEATURES.md`: no row change. The split is tooling, and no `import-structure`/QDF row exists in `FEATURES.md` to annotate with the new certification-refusal behaviour.
- This role's own agent file (`.claude/agents/pdfcer-librarian.md` ~line 611) also cites a now-stale `main.rs:10677` — left untouched, as it records a past incident (survivor 7, hard rule 11), not a live pointer.

**Still in flight:**
- `Pass 325.0` steps 1, 2, 3, 4, 6, 7 — all NOT STARTED.
- Two local, unpushed commits (`0fe973a2`, `ccf8daec`) plus this filing's own commit, pending push after the operator's own `tools/run-gates.sh` confirmation.

**For next session:**
- Push `main` (operator's own action, per this filing's request).
- Continue `Pass 325.0` at step 1 (dependency-edge measurement already done 2026-09-23) → step 2 (model-crate extraction).

**Sourcing (hard rule 8).** No shell this filing. All commits, line counts, module names, test/gate results and the defect account above are relayed from the requesting engineer's own report; not independently reproduced here.

## 2026-09-25 (593rd filing) — `Pass 329.0` SHIPPED (`3691999f`/`21af5926`): Tesseract as a third OCR engine, run as a subprocess, with a clean bundled static-MSVC build

**Shipped:**
- `Pass 329.0` (`3691999f`, `.gitattributes` fix `21af5926`) — `pdfcer ocr --ocr-engine tesseract [--ocr-lang eng+deu]`. Core parses TSV only (`ocr::tesseract_tsv::parse_tsv`, no process-spawning, wasm32-clean); CLI spawns `models/tesseract/tesseract.exe` (or `--model-dir`), PGM stdin / TSV stdout, per-word confidence. New `tools/tesseract/build-tesseract.py`: vcpkg overlay port, static MSVC triplet, `DISABLE_CURL`/`DISABLE_ARCHIVE`/`GRAPHICS_DISABLED` — 5.37 MB exe, imports only `KERNEL32.dll`, avoiding the LGPL DLLs the UB-Mannheim/MinGW builds carry. Not a Cargo dependency.

**Decisions made this session:**
- Decision `161` (`ARCHITECTURE.md` §12) — subprocess architecture, static-MSVC build rationale (GPL/LGPL avoidance), default bundled language `eng` only pending a new operator question `(cf)`.

**Findings + decisions:**
- The Tesseract `tsv` config-file name fails without a `tessdata/configs/` directory a minimal bundle omits; use `-c tessedit_create_tsv=1` instead.
- `GRAPHICS_DISABLED` also removes a `WS2_32.dll` (sockets) import pulled in by Tesseract's debug `ScrollView` feature.
- vcpkg caches an install even after an overlay port's files change (`vcpkg remove` first); `bootstrap-vcpkg.bat` fails via `cmd //c` from Git Bash (call `scripts/bootstrap.ps1`); overlay `.patch` files need `eol=lf`. All three written to `D:\dev\rag\rust\`.
- Measured on `fixtures/synthetic/ocr/scan.pdf`: 47/47 words, mean confidence 96.7%, median offset 2.04 pt (vs `ocrs` 2.60 pt) — fixture saturates, shows the pipeline works rather than ranking engines.
- Fixed in passing: `crates/pdfcer-cli/src/main.rs` had `exit`'s doc comment mis-spliced onto `mod clipboard;`.
- Routine, no Pass: `c3fed5bd` — OCRcer re-synced to local HEAD `4533f798d245` per decision 160's standing rule.

**Still in flight:**
- `main` has 3 unpushed local commits (`3691999f`, `21af5926`, `c3fed5bd`) plus this filing's own commit, pending push after `tools/run-gates.sh` green.
- A release is optional — needs `build-tesseract.py` run on the packaging machine first (the exe is built, not vendored as a binary).

**For next session:**
- Push `main`.
- Operator question `(cf)`: which languages beyond `eng` to bundle by default.
- `target-case/` (untracked, repo root, unknown origin, 2026-09-24) — left alone, not investigated this session.

**Sourcing (hard rule 8).** No shell this filing. All commits, measurements, gate/test results and RAG-lesson content above relayed from the dispatching engineer's report; not independently reproduced.

## 2026-09-24 (592nd filing) — `Pass 328.0` SHIPPED (`cc6cd70c`): one test binary per crate + reduced test debuginfo + `check-tests-harnessed.py`, fixing the `cargo test --workspace` OOM

**Shipped:**
- `Pass 328.0` (`cc6cd70c`) — 263 linked integration-test binaries collapsed to 3 (one per crate) via `autotests = false` + `tests/all.rs`; `[profile.test]` debuginfo to line-tables-only; new gate `tools/check-tests-harnessed.py` (CI `audits`, now 26 checks; `run-gates.sh`; `check-ci-parity.py`).

**Decisions made this session:** none new.

**Findings + decisions:**
- `cargo test --workspace`: 5,684 passed / 2 ignored — identical to baseline `1d7654aa` — in 166 s at default parallelism (previously needed `--jobs 2`). Link steps 263 → 3; test-executable footprint ~1.29 GB → 64 MB. `run-gates.sh` 35/36 green (the one red was the prior filing's oversized Next-up entry, closed by this filing).
- Merging the binaries turned separate-process test files into concurrent threads of one process; `tests/external_tools.rs`'s Inkscape oracles collided on Inkscape's own single-instance D-Bus registration, fixed with a `Mutex`.
- Three findings graduated to `D:\dev\rag\rust\` (each its own file, `index.md` updated): the `autotests = false` + `tests/all.rs` merge pattern and its silent-drop trap; the same-process concurrency hazard it exposes; `[profile.test]` scoping over the whole build graph including dependencies.

**Still in flight:** none named this filing.

**For next session:** nothing owed from this Pass.

**Sourcing (hard rule 8).** No shell this filing — commit hash, test counts, gate results and the three findings are relayed from the dispatching engineer's report of commit `cc6cd70c`, not independently reproduced here.

## 2026-09-24 (591st filing) — `Pass 328.0` filed (*Next up*): one test binary per crate + reduced test debuginfo + an unlisted-test gate, fixing the `cargo test --workspace` OOM

**Shipped:** none — filing only.

**Decisions made this session:** none new; scopes the engineer's own diagnosis into a Pass.

**Findings + decisions:**
- Operator (Ken, 2026-09-24) asked whether best-practice cleanup would make the gates easier to run on this machine. Engineer's answer: the real fix is fewer test binaries and lighter test debuginfo, not general cleanup — operator said yes to building that as the next Pass.
- Measured at `1d7654aa`: `crates/pdfcer-core/tests/` 161 files, `pdfcer-render/tests/` 53, `pdfcer-cli/tests/` 49 — 263 separately linked test binaries for one `cargo test --workspace` run, each statically linking `pdfcer-core` with full debuginfo. Observed OOM failures: `LNK1102` on an example, `rustc` exiting `0xc0000409` on `pdfcer-render`'s tests. Current workaround: `-j 2`.

**Still in flight:**
- `Pass 328.0` — NOT STARTED. Plan: one `tests/all.rs` per crate via `#[path] mod` inclusion + `autotests = false`; `[profile.test]` debuginfo to line-tables-only; a new gate refusing a `tests/*.rs` file absent from its crate's `all.rs`.

**For next session:**
- Build `Pass 328.0`. Acceptance: same passing count as `1d7654aa` (5,684, default features); `--all-features` and `-p pdfcer-core --no-default-features` still pass; `tools/run-gates.sh` green; new gate wired into CI + `check-ci-parity.py`; link-step count and `target/debug` size recorded before/after in the Shipped entry.

**Sourcing (hard rule 8).** No shell this filing — file counts, OOM symptoms and the passing-test count are relayed from the dispatching engineer's report of measurements at `1d7654aa`, not independently reproduced here.

## 2026-09-24 (590th filing) — `Pass 327.1` (`748d268c`/`998adb96` + 4 follow-on commits): `ocrcer-core` vendored + default ON, `ocrcer-engine` fast-forwarded into `main` locally (push pending); `Pass 327.2` filed (OCRcer LLM rescoring, BLOCKED)

**Shipped:**
- `Pass 327.1` (`748d268c`/`998adb96`) — `ocrcer-core` VENDORED at `vendor/ocrcer-core` (new `tools/sync-ocrcer.py`, copied from OCRcer's committed HEAD, never its working tree; commit `4862a3d7815429ce3b2c5616578bfbafd60e43b4`) replacing the local path dependency that blocked `Pass 327.0`'s merge. Adapter `ocr/engine_ocrcer.rs` synced too, now carries `MODEL_DIR`/`MODEL_FILE` consts. New gate `tools/check-ocrcer-vendored.py` (CI `audits` job, 25 checks; also in `run-gates.sh`). Feature `ocrcer` flipped to **DEFAULT ON** in `pdfcer-core`/`pdfcer-render`/`pdfcer-cli`; `ocrs` stays the default engine. `vendor/ocrcer-core` excluded from the workspace; `.gitattributes` marks it and the adapter `-text`.
- `Pass 327.1`, four follow-on commits, same Pass: `303287d55c4cf1aa3dc3a0b356e436536416d0ee` (core-api docs for the OCRcer engine, `docs/core-api/03-capabilities.md`); `68eb534181e8f6116f57753c6808a906ece89429` (`tools/package-portable.py` refuses a stale vendored OCRcer before building — decision `160`'s "latest version" directive extended to packaging; `docs/NEXT_SESSION.md` sync procedure); `2161a9cd1b3bd9eb5e25bf94a9903ed0e9ca2347` (`sync-ocrcer.py` writes `vendor/ocrcer-core/rustfmt.toml` with formatting disabled, since `cargo fmt --all` reaches path dependencies; `check-ci-parity.py` classifies the new gate LOCAL; fixes a 22-space string gap from `2039521c`); `e91d1095f3dba3d1123ae4d82e3a58b33d63aeb5` (CLI now imports the adapter's `MODEL_DIR`/`MODEL_FILE` instead of defining its own `OCRCER_MODEL_FILE` — resolves what the 589th filing left unconfirmed).

**Decisions made this session:**
- Decision `160` (`ARCHITECTURE.md` §12), amending `159`: vendoring replaces "path dependency now, pinned git later" as the merge condition; the default-OFF exception is withdrawn, feature is now default ON. Operator (Ken, 2026-09-24): *"always use the latest version of ocrcer available in d:\dev\ocrcer . github might be a few versions behind"* — vendoring from local beats a pinned git tag, which could still lag. §9 paragraph rewritten (not just appended to).

**Findings + decisions:**
- Four-file sweep for the now-stale "branch `ocrcer-engine` ONLY / NOT on `main` / BLOCKER / DO NOT MERGE" wording from the 589th filing: `ROADMAP.md` (`Pass 327.0` header + blocker paragraph struck and annotated RESOLVED, owed follow-ups (a)/(c) marked discharged), `docs/FEATURES.md` (row 220 rewritten), `ARCHITECTURE.md` (§9 paragraph rewritten, decision 159 gets a one-line forward-pointer footer, decision 160 added), `docs/PRIOR_ART.md` (*OCR engines* intro + `ocrcer-core` table row + a new superseding decision-log bullet). `docs/DEPENDENCIES.md` was already correct from `748d268c` — not touched.
- Test count, gate results and the smoke reading for `Pass 327.1` are relayed from the dispatching engineer, not reproduced (no shell this filing). `cargo test -p pdfcer-cli --features ocrcer --test ocr_engine`: 2 passed. Full `tools/run-gates.sh` sweep was reported "in progress," result not yet known to this filing.
- New Backlog entry `Pass 327.2` — OCRcer LLM rescoring (`ocrcer-llm` feature, one-file `.ocrl` add-on), BLOCKED on OCRcer's own 16b gates + an `integration/pdfcer/` LLM adapter. Explicitly NOT gated by R13 (an `.ocrl` file is data read by compiled-in code, not executable code fetched and run) — stated on the Pass so a future filing doesn't misapply R13. New `docs/FEATURES.md` *Planned* row, all boxes unticked.
- **Extension, same filing, four more commits arrived for the same Pass before this filing closed:** `303287d5` (core-api docs), `68eb5341` (`package-portable.py` refuses a stale vendored OCRcer — decision `160` extended to packaging), `2161a9cd` (vendored OCRcer exempted from `cargo fmt --all`, plus the string-gap fix), `e91d1095` (CLI now imports the adapter's `MODEL_DIR`/`MODEL_FILE`). `ROADMAP.md`'s `Pass 327.1` entry re-edited in place to add all four (still under the 150-line Shipped-entry cap) and `docs/FEATURES.md` confirmed to need no row change (row 220 already at core `[x]`/cli `[x]`/gui `[ ]`, unaffected by any of the four).
- `e91d1095` **resolves** what this filing's own earlier text (and `Pass 327.0`'s entry) had called "LIKELY DISCHARGED … not confirmed this filing (no shell)": `main.rs` did duplicate the `ocrcer.ocrw`/`MODEL_FILE` constant; it's now the adapter's own constant, imported. Both `ROADMAP.md` entries corrected to say DISCHARGED without qualification, citing the hash.

**Still in flight / owed:**
- (b) `pdfcer-gui` OCR-engine selection — still not done. (d), the pre-existing `tools/check-string-gaps.sh` failure at `pdfcer-cli` `main.rs` ~21192 from `Pass 326.2`'s `2039521c` — **now DISCHARGED by `2161a9cd`**, no longer open.
- Push of `ocrcer-engine`/`main` still gated on a green `tools/run-gates.sh` sweep, per the engineer's premise this filing; the four follow-on commits are also local and unpushed pending that same sweep.

**For next session:** Confirm the `run-gates.sh` sweep result before pushing (now six unpushed commits for this Pass: `748d268c`/`998adb96`/`303287d5`/`68eb5341`/`2161a9cd`/`e91d1095`). `Pass 327.2` stays BLOCKED until OCRcer reports its 16b gates passing and ships an LLM adapter.

**Sourcing (hard rule 8).** No shell this filing. Taken entirely from the dispatching engineer's own report of `748d268c`/`998adb96`/`303287d5`/`68eb5341`/`2161a9cd`/`e91d1095` and the two verbatim operator sentences relayed earlier in this filing; nothing independently re-verified against live source, commit contents, or gate output.

## 2026-09-24 (589th filing) — `Pass 327.0` (`7c520945`, branch `ocrcer-engine`, NOT on `main`): OCRcer as an opt-in second OCR engine, `pdfcer ocr --ocr-engine ocrs|ocrcer`, feature `ocrcer`

★ **AMENDED 2026-09-24 (590th filing, `Pass 327.1`, decision 160):** the "branch `ocrcer-engine`, NOT on `main`" framing and the "★ Blocker" note below are superseded — `ocrcer-core` is now vendored (not a path dependency) and the branch is fast-forwarded into `main` locally. See the 590th filing entry above.

**Shipped (on a branch):**
- `Pass 327.0` (`7c520945`) — `pdfcer-core` `ocr::engine_ocrcer` (OCRcer's `integration/pdfcer/ocrcer_engine.rs`, applied unmodified) behind the non-default feature `ocrcer`, forwarded by `pdfcer-render`/`pdfcer-cli`. `pdfcer ocr --ocr-engine ocrs|ocrcer`, default `ocrs`; model `ocrcer.ocrw` from `models/ocrcer` beside the exe or `--model-dir`, neither shipped nor downloaded. No feature → refused by name, exit 64; missing/malformed model → exit 1 naming the file. Engine name recorded in the text-layer marker and summary line. New `crates/pdfcer-cli/tests/ocr_engine.rs`.

**★ Blocker:** the branch must NOT merge to `main` while `ocrcer-core` is a local path dependency — Cargo reads optional path manifests, so any checkout without `../OCRcer` (GitHub CI included) cannot resolve the workspace. Unblocks when OCRcer is published and the dependency is pinned via git.

**Decisions made this session:**
- Decision `159` (`ARCHITECTURE.md` §12): OCRcer adopted opt-in by operator directive (Ken, 2026-09-24) — the one exception to the default-ON strippable-capability convention; `ocrs` stays the default; accuracy work later; path dependency now, pinned git later. §9 body paragraph added.

**Findings + decisions:**
- Gates per the engineer: `fmt`; `clippy -D warnings` (default and `--all-features`); `pdfcer-core` no-default ± `ocrcer` 4,207/0; `pdfcer-cli --features ocrcer` 519/0; wasm32 check both ways; `THIRD_PARTY_LICENSES.md` identical; `ocrcer-core` a `cargo tree` leaf. **Not run:** full workspace `--all-features` test / `tools/run-gates.sh` (memory, as `Pass 326.x`).
- Smoke reading, NOT a benchmark (debug, `scan.pdf` p1, 150 dpi): `ocrcer` 31.3 s, 49 words, mean confidence 69.8%, content 89.4% (42/47), median offset 2.91 pt; `ocrs` 34.5 s, no confidence, 100% (47/47), 2.60 pt.
- `docs/FEATURES.md`: new *Text* row "Choose the OCR engine", core `[x]` cli `[x]` gui `[ ]` Acrobat `?`, marked branch-only. `docs/PRIOR_ART.md`: `ocrcer-core` row (MIT; no font data in the model — feature vectors from rendered OFL-1.1/Apache-2.0/MIT faces listed in the model's `meta`, per OCRcer `NOTICE`) plus a decision-log bullet.
- Sweep: the carried `R251`/`R258` standing-rules discrepancy — `tools/check-ledger-numbers.py` says next free `R251`, and a grep of `ROADMAP.md` and `history/standing-rules-full.md` finds no `R251`–`R257` defined, so `R251` looks right. Not changed this filing; the next rule mint should confirm it first. `check-ledger-numbers`, `check-register-entry-size`, `check-passes-filed`, `check-commits-filed` all clean after this filing.

**Still in flight / owed:**
- (a) Switch `ocrcer-core` to a pinned git dependency, then merge. (b) `pdfcer-gui` engine selection. (c) OCRcer adapter nit: `engine_ocrcer` has no `MODEL_DIR`/`MODEL_FILE` constants (`engine_ocrs` has), so the CLI defines `OCRCER_MODEL_FILE` locally — reported to OCRcer. (d) Pre-existing `tools/check-string-gaps.sh` failure at `pdfcer-cli` `main.rs` ~21192, from `Pass 326.2`'s `2039521c`.
- Unchanged from the 588th filing: sweep then push `bb5a37a2`/`b44e6a03`/`2039521c` on `main`.

**For next session:** Do not merge or push `ocrcer-engine`. Fix (d) on `main` before the push sweep.

**Sourcing (hard rule 8).** `git show 7c520945` read; CLI symbols and test names confirmed by grep. Test counts, gate results and the smoke reading relayed from the engineer, not reproduced. This filing committed on `ocrcer-engine`.

## 2026-09-23 (588th filing) — `Pass 326.2` (`2039521c`): the CLI surface for `G039`/`G040`/`G041` — `pdfcer print --line-width`, `--poster-cut-marks`, `--poster-labels`

**Shipped:**
- `Pass 326.2` (`2039521c`) — `pdfcer print --line-width MM` sets `StrokeDisplay::Fixed` at the job DPI for the whole print job (new `print_render_options`), disclosed on stderr. `--poster-cut-marks`/`--poster-labels` (require `--poster`) set `PosterSpec`'s flags; `draw_poster_marks` strokes `cut_mark_segments` and renders `poster_tile_label` into `label_rect` via a one-line Helvetica/WinAnsi synthetic-PDF rasteriser — a non-WinAnsi label character prints `?`, count disclosed on stderr.

**Decisions made this session:** None architectural — a CLI-surface addition to two already-shipped engine capabilities (`Pass 326.0`/`326.1`), not a crate-boundary/library-choice/invariant call.

**Findings + decisions:**
- `docs/FEATURES.md`: new row (*Fonts & rendering*, directly below the hairline row) for `StrokeDisplay::Fixed` — core `[x]`, cli `[x]` (scoped to `pdfcer print --line-width` only), gui `[ ]`, Acrobat `?`. The hairline row's own cli stays `—`, unchanged — the Hairline mode itself still has no CLI route, only a short pointer to the new row was appended to its text. Poster cut-marks/labels row (*Printing*) cli moved `[ ]`→`[x]`. No new `tools/check-register-entry-size.py` finding: the new Fixed row is under the 1,200-char cap; the hairline row was already in `tools/register-entry-size-baseline.txt` as debt and its label prefix (the baseline key) is unchanged. **Amendment, same filing, before commit:** an earlier draft of this edit ticked the hairline row's cli box directly instead of adding the new row — caught and corrected before commit; no separate dated footer needed since nothing was yet pushed.
- Gate status, recorded honestly per hard rule 8: `clippy -p pdfcer-cli --all-targets -D warnings`, `cargo check` (`x86_64-unknown-linux-gnu`), `check-clap-help`, `check-cli-help-leads`, `fmt` ran and are clean per the dispatching engineer's report. **Full `tools/run-gates.sh` sweep NOT yet run** for this commit — same memory constraint as `Pass 326.0`/`326.1`; will run before push. Commit is local and unpushed.

**Still in flight:**
- `Pass 325.0` (crate split) remains not started past its step-1 measurement (587th filing).
- `pdfcer-gui` has not wired `--line-width`'s render option or the poster marks/labels geometry — both stay `gui [ ]`.

**For next session:** Run the full `tools/run-gates.sh` sweep and push `bb5a37a2`/`b44e6a03`/`2039521c` (plus any commits between) together once green.

**Sourcing (hard rule 8).** No shell tool this filing. Confirmed by `Grep`/`Read` against live source: `print_render_options`, `--line-width`/`--poster-cut-marks`/`--poster-labels`, `draw_poster_marks`, `render_poster_label`, `winansi_bytes` all present in `crates/pdfcer-cli/src/main.rs`; `docs/ROADMAP.md` and `docs/FEATURES.md` premises (Pass 326.0/326.1 filed with core `[x]`/cli `[ ]` — true for the poster row, but the hairline row's cli cell was `—` not `[ ]`, corrected in this filing's own edit) verified before editing; `tools/check-register-entry-size.py` and its baseline file read directly to confirm no new cap finding. **Relayed from the dispatching engineer's report, not independently reproduced:** exact test count (8 new, 4 sabotages), the end-to-end "Microsoft Print to PDF" smoke test, and the gate-clean claims. No commit was made this filing (no shell; the engineer commits).

## 2026-09-23 (587th filing) — `Pass 326.0`/`326.1` (`bb5a37a2`/`b44e6a03`): poster cut marks/labels get engine geometry (`G040`/`G041`), `StrokeDisplay::Fixed` (`G039`); `Pass 325.0`'s step-1 edge measurement filed to Backlog

**Shipped:**
- `Pass 326.0` (`bb5a37a2`) — `pdfcer_print::imposition::plan_poster` now reserves an 18 pt mark band itself (top when cut marks or labels, left when cut marks); new `mark_band_pt`/`cut_mark_segments`/`label_rect` hand back geometry only, the caller still draws. New free fn `poster_tile_label` (`G041`), `PosterLayout::tile_label` delegates to it. CLI still sets both flags false — output unchanged.
- `Pass 326.1` (`b44e6a03`) — new `StrokeDisplay::Fixed { device_px: f32 }` sets every stroke's device width in both directions (Hairline only ever thins); same reach as `Hairline`; disclosed on its own counter (`strokes_width_fixed`, not `strokes_hairlined`). No CLI flag, same reasoning as `Hairline`.

**Decisions made this session:** None architectural — both Passes are mechanism extensions (imposition geometry, a render option) answering direct channel requests (`G039`/`G040`/`G041`); no crate-boundary/library-choice/invariant call.

**Findings + decisions:**
- Gate status, recorded honestly per hard rule 8: `tools/run-gates.sh`'s full sweep was **NOT run** for either Pass — the operator said on 2026-09-23 the machine is short of memory for it. Per-crate `cargo test`/`clippy -D warnings` did run for `pdfcer-print` and `pdfcer-render` respectively (clean, per the dispatching engineer's report). Both commits are local and unpushed; pushing waits for a green gate run.
- `Pass 325.0`'s (crate split, Backlog) step-1 dependency-edge measurement was appended: the model layer only doc-links `crate::edit`, no code edge; the only real edges into `edit`/`settings` are a handful of leaf enums and `forms.rs`'s border/visibility types; `text_edit`/`dimension`/`vector` are the modules most coupled to `edit` and form an `editing` crate ABOVE the model, not a leaf. Not started — measurement only.
- `docs/FEATURES.md`: new *Printing* row for poster cut marks/labels (core `[x]`, cli `[ ]`, gui `[ ]`); the "Line weights off" hairline row (*Fonts & rendering*) gained a clause naming `Fixed` (core `[x]`, cli/gui unchanged — both already `—`/`[ ]` for the same reasoning `Fixed` shares with `Hairline`).

**Still in flight:**
- `Pass 325.0` remains **NOT STARTED** past its step-1 measurement — steps 2–7 (extract model crate, split leaf crates, facade, `main.rs` split, test-module moves, housekeeping) are still ahead.

**For next session:** Next free Pass family is **327**. `pdfce_FeatureRequests/INDEX.md` gained three rows (`G039`, `G040`, `G041`), newest first.

**Sourcing (hard rule 8).** No shell tool this filing (no Bash in the function list — same mismatch the 576th–586th filings recorded). Confirmed via `Read`/`Grep` against live source and docs: `Pass 325.0`'s Backlog entry, the existing Imposition and hairline `FEATURES.md` rows, the `G039`/`G040`/`G041` request+reply files in `pdfce_FeatureRequests/open/`. **Relayed from the dispatching engineer's report/the reply files, not independently reproduced:** exact test counts, sabotage detail, and the `fmt`/`clippy` clean claims for both Passes; the "machine short of memory" reason for skipping the full gate sweep. No commit was made this filing (`git commit -F` not run — no shell; the engineer commits).

## 2026-09-23 (586th filing) — `Pass 324.0`/`324.1` (`854773e2`/`b70eb431`): font collections render and SVG keep-text takes Type 1; 4 unfiled commits filed; 2 oversized `FEATURES.md` rows trimmed; `Pass 325.0` (crate split) filed to Backlog

**Shipped:**
- `Pass 324.0` (`854773e2`) — a supplied `.ttc`/`.otc` donor never rendered (`FontProgram::parse` routed `ttcf` to `skrifa::FontRef::new`, which refuses collections); fixed with `FontRef::from_index(data, 0)`. SVG keep-text takes the same collection donor now, via the existing sfnt route.
- `Pass 324.1` (`b70eb431`) — SVG keep-text now also keeps Type 1 (embedded `/FontFile`, or a supplied `.pfb`/`.pfa`), re-encoded as bare CFF and embedded as OpenType; refuses on non-1000 em or non-identity `FontMatrix`. `fallback_not_sfnt` now counts nothing on ordinary documents.

**Decisions made this session:**
- None new-architectural — both Passes are font-pipeline mechanism extensions (rule: no decision-log entry warranted). `Pass 325.0` (below, Backlog) is filed as a plan, not yet a decision; its own entry states the final crate shape is deliberately left open pending step 1's measurement.

**Findings + decisions:**
- `skrifa::FontRef::new` refuses a `ttcf` collection outright rather than defaulting to a face — `FontRef::from_index(data, 0)` is the fix, and it's safe because a TTC's per-face table-directory offsets count from the file start, same as a plain sfnt's. New `D:\dev\rag\rust\skrifa_fontref_new_refuses_font_collections_use_from_index.md`, indexed.
- Two `docs/FEATURES.md` rows were over `tools/check-register-entry-size.py`'s 1,200-char cap and NOT in the baseline (genuine new failures, confirmed by reading the baseline file): the malformed-PDF-recovery row (1,247 chars) and the redaction residual-scope row (1,374 chars). Both trimmed to a verdict + a pointer at the `ROADMAP.md` entry that already carries the full narrative (`Pass 283.0`/`283.1`, `Pass 310.0`–`310.2`) — no detail lost, both source entries confirmed still present and unabridged before trimming.
- Four commits flagged unfiled by `tools/check-commits-filed.py` — `96867932` (doc-only, drops a duplicated "# Errors" heading), `28dacdd4` (307-file regex pass stripping star-decoration markers, no words removed), `cbefe6d5` (condenses `CLAUDE.md` + `pdfcer-engineer.md`, 549+574 → 210+263 lines), `aee67efd` (moves bold emphasis off a CLI help paragraph's first word) — are filed by this entry's own citation of their hashes; no `ROADMAP.md` Pass entry needed for any (all doc/chore-only, no feature). `3b52ff20` (spec-librarian memory) is explicitly out of this role's territory, not filed here.

**Still in flight:**
- `Pass 325.0` (Backlog, filed this session) — split `pdfcer-core` (253,653 `.rs` lines, `edit.rs` alone 58,056) into a model crate plus narrow feature crates behind a facade, per the operator's own question about why it was ever one crate. 7-step plan, **NOT STARTED**. `ARCHITECTURE.md` §3 confirmed (this session) to have specced the single-crate shape from Pass 0 — no broken promise, a default nobody revisited.

**For next session:** Next free Pass family is **326** (`325.0` is filed but unbuilt). `docs/FEATURES.md`'s fonts row and SVG keep-text row both updated this filing (see the `Pass 324.0`/`324.1` Shipped entry's own table). A dated 2026-09-23 addendum covering both Passes was appended to `pdfce_FeatureRequests/open/reply_G033_..._FIXED.md`.

**Sourcing (hard rule 8).** No shell tool this filing (no Bash in the function list — same mismatch the 576th/583rd/584th/585th filings recorded). Confirmed via `Read`/`Grep` against live source: `FontRef::from_index`, `type1_cff.rs`, `WebFontError::Type1Convert`, `is_embeddable_program`'s PFB/PFA/`ttcf` acceptance, all in `crates/pdfcer-render/src/font/`; `ARCHITECTURE.md` §3's Pass-0 single-crate workspace layout; the four unfiled-commit hashes and subjects against `git status`'s own recent-commit listing (relayed by the environment, not a `git show`); the two `docs/FEATURES.md` over-cap rows against `tools/check-register-entry-size.py`'s own baseline file (confirmed absent from it before trimming). **Relayed from the dispatching engineer's report, not independently reproduced:** the exact test counts and sabotage detail for both Passes, and the `fmt`/`clippy` clean claims. No commit was made this filing (`git commit -F` not run — no shell; the engineer commits).

## 2026-09-23 (585th filing) — `Pass 323.0` (`38e385b2`): SVG keep-text also keeps text drawn from bare-CFF fonts (`G033` follow-up)

**Shipped:**
- `Pass 323.0` — the Unscoped Backlog remainder `Pass 322.0` filed (582nd filing): SVG's `KeepText` refused any font program that wasn't already an sfnt, which caught not just exotic CAD-embedded CFF but the bundled Standard-14 substitutes themselves (also bare CFF), so plain Helvetica/Times/Courier text always outlined. New `webfont::wrap_cff` frames a bare CFF program as a minimal `OTTO` sfnt (advances/bounds evaluated from the charstrings), refusing by name on a non-1000 em, >65,535 glyphs, or an unevaluable glyph. `svg_text::plan_run` now gates on `is_embeddable_program` (sfnt or bare CFF) instead of sfnt alone.

**Decisions made this session:** None — a font-subsetting mechanism extension, not a crate-boundary/library-choice/invariant call.

**Findings + decisions:**
- `SvgTextOutcome::fallback_not_sfnt` now counts Type 1 (`/FontFile`) only — same field name, doc comment updated, no API break. The CLI's `svg-text:` line is unchanged in shape.
- Tests: unit tests on the framed face's advances/lsb/glyph-count and its refusals; integration tests on `hello.pdf` (bundled Standard-14) and an embedded-Type1C fixture now export `KeepText` with every glyph within 0.05px of the outline export; sabotage on the gate and the wrap each failed 4 integration tests plus the unit test. Headless Chrome, fallback family stripped, drew the Type1C page with the same ink bbox as the outline export.
- No `Cargo.toml` change — `cargo tree` unaffected by construction. No writer change. `cargo test -p pdfcer-render`, `clippy -D warnings`, `fmt --check` all clean per the dispatching engineer's report.

**Still in flight:**
- Type 1 (`FontFile`) font programs still fall back to outlines in SVG keep-text — the Backlog entry is narrowed, not closed. EMF keep-text needs no such wrapping at all (never embeds a font program).

**For next session:** Next free Pass family is **324**. `docs/FEATURES.md`'s SVG keep-text row and its sibling Planned row (now Type-1-only) were updated in this filing; `G033`'s `INDEX.md` row updated and a missing row added for the separate 2026-09-20 `pdfcer-gui` note (already answered, 584th filing, `515c8241`).

**Sourcing (hard rule 8).** No shell tool this filing (no Bash in the function list — same mismatch the 576th/583rd/584th filings recorded). Confirmed via `Read`/`Grep` against live source: `wrap_cff`, `WebFontError::CffFrame`, `FontProgram::cff_metrics`, `is_embeddable_program` all exist in `crates/pdfcer-render/src/font/webfont.rs`/`svg_text.rs`/`font/program.rs`. **Relayed from the dispatching engineer's report, not independently reproduced:** exact test counts, the headless-Chrome measurement, and the clean gate results. `ROADMAP.md`, `docs/FEATURES.md`, this entry, and `pdfce_FeatureRequests/INDEX.md`/the reply addendum were all left as edited by this filing; no commit was made (`git commit -F` not run — no shell).

## 2026-09-23 (584th filing) — `FEATURES.md`-only correction: eight stale `gui` boxes ticked, four missing symbols added

**Shipped:** None — no Pass, no code changed. A `FEATURES.md` correction filing, source: `pdfcer-gui`'s
`open/note_2026-09-20-eight-gui-boxes-in-your-features-table-are-stale-and-four-of-our-symbols-are-not-in-your-file.md`.

**Decisions made this session:**
- Engineer's ruling on the ticking bar: `FEATURES.md`'s own stated bar — "an operator can reach it in a real `pdfcer-gui` build" — is satisfied by all eight rows the note names, including the one the note itself flagged as ambiguous under a hardened *reached-and-driven* reading. All eight `gui` boxes ticked on **reach**, worded as reached-not-driven where the note said so, never rounded up to "driven."

**Findings + decisions:**
- Eight rows corrected: "Open a PDF that contradicts itself" (driven, `load_anomalies`), "Ask which characters a text run will accept" (reached, not driven), "Read and write a pop-up's `/Open` state" (reached both ways, write half not driven), "Author a comment reply" (reached, not driven), "Write a field's `/DA`" (reached, not driven), "Scope the residual sweep by carrier visibility" (driven on the bytes), "Signing hardening" (driven, `signing` family — the row's own "`pdfcer-gui` has not wired `sign`" sentence was stale and is replaced), and the Planned "Paint `/MK` `/BG`/`/BC`" row, which now has all three of core/cli/gui and per the Planned section's own header ("a `[x]` here means the model or verb exists and only the named shell is missing") no longer belongs there — moved to *Implemented → Forms (AcroForm)*, directly beside the existing (and already-correct, per the note's §4) baked-colour row, reworded to state reach without a driven claim.
- Row 6's citation of `E001` as an open request was stale — `E001` closed 2026-09-17 (`archive/2026-09-17-E001-redaction-residual-scope-done.md`, `INDEX.md` CONSUMED) — fixed in place.
- Four symbols cited in the note as absent from `FEATURES.md` (`with_duplicate_keys`/`DuplicateKeyPolicy`, `FieldEdit::with_appearance`/`FieldAppearance`, `set_residual_scope`, `WidgetEdit::with_border_color`) were each grepped against `crates/` and confirmed at HEAD before being added to the cell of the row that already covers the capability, alongside the CLI/other-symbol spelling already present: `LoadOptions::with_duplicate_keys`/`DuplicateKeyPolicy` (`document.rs:1819`/`parser.rs:285`), `FieldEdit::with_appearance(FieldAppearance)` (`edit.rs:22688`/`22092`), `EditSession::set_residual_scope` (`edit.rs:9289`), `WidgetEdit::with_border_color` (`edit.rs:22860`, between `struct WidgetEdit` at `22404` and `struct WidgetEditOutcome` at `23096`).
- The note's §4 "not claiming" boundary held: the already-ticked Implemented `/MK` row (core/cli/gui all `[x]`) was left untouched, no claim made about its baking.

**Still in flight:**
- Two of this filing's corrections (`E001` closed, signing now driven) are the kind hard rule 11 asks to be swept for elsewhere the claim might be restated (`ROADMAP.md`, `ARCHITECTURE.md`). That broader sweep was out of scope for this dispatch (explicitly `FEATURES.md`-only) and is flagged as owed, not performed.
- A `Grep` for lines over 1,200 characters in `FEATURES.md` turned up dozens of pre-existing rows already well past the per-row cap this agent's own SIZE RULE states — this predates this filing and both edited/moved rows here are no longer past that norm than their neighbours; flagged for the engineer, not corrected here (out of scope, and correcting it would be exactly the "fix files to a standard" commission the documentation-first rule warns against doing via fan-out).

**For next session:** No new Pass filed. Next free Pass family remains **323** per the prior filing.

**Sourcing (hard rule 8).** No shell/Bash tool available this filing, despite the environment block's prose claiming one — consistent with the 576th filing's note of the same mismatch; treated the actual tool list, not the prose, as authoritative. All four symbol locations above were confirmed by `Grep`/`Read` against live `crates/` source at the time of writing, not relayed. Both `docs/FEATURES.md` and this entry were left **uncommitted** — no `git commit -F` was run; the dispatching engineer holds the changed paths.

## 2026-09-23 (583rd filing) — `Pass 322.1` (`88bd4144`): EMF export can keep text as real text records, closing `G033`

**Shipped:**
- `Pass 322.1` — `EmfOptions::with_text(EmfText::KeepText)` (default `Outlines`, both `#[non_exhaustive]`); a run that fits writes `EMR_EXTCREATEFONTINDIRECTW` (368-byte `LogFontExDv`, no axes) + `EMR_EXTTEXTOUTW` with a per-character `Dx` array pinning PDF position. Face = the embedded sfnt's typographic/family name, else derived from `/BaseFont` (Standard-14 → Arial/Times New Roman/Courier New). MS-EMF carries no font program, so a kept run draws whatever face is INSTALLED under that name — answers the scoping question the Backlog entry opened with. Fallback to outlines counted on `EmfExport.outcome.text: EmfTextOutcome` (`runs_as_text`, `fallback_paint`, `fallback_unmapped`, `fallback_geometry`, `fallback_symbol_face`, `runs_as_outlines()`). `TextRunInfo` gains `end` (pen after last advance). CLI `export-image --format emf --emf-text keep|outlines`, prints `emf-text: kept= outlines= paint= unmapped= geometry= symbol_face=` plus a note; refused by name on a non-EMF format. `copy-page` still emits outlines. Closes `G033` (SVG half shipped `Pass 322.0`, same day).

**Decisions made this session:** None — an export-mode addition, not a crate-boundary/library-choice/invariant call.

**Findings + decisions:**
- Verified through real GDI (`PlayEnhMetaFile` onto a `CreateDIBSection` DC, not GDI+ `System.Drawing.Imaging.Metafile`) — kept text overlays the outline export to ~1px; positive `Escapement` = counterclockwise, confirmed by playback. `D:\dev\rag\emf\text_records.md`'s "NEEDS VERIFICATION" flag on that sign convention is now VERIFIED (engineer already updated the RAG file).
- `docs/FEATURES.md`: new *Export* row (core `[x]` cli `[x]` gui `[ ]`) directly below the SVG keep-text row; the matching `Planned` row removed rather than left duplicated.
- `pdfce_FeatureRequests/INDEX.md`'s `G033` row and reply file were already updated (`PARTIAL` → `FIXED`) by the engineer at filing time — confirmed on read, no librarian action owed.
- `ROADMAP.md`'s `Pass 322.0` Shipped entry got a dated closure note (append-only — original text kept, not rewritten) pointing forward to this entry.

**Still in flight:**
- Bare-CFF/Type1-as-OpenType wrapping (Unscoped Backlog item, filed alongside `Pass 322.0`) still open — covers both SVG and EMF keep-text, no new entry needed for the EMF half.
- `pdfcer-gui` has not wired either keep-text mode — `FEATURES.md` `gui [ ]` on both SVG and EMF rows, not rounded up.

**For next session:** Next free Pass family: **323**. `G033` fully closed.

**Sourcing (hard rule 8).** No shell tool this filing. Confirmed via `Read`/`Grep` against live source: `EmfText`/`EmfTextOutcome`/`with_text` and the record writers exist in `crates/pdfcer-render/src/emf.rs`/`emf_text.rs`; `--emf-text` exists in `crates/pdfcer-cli/src/main.rs`. Relayed, not independently reproduced: the GDI-playback verification numbers, the sabotage pass/fail detail, and the `tools/run-gates.sh` low-memory-kill circumstance.

## 2026-09-23 (582nd filing) — `Pass 322.0` (`5425d2be`): SVG export can keep text as real `<text>`, font embedded (`G033`, PARTIAL)

**Shipped:**
- `Pass 322.0` — `SvgOptions::text = SvgText::{Outlines, KeepText}`; when asked, the recorder wraps each shown string in a display-list `Op::Text` and the SVG writer embeds each run's font as an OTS-sanitizer-clean sfnt `@font-face` (new `svg_text.rs`, `font/webfont.rs`). Per-run fallback to outlines on any of seven named reasons, counted on `SvgExport.outcome.text: SvgTextOutcome`. `pdfcer export-image --format svg --svg-text keep|outlines`. New `pdfcer-core` read: `ExtractFont::unicode_for_code`. SVG only — EMF still outputs outlines, reserved as `Pass 322.1` (*Backlog*, NOT STARTED, scoping needed for MS-EMF text records).
- Fixed on the way (same commit): three CLI doc comments welded onto `ImageFormatArg` since `Pass 309.1` — moved back onto `DxfUnitArg`/`ProducerArg`/a third. Sixth instance of this project's recurring doc-splice-on-insertion finding, filed as an `R197` dated instance.

**Decisions made this session:** None — a new export mode plus a font-subsetting helper, not a crate-boundary/library-choice/invariant call.

**Findings + decisions:**
- `docs/FEATURES.md`: new *Export* row (core `[x]` cli `[x]` gui `[ ]`), two new *Planned* rows (EMF keep-text `Pass 322.1`; wrap bare CFF/Type1 as OpenType, Unscoped).
- `pdfce_FeatureRequests/INDEX.md`: rows added for `G033`–`G038` this filing (all six had been missing since their own Shipped entries, per the prior five filings' own finding).
- `D:\dev\rag\rust\doc_comments_concatenate_silently_so_a_moved_variant_orphans_two.md`: sixth dated instance appended (struct-field/clap-arg form).

**Still in flight:**
- `Pass 322.1` (EMF keep-text) NOT STARTED — needs an MS-EMF text-record scoping read; a background research dispatch on exactly that (task `ae13ea6e44486b60f`) was already in flight at filing time.
- The bare-CFF/Type1-as-OpenType wrapping item is Unscoped, no Pass ID yet.
- `pdfcer-gui` has not consumed `KeepText` — `FEATURES.md` `gui [ ]`, not rounded up.

**For next session:** Next free Pass family: **323**. Check whether the MS-EMF background dispatch's findings landed anywhere before re-deriving them for `Pass 322.1`.

**Sourcing (hard rule 8).** No shell tool this filing — Bash absent from the available function list despite the environment's own shell-availability claim. Confirmed via `Read`/`Grep` against live source: `SvgOptions`/`SvgText`/`SvgTextOutcome`, `svg_text.rs`, `font/webfont.rs`, `unicode_for_code`, the CLI flag, the three doc-comment moves. Relayed, not independently reproduced: the sabotage pass/fail detail and the headless-Chrome check.

## 2026-09-23 (581st filing) — `Pass 321.0` (`9edcc9b9`): `/ToUnicode` written as a stream, so embedded add-text extracts

**Shipped:**
- `Pass 321.0` — `font_embed::build_objects` gains a 4th parameter, a staged `/ToUnicode` stream object; new `FontEmbedPlan::to_unicode_cmap()`. Add-text with an embedded donor face (`--embed-font`) now writes `/ToUnicode` as a STREAM (ISO 32000-1 §9.10.3), not a string — the string form was silently ignored by every conforming reader, including pdfcer's own extractor, so text added in an embedded face extracted as nothing.

**Decisions made this session:** None.

**Findings + decisions:**
- Found while building `Pass 322.0` (`G033`, next entry, chronologically after this commit) — the kept-text SVG export read `unicode_for_code == None` for every embedded add-text glyph.
- The prior test for this code path only checked the literal string `"/ToUnicode"` appeared in the output — vacuous, and it passed on the defective (string-form) code.
- New `C:\personal_rag\pdf\` lesson filed: a `/ToUnicode` written as a string instead of a stream is silently dropped by conforming readers, no error surfaced.
- `docs/FEATURES.md`: no box change — the add-text-with-embedded-font row never claimed extractability; this is a correctness fix under an existing gap, not a new capability.

**Still in flight:** None specific to this Pass.

**For next session:** Next free Pass family: **322** (superseded within this same session by `Pass 322.0` above — next free is now **323**).

**Sourcing (hard rule 8).** No shell tool this filing. Confirmed via `Read`/`Grep`: `build_objects`'s 4th parameter, `to_unicode_cmap`, the new test and its sabotage description, the add-text call site. Not independently re-run.

## 2026-09-23 (580th filing) — `Pass 320.0` (`2fca11bf`): merge consecutive text runs into one (`G035`)

**Shipped:**
- `Pass 320.0` — new `EditSession::merge_text_runs(page, object, runs: &[usize], &MergeOptions) -> Result<MergeReport, FormatError>` joins two or more consecutive show operators of one text object into one, first run's show carries the joined text (separator `none`/`space`/custom text), later runs' shows spliced empty, positioning operators between them untouched. `MergeFit::Span` (default) sets `Tz` so the merged run spans first-run origin to last-run end; `Natural` keeps the first run's `Tz`. Differing `Tz` between runs is allowed (the OCR case); every other text-state parameter must match or the merge refuses by name. New `FormatError`/`VectorEditError` variants, none mutating; preflight `vector::text_merge_refusal`. CLI `pdfcer text-run-merge --object N --run A,B[,C] [--separator] [--fit]`.
- Fixed on discovery, same commit: the vector decomposer closed a `TJ` run per STRING instead of once per operator, truncating hit-testing/run bounds on any multi-string `TJ` and dropping a run entirely when it opened with an empty string.

**Decisions made this session:** None — a new verb plus a decomposer bug fix, not a crate-boundary/library-choice/invariant call.

**Findings + decisions:**
- `docs/core-api` verb count 249 → 250 (already updated by the dispatching engineer at read time).
- `docs/FEATURES.md`: new row, Text section, core `[x]` cli `[x]` gui `[ ]` (not consumed by `pdfcer-gui` yet); confirmed under the 1,200-char cap by `^.{1200,}$` re-check post-edit (row not in the offending list).
- No `personal_rag/pdf` lesson — internal API-completeness fix and decomposer bug fix, not a producer-divergence finding.
- **Fifth instance of the "wrapped string literal loses its trailing backslash" defect, `R243` dated-instance note added.** Eight wrapped string literals in the new `#[error]` messages carried 10-space gaps, caught only by running the CLI demo. Also the 12th cross-project occurrence — append owed to `D:\dev\rag\rust\a_multiline_string_literal_that_loses_its_trailing_backslash_bakes_a_visible_gap_mid_sentence.md`.
- **`pdfce_FeatureRequests/INDEX.md` re-checked, same finding as the last several filings.** Grepped for `G035` this session — absent, consistent with `G034`/`G036`/`G037`/`G038` also being absent. Flagged to the engineer as owed cleanup; no `G035` row added on the same unverified basis. The reply file itself (`open/reply_G035_text_runs_can_be_merged_FIXED.md`) does exist, confirmed by `Glob`.

**Still in flight:**
- The `R251`/`R258` standing-rules ledger discrepancy flagged by prior filings is still unresolved — not re-verified this session either.
- `pdfcer-gui` has not consumed `merge_text_runs`/`text_merge_refusal` — `FEATURES.md` `gui [ ]`, not rounded up.
- `pdfce_FeatureRequests/INDEX.md` is missing rows for `G034`, `G035`, `G036`, `G037`, and `G038` — owed to whoever maintains that file (not one of this role's five tiers).
- `tools/run-gates.sh`'s `cargo test -p pdfcer-core --no-default-features` leg and the full sweep did not run for this commit (killed for memory) — relayed, not independently confirmed.

**For next session:** Next free Pass family: **321**. `INDEX.md` cleanup above is outstanding. This filing appended the RAG occurrence note to `D:\dev\rag\rust\` in the same session — check it landed if picking this back up.

**Sourcing (hard rule 8).** No shell tool this filing — the environment's own shell-availability claim did not match the actual function list available to me (Bash absent; only Read/Write/Edit/Glob/Grep/WebSearch/WebFetch). Independently confirmed via `Grep`/`Read` against live source: `merge_text_runs` call site and signature, `MergeOptions`/`MergeSeparator`/`MergeFit`/`MergeReport`, all nine new error variants across `crates/pdfcer-core/src/text_edit/merge.rs`, `edit.rs`, `vector/edit.rs`, `text_edit/format.rs`; `text-run-merge` CLI wiring in `crates/pdfcer-cli/src/main.rs`; `crates/pdfcer-core/tests/text_run_merge.rs` holds exactly 13 `#[test]` functions; `a_tj_runs_box_covers_every_string_in_the_array` exists in `crates/pdfcer-core/src/vector/decompose.rs`; `docs/core-api/index.md` already states "all 250 public verbs"; `README.md` already states "155 working subcommands"; `pdfce_FeatureRequests/INDEX.md` independently Grepped and confirmed to lack a `G035` row; `open/reply_G035_text_runs_can_be_merged_FIXED.md` independently confirmed to exist via `Glob`. This session's own git-status context lists `2fca11bf` at `HEAD`, subject *"text_edit: merge consecutive text runs into one (G035)"*, which corroborates the commit and its one-line description but is not a `git show`. The diffstat, the exact sabotage-mutation pass/fail count, the `tools/run-gates.sh` memory-kill detail, and the "269 result groups ok, 0 failed" test figure are **relayed from the dispatching engineer's report, not independently re-run**.

## 2026-09-23 (579th filing) — `Pass 319.0` (`4594e17c`): fit one text run to a page width through `Tz` (`G038`)

**Shipped:**
- `Pass 319.0` — new `EditSession::set_text_run_width(page, object, run, width_pts)` / `pdfcer text-run-width`. Runs the existing whole-operator `format_text` path with `FollowerDisposition::Pin` and a new `FormatRequest::fit_width`; the computed `Tz` is ABSOLUTE (replaces an existing `Tz` rather than compounding it; fitting twice is a fixed point). New refusals `BadTargetWidth`/`WidthFitKerned`/`NoAdvanceWidth`/`TextRunHasNoWidth`, none mutating anything; preflight `vector::text_run_width_refusal`. Disclosure: "fitted to W pt wide by setting its horizontal scaling to P%; nothing after it moved." Bug fixed on the way: `disclosure_h_scale` always claimed the rest of the line was relaid out, even under `Pin` — now takes a `pinned` flag.

**Decisions made this session:** None — a new verb plus a disclosure-string bug fix, not a crate-boundary/library-choice/invariant call.

**Findings + decisions:**
- `docs/core-api` verb count 248 → 249 (already updated by the dispatching engineer at read time); two new table rows in `02-editing-and-saving.md`.
- `docs/FEATURES.md`: new row, Text section, core `[x]` cli `[x]` gui `[ ]` (not consumed by `pdfcer-gui` yet).
- No `personal_rag/pdf` lesson — internal API-completeness fix, not a producer-divergence finding.
- **`pdfce_FeatureRequests/INDEX.md` discrepancy found and NOT repeated.** Grepped for `G034`/`G036`/`G037`/`G038` this session — none present, despite the three most recent Shipped entries each claiming (unverified, by their own Sourcing paragraphs) that a row was added or closed there. Flagged to the engineer as owed cleanup; no `G038` row added on the same unverified basis.

**Still in flight:**
- The `R251`/`R258` standing-rules ledger discrepancy flagged by prior filings is still unresolved — not re-verified this session either.
- `pdfcer-gui` has not consumed `set_text_run_width`/`text_run_width_refusal` — `FEATURES.md` `gui [ ]`, not rounded up.
- `pdfce_FeatureRequests/INDEX.md` is missing rows for `G034`, `G036`, `G037`, and now `G038` — owed to whoever maintains that file (not one of this role's five tiers).

**For next session:** Next free Pass family: **320**. `INDEX.md` cleanup above is outstanding.

**Sourcing (hard rule 8).** No shell tool this filing. Independently confirmed via `Grep`/`Read` against live source: `set_text_run_width`, `fit_width`, `text_run_width_scale`, `BadTargetWidth`, `WidthFitKerned`, `NoAdvanceWidth`, `TextRunHasNoWidth`, `text_run_width_refusal` present in `crates/pdfcer-core/src/edit.rs`, `crates/pdfcer-core/src/vector/edit.rs`, `crates/pdfcer-core/src/text_edit/format.rs`, `crates/pdfcer-core/src/vector/mod.rs`; `text-run-width` wiring present in `crates/pdfcer-cli/src/main.rs`; `crates/pdfcer-core/tests/text_run_width.rs` holds exactly 11 `#[test]` functions; `docs/core-api/index.md` already states "all 249 public verbs"; `docs/core-api/02-editing-and-saving.md`/`03-capabilities.md` already carry the new rows/section; `README.md` already states "154 working subcommands"; `disclosure_h_scale` at `text_edit/format.rs:5701` takes a `pinned: bool` parameter; `pdfce_FeatureRequests/INDEX.md` independently Grepped this filing and confirmed to carry none of `G034`/`G036`/`G037`/`G038`. This session's own git-status context lists `4594e17c` at `HEAD`, subject *"text_edit: fit one text run to a page width through Tz (G038)"*, which corroborates the commit and its one-line description but is not a `git show`. The diffstat, the sabotage-mutation count, and the `tools/run-gates.sh` memory-kill detail are **relayed from the dispatching engineer's report, not independently re-run**.

## 2026-09-23 (578th filing) — `Pass 318.0` (`01c0c1ce`): the OCR sandwich layer now carries an identity, so a re-run replaces it instead of stacking a second one (`G036`)

**Shipped:**
- `Pass 318.0` — new `pdfcer_core::ocr::marker` wraps each OCR sandwich layer in `/pdfc_OCR << /Producer (pdfcer) /Version 1 /Engine (…) >> BDC … EMC`; identity is the whole `/Contents` stream bounded by that exact `BDC`/`EMC` pair, so unmarked third-party mode-3 text is never touched. `find_ocr_layers`/`page_ocr_layers` probes; `EditSession::find_ocr_layers`/`remove_ocr_layer` (one undo entry, frees the stripped stream+font objects when unreferenced); `OcrLayerOptions::with_existing(Replace default / Refuse / Stack)`. CLI: `pdfcer ocr --existing replace|refuse|stack`. Bug fixed on the way: `EditSession::dirty_set` mis-flagged a create-then-free-within-session object as an orphan.

**Decisions made this session:** None — a marked-content identity scheme plus a query/removal verb pair and a dirty-set bug fix, not a crate-boundary/library-choice/invariant call.

**Findings + decisions:**
- The pre-existing one-shot (non-session) OCR free function has no route to free an old layer's objects on removal — a core-only limitation, not built out this Pass; the session-based writer is the only one that can clean up after itself.
- `docs/core-api` verb count 246 → 248 (`find_ocr_layers`, `remove_ocr_layer`); `tools/check-core-api-verbs.py` PASS.
- `docs/FEATURES.md`: the Planned row from the 577th filing moved to *Implemented* under OCR — core `[x]`, cli `[x]`, gui `[ ]` (no list/remove UI yet). Re-checked against the file's own `^.{1200,}$` matches post-edit — the new row does not appear in that list.
- No `personal_rag/pdf` lesson — an internal identity/API-completeness fix on pdfcer's own writer output, not a producer-divergence finding.
- Open operator question `(ce)` (register `pdfc` on Adobe's public tag list?) is still open; this Pass shipped `/pdfc_OCR` unregistered, per the stated default.

**Still in flight:**
- Open operator question `(ce)` unresolved (default taken, filing stays available as a later independent step).
- The `R251`/`R258` standing-rules ledger discrepancy flagged by prior filings is still unresolved — not re-verified this session either.
- `pdfcer-gui` has not consumed `find_ocr_layers`/`remove_ocr_layer` — no list/remove UI (`FEATURES.md` `gui [ ]`, not rounded up).

**For next session:** `pdfce_FeatureRequests/INDEX.md` `G036` row closed SHIPPED — the engineer's own reply/done artifacts in `open/`/`done/` were not independently confirmed, no shell this filing. Next free Pass family: **319**.

**Sourcing (hard rule 8).** No shell tool this filing. Independently confirmed via `Grep`/`Read` against live source: `LAYER_TAG`, `LAYER_PRODUCER`, `LAYER_VERSION`, `find_ocr_layers`, `page_ocr_layers`, `OcrLayerRef` in `crates/pdfcer-core/src/ocr/marker.rs`; `OcrLayerOptions`/`ExistingLayers` in `crates/pdfcer-core/src/ocr/layer.rs`; `EditSession::find_ocr_layers`, `EditSession::remove_ocr_layer`, `CommandKind::RemoveOcrLayer` in `crates/pdfcer-core/src/edit.rs`; `layers_replaced` in `crates/pdfcer-cli/src/main.rs`; `crates/pdfcer-core/tests/ocr_layer_marker.rs` holds exactly 10 `#[test]` functions; `docs/core-api/02-editing-and-saving.md` already carried the `find_ocr_layers`/`remove_ocr_layer` rows citing `Pass 318.0` at read time; no prior `ROADMAP.md`/`SESSION_LOG.md` entry recorded `Pass 318.0` as SHIPPED before this filing. This session's own git-status context lists `01c0c1ce` at `HEAD`, subject *"ocr: mark the layers pdfcer writes, and replace them on a re-run (G036)"*, which corroborates the commit and its one-line description but is not a `git show`. The full diffstat, the 7/7 sabotage detail, and the `tools/run-gates.sh` memory-kill detail are **relayed from the dispatching engineer's report, not independently re-run**.

## 2026-09-23 (577th filing) — `Pass 317.0` (`53b939b2`) + `Pass 316.0` (`70d8ba00`): a glyph now maps to its surgery run, and text carries an explicit rendering mode end to end (`G037` + `G034`)

**Shipped:**
- `Pass 317.0` — new `vector::text_locate`: `locate_text_run`/`locate_text_runs` join extraction's per-glyph provenance to the editable-run model (`TextRunRef::Page`/`Form`) by matching operator-span end bytes, so a clicked or searched glyph now resolves to the run a caller can `move_text_run`/`edit_text` on. Form placements disambiguated by CTM; no agreeing leaf → `None`, never a guess.
- `Pass 316.0` — `FormatRequest::render_mode`/`AddTextRequest::with_render_mode` + `--render-mode` on `format-text`/`add-text` give both existing and newly added text an explicit `Tr` (§9.3.6 Table 106); refuses an out-of-range mode or a conflict with synthetic bold. Fixed a defect on the way: added text previously inherited `Tc`/`Tw`/`Tz`/`Ts`/`Tr` from the surrounding stream instead of resetting them.

**Decisions made this session:** None — a new query/join function plus a builder/CLI surface and bug fix on an existing writer path; neither is a crate-boundary/library/invariant call. No decision-log entry for either Pass.

**Findings + decisions:**
- No `R221` instance for either Pass — `G037` is a query, not an exported guard predicate; `G034` is a builder surface plus a bug fix, not a predicate-restatement pattern. Declined rather than forced to fit the recent run of `R221` instances (`Pass 314.0`, `Pass 315.0`).
- `docs/core-api/02-editing-and-saving.md` §1.10.0a (`locate_text_run`) and `03-capabilities.md`'s rendering-mode table row + `with_render_mode` prose were both already present at read time — the engineer wrote them ahead of this filing; confirmed present, not written by it. Verb count unchanged at 246 (`locate_text_run` is a free `vector` function, not an `EditSession` verb).
- `docs/FEATURES.md`: two new rows (Text section) — G037's join, core `[x]`/cli `[ ]`/gui `[ ]`; G034's render-mode control, core `[x]`/cli `[x]`/gui `[ ]`. The existing "`Tr` 4–7 text-clipping render modes" Planned row got a clarifying note (no box moved): WRITING any `Tr` 0–7 now works (this filing); PAINTING the actual clip effect of modes 4–7 is a separate, still-unbuilt capability. All three edits re-checked against `FEATURES.md`'s own `^.{1200,}$` matches post-edit — none of the three new/edited rows appear in that list.
- No `personal_rag/pdf` lesson for either Pass — both are internal API-completeness fixes (a join function, a writer control), not observations about a real-world producer's divergence from spec.

**Still in flight:**
- `Pass 318.0` (`G036`, *Next up*) — the OCR sandwich layer carries no identity, so a re-run stacks a second invisible layer instead of replacing the first. Design recorded in `ROADMAP.md`'s *Next up* and a new `docs/FEATURES.md` Planned row: marked-content wrapper `/pdfc_OCR << /Producer /Version /Engine >>`, `find_ocr_layers`/`page_ocr_layers` probes, `EditSession::find_ocr_layers`/`remove_ocr_layer`, `OcrLayerOptions` existing-layer policy. Not built yet — this filing records the design only.
- New open operator question **(ce)**: whether to register the `pdfc` tag prefix on Adobe's public `adobe/pdf-names-list` before `Pass 318.0` ships (ISO 32000-1 Annex E: "shall be registered"; ISO 32000-2: softened to "should"; source `D:\Dev\Rag-Specialized\PDF_Spec\iso32000\iso32000__annex__e.md`, new this session by the spec-librarian). Default if unanswered: ship `/pdfc_OCR` unregistered. Operator-question ceiling moves `(cd)` → `(ce)`, next free `(cf)`.
- The `R251`/`R258` standing-rules ledger discrepancy flagged by prior filings is still unresolved — not re-verified this session either.

**For next session:** `pdfce_FeatureRequests/INDEX.md` rows for `G037` and `G034` closed SHIPPED, `G036` still open (`Pass 318.0` in progress) — the engineer's own reply/done artifacts in `open/`/`done/` were not independently confirmed, no shell this filing. Next free Pass family: **319**.

**Sourcing (hard rule 8).** No shell tool this filing. Independently confirmed via `Grep`/`Read` against live source: `text_locate`, `TextRunRef`, `locate_text_run`, `locate_text_runs` present in `crates/pdfcer-core/src/vector/text_locate.rs` and re-exported from `vector/mod.rs`; `render_mode`, `InvalidRenderMode`, `ConflictingRenderMode`, `render_mode_change`, `with_render_mode`, `TextRenderMode`, `emit_state_prelude` present across `crates/pdfcer-core/src/text_edit/` and `text_state.rs`; `crates/pdfcer-core/tests/text_run_locate.rs` holds exactly 4 `#[test]` functions, `crates/pdfcer-core/tests/text_render_mode.rs` holds exactly 10; `docs/core-api/02-editing-and-saving.md` and `03-capabilities.md` already carried both features' documentation at read time; no prior `ROADMAP.md`/`SESSION_LOG.md` entry named `G037`/`Pass 317`, `G034`/`Pass 316`, or `G036`/`Pass 318` before this filing; `(ce)` confirmed genuinely unused by grepping `\(c[e-z]\)` across `ROADMAP.md` before minting it. This session's own git-status context lists `53b939b2` at `HEAD` (subject *"vector: map an extracted glyph to its surgery run (G037)"*) and `70d8ba00` one commit behind (subject *"text_edit: set text rendering mode on format and add (G034)"*), which corroborates both commits and their one-line descriptions but is not a `git show`. The full 40-char hashes, diffstats, test-sabotage results (11/11, 5/5), and `tools/run-gates.sh` 34-command PASS are **relayed from the dispatching engineer's report, not independently re-run**.

## 2026-09-23 (576th filing) — `Pass 315.0` (`ca8f7c55`): a set of text runs moves as one edit, not one at a time (`G030`)

**Shipped:** `Pass 315.0` — `move_text_run`/`move_text_run_in_form` (`Pass 305.0`) had no set-taking twin, so the `Inherited` guard refused a legal whole-line move whenever a caller tried to move every affected run one at a time (moving run 1 first displaces run 2, which the guard then correctly refuses to move again). Fixed with `vector::plan_move_text_runs`: an operand rewrite over the whole listed set in one pass, restoring a run displaced only because an earlier LISTED run moved, so unlisted runs never move; a single-element set is byte-identical to `move_text_run`'s own output. New `VectorEditError::EmptyTextRunMove`. `EditSession::move_text_runs`/`move_text_runs_in_form` — one command, one undo entry per call, whatever the set size. CLI: `pdfcer text-run-move --run` now takes a list.

**Decisions made this session:** None — a set-taking sibling of an existing verb plus one new error variant, not a crate-boundary/library/invariant call. No decision-log entry.

**Findings + decisions:**
- New `pub vector::text_run_move_refusal_of_set(&TextObject, &[usize]) -> Option<VectorEditError>` — the verb's own guard, exported so a downstream shell asks the real predicate instead of hand-deriving the set-membership rule from the single-run guard's doc comment. `R221` gains a 13th dated instance in its RAG ledger (`D:\dev\rag\rust\a_capability_predicate_that_restates_its_accepting_function_will_drift_ask_the_function_instead.md`) — same shape as instance 12 (`Pass 314.0`) one filing earlier, on the sibling verb; the ledger's own "Cost" paragraph corrected from a stale "eleven times" to "thirteen times."
- `docs/core-api/02-editing-and-saving.md` + `index.md`: verb count 244 → 246 (`move_text_runs`, `move_text_runs_in_form`), already updated by the engineer ahead of this filing — confirmed present, not written by it.
- `docs/FEATURES.md`: no new row — a short note appended to the existing "Move one text run…" row naming the set-move verb; re-checked under the 1,200-char register cap (row was already within ~120–150 chars of it before the edit — narrow margin, worth remembering if this row grows again).
- No `personal_rag/pdf` lesson — an internal API-completeness fix, not a producer-divergence finding; same posture as `Pass 314.0`. The motivating file (`SW41177.pdf`) already has three 2026-09-22 lessons on record for this object's 237-run shape.

**Still in flight:** Nothing new opened by this Pass. `pdfcer-gui` has not consumed the set-move verb or `text_run_move_refusal_of_set` (`FEATURES.md` `gui [ ]`, not rounded up). The `R251`/`R258` standing-rules ledger discrepancy flagged by prior filings is still unresolved — not re-verified this session either.

**For next session:** `pdfce_FeatureRequests/INDEX.md` `G030` row closed SHIPPED; `open/reply_G030_move_text_runs_set_verb_FIXED.md` is the engineer's own artifact, not written by this filing. Next free Pass family: **316**.

**Sourcing (hard rule 8).** No shell tool this filing. Independently confirmed via `Grep`/`Read` against live source: `plan_move_text_runs`, `text_run_move_refusal_of_set`, `EmptyTextRunMove`, `move_text_runs`, `move_text_runs_in_form` all present in `crates/pdfcer-core/src/vector/edit.rs`, `crates/pdfcer-core/src/edit.rs`, `crates/pdfcer-core/src/vector/mod.rs` and `crates/pdfcer-cli/src/main.rs`; `crates/pdfcer-core/tests/text_run_set_move.rs` holds exactly 6 `#[test]` functions; `docs/core-api/` already carried the new rows and the 246 count at read time; no prior `ROADMAP.md`/`SESSION_LOG.md` entry named `G030` or `Pass 315` before this filing (fresh family, not a promotion); the edited `docs/FEATURES.md` row re-checked against that file's own `^.{1200,}$` matches post-edit and does not appear in that list. This session's own git-status context lists `ca8f7c55` at `HEAD` with the subject line *"vector: move a set of text runs as one edit (G030)"*, which corroborates the commit and its one-line description but is not a `git show`. The full 40-char hash, the diffstat, and the gate-sweep results are relayed from the dispatching engineer's report, not independently re-run.

## 2026-09-23 (575th filing) — `Pass 314.0` (`c6ab2104`): `move_objects`/`move_objects_in_form` now move TEXT objects by operand rewrite, not just paths (`G029`)

**Shipped:** `Pass 314.0` — `EditSession::move_object`/`move_objects`/`move_objects_in_form` refused every text object with `NotAPath`, while `transform_objects` already accepted them via a `q…cm…Q` wrapper — the cheap, wrapper-free move the verb's own name promised did not exist. Fixed with operand rewrite, no wrapper: every `Tm` gets the delta on `e`/`f`; the first `Td` before any `Tm` gets it; later `Td` steps stay verbatim; an object opening with `TD`/`T*`/a show operator gets `" dx dy Td"` inserted after `BT`, disclosed (rule 4). Delta mapped through each object's own CTM inverse. New `VectorEditError::TransformInsideTextObject` refuses a `cm` inside `BT…ET` (illegal per §8.2 Fig. 9) rather than half-move. Images unaffected (`NotAPath { kind: "image", .. }`). New pub `object_move_refusal` (the real guard, exported so `pdfcer-gui`'s greying calls it instead of restating it — `R221`, 12th instance), `plan_move_objects`, `plan_move_text_object`. CLI: `pdfcer object-move` takes text now.

**Decisions made this session:** None — extends the existing move/operand-rewrite vs. transform/matrix-wrap split to a second object kind; not a crate-boundary/library/invariant call.

**Findings + decisions:**
- `R221` gains a 12th dated instance in its RAG ledger (`D:\dev\rag\rust\a_capability_predicate_that_restates_its_accepting_function_will_drift_ask_the_function_instead.md`) — the export exists specifically so the GUI's own greying logic asks the real predicate instead of maintaining a parallel description of "can this move."
- `docs/FEATURES.md`: no new row — row 221 ("Move or delete a whole object") and row 227 (in-form editing verbs) both got short notes; core/cli ticked, `gui [ ]` not rounded up (`pdfcer-gui` has not consumed the fix).
- No `personal_rag/pdf` lesson — an internal API-completeness fix, not a producer-divergence finding; the motivating file (`SW41177.pdf`) already has three 2026-09-22 lessons on record for this same object's 237-run shape.

**Still in flight:** Nothing new opened by this Pass. `pdfcer-gui` has not consumed `object_move_refusal`/text-object move (`FEATURES.md` `gui [ ]`). The `R251`/`R258` standing-rules ledger discrepancy flagged by prior filings is still unresolved — not re-verified this session either.

**For next session:** `pdfce_FeatureRequests/INDEX.md` `G029` row closed SHIPPED; `open/reply_G029_move_objects_now_moves_text_FIXED.md` is the engineer's own artifact, not written by this filing.

**Sourcing (hard rule 8).** No shell tool this filing. Independently confirmed via `Grep`/`Read` against live source: `object_move_refusal`, `plan_move_objects`, `plan_move_text_object`, `TransformInsideTextObject` all present in `crates/pdfcer-core/src/vector/edit.rs`; no prior `ROADMAP.md`/`SESSION_LOG.md` entry named `G029` or `Pass 314` before this filing; both edited `docs/FEATURES.md` rows re-checked against that file's own `^.{1200,}$` matches post-edit — row 221 does not appear in that list, row 227 does but was already present pre-edit (baselined, label prefix unchanged by the edit). Commit hash `c6ab2104f18dd4264fc007e3b291697267f418e3`, its diffstat, test counts and gate-sweep results are relayed from the dispatching engineer's report, not independently re-run.

## 2026-09-22 (574th filing) — `Pass 313.0` (`ca57bfcd`): the text-preview cap is now per RUN, not per whole object (`G031`)

**Shipped:** `Pass 313.0` — `pdfcer_core::vector::MAX_TEXT_PREVIEW_CHARS` (`crates/pdfcer-core/src/vector/decompose.rs`) was a 64-char budget for a **whole** text object; on `SW41177.pdf` page 0, object 5871's 237 show-operator runs, `TextObject::run_text(i)` returned `Some("")` for every run past the first few. Fixed: the cap is now per show operator (256 chars, reset at each run's own open), plus a new public `MAX_TEXT_PREVIEW_PAGE_CHARS: usize = 1 << 20` bounding the decomposition as a whole — the actual memory ceiling the old single number stood in for. Past the page ceiling a run reads `Some("")` and its object's `truncated` flag is set, so a cut run stays distinguishable from a genuinely-empty one. No struct shape changed (`TextObject`/`TextPreview` are not `#[non_exhaustive]`); no `Cargo.toml` change; not a writer change.

**Decisions made this session:** None — a budget-scoping fix plus one new public constant, not a crate-boundary/library/invariant call. No decision-log entry.

**Findings + decisions:**
- New `personal_rag/pdf` lesson: the same 237-run SolidWorks `BT`…`ET` already on record for breaking hit-testing (2026-08-04 lesson) also breaks a per-object preview-character budget — same structural cause, a different consequence. Checked first against that lesson and the two other 2026-09-22 `SW41177.pdf` lessons and confirmed distinct, not a duplicate.
- `docs/FEATURES.md`: no new row and no box change — this corrects an already-`[x]`-core/`[x]`-cli/`[ ]`-gui capability, not a new one. A short note appended to the "Split one text object into several" row naming the fix; re-checked against the 1,200-char register cap after the edit.

**Still in flight:** Nothing new opened by this Pass. `pdfcer-gui` still has not consumed this fix (`FEATURES.md` `gui [ ]`, not rounded up). The `R251`/`R258` standing-rules ledger discrepancy flagged by prior filings is still unresolved — not re-verified this session either.

**For next session:** `pdfce_FeatureRequests/INDEX.md` `G031` row closed SHIPPED; `open/reply_G031_text_preview_cap_is_now_per_run_FIXED.md` is the engineer's own artifact, not written by this filing.

**Sourcing (hard rule 8).** No shell tool this filing. Independently confirmed via `Grep`/`Read` against live source: `MAX_TEXT_PREVIEW_CHARS`, `MAX_TEXT_PREVIEW_PAGE_CHARS`, `TextPreview::Decoded::truncated` and `TextObject::run_text` all present in `crates/pdfcer-core/src/vector/decompose.rs`; no prior `ROADMAP.md`/`SESSION_LOG.md` entry named `G031` or `Pass 313` before this filing; the edited `docs/FEATURES.md` row re-checked against that file's own `^.{1200,}$` matches post-edit and does not appear in that list. Commit hash `ca57bfcd`, its full 40-char form, the diffstat, test counts and gate-sweep results are relayed from the dispatching engineer's report, not independently re-run.

## 2026-09-22 (573rd filing) — `Pass 312.0` (`d5b23d66`): `SplitGranularity::Line` breaks on clear space, not only on a baseline change (`G032`); also closed a register-entry-size overage left by the 572nd filing

**Shipped:** `Pass 312.0` — `runs_share_a_line` compared only orientation and baseline, so a SolidWorks bill-of-materials table (row-major, one show operator per cell) and a sheet border's zone letters (one operator each), both sharing a baseline with unrelated neighbours, welded into single "lines": 565 across a 36-page drawing, widest holding 709.4pt of blank paper, blocking GUI move/delete/redact on one BOM cell. Fixed with a horizontal clear-space/backward-jump test layered on the baseline test, thresholds exposed as new public `LineSplitOptions` (`max_gap`/`max_backward`, defaults 1.0/0.5 line heights, set from the fixture's own bimodal gap measurement), skipped under `TextBoundsBasis::EmBox`. No `Cargo.toml` change; not a writer change.

**Decisions made this session:** None — a splitting-criterion fix plus a new options struct, not a crate-boundary/library/invariant call. No decision-log entry.

**Findings + decisions:**
- Housekeeping first: the 572nd filing's own `docs/FEATURES.md` addition for `Pass 311.0`/`G028` had grown to 1,676 characters against the register-entry-size cap of 1,200 — trimmed to a verdict plus a short paragraph citing `0c0145ac`; the detail stays in that commit's message, per this project's size-rule discipline.
- New `personal_rag/pdf` lesson: SolidWorks writes a table row-major and a sheet border's zone letters one at a time, both on a shared baseline with unrelated neighbours, so "consecutive in stream + same baseline" is a producer convention, not a guarantee — any editor grouping lines by baseline plus stream-adjacency alone has this same gap on CAD/table-shaped output. Includes the measured bimodal gap distribution. Checked first against the adjacent 2026-09-15 lesson (a repositioning-order defect, not a grouping-criterion one) and confirmed distinct, not a duplicate.

**Still in flight:** Nothing new opened by this Pass. `pdfcer-gui` still has not consumed this fix (`FEATURES.md` `gui [ ]`, not rounded up). The `R251`/`R258` standing-rules ledger discrepancy flagged by the 572nd filing is still unresolved — not re-verified this session either.

**For next session:** `pdfce_FeatureRequests/INDEX.md` `G032` row closed SHIPPED; `open/reply_G032_line_split_now_breaks_on_clear_space_FIXED.md` is the engineer's own artifact, not written by this filing.

**Sourcing (hard rule 8).** No shell tool this filing. Independently confirmed via `Grep`/`Read` against live source: `LineSplitOptions`, `text_object_line_split_points`, `text_runs_share_a_line` all present in `crates/pdfcer-core/src/vector/edit.rs`; no prior `ROADMAP.md`/`SESSION_LOG.md` entry named `G032` or `Pass 312` before this filing; both edited `docs/FEATURES.md` rows re-checked line-by-line against the file's own `^.{1200,}$` matches post-edit and neither appears in that list. Commit hash `d5b23d66`, its full 40-char form, the diffstat, test counts and gate-sweep results are relayed from the dispatching engineer's report, not independently re-run.

## 2026-09-22 (572nd filing) — `Pass 311.0` (`0c0145ac`): a span edit lands where the match began, not where it ended (`G028`)

**Shipped:** `Pass 311.0` — a multi-operator `edit_text` used to write the replacement into the operator holding the match's **end**, empty the leading operators, and never reposition the survivor back to where the match began: a 9-operator span on a SolidWorks sheet (`SW41177.pdf`) teleported a whole line 240.16pt right, `followers_repositioned=0`. Fixed in `text_edit::edit::plan_edit_target` with three new private helpers — `line_x`/`advance_before` measure the leading-operator shift from real text-matrix origins instead of summed glyph advances, and `narrow_span` trims a find/replace pair to the part that actually differs when they share a prefix or suffix. `reposition_followers` also picked up two independent fixes found while measuring the fixture: a drift-sized `Td` `ty` (≤0.01pt, SolidWorks' own ±0.00057 noise) is now the same line during reflow, and a `Tm` follower under a scaled matrix now moves the correct user-space distance. `Reflow` and `Pin` now differ, as they should — previously byte-identical for a spanning edit.

**Decisions made this session:** None — a positioning bug fix plus new private helpers, not a crate-boundary/library/invariant call. No decision-log entry.

**Findings + decisions:**
- The bug needed a real multi-`/Contents`-stream SolidWorks export to reproduce; five synthetic fixtures built by the reporter (absolute `Tm` per fragment, split streams, re-issued `Tf`, jittered baseline, 24-operator per-glyph) all compensated correctly, narrowing the search but not landing it — the eventual cause was in the geometric measurement, not in `same_line` or the stream-merge timing the addendum suspected.
- New `personal_rag/pdf` lesson: SolidWorks leaves a producer gap on top of the space-glyph advance between show operators, so an edit that merges/deletes operators must remove that gap too, measured from real text-matrix origins rather than summed glyph advances — distinct from the `Pass 304.0` same-line-tolerance lesson and the per-glyph-operator matching-defeat lesson (both grepped first, neither covers this).

**Still in flight:** Nothing new opened by this Pass. `pdfcer-gui` still has not consumed `operators_spanned` or this fix (`FEATURES.md` `gui [ ]`, not rounded up).

**For next session:** `pdfce_FeatureRequests/INDEX.md` `G028` row closed SHIPPED; `open/reply_G028_*.md` is the engineer's own artifact, not written by this filing. Unresolved from this filing: `ROADMAP.md`'s standing-rules "next free" ceiling has one ledger row saying `R251` (the `v0.55.0` entry) against two more recent ones saying `R258` (`Pass 309.x`/`310.x`) — not re-verified this session, flagged for whoever mints the next rule.

**Sourcing (hard rule 8).** No shell tool this filing. Independently confirmed via `Grep` against live source: `plan_edit_target`, `reposition_followers`, `line_x`, `advance_before`, `narrow_span` all present in `crates/pdfcer-core/src/text_edit/edit.rs`; no prior `ROADMAP.md` entry named `G028` or `Pass 311` before this filing. Commit hash `0c0145ac`, its message, test counts and gate-sweep results are relayed from the dispatching engineer's report, not independently re-run.

## 2026-09-17 (571st filing) — `v0.55.0` released (`229e8635`): 9 commits, the same day as `v0.54.0`, because redaction was eating text nobody marked

**Shipped:** No new Pass — a release filing. `v0.55.0` cut from `229e8635` ("chore: v0.55.0"), tag and `main` both at that commit on `origin`, GitHub release published (not a draft) with zip + sha256, OneDrive slot `pdfcer2` updated (0.53.0 → 0.55.0; `pdfcer1` keeps 0.54.0 as the rollback, `R229`'s alternating scheme). **The second release of the day** — `v0.54.0` shipped this morning; the operator reported redaction destroying text he never marked, `Pass 310.0`–`310.2` fixed it, and this release carries the fix.

**What it carries:** **9 commits in `v0.54.0..v0.55.0` = 5 Pass commits + 3 librarian filings + 1 version bump.** `Pass 309.0` (`46072a07`) + `309.1` (`3284d5b9`) — `pdfcer --help`'s stale first sentence, then Markdown asterisks and internal Pass IDs shipping to the terminal. `Pass 310.0` + `310.1` (`ce474dae`) — the residual sweep's two causes and `--residual-scope`. `Pass 310.2` (`bb1bd994`) — the detector/actor case mismatch, closed by the shared `text_match_ranges`. Filings `75ecc7de`, `79750fde`, `7419fb2f`. Individual Passes already filed under their own entries; not refiled here. ★ The dispatch said "seven commits" and named seven; the range measures **9**, the extra two being `67a9a8c3` (`v0.54.0`'s own release record, committed after that tag) and the bump itself. Filed in both forms per hard rule 10(a).

**Decisions made this session:** none — a release filing carries no architectural decision. Highest decision record confirmed still 158.

**Findings + decisions:**
- **★ CI WAS NOT GREEN AT FILING TIME, and the entry says so rather than inheriting `v0.54.0`'s wording.** `gh run list`, run by this filing: the CI run at `229e8635` is `in_progress`, conclusion empty; so is the run one commit behind it. `python tools/verify-release.py v0.55.0`, re-run by this filing: **3 problems**, two of them CI — *"CI is finished at the tagged commit"* and *"CI is GREEN at the tagged commit"*. Seven checks ok (tag exists / at HEAD / pushed, `origin/main` contains it, release has assets, OneDrive holds 0.55.0, 0.54.0 retained). Recorded as **watched, not green**; the next reader owes a `gh run list --limit 3`.
- **The third `verify-release` FAIL is a false alarm and is recorded as one.** "Working tree clean" FAILs on `docs/NEXT_SESSION.md` — the engineer's own post-release handoff amendment, made **after** the tag. `git status --porcelain` returned empty at the start of this filing and ` M docs/NEXT_SESSION.md` minutes later. The release was built from a clean tree; the gate is comparing the tag against a tree that has since moved on in documentation only.
- **★★ The smoke test reproduced the operator's own bug from the packaged artifact, and that is a different claim from the one `v0.54.0`'s smoke test made.** Copied outside `D:\builds`, `redact-mark --search "INVOICE 4412"` then `redact-apply` on the two-page fixture: page 2 extracts as `INVOICE summary`, intact; the released `v0.54.0` binary gives `XXXXXXX summary` on the same input. `--version` reports `pdfcer 0.55.0`, rev `v0.55.0`, `iccce 0.3.0 (rev a4d9003b)`. A `--version` + `inspect` smoke test proves the folder is not corrupt; this one proves the release ships the fix it exists for.
- **`gh release create <tag> <assets…>` is not atomic in the way it appears.** Two `HTTP 500: Error saving asset` failures before the third attempt succeeded, and **the first failure rolled the release back** — `gh release view v0.55.0` answered "release not found" while a live release id sat in the error URL. Create-empty then `gh release upload --clobber`, retried, is the shape that worked. Filed as a new cross-project finding at `D:\dev\rag\gh-cli\release_create_with_assets_is_not_atomic.md` (a `gh` CLI property, not a pdfcer one — the next project to cut a release here meets it too), plus a `ROADMAP.md` Backlog entry for a `tools/` wrapper. **Deliberately NOT a standing rule:** `R229` and `verify-release.py` cover whether a release is *correct*; this is about getting it *published without losing it*, which is tooling.
- `docs/FEATURES.md`: **no rows changed** — a release ships no new capability, and the redaction residual-scope row (line 331) was written correct by the 570th filing. Said explicitly so the silence is not read as a missed sweep.

**Still in flight:** the `/CO` indirect-array Backlog item; the widened `FieldEdit`/`WidgetEdit`-audit item; the shared-resource-decoupling item (569th filing); and now the `gh release` wrapper item. Next free Pass family `311`; next free rule `R251` per the live ceiling.

**For next session:** confirm CI's colour at `229e8635` before describing `v0.55.0` as verified — `gh run list --limit 3`. Nothing else opened by this filing.

**Sourcing (hard rule 8).** ★ **This filing HAS a shell, and the release mechanics are CHECKED, not relayed** — the inverse of the 567th filing, which recorded `v0.54.0` entirely on trust and closed by asking a future session to confirm it. Verified in `D:\Dev\pdfcer`: `git rev-list -n1 v0.55.0` and `git rev-parse HEAD` both `229e8635efd3cdbf98e02660ff82067515d4840e`; `git ls-remote --tags origin v0.55.0` (tag object `2d229131`); `git branch -r --contains 229e8635` lists `origin/main`; the 9-commit range by `git log --oneline` + `git rev-list --count`, each commit re-confirmed by `git log -1`; `gh release view v0.55.0 --json` for title, publish time, draft flag, asset names, byte counts and sha256 digests; `gh run list --limit 8 --json` for CI; `ls -la`/`du -sb` on `D:\builds\pdfcer-20260917-1639-229e863` and `cat BUILD-INFO.txt` for the build; `python tools/verify-release.py v0.55.0` for the nine-check result. **Still relayed, not re-run:** the gate-sweep totals (34 commands, 420 `test result: ok`), the clippy/fmt results, the individual doc-gate re-runs, and the smoke-test transcript — the binary comparison was not repeated here.

## 2026-09-17 (570th filing) — `Pass 310.0` + `310.1` shipped (`ce474dae`) and `Pass 310.2` one commit later (`bb1bd994`): redaction stops eating text the operator never marked; this filing read an uncommitted working tree and mis-attributed `310.2`'s commit

**Shipped:** All three Passes filed at the 569th filing (`Pass 310.0`/`310.1`/`310.2`) moved to *Shipped* — `310.0`/`310.1` under commit `ce474dae`, `310.2` under `bb1bd994`, its successor and the current tip of `main`. New core surface: `redact::ResidualScope` (`MarkedOnly`/`HiddenCarriers` default/`WholeDocument`, `#[non_exhaustive]`), `RedactOptions`, `apply_redactions_with`, `CarrierAction::FoundNotScrubbed`, `RedactionReport::residual_matches_left`/`has_unscrubbed_matches()`, `EditSession::set_residual_scope`/`residual_scope`. Needle sets split: tokens for invisible carriers, whole redacted runs only for drawable content — closing the operator's reported bug (an unmarked word sharing a token with a redacted run was blanked document-wide). Drawable scrubbing OFF by default. `Pass 310.1` (CLI): `--residual-scope` on `redact-apply`/`redact-offpage` as a CLI-local `ValueEnum` + `From` impl (no core `clap` derive); the three residual-sweep counters now print unconditionally (`R151` instance).

**`Pass 310.2` (`bb1bd994`).** A detector/actor case-sensitivity mismatch: `bytes_contain_text` was ASCII-case-insensitive/UTF-16BE-aware, its actors `blank_in_strings`/`replace_all_bytes` were exact-byte case-sensitive, and `carrier_xmp` carried the mirror image (the actor WAS the detector, so a case-differing quote read `Absent` rather than merely unscrubbed). Closed by one shared `text_match_ranges` (`redact.rs:3925`) called by detector and actors alike, its doc comment naming `R245`; `blank_in_strings` moved from `&[u8]` to `&str`, deleting the two-call-per-needle dance (ASCII then UTF-16BE) at every call site and with it the chance of adding one call and forgetting the other. `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo fmt --all` applied. `redaction_residual_sweep.rs` **14 → 16**, all green; **ablated** — restoring exact-byte matching in the actors fails exactly those two tests and no others. `a_case_variant_in_an_xmp_packet_is_still_scrubbed` deliberately asserts on the saved bytes and `residual_sweep_objects_scrubbed`, **not** on the `xmp` carrier row: that fixture's packet is unreferenced, the catalog carrier is honestly `Absent`, and the metadata branch is what handles it. Its first draft asserted on the carrier row and failed for exactly that reason — a test aimed at the wrong object looks like a defect in the code.

**★ Correction, same day, before commit — this entry first attributed `Pass 310.2` to `ce474dae` and said the dispatch's "310.2 still open" claim was wrong.** Both halves were mistaken, and the cause is worth more than the fix. **The dispatch was accurate when it was written.** This filing, holding no shell, checked "310.2 is still open" against the **live working tree**, found `text_match_ranges` and both new tests on disk, and concluded the dispatch was stale. What it had read was **uncommitted work in progress** — `bb1bd994` was made after the filing began. **Reading a file cannot tell you which commit introduced it**: a working tree shows what is on disk, only `git show <hash>` shows what a commit contains. (The dispatch's "13 → 14" was likewise consistent — 14 was the count *before* `310.2`.) Filed as an `R87` dated instance running the inverse of every prior one: agent→record rather than engineer→agent, same class of error. Caught by the engineer's post-filing `git log`, which is `R87`'s second clause, and catchable only because the *Sourcing* paragraph named the hash **relayed, not verified**.

**Decisions made this session:** none — a filing session, no new `ARCHITECTURE.md` §12 entry (highest decision record confirmed still 158 via Grep).

**Findings + decisions:**
- `R245` (guard/key/disclosure on one family member shipped, not the whole family) — 11th dated instance filed in `ROADMAP.md` Standing rules, for the detector/actor case-mismatch fix.
- `R247` (a doc comment's behavioural guarantee is unenforced until a test exists) — 6th dated instance filed, for the "abandoned content" guarantee no test enforced before `ResidualScope`.
- `R87` (hashes are engineer-verified, never filed on trust) — dated instance filed, for this filing's own mis-attribution of `Pass 310.2` to `ce474dae`. The generalisable half: **a doc-writing agent with no shell cannot distinguish committed from uncommitted work, and a working tree read as commit evidence produces a confidently wrong hash in a document that is then cited BY hash** — a cost this project has already paid once (`tools/check-cited-commits-exist.py`, `0d9f4df`, fourteen pre-existing casualties). Cheap mitigation, already practised: a dispatch states, and a filing repeats, that hashes are relayed rather than verified.
- Declined to write a `personal_rag/pdf` lesson for the dispatch's suggested finding ("a belt-and-braces residual sweep with no liveness test is a destructive pass wearing the name of a precaution") — its mechanism is `R247`'s, not a fresh PDF-domain finding; filed as `R247`'s dated instance instead, with the reasoning stated in the ROADMAP entry.
- `docs/FEATURES.md`: removed the old *Planned* row (redaction residual-sweep scoping) and added one new *Implemented* row under *Redaction & security* — `[x]` core / `[x]` cli / `[ ]` gui (heads-up `E001` filed in the FeatureRequests channel for the unwired `set_residual_scope`).

**Still in flight:** none new. Next free Pass family is `311`.

**For next session:** none flagged by this filing beyond the standard queue.

**Sourcing (hard rule 8).** Two passes; the difference between them is the `R87` instance above.

*Original filing, NO SHELL.* Confirmed via `Grep`/`Read` against live source: `ResidualScope`/`RedactOptions`/`apply_redactions_with`/`CarrierAction::FoundNotScrubbed` in `crates/pdfcer-core/src/redact.rs`; `set_residual_scope` in `crates/pdfcer-core/src/edit.rs:9280`; `--residual-scope`/`ResidualScopeArg` in `crates/pdfcer-cli/src/main.rs`; `text_match_ranges` (line 3925) and both new test names/assertions in `crates/pdfcer-core/tests/redaction_residual_sweep.rs`; 16 total `#[test]` functions in that file; `ARCHITECTURE.md` §12 highest decision 158. **Every one of those readings was of the WORKING TREE** — which is why `310.2` was correctly called done and wrongly attributed.

*Correction pass, HAS a shell.* Both hashes verified by `git show --stat --format='%H%n%s%n%ad'` in `D:\Dev\pdfcer`: `ce474daec913e2b19a89dd75fcafb122d01fc6b2` — *"Pass 310.0 + 310.1: redaction stops eating text the operator never marked"*, 16:09:11 −0400, 7 files, +797/−52; `bb1bd994269bb981a2b69d54a2c2eb9992bf41cf` — *"Pass 310.2: one matching rule, shared by the detector and the actors"*, 16:23:55 −0400, 2 files, +179/−66. `git log --oneline -5` puts `bb1bd994` at `main`'s tip with `ce474dae` its parent. Still **relayed, not re-run**: the gate-sweep output (34 commands, 420 `test result: ok`), the end-to-end binary comparison against `v0.54.0`, `310.2`'s clippy/fmt result, its 14 → 16 test count, and the ablation.

## 2026-09-17 (569th filing) — operator request filed as `Pass 310.0`–`310.2` (redaction residual-sweep scoping) + one Backlog entry (shared-resource decoupling), no code shipped

**Filed, not shipped:** operator report 2026-09-17, verbatim: *"I noticed that when I use the redaction tool on some content, if other content matches I haven't selected also gets removed or replaced with X. I'd like the option to only redact the content I have actually selected."* Filed as three `Next up` entries, family `310` (confirmed next-free against the live ledger — `309` is the newest Shipped family): `Pass 310.0` (new `ResidualScope`/`RedactOptions` core API, default narrows the residual sweep's content-stream matching to invisible carriers only, everything declined still disclosed via new `CarrierAction::FoundNotScrubbed`), `Pass 310.1` (CLI `--residual-scope` flag on `redact-apply`/`redact-offpage`, plus an R151-shaped fix — `cmd_redact_apply`/`cmd_redact_offpage` compute residual-sweep counts today and print none of them), `Pass 310.2` (bug, fix-on-discovery: the sweep's detector `bytes_contain_text` is case-insensitive/UTF-16BE-aware, its actors `blank_in_strings`/`replace_all_bytes` are case-sensitive exact-byte, producing false `DisclosedNotScrubbed` reports).

**Decisions made this session:** none — a filing session, no `ARCHITECTURE.md` §12 entry. `Pass 310.0`'s entry explicitly records why this is NOT an `R249` violation (scoping what the sweep ACTS ON by carrier visibility, a static classification, not a computed reachability walk; every declined match stays disclosed) — written into the Pass entry itself so a future session doesn't misread "narrowed the residual sweep" as a collision.

**Findings + decisions:**
- A diagnosis this session corrects decision 146's "owed item 18" premise (`ROADMAP.md` ~:4118-4126): it said blanking abandoned-stream text "needs no reachability walk (pdfcer already knows which content streams its own surgery rewrote)" — the shipped `blank_show_strings` (`redact.rs:3320`) carries no such restriction and blanks matching text in any content stream it scans, including ones pdfcer never touched. Recorded as a correction inside `Pass 310.0`'s entry, not a new decision record (diagnostic correction, not an architecture change).
- `docs/FEATURES.md`: one new *Planned* row (redaction residual-sweep scoping), `core`/`cli` unticked, `gui` marked `?` — tracked separately via the engineer's own filing to `D:\Dev\FeatureRequests\pdfce_FeatureRequests`, per the dispatch instruction not to tick a GUI box this project doesn't own.
- `docs/ROADMAP.md`'s new Backlog entry (shared-resource decoupling hazard) deliberately states an open question rather than an answer: whether pdfcer's existing redaction surgery already decouples a shared resource before editing it, or mutates it in place today. Not measured this filing.

**Still in flight:** `Pass 310.0`–`310.2` are Next-up only, none started. Unchanged from the 568th filing: the `/CO` indirect-array Backlog item, the widened `FieldEdit`/`WidgetEdit`-audit Backlog item, the shared-resource-decoupling item now added by this filing.

**For next session:** `Pass 310.0` is the build order (310.1/310.2 depend on it). The shared-resource Backlog item needs a targeted fixture before it can be scoped into a Pass — not yet built.

**Sourcing (hard rule 8).** No shell tool this filing. File:line citations (`redact.rs:3320`, `:3249`, `:3615`, `:3640`, `:3140-3157`; `main.rs:1879`, `:1209`, `:28029-28080`, `:42615`) and the Acrobat parity-ground summary are relayed from the dispatching engineer's report, not independently re-verified against live source this filing.

## 2026-09-17 (568th filing) — `Pass 309.0` (`46072a07`) + `Pass 309.1` (`3284d5b9`): `pdfcer --help`'s stale Pass-0 claim fixed, then swept project-wide

**Shipped:** `Pass 309.0` + `Pass 309.1`, new Pass family minted this filing, next free family `310`. Found during `v0.54.0`'s own packaging smoke test (see the 567th filing below), closes that filing's Backlog entry. `309.0`: `Cli`'s `long_about` said "Pass 0 implements `inspect`; the remaining subcommands are stubs" — true at Pass 0, false since roughly Pass 5, published in `v0.54.0` as the lead sentence of `pdfcer --help`. Rewritten to point at the printed subcommand list rather than restate a count; new gate `tools/check-clap-help.py` derives working/stub counts from the `Command` enum and checks `README.md`'s prose (itself wrong — 149, not 153 — until this Pass). `309.1`: the same defect class generalised — 99 of 156 subcommand summaries shipped literal `**bold**` Markdown asterisks and 70 named an internal Pass ID. Fixed at runtime (`scrub_help`, walks the whole `Command` tree) for `about`/`long_about`/arg help; fixed in source for 47 `ValueEnum`s' variant docs (no runtime setter preserves the typed `EnumValueParser`) and 50 Pass-ID-naming prose lines (reworded, not renumbered). New test `cli_help_ships_no_internal_markup`. `Cargo.toml`'s published `description` carried the same claim; corrected too.

**Decisions made this session:** none — a bug fix plus a runtime-vs-source doc-comment technique, not a crate-boundary/library/invariant call. No `ARCHITECTURE.md` §12 entry.

**Findings + decisions:**
- **A `///` doc comment in clap-derive is shipped UI, not just rustdoc source.** Two incompatible readers — `cargo doc` renders Markdown, a terminal prints it verbatim. Detect by rendering `long_help()`/`render_long_help()` in a test and asserting on the string; a source-text scan misses markup straddling clap's hard-wrap joins. New file in `D:\dev\rag\rust\`.
- **`about`/`long_about`/arg help are mutable at runtime; `ValueEnum` variant help is not.** `Command::mut_args`/`mut_subcommand` cover the whole tree (and every future subcommand) in one pass; `PossibleValue` help has no setter that preserves the typed `EnumValueParser`, so a `ValueEnum`'s docs must be fixed in source. Gotcha: `mut_subcommand`'s closure takes the subcommand by value — collect the name list before iterating. New file in `D:\dev\rag\rust\`.
- `docs/FEATURES.md`: **no rows changed** — operator-facing copy correctness on a capability the `cli` column already claims, no verb added/removed/widened. Said explicitly so it isn't read as a missed sweep.
- `docs/ROADMAP.md`: new `Pass 309.0` + `Pass 309.1` Shipped entry added at top (above `v0.54.0`'s release entry, which precedes it chronologically but was filed for older commits); the 567th filing's `--help` `long_about` Backlog entry closed in place, original text kept legible.

**Still in flight:** unchanged from the 567th filing below — the `/CO` indirect-array Backlog item, the widened `FieldEdit`/`WidgetEdit`-audit Backlog item.

**For next session:** confirm the tag/release/OneDrive state with a shell before relying on the 567th filing's release entry for more than a record — still unconfirmed as of this filing, no shell tool either session.

**Sourcing (hard rule 8).** No shell tool this filing. Independently confirmed via `Grep`/`Read` against live source: `fn scrub_help` and `fn cli_help_ships_no_internal_markup` present in `crates/pdfcer-cli/src/main.rs`; `README.md` reads "153 working subcommands (plus three that announce themselves as not yet implemented)"; `crates/pdfcer-cli/Cargo.toml`'s `description` no longer names Pass 0; `tools/check-clap-help.py` exists on disk. Commit hashes, test counts and gate-sweep results are relayed from the dispatching engineer's report, not independently re-run.

## 2026-09-17 (567th filing) — `v0.54.0` released (`8a2162ab`): 129 commits since `v0.53.0`, no new Pass

**Shipped:** No new Pass — a release filing. `v0.54.0` cut from `8a2162ab` ("chore: v0.54.0"), tag + `main` both pushed to `origin` at the same commit, GitHub release published (not a draft, not a prerelease, marked latest) with zip + sha256, OneDrive slot `pdfcer1` updated (0.52.0 → 0.54.0; `pdfcer2` keeps 0.53.0 as rollback). `python tools/verify-release.py v0.54.0` clean, nine checks ok.

**What it carries:** the transparency-group compositing fix (783 s → 8 s at 4× on the operator's street map, raster hash identical), viewport culling for images, the parser's per-stream token-density reservation, the `split-text-object`/`text-run-move` CAD title-block arc, eleven answered `pdfcer-gui` requests `G017`–`G027`, km/yd/mi units, and the off-page redaction residual fix (17 known-affected drawings → 7, 0 fully-off residuals). Individual Passes already filed under their own dates; not refiled here.

**Decisions made this session:** none — a release filing carries no architectural decision.

**Findings + decisions:**
- **Release build linked fine on this machine.** `cargo build --release -p pdfcer-cli -j 2` finished in 13m 12s, exit 0 — contradicts `docs/NEXT_SESSION.md`'s standing warning that `pdfcer-cli` will not release-link here. Amended in place (struck, not deleted) as a tendency rather than a certainty, matching the file's existing 2026-09-14 `run-gates.sh` correction.
- **A stale claim found during the packaging smoke test, filed not fixed:** `pdfcer --help`'s top-level `long_about` still says most subcommands are stubs; 149 work and 3 are stubs. `tools/check-cli-help-leads.py` doesn't cover it — checks subcommand doc-comment leads, not the top-level struct text. New Backlog entry filed.
- `docs/FEATURES.md`: **no rows changed** — a release ships no new capability. Said explicitly so this isn't read as a missed sweep.
- `docs/NEXT_SESSION.md`: STATE section updated to workspace `0.54.0`/last release `v0.54.0`; release-link warning amended per above.

**Still in flight:** unchanged from the 566th filing below — the `/CO` indirect-array Backlog item, the widened `FieldEdit`/`WidgetEdit`-audit Backlog item — plus this filing's new `--help` `long_about` item.

**For next session:** confirm the tag/release/OneDrive state with a shell (`git describe --tags --abbrev=0`, `gh release view v0.54.0`) before relying on this entry for more than a record — no shell tool this filing.

**Sourcing (hard rule 8).** No shell tool this filing. `Cargo.toml`'s `version = "0.54.0"` and the `long_about` literal confirmed directly by `Grep`/`Read` against live source. All git/GitHub/OneDrive/build-timing figures above are relayed from the dispatching engineer's report, not independently checked.

## 2026-09-16 (566th filing) — `Pass 308.9` (`22bdf80c`): the duplicate rule is public too; `G027` closed

**Shipped:** `Pass 308.9` (`22bdf80c`), new Pass minted this filing (family now `308.0`–`308.9`, next free family `309` unchanged). Answers `pdfcer-gui`'s `G027`, filed within two hours of `G026`'s delivery: *"this is `G026` with the nouns changed."* `pub fn duplicate_choice_export(options: &[ChoiceOption]) -> Option<&str>` added to `edit`, beside `choice_option_order`/`sort_choice_options`; `Pass 308.7`'s private `refuse_duplicate_exports` is now a two-line wrapper over it. Returns the offending VALUE rather than a `bool`.

**Decisions made this session:** none — a private function made public plus a doc-comment cross-link, not a crate-boundary/library/invariant call. No `ARCHITECTURE.md` §12 entry.

**Findings + decisions:**
- **★★ The finding, escalated per the requester's own flag.** `Pass 308.8` (`G026`, one commit earlier, same file) exported the `/Opt` ordering and its own doc comment argued the GENERAL case — two independent implementations of one rule agree until one of them is improved, and no test on either side can catch the drift. `Pass 308.7` (`G025`), shipped in the same commit, put the `/Opt` duplicate refusal on both writing verbs and left it private — the identical shape the just-written sentence predicted, one function away, in the same file. ⇒ *A finding accepted in one place is not thereby applied everywhere it holds.* New file `D:\dev\rag\rust\a_finding_accepted_in_one_place_is_not_thereby_applied_everywhere_it_holds.md`, cross-linked from `308.8`'s own RAG entry as its missing second half.
- **Design decision recorded from the requester's own words:** (b) — an `EditError` carrying its own operator sentence — was declined as the weaker fix: a sentence is a second description of the rule and can drift from it; a predicate cannot.
- **A test pins something nobody asked about:** two options may share a DISPLAY string, and only `export` is the identity — refusing a repeated display would be pdfcer inventing a constraint the standard does not have.
- `docs/ROADMAP.md`: new `Pass 308.9` Shipped entry added at top, above `Pass 308.7`/`308.8`'s.
- `docs/FEATURES.md`: field-property row (line 312) updated — the duplicate-export rule is now public.
- `D:\Dev\FeatureRequests\pdfce_FeatureRequests\INDEX.md` gained one row, `G027`, newest-first, above the `G026` row.

**Still in flight:** owed items unchanged from the 565th filing below (the `/CO` indirect-array Backlog item, the widened `FieldEdit`/`WidgetEdit`-audit Backlog item).

**For next session:** nothing new opened by this filing; the request closed same-day.

**Sourcing (hard rule 8).** No shell tool this filing. `.git/COMMIT_EDITMSG` (HEAD's full message) and `.git/refs/heads/main` read directly, confirming HEAD is `22bdf80c...` with this exact title/body. **Independently confirmed via `Read`/`Grep` against live source at HEAD:** `pub fn duplicate_choice_export` and `fn refuse_duplicate_exports` both present in `crates/pdfcer-core/src/edit.rs`; both archived request/reply files confirmed present.

## 2026-09-16 (564th and 565th filings, one commit) — `Pass 308.7` + `Pass 308.8` (`56351185`): one guard, one comparator, one meaning; `G025`/`G026` closed

**Shipped:** `Pass 308.7` + `Pass 308.8` (`56351185`), new Passes minted this filing (family now `308.0`–`308.8`, next free family `309` unchanged). **One commit, two Pass IDs, deliberate** — the two fixes interleave inside `edit_field`; `check-passes-filed.py` treats a multi-claim commit as a note, not a failure. Two `pdfcer-gui` reports about the same verb, minutes apart, both about a rule that existed in one place and was needed in two.

**`Pass 308.7` (`G025`):** `add_choice_field` refused a duplicate `/Opt` export; `edit_field` wrote one. The guard existed, was worded, was in `# Errors`, and was unreachable from the verb an operator actually drives — placement is reached once per field, editing every time anybody adjusts an existing one. Moved to a shared `refuse_duplicate_exports`, called by both.

**`Pass 308.8` (`G026`):** `NewChoiceField::sorted(true)` sorted the array; `FieldEdit::with_sort(true)` set `/Ff` bit 20 and sorted nothing — same word, same crate, same field type, opposite meanings. Comparator exported (`choice_option_order`/`sort_choice_options`, public in `edit`); `edit_field` now sorts when `options` and `sort: Some(true)` arrive in the SAME edit; `with_sort`'s doc names its sibling. New `FieldEditOutcome::options_sorted`.

**Decisions made this session:** none — a guard relocation, a comparator export, and a same-edit sort rule, not a crate-boundary/library/invariant call. No `ARCHITECTURE.md` §12 entry.

**Findings + decisions:**
- **`Pass 308.7`'s guard-placement finding.** The guard's own comment was the argument for moving it: *"a duplicate export is unselectable, because the fill verb resolves to the first match"* is a statement about the written FILE, not about how the field came to be written — wrong place from the start, not merely missing from one verb. The requester's own judgement call (offered, not guessed, and right): only the list being WRITTEN is checked, so a document arriving with a pre-existing duplicate stays editable — the same call the Table 230 gate makes for a nonconforming bit-19 file. ⇒ *pdfcer refuses to AUTHOR the defect and refuses to make a file carrying it unusable — different obligations, only the first is a refusal.*
- **★★ `Pass 308.8`'s generalisable finding, escalated per the requester's own flag.** The shell had copied the private `sort_by` line rather than calling a public comparator, because none existed; `sort_claim_unmet`'s test (`is_sorted`) checks a CONSEQUENCE of the comparator, not its identity, so the two copies agreed only by being the same code — a future improvement to the ordering would desync them with nothing on either side able to catch it. ⇒ *Two independent implementations of one ordering agree until one of them is improved, and no test on either side can catch the drift because each stays internally consistent.* New file `D:\dev\rag\rust\a_private_comparator_copied_by_a_caller_is_a_second_implementation_that_can_only_diverge.md` — a sibling of `two_representations_of_one_fact_can_disagree_until_a_third_thing_derives_one_from_the_other.md` (a value written twice) and `a_conversion_reimplemented_on_a_second_code_path_diverges...md` (two paths inside one codebase); this instance crosses a crate boundary because the API gave the caller no other door.
- `docs/ROADMAP.md`: new combined `Pass 308.7` + `Pass 308.8` Shipped entry added at top, above `Pass 308.6`'s.
- `docs/FEATURES.md`: field-property row (line 312) updated — option-list editing now refuses duplicate exports from both doors, and honours a sort claim supplied with the full replacement list.
- `D:\Dev\FeatureRequests\pdfce_FeatureRequests\INDEX.md` gained two rows, `G025` and `G026`, newest-first, above the `G024` row.
- **Docs counts checked, found unchanged:** `docs/core-api/`'s verb count and `EditError` count needed no edit — `choice_option_order`/`sort_choice_options` are free functions in the `edit` module (confirmed by grep: not inside `impl EditSession`), and no new `EditError` variant was added.

**Still in flight:** owed items unchanged from the 563rd filing below (the `/CO` indirect-array Backlog item, the widened `FieldEdit`/`WidgetEdit`-audit Backlog item).

**For next session:** nothing new opened by this filing; both requests closed same-day.

**Sourcing (hard rule 8).** No shell tool this filing. `.git/COMMIT_EDITMSG` (HEAD's full message) and `.git/refs/heads/main` read directly, confirming HEAD is `56351185...` with this exact title/body. **Independently confirmed via `Read`/`Grep` against live source at HEAD:** `fn choice_option_order`, `fn sort_choice_options`, `fn refuse_duplicate_exports`, `pub options_sorted: bool` on `FieldEditOutcome`, and both Passes' inline citations present in `crates/pdfcer-core/src/edit.rs`; both archived request/reply file pairs confirmed present.

## 2026-09-16 (563rd filing) — `Pass 308.6` (`d2fa7352`): the scripts pdfcer already classifies can now be WRITTEN; `AdvisoryHelper::Keystroke` refused as unemittable; `G024` closed

**Shipped:** `Pass 308.6` (`d2fa7352`), a new Pass minted this filing (family now `308.0`–`308.6`, next free family `309` unchanged). Answers `pdfcer-gui`'s `G024`: *"the scripts you already classify cannot be written."* New module `form_script::emit` inverts `classify` over the same whitelist; three new `EditSession` verbs (`set_field_format`, `set_field_validation`, `set_field_calculation`, `Option<Helper>`, `None` clears), plus `FieldScriptChange`, `CalcOrderChange`, three new `EditError` variants, and CLI `set-field-script`. A format writes its `AF*_Keystroke` twin into `/AA /K` unasked; a calculation writes its `/CO` entry in the same undoable command — `Pass 308.4`/`308.5`'s shape a third time. Verb count 239 → 242, `EditError` 135 → 138.

**Decisions made this session:** none — a new writable capability plus a refusal in an existing type, not a crate-boundary/library/invariant call. No `ARCHITECTURE.md` §12 entry.

**Findings + decisions:**
- **★★ Writing the inverse found a live reader bug, on the first round-trip run.** `AFDate_FormatEx` pairs with `AFDate_KeystrokeEx`; the classifier matched the keystroke family with `ends_with("_Keystroke")`, which never matches that name — a real Acrobat-authored explicit-date field was losing its `/K` disclosure. Fixed in the same commit. ⇒ *An inverse is a test of the original* — nothing had exercised that name because the read side only ever met the names real files happened to carry, and the fixtures were written from the same list as the code.
- **★★ A second finding, about a TYPE.** The request assumed `set_field_validation(Option<AdvisoryHelper>)` was writable in full; half of it is not. `AdvisoryHelper::Keystroke` captures only the helper's NAME for disclosure — its arguments are never read — so re-emitting one would destroy the input filter it names while reporting success. Refused via `NotEmittable::KeystrokeArgumentsNotCaptured`. ⇒ *A type that captures a value for DISCLOSURE is not automatically a type that can reconstruct it.*
- **`pdfcer-acrobat-librarian` dispatched (hard rule 12)** — the codebase had no statement of which field kinds carry which script tab. New file `D:\Dev\Rag-Specialized\Acrobat_Features\forms__format_validate_calculate_tab_availability.md` (24th `forms__*` file): text and combo choice fields carry all three tabs, a **list box carries none** despite being a `/Ch` exactly as a combo box is (reasoned inference, not confirmed). A signature's *Signed* script and a barcode's *Value* script refused by name, out of scope.
- **Known gap, filed not fixed:** the existing field-paste path's `/CO` append matches only a direct `Object::Array`; an indirect `/CO` reference is silently replaced by a fresh one-entry array. The new verb inherits it through the shared `acroform_write` seam. New Backlog entry filed (below).
- **Two new `D:\dev\rag\rust\` files:** `an_inverse_function_is_a_test_of_the_original_a_whitelist_that_is_both_parser_and_oracle_cannot_find_its_own_gaps.md`, `a_type_that_captures_a_value_only_for_disclosure_cannot_be_assumed_reconstructible_for_writing_it_back.md`.
- `docs/ROADMAP.md`: new `Pass 308.6` Shipped entry added at top, above `Pass 308.5`'s. The "audit every `FieldEdit`/`WidgetEdit` property" Backlog item widened to "for any modelled value: can it be written back, and does the writer read it?" (third instance on the sibling shape). New Backlog entry filed for the `/CO` indirect-array bug.
- `docs/FEATURES.md`: new row under *Forms (AcroForm)*, `core [x] · cli [x] · gui [ ]`, for writing Format/Validate/Calculate scripts.
- `D:\Dev\FeatureRequests\pdfce_FeatureRequests\INDEX.md` gained a `G024` row, newest-first, above the `G023` row.

**Still in flight:** owed items unchanged from the 562nd filing below, plus the new `/CO` indirect-array Backlog item (unscoped, no Pass ID) and the widened `FieldEdit`/`WidgetEdit`-audit Backlog item.

**For next session:** the `/CO` indirect-array bug is a real latent data-loss path (the existing paste verb), not merely a limitation of the new one — worth scoping ahead of cosmetic work.

**Sourcing (hard rule 8).** No shell tool this filing. `.git/COMMIT_EDITMSG` (HEAD's full message) and `.git/logs/HEAD` (the reflog) were read directly, confirming HEAD is `d2fa7352` with this exact title/body, preceded by `c33fdf46` (the combined `308.4`/`308.5` filing). **Independently confirmed via `Read`/`Grep` against live source at HEAD:** `enum NotEmittable`, `fn emit`, `KeystrokeArgumentsNotCaptured`, `fn set_field_format`/`set_field_validation`/`set_field_calculation`, `struct FieldScriptChange`/`CalcOrderChange` present in `crates/pdfcer-core/src/{form_script/emit.rs,edit.rs}`; `emit.rs` contains exactly 8 `#[test]` functions, `field_script_authoring.rs` exactly 18; `set-field-script`/`SetFieldScript` present in `crates/pdfcer-cli/src/main.rs`; `docs/core-api/index.md`'s count row reads 242 verbs / 138 variants / 5,575 lines / 184 clauses, matching; the Acrobat RAG file and its index entry both confirmed present.

## 2026-09-16 (562nd filing) — `Pass 308.5` (`20e539a2`): a `/Btn` rotation is now BAKED, not merely declared; the staleness disclosure named the wrong set; `G023` closed

**Shipped:** `Pass 308.5` (`20e539a2`), a new Pass minted this filing (family now `308.0`–`308.5`, next free family `309` unchanged). Answers `pdfcer-gui`'s `G023`, relaying an operator report: *"the rotate buttons don't work for check boxes."* `rotate_widget` always called the button regeneration path, but `build_button_states` — the function that actually paints the plate — had no angle parameter, so a button pdfcer had drawn was rewritten byte-identical and reported `Ok(true)`/turned while nothing moved. `build_button_states` now takes the quarter turn, authors into an `h`×`w`-swapped `/BBox` for 90/270, and emits `quarter_turn_matrix` (the same construction the text path already uses). The four "as-stored vs as-staged" button properties are bundled into one private `ButtonLook`. 9 core tests, 2 sabotage runs confirmed the gap.

**Decisions made this session:** none — a redraw-completeness bug fix plus a disclosure-wording correction, not a crate-boundary/library/invariant call. No `ARCHITECTURE.md` §12 entry.

**Findings + decisions:**
- **Where the turn actually lives — the requester's prediction was right, for the wrong reason.** They warned rotationally-symmetric styles (`Circle`/`Square`/`Cross`) would fail a naive bytes-changed assertion. Correct conclusion, wrong cause: a button's content stream is never drawn turned — it's drawn upright and `/Matrix` turns the XObject — so the authored bytes change only when the box changes SIZE (90/270 **and** `w ≠ h`). At 180° every style is byte-identical; a square `Check` box is byte-identical at 90°, an oblong `Circle` box is not — opposite of the style-keyed prediction in both directions. Two of this Pass's own first-draft tests failed against a working fix before this was understood.
- **Second defect, same request:** `appearance_stale`'s sentence enumerated stale cases BY NAME ("a push button's caption artwork" first) instead of by the deciding PROPERTY (is the artwork pdfcer's own?) — backwards for buttons since this Pass, and silent about a foreign check box. `pdfcer-gui` had built a capability inventory from the wording and recorded the wrong answer for check boxes and radios. Now names the property.
- **Consequence, disclosed:** a widget rotated by an earlier pdfcer build now reads as foreign and `rotate_widget` discloses rather than redraws it; rotating again from this build corrects it.
- **Two new `D:\dev\rag\rust\` files this session** (shared with `Pass 308.4` below — see that entry's findings for the second one): `a_regeneration_success_flag_that_reads_fewer_staged_inputs_than_it_writes_still_reports_full_success.md` — the general mechanism: a regeneration boolean answers "did I produce bytes," never "did those bytes reflect every staged input," found on two different keys (`/Q`, button rotation) six hours apart. The disclosure-enumeration finding above is folded into the same file as a second section, per the project's practice of not minting a standing rule for a shared moral at n=1.
- `docs/ROADMAP.md`: new `Pass 308.5` Shipped entry added at top, above `Pass 308.4`'s (this session's other Pass). `docs/FEATURES.md`: Rotate-a-widget row corrected to name the deciding property instead of "a push button's caption artwork," and to record that a pdfcer-drawn button now redraws.
- `D:\Dev\FeatureRequests\pdfce_FeatureRequests\INDEX.md` gained a `G023` row, newest-first, above the `G022` row. A new Backlog item filed: an audit of every `FieldEdit`/`WidgetEdit` property against whether its regeneration path actually reads it (`pdfcer-gui`'s own recommendation).

**Still in flight:** owed items unchanged from the 561st filing below; the new Backlog audit item is unscoped, no Pass ID.

**For next session:** consider scoping the `FieldEdit`/`WidgetEdit` audit into a real Pass — `pdfcer-gui` predicts it will find more of the same shape.

**Sourcing (hard rule 8).** No shell tool this filing. `.git/COMMIT_EDITMSG` (HEAD's full message) and `.git/logs/HEAD` (the reflog) were read directly, confirming HEAD is `20e539a2` with this exact title/body and confirming its position after `503ad9d4` (`Pass 308.4`) and `5d43d2ea` (`Pass 308.3`'s filing). **Independently confirmed via `Read`/`Grep` against live source at HEAD:** `struct ButtonLook`, `fn build_button_states`, `quarter_turn_matrix` present in `crates/pdfcer-core/src/edit.rs`; `button_rotation_bakes.rs` contains exactly 9 `#[test]` functions; `docs/core-api/02-editing-and-saving.md:3243` carries the cited heading; `docs/core-api/index.md`'s count row already reads 5,497 lines / 183 clauses.

## 2026-09-16 (561st filing) — `Pass 308.4` (`503ad9d4`): `/Q` now REDRAWS, not merely records; clearing it means INHERIT, not left-align; `G022` closed

**Shipped:** `Pass 308.4` (`503ad9d4`), a new Pass minted this filing (family now `308.0`–`308.4`, next free family `309` unchanged). Answers `pdfcer-gui`'s `G022`: *"quadding is written but never redrawn."* `edit_field`'s `layout_changed` gate never checked `edit.quadding.is_some()`, so a quadding-only edit never redrew (reported `appearance_regenerated: false`, honestly, but with no variant able to say *recorded, not painted*). Gating alone would not have been enough: the appearance engine reads `field.quadding` off a PRE-COMMAND snapshot, so a naive fix would have re-baked the OLD justification while reporting success. Both are fixed: the gate now includes quadding, and the engine reads the just-staged value.

**Decisions made this session:** none — a redraw-completeness bug fix plus a citation correction, not a crate-boundary/library/invariant call. No `ARCHITECTURE.md` §12 entry.

**Findings + decisions:**
- **`/Q` is INHERITABLE** (§12.7.3.2: own → ancestors → `/AcroForm` → default) — a fact neither the original request nor this Pass's first draft had. `clearing_quadding()` means *inherit again*, not *left-align*; resolving a removal to `Quadding::default()` would left-align a field under a parent stating centred. New private `EditSession::inherited_quadding`, which duplicates `forms.rs`'s own precedence walk — a real, documented cost.
- **★★ The testing finding.** The naturally-suggested test (compare rendered x-offsets between two quaddings) PASSED against the deliberately-broken build: a one-step-stale read shifts both samples in the comparison by the same amount, and their relative ORDER survives the shift. ⇒ *An off-by-one that shifts every sample preserves every comparison BETWEEN samples.* Replaced with a width-free absolute assertion (box geometry minus measured string width, pinned to no other sample). Confirmed by sabotage: the relative test stayed green against the reverted fix; the absolute one did not. Filed as its own general-methodology finding: `D:\dev\rag\rust\a_relative_comparison_between_samples_cannot_catch_an_offset_that_shifts_every_sample_by_the_same_amount.md`.
- **Citation corrected, eight places:** `/Q` is §12.7.3.**3** Table **222**, not §12.7.**4**.3 Table **233** (the signature-field `/Lock` dictionary — a real table about something else, which is how the error survived). Fixed in `EditError::QuaddingInvalid`'s message and both CLI help texts; verified against the spec RAG before changing anything.
- **New `D:\dev\rag\rust\` file** `a_regeneration_success_flag_that_reads_fewer_staged_inputs_than_it_writes_still_reports_full_success.md` — shared with `Pass 308.5` above (same mechanism, six hours apart, on a different key).
- `docs/ROADMAP.md`: new `Pass 308.4` Shipped entry added, above `Pass 308.3`. `docs/FEATURES.md`: field-property row (line 313) gained a clause on redraw + inheritance; re-measured under the 1,200-character cap after editing.
- `D:\Dev\FeatureRequests\pdfce_FeatureRequests\INDEX.md` gained a `G022` row, newest-first, above the `G021` row.

**Still in flight:** owed items 5, 14, 34, 36 unchanged; new Backlog item (unscoped, no Pass ID) — audit every `FieldEdit`/`WidgetEdit` property against whether regeneration actually reads it, filed with `Pass 308.5`'s entry above.

**For next session:** see `Pass 308.5`'s entry above (same session) — the two Passes are the same defect on two keys and belong read together.

**Sourcing (hard rule 8).** No shell tool this filing. `.git/logs/HEAD` (the reflog) was read directly, confirming `503ad9d4`'s exact commit subject and its position in the sequence `5d43d2ea` → `503ad9d4` → `20e539a2`. Its full commit body could not be read the same way `Pass 308.5`'s was (not `HEAD`; its loose git object is zlib-compressed) — the reasoning above is taken from the dispatching engineer's own report. **Independently confirmed via `Read`/`Grep` against live source at HEAD:** `fn inherited_quadding`, `EditError::QuaddingInvalid` present in `crates/pdfcer-core/src/edit.rs` with the corrected `§12.7.3.3 Table 222` citation confirmed by direct read; `quadding_redraws.rs` contains exactly 8 `#[test]` functions; `docs/core-api/02-editing-and-saving.md:3206` carries the cited heading.

## 2026-09-15 (560th filing) — `Pass 308.3` (`20bfb259`): colour REMOVAL — `MkColorEdit::{Set,Remove}` replaces the overloaded `Option<MkColor>`; `G021` closed; a stale "does not paint" survivor found and flagged, not fixed here

**Shipped:** `Pass 308.3` (`20bfb259`), a new Pass minted this filing (family now `308.0`–`308.3`, next free family `309` unchanged). Answers `pdfcer-gui`'s `G021`, a same-day follow-up to `G020`: *"a `/MK` colour can be set and changed, but never removed."* `WidgetEdit::background`/`::border_color` change type from `Option<MkColor>` to `Option<MkColorEdit>` (`MkColorEdit { Set(MkColor), Remove }`); `with_background`/`with_border_color` keep their exact signatures (now `const fn`), `without_background()`/`without_border_color()` are new, `MkColorEdit::resolved() -> Option<MkColor>` is the single reader shared by the dictionary writer and the regenerator. CLI: `edit-widget --background unset`/`--border-color unset` via new `parse_mk_colour_edit`. The five `add-*` creation verbs get no `unset`. 6 tests added to `widget_colour_at_creation.rs` (16 → 22), 3 to `widget_properties.rs` (→ 11).

**Decisions made this session:** none — a type fix plus a CLI flag, not a crate-boundary/library/invariant call, same call as `Pass 308.0`/`308.1`/`308.2`. No `ARCHITECTURE.md` §12 entry.

**Findings + decisions:**
- **The bug was in a type, not in logic.** `/MK` `/BG` has three reachable states (absent, empty array, colour); the read model distinguished all three, the write model's `Option<MkColor>` had already spent its `None` on "this edit does not mention the key," so the third state had no spelling. Third instance of read/write asymmetry on `/BG`/`/BC` (after `Pass 249.1`/`262.2`), and the first where both directions existed and one *state* did not. Filed as a third instance (existing file, no new one) on `D:\dev\rag\rust\a_revision_gated_reserved_field_needs_option_not_a_default_bool.md` — widening its trigger set to "an `Option` already claimed for one meaning cannot carry a distinct third state; the fix is a named enum, not `Option<Option<T>>`."
- Requester's preferred fix (`with_background(Option<MkColor>)`, i.e. `Option<Option<MkColor>>`) declined by name, with reasons, in favour of `MkColorEdit`; a thin `with_background_state` wrapper offered conditionally if the pattern recurs elsewhere in their tree.
- **Second instance in two days** of a requester declining an available workaround (writing `/MK` directly, bypassing `EditSession`) and reporting the refusal instead (decision 058) — first was `G017`'s `move_text_run` report (`Pass 305.0`). Recorded as a dated instance, not a new rule.
- A removal regenerates the appearance and falls back to the builder's own default — removing `/BG` from a push button restores the plate grey. `/BC` remains "only a byte" (absent/empty both resolve to black in `WidgetChrome::stroke`) until any default there stops being black.
- Stale "honest limit" doc paragraph on `WidgetEdit::border_color` (claiming pdfcer's renderer does not paint `/MK`, superseded by `Pass 308.0`) retired in this commit.
- **⚠ Sweep found a live survivor, not fixed here:** `docs/core-api/01-reading-and-model.md:2581-2582` still asserts *"pdfcer's own renderer does not paint `/MK` colours (R43, named-not-painted)"* in the `forms::Widget` `/MK` colour-pair section — false since `Pass 308.0`. Flagged to the engineer as owed work (that file is the engineer's own contract doc, not this role's to edit); **the engineer closed it on receipt, in this same commit** — struck in place with the correction beside it, and `docs/core-api/index.md`'s line count moved with it. No other survivors found sweeping `ROADMAP.md`/`FEATURES.md`/`ARCHITECTURE.md`. ⇒ *The hand-off worked: the role that may not edit the file found the claim, and the role that owns it fixed it before either was committed.*
- `docs/ROADMAP.md`: new `Pass 308.3` Shipped entry added at top. `docs/FEATURES.md`: field-property row (line 312) and the `/MK`-painting Planned row (line 463) both updated to mention removal; both re-measured under the 1,200-character cap after editing (neither was already close enough to the cap to risk it, confirmed by grep, not assumed).
- `D:\Dev\FeatureRequests\pdfce_FeatureRequests\INDEX.md` gained a `G021` row, newest-first, above the `G020` rows.

**Still in flight:** owed items 5, 14, 34, 36 (unchanged); new owed-to-engineer item — the `01-reading-and-model.md` survivor above, not tracked in this project's own owed-item ledger since it's the engineer's document.

**For next session:** correct the `01-reading-and-model.md` survivor; dispatch `pdfcer-spec-librarian` on owed item 36 if not already done; **`docs/NEXT_SESSION.md`'s "full sweep works now" framing should be corrected to "coin-flip, split procedure is the dependable fallback"** — this filing's own gate run stalled on `cargo test --workspace` under memory pressure hours after two clean sweeps.

**Sourcing (hard rule 8) — no shell this filing.** Commit hash `20bfb259` and its full reasoning taken from `.git/COMMIT_EDITMSG` (read directly) and the requesting engineer's own dispatch; push status and gate-run details relayed, not independently re-run — no shell available to this filing. **Independently confirmed via `Read`/`Grep` against live source at HEAD:** `enum MkColorEdit`, `fn resolved`, `fn without_background`, `fn without_border_color` present in `crates/pdfcer-core/src/edit.rs`; `fn parse_mk_colour_edit` present in `crates/pdfcer-cli/src/main.rs`; `widget_colour_at_creation.rs` contains exactly 22 `#[test]` functions, `widget_properties.rs` exactly 11; `docs/core-api/02-editing-and-saving.md:3167` carries the cited section naming `Pass 308.3`/`G021`; `docs/core-api/index.md`'s count row already reads 5,408 lines; `docs/core-api/01-reading-and-model.md:2581-2582` confirmed still carrying the stale claim, by direct read.

## 2026-09-15 (559th filing) — `Pass 308.1` (`86ede66b`): colour at field-CREATION time on all five `New*` widget specs; `G020` fully closed; a phantom `/MK` key found and retired; second instance filed to an existing `D:\dev\rag\rust\` lesson

**Shipped:** `Pass 308.1` (`86ede66b`), the last owed third of `G020` (O202: *"the forms objects have no way to edit their colour before or after placement"*). All five creation specs (`NewTextField`, `NewCheckBox`, `NewRadioButton`, `NewChoiceField`, `NewPushButton`) gain `pub chrome: annot_author::WidgetChrome` plus `with_background`/`with_border_color`; new `creation_chrome()`/`push_button_creation_chrome()`/`insert_mk_chrome` helpers in `edit.rs`; every `add_*` verb hands one `WidgetChrome` to both the appearance builder and `/MK`. CLI: `--background`/`--border-color` on all five `add-*` verbs, `list-fields --widgets` prints both fields, shared `parse_creation_chrome`/`CreationChrome`/`mk_colour_token`. **`G020` is now fully closed** — `Pass 308.0`/`Pass 308.2` (`bd8059f2`, earlier the same day) shipped the edit-time half.

**Decisions made this session:** none — a bug fix (a `/MK` key describing artwork never drawn) plus a builder-parameter threading, not a crate-boundary/library/invariant call, same call as `Pass 308.0`/`Pass 308.2`. No `ARCHITECTURE.md` §12 entry.

**Findings + decisions:**
- `add_text_field`/`add_choice_field` always wrote `/MK /BC [0 0 0]` while drawing no frame — harmless until `Pass 308.0` made `edit_widget` regenerate FROM `/MK`, which would have materialised a frame on a widget's first resize that had never existed. Fixed by retiring the phantom key rather than painting the frame, preserving the stated invariant that a text field draws no box by default. Filed as a **second, inverse-direction instance** on the existing lesson (no new file): `D:\dev\rag\rust\two_representations_of_one_fact_can_disagree_until_a_third_thing_derives_one_from_the_other.md` — `Pass 308.0` hit it from the paint side (RGB `/MK` vs. gray artwork), `Pass 308.1` from the record side (a key describing artwork never drawn); both trace to writing a dictionary and a stream from two literals instead of one value.
- Considered and rejected: painting Acrobat's documented creation-floor border instead of retiring the key. Rejected because it would repaint every pdfcer-created text field for a change nobody asked for.
- `EditWidgetArgs`'s doc comment had been spliced onto `parse_mk_colour`'s in `main.rs` — the exact failure `check-doc-block-spliced.py` exists for, in a file that gate doesn't scan. Unspliced; gate-coverage gap noted, not chased.
- `docs/ROADMAP.md`: new `Pass 308.1` Shipped entry added at top; the `Pass 308.0`/`Pass 308.2` entry's "still owed" clause and its ledger row struck-through and dated-corrected in place (append-only: old text kept legible, correction appended) rather than silently rewritten. `Pass 308.1`'s *Next up* entry deleted outright (shipped entries leave no remnant pointer).
- `docs/FEATURES.md`: field-property row (line 312) and the `/MK`-painting Planned row (line 463) both updated — `G020` closed, the painting row stays *Planned* pending `pdfcer-gui` wiring only.

**Still in flight:** owed items 5, 14, 34, 36 (unchanged); `G020` fully closed, nothing of it remains queued. `pdfcer-gui`'s `INDEX.md` row for the delivery notice is owed to their tree, not this project's.

**For next session:** dispatch `pdfcer-spec-librarian` on owed item 36 (erratum #56 sourcing) if not already done; no other front-of-queue item changed by this filing.

**Sourcing (hard rule 8) — no shell this filing.** Commit hash `86ede66b` and its push status taken from the requesting engineer's own dispatch, not independently confirmed via `git log`/`git show` — no shell available to this filing. **Independently confirmed via `Read`/`Grep` against live source at HEAD:** `WidgetChrome::with_background`/`with_border_color`, `creation_chrome`, `push_button_creation_chrome`, `insert_mk_chrome` present in `crates/pdfcer-core/src/edit.rs`; `crates/pdfcer-core/tests/widget_colour_at_creation.rs` exists with exactly 16 `#[test]` functions; `crates/pdfcer-cli/tests/add_fields.rs` contains the background/border-color flag tests; `parse_creation_chrome`/`CreationChrome`/`mk_colour_token` present in `crates/pdfcer-cli/src/main.rs`; `docs/core-api/index.md`'s `02-editing-and-saving.md` row already reads 5,369 lines, matching the dispatch's claim.

## 2026-09-15 (558th filing) — `Pass 308.0` + `Pass 308.2` (`bd8059f2`): `/MK` `/BG`/`/BC` are baked into the `/AP` the four widget builders draw, not merely round-tripped; `Pass 308.1` (creation-time colour) still owed; two general findings sent to `D:\dev\rag\rust\`, one third dated instance added to an existing lesson

**Shipped:** `Pass 308.0` + `Pass 308.2` (`bd8059f2`), the first two of `G020`'s three Passes. `annot_author::WidgetChrome` threaded through all four appearance builders; `edit_widget`'s `needs_regen` covers both colour fields; `PendingWidgetEdit` carries staged chrome for the redraw while the ownership test reads the widget's stored colours; new `AppearanceOutcome` (`NotNeeded` / `Regenerated` / `RecordedNotPainted(String)`) on `WidgetEditOutcome::appearance`. Eleven new tests (`crates/pdfcer-core/tests/widget_colour_appearance.rs`, independently confirmed present with exactly 11 `#[test]` fns). `docs/core-api/02-editing-and-saving.md` gained a section; verb count unchanged. **`Pass 308.1` (colour carried at field-creation time) is still owed** and stays in *Next up* — say so wherever `308.0`/`308.2` are cited, since today's created-field colour lands via create-then-edit, not the operator's stated "before placement."

**Decisions made this session:** none — a bug fix plus two builder-default corrections and a formatting-precision fix, not a crate-boundary/library/invariant call. No `ARCHITECTURE.md` §12 entry.

**Findings + decisions:**
- A pre-existing mismatch (push-button creation wrote `/MK` `/BG`/`/BC` as DeviceRGB while the plate was always painted DeviceGray) had been harmless for months and became load-bearing the moment `308.0`'s ownership test started comparing the two — an older-built push button now discloses `RecordedNotPainted` rather than being silently repainted. Filed as a general finding, not a project decision: `D:\dev\rag\rust\two_representations_of_one_fact_can_disagree_until_a_third_thing_derives_one_from_the_other.md` (new file).
- The four builders' correct `None` defaults point in opposite directions (a text field draws no box; a push button still draws its plate grey) — both traps live inside the builder, and the test file asserts the unchanged half as hard as the changed half. Filed with a testing-discipline corollary ("threading a new parameter through a builder owes a test that its absence reproduces pre-change output byte-for-byte"): `D:\dev\rag\rust\a_builder_parameter_threaded_through_an_existing_call_site_owes_an_absence_is_unchanged_test.md` (new file).
- `MkColor`'s `f32` components were about to be written as `f64::from(component)`'s own 17-digit shortest-round-trip string; fixed by formatting at the original `f32` width. Filed as a THIRD dated instance on the existing lesson (widening, not arithmetic, is the mechanism this time): `D:\dev\rag\rust\shortest_roundtrip_float_format_needs_derived_value_rounding.md`.
- `docs/core-api/02-editing-and-saving.md`'s `rotate_widget` section cites PDF Association erratum #56 as adding `/MK` to §12.5.2's ignore-list; `pdfcer-gui` read the same claim and could not source it. Left as written (a sourced claim is not deleted because a third party couldn't find it) but opened as **new owed item 36**: dispatch `pdfcer-spec-librarian` to verify it.
- `docs/FEATURES.md`: Forms field-property row corrected (colour is now painted, `Pass 308.1` flagged separately owed); the *Planned* row for this capability ticks `core`/`cli`, stays in *Planned* pending `308.1` + `pdfcer-gui` wiring.

**Still in flight:** owed items 5, 14, 34, **36** (new); `Pass 308.1` is now the sole remainder of `G020` and the front of the *Next up* queue.

**For next session:** build `Pass 308.1` (creation-time colour for the five `New*` field specs); separately, dispatch `pdfcer-spec-librarian` on owed item 36 (erratum #56 sourcing).

**Sourcing (hard rule 8) — no shell this filing.** Commit hash `bd8059f2` and its push status taken from the requesting engineer's own dispatch, not independently confirmed via `git log`/`git show` — no shell available to this filing. **Independently confirmed via `Read`/`Grep` against live source at HEAD:** `WidgetChrome`/`RecordedNotPainted`/`AppearanceOutcome` present in `crates/pdfcer-core/src/edit.rs` and `annot_author.rs`; `crates/pdfcer-core/tests/widget_colour_appearance.rs` exists with exactly 11 `#[test]` functions, matching the dispatch's count; both edited `docs/FEATURES.md` rows confirmed under the 1,200-character cap (regex-measured, not assumed) before filing.

## 2026-09-15 (557th filing) — `Pass 307.0` (`729cf6db`): `EditSession::page_tab_sequence` computes a VISIT order for all six `/Tabs` states; decision 158 corrects the request's membership assumption; `R197` gains a dated instance; a doc-comment-splice gate blind spot recorded

**Shipped:** `Pass 307.0` (`729cf6db`). `EditSession::page_tab_sequence(page_index) -> TabSequence { order, stated, derived, notes }` answers `pdfcer-gui`'s `G019` (O204: tabbing a form reached the shell's own menus). `/A`/`/W` read off the array; `/R`/`/C` compute from `/Rect` geometry with `/Rotate` and `/ViewerPreferences /Direction` applied; `/S` derives from the structure tree or returns an empty sequence with a note rather than guessing; `Absent`/unknown `Other` fall back to `/Annots` order, disclosed by name (rule 4). Plus `TabOrderBasis` (6 variants), `TabExclusion` (4), `AnnotFlags::TOGGLE_NO_VIEW`, settings `widget_tab_tail`/`tab_row_tolerance`, CLI `pdfcer tab-order`, three synthetic fixtures under `fixtures/synthetic/tab-order/`, 23 core unit tests, 14 CLI black-box tests. `docs/core-api/02-editing-and-saving.md` gained three contract rows; verb count 234 → 239 (per the dispatch, not independently re-run this filing).

**Decisions made this session:** decision 158 (`ARCHITECTURE.md` §12) — the tab-visit sequence's membership rule follows §12.5.3's interaction bar (`Hidden`/`NoView` excluded, `TrapNet`/`Popup` excluded as pdfcer's own reading, `ToggleNoView` honoured as a reversal), correcting the request's own assumption of "every annotation, filtered by the caller." §12.5.1 itself defines no visit-order geometry at all — sourced from `pdfcer-spec-librarian`'s new `iso32000__ref__tab_order_derivation.md` (`TABD-*`) — so pdfcer's `/R`/`/C` grouping tolerance is a disclosed invention, not a spec reading.

**Findings + decisions:**
- `R197` ("a stated derivation is not a maintained derivation") gains a dated instance: `docs/core-api/index.md`'s line/clause-count citations of `03-capabilities.md` went stale the moment `0b48b3e2`/`2c199faa` (555th filing, `G018`) grew that file, and `main` read red on `check-core-api-verbs.py` until `729cf6db` re-derived both figures. Recorded as the control working as designed — the gate caught the drift on the first push after it happened — not as a new failure mode.
- A fourth-instance addendum filed to `D:\dev\rag\rust\doc_comments_concatenate_silently_so_a_moved_variant_orphans_two.md`: a struct-field-form recurrence (`crates/pdfcer-core/src/settings/mod.rs`, `quad_point_order`'s doc block welded onto `xref_entry_eol`, leaving the latter undocumented) that `check-doc-block-spliced.py` — the gate built specifically for this failure — also did not catch, because its one detection rule (the same rustdoc heading appearing twice in one block) does not fire on heading-free prose. Recorded as a gate blind spot, not chased to a fix this filing.
- `docs/FEATURES.md`'s *Planned* row for this capability ticks `core`/`cli`; stays in *Planned* pending `pdfcer-gui` wiring (`gui [ ]`).

**Still in flight:** unchanged — owed items 5, 14, 34; `Pass 308.0`–`308.2` (`G020`) remain the front of the *Next up* queue.

**For next session:** the `check-doc-block-spliced.py` blind spot on heading-free struct-field docs is unclaimed — no fix proposed, no rule minted.

**Sourcing (hard rule 8) — no shell this filing.** Account taken from the requesting engineer's own dispatch report of `Pass 307.0`'s shipped work, including the commit hash, test counts, and the four numbered findings. Not independently re-verified against live `crates/pdfcer-core/` source, commit contents, or gate results — no shell available to this filing; a session with one should confirm `729cf6db` before treating the code-level and count claims above as ground truth.

## 2026-09-15 (556th filing) — two inbound `pdfcer-gui` requests filed to `ROADMAP.md` *Next up*: `G019` (a derived tab-visit order) and `G020` (paint `/MK` `/BG`/`/BC`), split into `Pass 307.0` and `Pass 308.0`–`308.2`

**Shipped:** not a Pass — a filing dispatch. Two requests read in full from `pdfce_FeatureRequests/open/`, both against `pdfcer-core` at `0b48b3e2`/v0.53.0.

**Decisions made this session:** none — both requests ask for a fix already decided (R43 for `G020`; the engine-not-shell placement echoed from `array_order_governs`'s own precedent for `G019`), so no new `ARCHITECTURE.md` §12 entry.

**Findings + decisions:**
- `request_G019_no_derived_tab_order_for_Tabs_R_C_or_S.md` (O204: tabbing a form cycles the GUI's own menus instead of the page's widgets): `PageTabs` classifies all six `/Tabs` states and `array_order_governs` correctly answers `Nothing`/`Widgets`/`Everything`, but nothing in the crate computes the VISIT sequence those answers imply for `/R`/`/C`/`/S`/`Absent`/`Other`. Filed as `Pass 307.0` — `EditSession::page_tab_sequence(page_index) -> TabSequence { order, stated, derived, notes }`, one call covering all six states, rule-4 disclosed.
- `request_G020_MK_BG_and_BC_are_round_tripped_but_never_painted.md` (O202: *"the forms objects have no way to edit their colour before or after placement"*): `/MK` `/BG`/`/BC` read and write both shipped (`Pass 249.1`, `Pass 262.2`), but none of the four appearance builders paints either colour, and `edit_widget`'s `needs_regen` omits both fields — a colour-only edit reports `appearance_regenerated: false` with no way to say *recorded, not painted*. Split into `Pass 308.0` (bake the colour into the builders + wire `needs_regen`, must ship together), `Pass 308.1` (carry the colour at field-creation time), `Pass 308.2` (a third `WidgetEditOutcome` state for the inert case, may fold into `308.0`).
- Both requests invoke decision 058 (a shell reporting a workaround is a finding about pdfcer's own boundary, not a favour asked). `G020`'s framing states rule 4's own one-line test directly: painting `/MK` at display time rather than baking it into `/AP` would make the editing canvas differ from the same document saved and reopened.
- Existing `docs/FEATURES.md` row (field-property editing, Forms) corrected in place — it described background/border colour as an ordinary writable property with no caveat that painting is missing; now states "recorded, not painted" and points at `Pass 308.0`. Two new *Planned* rows added for the two new capabilities.

**Still in flight:** unchanged — owed items 5, 14, 34 (see `ROADMAP.md` *Next up*'s owed-work carry line and `docs/NEXT_SESSION.md`).

**For next session:** `Pass 307.0` and `Pass 308.0` are the front of the queue, in that order, per the engineer's own stated intent to build them this session.

**Sourcing (hard rule 8) — no shell this filing.** Both request files read in full via `Read` at `D:\Dev\FeatureRequests\pdfce_FeatureRequests\open\`. File:line citations in the new `ROADMAP.md`/`FEATURES.md` entries are copied verbatim from the requesting engineer's own grep/read results, not independently re-verified against live `crates/pdfcer-core/` source (no shell available to this filing).

## 2026-09-15 (555th filing) — engine commit `2c199faa`, doc-only: `docs/core-api/03-capabilities.md` §1 had quoted `FEATURES.md`'s fourth column (Acrobat) as its third (gui) for two shipped rows; no code changed, no Pass, no `FEATURES.md` change owed

**Shipped:** not a Pass — no code changed. `2c199faa` corrects `docs/core-api/03-capabilities.md` §1 (ce dimensions), which told every consuming session that the ce-dimension **style cascade** and **tolerance** were `core [x] · cli [x] · gui [ ]` and named them "the single largest ready-made opportunity in this document." Both have shipped in `pdfcer-gui` — eleven overridable properties in the Properties-panel editor, each with an override checkbox and a sentence naming which tier supplied the value in force; the dimension-group window sets the middle tier's five defaults. Also corrected in the same commit: the citation `FEATURES.md:103-104` had drifted onto an overprint/PCS paragraph (the real rows are `:248` style cascade, `:250` tolerance), and two other "`gui [ ]` panel" phrases in §1 now read as the recipe a new shell needs to rebuild a shipped panel, not as a gap.

**Decisions made this session:** none — a citation correction to a project document, not a crate-boundary/library/invariant decision. No `ARCHITECTURE.md` §12 entry.

**Findings + decisions:**
- **Root cause:** `FEATURES.md` rows carry **four** columns — core / cli / gui / **Acrobat** — and the corrected paragraph had quoted the fourth as the third. Every tick in the row was real; only which header the last one belonged to was wrong. A three-column reading of a four-column table produces a confident, specific, wrong `gui [ ]`, and fails silently — nothing downstream can tell "shipped, mis-shelved under the wrong header" from "actually unbuilt."
- **The shape is a repeat, not a new one.** The 2026-09-11 audit recorded in the same file's §11 already named these two rows by name as stale and corrected the **table**. The prose paragraph 3,400 lines above it — the more authoritative-looking of the two statements — was never re-read. That is the exact failure §11 itself already articulated (quoted in the correction, `docs/core-api/03-capabilities.md:117-118`): a correction landing on one mirror of a claim is a half-finished correction while a second mirror of the same claim goes unchecked. **Two independent readers got an off-by-one-row/column wrong on the same table in the same week** — evidence about the table's legibility, not about either reader.
- **One claim in the originating report (G018 §3) ran the other way and is recorded rather than dropped:** it asked whether the style cascade should be made separately trackable, reading it as having "no row of its own" in `FEATURES.md`. It has had one all along — `FEATURES.md:248` — with its own `gui [x]`. The report had read `:250`'s "inheriting through the style cascade like any other property" (tolerance's row pointing *at* the cascade) as the cascade's only mention. Same off-by-one-row shape as the defect it caught, one row up.
- **Verified independently this filing** (not taken on the dispatch's characterization alone): read `FEATURES.md:240-254` directly. Row `:248` (style cascade) reads `[x] | [x] | [x] | ?` and row `:250` (tolerance) reads `[x] | [x] | [x] | **[ ]**` against the header `| core | cli | gui | Acrobat |` — confirms gui is ticked on both, Acrobat is the unticked/unknown one. **No `FEATURES.md` change is owed by this filing** — stated explicitly per the dispatch's instruction not to leave it silent.
- **Standing-rules index checked, no mint:** `R246` ("a correction is not complete until it reaches every corpus this project *reads*, not merely every tree it *writes*") is adjacent in shape but scoped to external corpora, not a document's own internal mirrors. No existing rule covers "count the columns before quoting a table row." This is recorded as an **n=2 candidate** (the 2026-09-11 audit's table-fix, now this prose-fix, same table, same week) rather than minted — consistent with this project's own practice of flagging low-n patterns (e.g. `R247`'s multi-filing reserved-but-unclaimed period) before committing a number to them. If a third instance of this specific shape (four-column `FEATURES.md` row mis-cited by column) turns up, it should mint rather than accrue a third silent dated note.
- Channel exchange closed: request + reply archived as `2026-09-15-G018-core-api-03-marks-ce-dimension-tolerance-as-gui-unbuilt-{request,reply}.md`, `INDEX.md` row added (per the dispatch; not independently re-verified against the FeatureRequests channel directory this filing — no shell).

**Still in flight:** unchanged — owed items 5, 14, 34.

**For next session:** the n=2 four-column-mis-citation candidate above is unclaimed; the `pdfcer-render` f32 text-matrix Backlog item (551st/554th filings) remains unscoped.

**Sourcing (hard rule 8) — no shell this filing.** Account taken from the requesting session's own report (commit `2c199faa` cited, not independently confirmed via `git log` — no shell tool available to this filing). Independently confirmed against live source via `Read`: `docs/core-api/03-capabilities.md` lines 100-144 (the correction block, already landed at HEAD) and `docs/FEATURES.md` lines 240-254 (rows `:248`/`:250` and the four-column header, confirming both are already `gui [x]`). The FeatureRequests channel archive/`INDEX.md` row was not independently checked — relayed from the dispatch only.

## 2026-09-15 (554th filing) — `Pass 306.0`: `split_text_object` cuts one `BT`…`ET` into several; a same-session defect fix stops a same-baseline re-anchor being dragged as a follower; stale `next free R257` ledger figure corrected

**Shipped:** `Pass 306.0` (no commit hash supplied to this filing) — `split_text_object`/`text_object_split_plan`, CLI `text-object-split --granularity run|line`. Cuts a `BT`…`ET` by inserting `ET BT <run's own Tm>` before each cut; nothing else moves, because `BT`/`ET` reset only `Tm`/`Tlm`. Also shipped, no Pass ID: a fix to `text_edit::edit::reposition_followers`, which treated a same-baseline re-anchor placed BEHIND the edited run (a SolidWorks note's bullet, written after its text and to the left) as that line's tail and dragged it sideways.

**Decisions made this session:** decision 157 (`ARCHITECTURE.md` §12) — a cost recorded in three sites of this crate as a reason not to split a text object was never priced; `BT`/`ET` reset only `Tm`/`Tlm`, so the split costs one restated operator, no preamble, no `restore_ops`.

**Findings + decisions:**
- The follower-repositioning defect survived because the walk COMPENSATED the next line — everything downstream of the damage stayed exactly where the producer put it, so only a byte-level assertion on the moved operator catches it, not a geometry-only one.
- `same_line` answers "same baseline?"; its caller had been reading that as "is the continuation of the line?" — a predicate named for a cheap geometric fact was asked an expensive ordering question it was never built to answer.
- Third instance of the doc-comment-concatenation shape (enum variant, struct field, now a bare private function): `same_line`'s doc comment had welded onto `reposition_followers`; `check-public-fns-documented.py` cannot see this form, since it only covers `pub` items.
- A written claim ("the split renders bit-identically") was tested and was FALSE for a deep `Td` chain: `pdfcer-render`'s text matrix is f32 where its CTM is already f64 (decisions 081/151) — flagged to `ROADMAP.md` *Backlog* as its own item, not fixed this Pass. Differing pixels were scattered over the WHOLE sheet, not localised at the cut, which is what said "precision", not "structural error."
- **Ledger correction (hard rule 8):** the "next free `R257`" figure carried in every ledger table's *before* column since at least the 519th filing was stale — `R257` was minted at the 547th filing and has two dated instances since. Corrected to `R257` used, next free `R258`, measured by grepping this file's own *Standing rules* section rather than trusting the carried figure.
- Written to `C:\personal_rag\pdf\`: a numbered-note producer (SolidWorks) writes its bullet after its text, to the left, on the same baseline — same-baseline is not the same fact as line-continuation order. Written to `D:\dev\rag\rust\`: the f32/f64 text-matrix asymmetry (new file, cross-referenced against the existing CTM-precision finding), a dated addendum to the doc-comment-concatenation file (function form), and a new file on the same-baseline-vs-continuation predicate shape.

**Still in flight:** unchanged — owed items 5, 14, 34.

**For next session:** the `pdfcer-render` f32 text-matrix Backlog item (above) is unscoped and unclaimed.

**Sourcing (hard rule 8) — no shell this filing.** Account taken from the requesting engineer's own conversational report, itself sourced from the operator's direct diagnosis on his own working file (`SW41177.pdf`). Independently confirmed against live source via `Read`/`Grep`: `crates/pdfcer-core/src/text_edit/edit.rs` (`FOLLOWER_ORIGIN_EPSILON`, `re_anchors_before_anchor`, both named tests), `crates/pdfcer-core/src/vector/edit.rs`/`edit.rs`/`vector/mod.rs`/`tests/text_object_split.rs` (all five new error variants, `split_text_object`), and `crates/pdfcer-cli/src/main.rs` (`text-object-split`, its doc comments already citing `Pass 306.0`). No commit hash supplied; push/CI state not checked — no shell available to this filing.

## 2026-09-15 (553rd filing) — `main` red ~10 h on a documentation-only gate; two pushes landed on it without reading CI's colour; `R217` gains an 8th amendment note; `Pass 305.0`'s hash backfilled

**Shipped:** not a Pass. `96958657` restores `main` to green — it carries `Pass 305.0`'s previously-deferred code commit (hash now recorded against that entry, below) together with a trim of two `docs/FEATURES.md` rows found over `check-register-entry-size.py`'s 1,200-character cap (the reflow row was 1,223 characters, not carried in the baseline).

**Decisions made this session:** none new architecturally — a trim fix and a process finding, no crate-boundary or invariant change; no `ARCHITECTURE.md` §12 entry.

**Findings + decisions:**
- `main` was red from 2026-09-14 18:36Z to 2026-09-15 04:14Z on `check-register-entry-size.py` alone — no code involved. `Pass 304.0`'s push (`2a862742`, 02:37Z) landed on that red without reading CI's colour first and inherited it rather than caused it.
- `R217`'s 8th amendment note (*Standing rules*, `ROADMAP.md`): the rule's mechanism — read CI's colour from GitHub, don't infer it — fired through a gate (`check-register-entry-size.py`) other than the filing gate (`check-commits-filed.py`) the rule was minted for, on a documentation-only edit. No new rule number.
- The fix was a TRIM, not a baseline bump, on the principle that the baseline is debt and the direction is down; both over-cap rows were shortened by deleting reasoning that already lived in `ROADMAP.md`/the commit message, and no fact was lost.
- A separate docs-only commit, `docs: record the ten hours main was red, and what the two pushes onto it missed`, wrote the `docs/NEXT_SESSION.md` section this filing draws from; its hash was not supplied to this filing and is not independently verified here.

**Still in flight:** unchanged — owed items 5, 14, 34.

**For next session:** none opened.

**Sourcing (hard rule 8) — no shell this filing.** Incident account and hash `96958657` confirmed by `Read` against the live `docs/NEXT_SESSION.md` file, not taken on the operator's dispatch alone. `git log --format=%H -1 96958657` not run — no shell tool available to this filing; a session with one should confirm.

## 2026-09-14 (552nd filing) — `Pass 305.0` (`G017`, `96958657`): `move_text_run` and its in-form twin complete the text-run family; three missing `*_in_form` deletes closed in the same Pass

**Shipped:** `Pass 305.0` (`96958657`). `move_text_run`/`move_text_run_in_form` give text runs the move verb every other part kind (subpath, node) already had. Three placement paths per run: rewrite a `Tm`'s `e`/`f`, rewrite a `Td`'s `tx`/`ty`, or INSERT a `Td` (disclosed) for `TD`/`T*`/`'`/`"`/implicit-`BT`-origin runs — `TD` is deliberately never treated as an ordinary relative pair, because Table 108 defines it as `−ty TL` then `tx ty Td`, so rewriting its `ty` would silently re-space every later `T*`. The successor run is compensated; `RunPositioning::Inherited` is refused on either side (`TextRunHasNoPositionOfItsOwn`/`MoveWouldMoveNextRun`), citing decision 027's posture rather than a new decision.

Taken together with the request's second row: the `*_in_form` family had five moves and one delete, an asymmetry running the OPPOSITE way from page content — `R245`'s 10th dated instance. `delete_text_run_in_form`, `delete_subpath_in_form`, `delete_node_in_form` shipped alongside `move_text_run_in_form`. Ten `*_in_form` verbs now, not six.

**Decisions made this session:** none new architecturally. The refuse-vs-compensate split cites existing decision 027 (refuse what has no good reading) rather than a new decision — no `ARCHITECTURE.md` §12 entry.

**Findings + decisions:**
- `TextRun` gained `text_matrix: Matrix` (the `Tm` at the run's origin) — a text drag crosses both the CTM and the run's own text matrix (§9.4.4), and a fix converting through the CTM only is wrong specifically on rotated text; pinned by new fixture `runs-rotated-td.pdf`.
- New fixtures: `text/runs-td-relative.pdf`, `text/runs-tstar-leading.pdf`, `text/runs-rotated-td.pdf`, `forms-xobject/title-block-form.pdf` (first form fixture carrying text; first three-anchor polyline in that directory, needed because a rectangle's corners aren't node-editable).
- One real defect found and fixed mid-work: a token-gap search used an off-by-one (`tokens.end + 1` against an exclusive bound), misclassifying some `Td`/`Tm` runs as opaque — geometry came out right, bytes came out long; caught only because byte-shape assertions ran beside the geometry ones.
- Written to `C:\personal_rag\pdf\` as a new empirical finding (`lesson_20260914_td_conflates_leading_and_position_so_rewriting_its_ty_silently_respaces_every_later_tstar.md`) — a spec-trap-meets-real-producer shape (CAD title blocks routinely use `TD` between lines), not a Rust/egui-ecosystem one.
- `docs/FEATURES.md`: new "Move one text run…" row; "Edit geometry INSIDE a form XObject" row corrected in three places (verb count, text-in-form editability, and the CLI-caller split re-measured against `crates/pdfcer-cli/src/main.rs` at six of ten lacking a caller, not four of six as previously stated).

**Still in flight:** unchanged — owed items 5, 14, 34.

**For next session:** none opened.

**Sourcing (hard rule 8) — no shell this filing.** Full account taken from the requesting engineer's own reply document (`reply_G017_move_text_run_SHIPPED_with_its_in_form_twin_and_the_three_missing_deletes.md`). Independently confirmed against the live working tree via `Read`/`Grep`: `crates/pdfcer-core/src/edit.rs`, `crates/pdfcer-core/src/vector/edit.rs`, `crates/pdfcer-core/src/vector/decompose.rs`, the four named fixtures, and `crates/pdfcer-core/tests/text_run_move.rs` all present at HEAD. The FEATURES.md CLI-caller count was re-measured independently against `crates/pdfcer-cli/src/main.rs`, not copied from the reply's own arithmetic. Test/clippy/fmt results relayed from the requesting engineer's report, not independently re-run — no shell. **Commit hash `96958657` added 2026-09-15**, per the operator; not independently verified via `git log` (no shell this filing either) — confirm with `git log --format=%H -1 96958657` in a session that has one.

## 2026-09-14 (551st filing) — `Pass 304.0` (`2a862742`): one visual line spans across show operators again on a CAD file whose exporter restates `Tz` and nudges `Td`'s vertical between fragments

**Shipped:** `Pass 304.0` (`2a862742`). Operator-direct request (no topic key — not a `pdfce_FeatureRequests`/`iccce_FeatureRequests` exchange). SolidWorks (and CAD exporters generally) can write one visual line of note text as several show operators, each restating `Tz` and nudging `Td`'s vertical component by a float round-trip between fragments. `text_edit/edit.rs`'s `spannable` compared `Tz` with `==` and required `Td`'s `ty` to be exactly `0.0`, so the span-editing route refused text crossing a fragment boundary on such a file.

**Decisions made this session:** none new architecturally — a bug fix widening two measured comparison tolerances, no crate-boundary or invariant change; no `ARCHITECTURE.md` §12 entry.

**Findings + decisions:**
- Two named, measured tolerances: `SPAN_H_SCALE_TOLERANCE` (`0.001`, a ratio, on `Tz`) and `SPAN_LINE_DRIFT_TOLERANCE` (`0.01`, unscaled text units, on `Td`'s `ty`). Every other `spannable` comparison (font, size, MCID, char spacing, word spacing) stays exact.
- The vertical tolerance is the one that was got wrong on the first cut: `Td` translates the line matrix, so the observed drift is the producer's raw noise multiplied by the text matrix's y-scale. A flat comparison fixed one note (`0.00057 × 13.2 = 0.0075`) and left the note beside it (`0.00661 × 13.2 = 0.087`) still refusing — same producer, same page, different font size. `same_line` now scales the tolerance by `hypot(a[2], a[3])`, falling back to `1.0` for a degenerate matrix.
- Three regression tests pin it, including one confirming a real line break (`0 -1.72646 Td`, three orders of magnitude past the noise) still separates two lines.
- On the operator's own 36-sheet drawing set, all six balloon-bearing notes are now editable and survive save-and-reopen — previously every one refused with "not found in an editable run" about text plainly on the page.
- Written to `C:\personal_rag\pdf\` as a new empirical finding (`lesson_20260914_cad_exporters_restate_tz_and_perturb_tds_baseline_between_fragments_of_one_visual_line.md`) — a producer-divergence-from-spec shape, not a Rust/egui-ecosystem one.

**Still in flight:** unchanged — owed items 5, 14, 34.

**For next session:** none opened.

**Sourcing (hard rule 8) — no shell this filing.** Commit hash and the full account above taken from the requesting engineer's report. Independently confirmed against live source via `Read`/`Grep`: `crates/pdfcer-core/src/text_edit/edit.rs` — `SPAN_H_SCALE_TOLERANCE`/`SPAN_LINE_DRIFT_TOLERANCE` constants and doc comments, `spannable`'s `Tz`-tolerance comparison, and `same_line`'s `hypot(a[2], a[3])`-scaled drift comparison all match the quoted reasoning verbatim. Push/CI state not independently checked — no shell.

## 2026-09-14 (550th filing) — `pdfcer-gui` KEEPS `ReflowApplyError::PageEditedThisSession` (closes `G015`/`G016`); the construct-vs-receive distinction sharpens `naming_every_field_of_a_tracked_upstream_struct...md`

**Shipped:** nothing — librarian-only, no Pass. Records `42b47f30`, a doc-comment-only commit in `pdfcer-core` (no code-behavior change): `pdfcer-gui` consumed `G016` and answered the question `G015` put to them — whether to delete the now-unconstructed `ReflowApplyError::PageEditedThisSession` variant from their exhaustive `match`.

**Decisions made this session:** none new architecturally — this is `pdfcer-gui`'s own call about its own consuming code, not a `pdfcer-core` boundary redraw, so no `ARCHITECTURE.md` §12 entry. Appended a dated refinement instead to `D:\dev\rag\rust\naming_every_field_of_a_tracked_upstream_struct_makes_a_new_field_a_compile_error_instead_of_silence.md`, whose existing generalisation ("a loud break beats a quiet default at a boundary you are tracking") never said when the loud break stops being the right trade — this supplies the missing half.

**Findings + decisions:**
- `pdfcer-gui` kept the variant, reasoning entered directly into `pdfcer-core`'s own doc comment (`crates/pdfcer-core/src/text_edit/reflow_apply.rs:282-300`): (1) their `match` is compiler-proved exhaustive, so replacing the arm with `unreachable!()` would turn a *future* guard reinstatement into a panic on the one path whose job is failing safely; (2) removal's only benefit is a one-time compile break telling them something their own completeness gate already re-checks every commit; (3) `G013`'s struct-field-naming trade took the opposite call because that symbol was one they **construct** — this one they only **receive**.
- Generalisation, credited to `pdfcer-gui`, verbatim from the doc comment: "a loud compile break is worth it for a symbol a consumer constructs, and is worse than a gate for one it only receives." Construction must be *told* about a shape change (a break is the only reliable channel); reception only needs to *know* the current shape, which a continuous gate verifies better than a one-time break.
- Filed as a dated addendum to the existing `naming_every_field_of_a_tracked_upstream_struct...md`, not a new file or a standing rule: one instance on each side of the construct/receive distinction (`G013` construct-side, this `receive`-side) is thin for a mint, per the reporting engineer's own explicit caution against minting on their recommendation alone.

**Still in flight:** unchanged — owed items 5, 14, 34.

**For next session:** none opened. No reply owed — `pdfcer-gui`'s consumption note closes both `G015` and `G016` and asks nothing.

**Sourcing (hard rule 8) — no shell this filing.** Doc-comment text and commit hash (`42b47f30`) taken from the requesting engineer's report. Independently confirmed against live source via `Read`/`Grep`: `crates/pdfcer-core/src/text_edit/reflow_apply.rs:258-301` and `:395-398` (the kept-variant doc comment and the still-live `match` arm consuming it) match the quoted reasoning verbatim. Push/CI state not independently checked — no shell.

## 2026-09-14 (549th filing) — `Pass 303.0` (`G015`) removes a reflow guard that outlived the code change that made it wrong; `G016` catches the doc-comment half `G015` missed; a reply-after-commit gate blind spot recorded, not minted

**Shipped:** `Pass 303.0` (`025d703d`) — `reflow_block` no longer refuses a page split into multiple producer-authored `/Contents` streams. Plus `3f416fbd` (`G016`, not a Pass) — `reflow_block`'s own rustdoc still promised the refusal `G015` had just removed.

**Decisions made this session:** none new architecturally. Judgment call: the reply-after-commit gate blind spot (below) is recorded as a flagged finding, not minted as a standing rule — one instance in this project's own register, below the two-instance mint bar, and the reporting engineer explicitly declined to characterize it further.

**Findings + decisions:**
- `reflow_block`'s `ReflowApplyError::PageEditedThisSession` guard fired on any page carrying a non-empty extra `/Contents` stream, read as text the operator added this session and refused with a "save and reopen" remedy that could never work — ISO 32000-1 §7.8.2 permits the split and CAD exporters use it routinely. Correct at `Pass 251.0` (planner read the base document); false since `Pass 257.0` moved the planner onto the session view, where an appended run is already inside the plan's source and survives — the guard's own comment asserted "Still true after `Pass 257.0`" and nobody had re-measured it.
- Measured before removal: three separate appended runs survive a reflow, each once. Measured after, on the operator's own eight-stream sheet: the block now refuses correctly as `R-INV-4` (composite/CIDFont, FF-E deferred) instead of falsely as "text was added this session." This replaces a false refusal with a true one; it does not give reflow on that specific block.
- The `PageEditedThisSession` variant is kept, not removed — nothing constructs it any more, but deleting it is a breaking enum change left to `pdfcer-gui`'s own call.
- `G016`: the same commit that removed the guard updated the variant's own doc comment and not `reflow_block`'s — two places stated one fact, one changed. Corrected in `3f416fbd`. `R247`'s 5th dated instance.
- A delivery reached the channel as a commit before it reached it as a reply: `G015` shipped with no reply written, and `pdfcer-gui` found it via `git log` while checking something unrelated. `tools/check-requests-scoped.py` was green throughout — correctly, since it reds only on "scoped in `ROADMAP.md` AND unanswered," and a *worked-but-not-yet-answered* request is a third state indistinguishable from "untouched" to that gate. Its own header already states this limit and declines to widen the gate to match commits against topic keys. Not minted as a rule (see Decisions above).

**Still in flight:** unchanged — owed items 5, 14, 34.

**For next session:** none opened. Reply already sent (`reply_G015_and_G016_late_and_you_were_right_to_say_so_SHIPPED.md`).

**Sourcing (hard rule 8) — no shell this filing.** Commit hashes, test counts and gate results taken from the requesting engineer's own report. Independently confirmed against the live tree via `Read`/`Grep`: `crates/pdfcer-core/src/edit.rs:11396-11499` (`reflow_block`'s corrected doc header and the removed-guard comment block), `crates/pdfcer-core/src/text_edit/reflow_apply.rs:203-283` (`ReflowApplyError::PageEditedThisSession`'s doc comment stating it is no longer constructed), the renamed tests in `content_edit_no_duplication.rs`/`reflow_decline.rs`, and `tools/check-requests-scoped.py`'s own header (confirms the gate's stated limit and the quoted line already on disk). Push/CI state not independently checked — no shell.

## 2026-09-14 (548th filing) — the "six missing RAG findings" from the 547th filing were a false positive; ledger denominator corrected, script recommended instead of hand-verification

**Shipped:** nothing — librarian-only, no Pass. Closes the 547th filing's own flagged `D:\dev\rag\rust\` ledger discrepancy, dispatched as an index check.

**Decisions made this session:** none new architecturally. Judgment call: an index-completeness check for `D:\dev\rag\rust\`/`egui\` should NOT live in pdfcer's `tools/` — the tree is shared across projects, not part of this repo's build, and a CI gate coupling this repo to an external, machine-local path outside its control would be the wrong boundary. Recommended it live in `D:\dev\rag\` itself as a manual/periodic script, run as part of this role's own "index check" protocol rather than wired into any single project's CI.

**Findings + decisions:**
- Six filenames flagged as unindexed were checked by reading `index.md` directly at the cited line ranges. **All six were already indexed** — under an older `` - `file.md` `` prose-bullet convention this file used before it standardized on `- [Title](file.md) — hook`. A single-pattern grep (`^- \[`) undercounts by exactly the number of files still carrying the older style (9 found).
- Had the six been written as new bullets, `index.md` would have gained six duplicate entries, one of them for a file (`a_sabotage_can_only_be_as_discriminating_as_the_fixture_it_runs_on.md`) whose existing entry already runs to 17 dated instances. Caught by reading before writing — the "grep before writing a lesson" discipline this role is bound to — and a reminder that a bullet-count mismatch is not itself proof of a missing entry.
- Ledger corrected on the denominator only: `Glob D:\dev\rag\rust\*.md` = 369 files (confirmed twice), minus 2 named meta files = **367 finding files**. The prior "169" carried figure is superseded, not reconciled to a "true" index-entry count, because no reliable way to count index entries by hand exists while the file mixes bullet conventions.
- `D:\dev\rag\egui\index.md` shows the same shape at a glance (203 bracket-bullets against roughly 215 candidate finding files) — flagged, not investigated, for the identical reason: hand-checking would risk the same false positive just avoided here.

**Still in flight:** unchanged — owed items 5, 14, 34.

**For next session:** if an index-completeness script for `D:\dev\rag\rust\`/`egui\` gets written (recommended location: `D:\dev\rag\`, not pdfcer's `tools/`), run it before ever again flagging a specific filename as "unindexed" by hand-grep.

**Sourcing (hard rule 8).** `Glob D:\dev\rag\rust\*.md` run twice this filing, both returning 369. `Grep '^- \['` = 362, `Grep '^- '` = 382, `Grep` backtick-bullet pattern = 9 — all against `D:\dev\rag\rust\index.md` directly. Each of the six candidate files' existing index entries verified by direct `Read`/`Grep` at the line numbers cited above, not inferred from the count.

## 2026-09-14 (547th filing) — `pdfcer-gui`'s `R8` closes the 546th filing's five unresolved verbs; `R257` gains a third same-day-adjacent instance; a stale RAG-directory ledger count flagged

**Shipped:** nothing — librarian-only, no code commit, no Pass. `pdfcer-gui` consumed the `G014` addendum with nothing owed either way and volunteered two things: a resolution to this project's own open item, and a third instance of `R257`.

**Decisions made this session:** none new. Declined to mint a new RAG rule for "searching a command catalogue by a claim's stated verb misses a capability named by its gesture" — one instance (`reorder` → `move_up`/`move_down`), recorded in `ROADMAP.md`'s `00b6360` amendment at `n=1`, below this project's own two-instance mint bar.

**Findings + decisions:**
- **Closed:** the 546th filing's "five verbs (extract/insert/delete/reorder/rotate) unresolved, not verified" line. `pdfcer-gui`'s `R8` makes GUI-reachability measurable (command-catalogue membership + ribbon-manifest placement); all five confirmed reachable. `reorder` has no command by that name — it's `move_up`/`move_down` — which is why a by-name search had found nothing rather than confirming it.
- **`R257` (minted 545th filing) gains a third dated instance, credited to `pdfcer-gui`, a different direction again:** a resume-document correction cited a captured trace file inside a `.gitignore`d directory to support a UI-naming claim; it resolved on exactly the one machine holding that untracked file. Their framing, preserved verbatim: "the right filename in the wrong repository" (ours) vs. "the right filename in no repository" (theirs) — both a path resolving *somewhere* mistaken for one resolving *anywhere*.
- `D:\dev\rag\rust\a_citation_that_does_not_name_its_repository_is_not_a_citation.md` widened accordingly (Instance 3 + a generalisation paragraph moving the discriminator from "repository" to "resolves somewhere vs. anywhere"); `index.md` bullet updated same edit.
- **Flagged, not resolved:** `D:\dev\rag\rust\` ledger tracking in `ROADMAP.md`'s ledger tables carried "169 findings" from the 543rd filing; `Glob D:\dev\rag\rust\*.md` this filing returns 369 files. Not reconciled here — recorded as a measured figure per hard rule 8, with an `index check` flagged as the right next step.

**Still in flight:** unchanged — owed items 5, 14, 34 (item 35 was discharged/attached at the 543rd filing).

**For next session:** the `D:\dev\rag\rust\` ledger discrepancy (169 vs. 369) is available as a scoped `index check` if worth reconciling; not opened as a numbered owed item.

**Sourcing (hard rule 8).** `Glob` used directly against `D:\dev\rag\rust\*.md` this filing (369 files, first 100 read for meta-file identification). `pdfcer-gui`'s `R8` mechanism and the five-verb resolution table taken from their own dispatch message, not independently re-derived from `pdfcer-gui`'s source (out of this role's remit — that repository is theirs). `README.md`/`FEATURES.md` state not re-checked this filing; no change was needed against either.

## 2026-09-13 (546th filing) — the 545th filing's own "left unaudited" claim went stale within the hour; `00b6360` closes the sweep and finds a fifth wrong claim

**Shipped:** `00b6360` — not a Pass, a second published-claim correction to `README.md`'s "Working today" paragraph, following on from `17e35e5` (545th filing, immediately below). `pdfcer-gui`'s defect `D3`, continued.

**Decisions made this session:** none new. Whether a filing's "not yet audited, offered as separate work" line going false within the hour — with no shell available to catch the concurrent audit before publishing — earns its own standing rule is left to the engineer's judgement. It rhymes with the "a commit landed on `main` while this entry was being written" shape already on record twice in this file (two-hundred-and-sixteenth-filing box, and the `(ca)` operator-question entry's suite-name count), but those were both caught *before* publishing, with a shell in hand; this one was not, and is being corrected after the fact instead. Not filed as an `R257` instance — `R257`'s mechanism is a citation silently resolving to the wrong repository, which this is not — though the correcting engineer drew the parallel and it is recorded here so a future reader can judge for themself.

**Findings + decisions:**
- `17e35e5`'s (545th filing's) own "Explicitly not audited... offered as separate work" line was true when written and false minutes later: the same session took up the offer, unasked, in `00b6360`.
- **Four claims spot-checked and left standing:** OCR (ticked core/cli/gui; a dev build without model files refuses precisely, and `package-portable.py` stages the models for what ships, so the ticked claim matches what actually ships); merge (`FEATURES.md` row 174, GUI ticked); whole-page cut/copy/paste (row 176, GUI ticked); signature verification, trust evaluation, signing, text extraction, and rotated-baseline text extraction (rows found, GUI ticked).
- **One fifth wrong claim, the same shape as `17e35e5`'s original four:** `split` was listed among page operations ("merge, split, extract, insert, delete, reorder, rotate") as though GUI-reachable. `FEATURES.md` row 175 states, in bold, **"Not reachable in `pdfcer-gui`"** — unbuilt, sitting in *Planned*, not merely hard to find (R9's convention draws an unbuilt capability as nothing, not a disabled control). Also narrower than the bare verb implies: `EveryN` criterion only, no bookmark- or size-based split. Both facts folded into the README using `17e35e5`'s own convention.
- **Five verbs the audit could not resolve either way:** extract, insert, delete, reorder, rotate. No `FEATURES.md` row was found naming these individually to confirm the GUI claim. Left standing in the README, unflagged — the audit found nothing wrong with them, but this entry records that as *unresolved*, not *verified*, so a later reader does not mistake silence for a check that happened.
- **Flagged, not corrected here:** the engineer's own channel reply (`reply_G014_..._SHIPPED.md`) offered the full sweep as separate, unrequested work, then performed most of it before the offer could be taken up or declined. The offer and the action now disagree in the channel's own record. A short follow-up correcting the offer is recommended; sending it is the engineer's/operator's call, not this role's.

**Still in flight:** unchanged from the 545th filing — owed items 5, 14, 34, 35.

**For next session:** the five unresolved verbs (extract/insert/delete/reorder/rotate) remain available as a small, scoped follow-up if the README's remaining silence on them is ever worth closing; not opened as an owed item, since the current text is honest about not knowing rather than wrong.

**Sourcing (hard rule 8) — no shell this filing.** Taken from the correcting engineer's own dispatch message, which states the four spot-checks, the `split` mechanism, the `FEATURES.md` row 175 citation, and the `EveryN`-only CLI behaviour; not independently re-verified against live source or `git log` from here.

## 2026-09-13 (545th filing) — `README.md`'s "Working today" paragraph corrected; `R257` minted for the mechanism that let the underlying defect survive a month

**Shipped:** `17e35e5` — not a Pass, a published-claim correction to `README.md`, filed under its own commit-hash heading (same precedent as `f16e266`+`5917ece`, 511th filing). `pdfcer-gui`'s defect `D3`.

**Decisions made this session:** mint standing rule `R257` and write a new cross-project RAG file at `D:\dev\rag\rust\a_citation_that_does_not_name_its_repository_is_not_a_citation.md`, rather than leave the finding as a one-off correction — this project's own two-instance mint bar was cleared same-day, in two different directions, by the same mechanism.

**Findings + decisions:**
- `README.md` claimed **Bates numbering** and **PDF/A validation and conversion** as working, and listed imposition without its `pdfcer`-only qualifier. All three contradicted sources already in the repo: `docs/FEATURES.md` rows 513/515 (unticked on all three surfaces) and row 414/495 (imposition CLI-only, GUI gap separately recorded), plus `pdfcer --help` printing `[not yet implemented]` on exactly those three verbs. Verified directly against `FEATURES.md` and `--help`, not taken from the request.
- A fourth, unrequested claim corrected the same edit: the README's subcommand count (139) was stale against the binary's 149 working + 3 stub commands.
- **The history is the finding.** The defect record cited `README.md:20-22` with no repository named, and both `pdfcer` and `pdfcer-gui` have a `README.md`. A 2026-09-13 session closed the defect as resolved by checking `pdfcer-gui`'s files — every individual claim in that closure was true, and it was about the wrong repository. Same-day, opposite-direction instance in `D:\dev\rag\rust\a_completeness_guard_can_be_lost_three_different_ways.md`, where a possessive ("its own") credited `pdfcer-gui` with tests belonging to `pdfcer-core`.
- **`R257` minted**: a citation lacking its owning repository is not ambiguous to its writer, only to a later reader — and it fails by resolving, silently, to a real wrong target rather than by breaking visibly. Full derivation graduated to the cross-project RAG (see above), indexed in that tree's `index.md`.
- **Explicitly not audited:** the remaining ~17 claims in the same README paragraph. Not implied fixed or verified by this filing; offered as separate work.

**Still in flight:** unchanged from the 544th filing — owed items 5, 14, 34, 35.

**For next session:** none opened by this filing; a full README-claims sweep against `FEATURES.md` remains available but unscoped.

**Sourcing (hard rule 8) — no shell this session.** `.git/packed-refs` reads `17e35e55886d3e50778b0c4d9bca506c9b8438be refs/heads/main`; `.git/COMMIT_EDITMSG` (verbatim) supplied the exact commit wording. Independently verified against live source via `Read`/`Grep`: `README.md` lines 24, 30, 58–61 carry the corrected text; `docs/FEATURES.md` lines 414, 495, 513, 515 match the cited rows. Not checked against `origin/main`.

**★ AMENDMENT 2026-09-13 (546th filing, `00b6360`).** The line above reading "Explicitly not audited... offered as separate work" went false minutes after this entry was written: the same session took up the offer, unasked, and found a fifth wrong claim (`split`, listed as though GUI-reachable; `FEATURES.md` row 175 says otherwise). See the 546th filing, immediately above, for the corrected accounting — five claims plus the subcommand count corrected, four spot-checked and left standing, five verbs (extract/insert/delete/reorder/rotate) still unresolved. Kept legible above rather than rewritten, per this project's history discipline.

## 2026-09-13 (544th filing) — a completeness-guard finding's own dated refinement, credited to `pdfcer-gui`: a guard can be BORROWED, not just deleted or self-testing

**Shipped:** nothing — librarian-only, no code commit, no Pass, nothing owed either way. `pdfcer-gui` consumed the `G013`/slices notice and shipped at pin `5e17017`; this filing is purely a RAG refinement they volunteered on their way past.

**Decisions made this session:** appended a dated refinement to `D:\dev\rag\rust\a_completeness_guard_can_be_lost_three_different_ways.md`'s mode-3 section rather than mint a new file or a new standing rule — `R256`'s text is specifically about the `[Self; N]`-widening mechanism (mode 1), and this is a sharper cut of mode 3, not a fourth mode.

**Findings + decisions:**
- `pdfcer-gui`'s `snap_marker_shapes` matches `SnapKind` exhaustively today and genuinely fails to compile on a 9th variant — real protection, but **borrowed** from nothing: nobody ties its exhaustiveness to `priority()`'s. The day a `_ => Vec::new()` arm is added (the pattern already used in the neighbouring `info_label`), the compile-time protection disappears with no error and no test noticing, because the two safeguards were never independent in the first place.
- The sharpened question this adds to the file's "recognising which of the three" checklist: not just *does a guard cover the property it's named for*, but *is that coverage its own or borrowed from a neighbouring function's current shape*. "A borrowed guard has no owner, so nobody is told when it is returned."
- Recorded honest limit from the same finding: neither guard catches two `SnapKind`s whose markers are visually indistinguishable on screen — only shape *count* is checked, not rendered pixels. Out of reach of an exhaustive match by construction, same as ordering/uniqueness were in mode 3 itself.
- `pdfcer-gui` self-checked before reporting: their own two replacement guards (`all_contains_every_variant`, `all_lists_every_kind_with_a_unique_contiguous_rank`) are owned, not borrowed — each carries its own exhaustive match inside the test body. Worth keeping as the template for anyone replacing a lost or borrowed guard: verify the replacement doesn't just move the borrowing one level over.

**Still in flight:** unchanged from the 543rd filing — owed items 5, 14, 34, 35 as previously stated.

**For next session:** none opened by this filing.

**Sourcing (hard rule 8) — no shell this session.** The refinement text and `pdfcer-gui`'s pin (`5e17017`) are taken from the dispatching engineer's relay, not independently checked against `pdfcer-gui`'s test files from here.

## 2026-09-13 (543rd filing) — item 35 gets a standard to measure against, credited to `pdfcer-gui`; a general test-design finding graduates to `D:\dev\rag\rust\`

**Shipped:** nothing — librarian-only, no code commit.

**Decisions made this session:** attach `pdfcer-gui`'s two premise-assertion tests (for their object-recovery panel's control/damaged fixtures) and the three rules they drew from writing them, to owed item 35 as the standard the eventual survey measures against. The 196/44/152 detector figures from the 542nd filing are left exactly as filed — this addendum does not make them any more quotable, per the operator's explicit instruction.

**Findings + decisions:**
- The three rules: (1) two independent assertions when a fixture has two properties that can fail in opposite directions ("still recovered" vs. "still lossless"); (2) assert exact values, never a count, when the field's shape (a `Vec`, not a tally) implies which value matters; (3) a premise-assertion's failure message should name the downstream consequence ("the driven check is no longer measuring anything"), not restate the failed condition.
- The cost argument for the item at all: a fixture that quietly stops meaning what a driven check assumes costs one second to catch as a unit failure, versus up to ninety minutes as a driven-check failure that wears the costume of an application defect and sends the debugger looking in the wrong file.
- Judged general enough to graduate: written to `D:\dev\rag\rust\a_fixture_premise_test_needs_two_assertions_the_exact_values_and_a_consequence_named_message.md`, indexed in that tree's `index.md`, credited to `pdfcer-gui`.

**Still in flight:** owed items 5, 14, 34 unchanged; item 35 widened (standard attached, size still unknown, still unscoped).

**For next session:** unchanged from the 542nd filing — whoever takes item 35 reads the files against the three rules above, not the regex; expected output smaller than 152.

**Sourcing (hard rule 8) — no shell this session.** The two tests' text and the three rules are taken from the dispatching engineer's relay of `pdfcer-gui`'s message, not independently checked against their test files or the channel directory from here.

## 2026-09-13 (542nd filing) — new owed item 35: the fixture-premise-assertion survey, opened with a caveat attached to its own measurement

**Shipped:** nothing — librarian-only, no code commit. Answers the 541st filing's own "for next session" question (open the corpus-fixture-invariant survey as an owed item?) with yes.

**Decisions made this session:** open the survey as `ROADMAP.md` owed item 35 — *"survey how many fixture-consuming tests assert the fixture's own premise; size unknown"* — rather than close the question unresolved a second filing running.

**Findings + decisions:**
- A throwaway phrase-detector over `crates/*/tests/*.rs` found 196 files referencing a fixture path, 44 of those 196 (22%) matching premise-assertion language, 152 not matching. **Recorded as a detector output, not a coverage census, and flagged that hard in the ROADMAP entry**: the detector has a false-negative side (a bare `assert!` with no prose scores as a gap) and the denominator itself is loose (mentioning a fixture path is not the same as depending on one of its properties). Two files spot-checked against the classification, both defensible — not enough to certify the other 194.
- This is the same shape hard rule 10 exists for, applied to a number that never even reached a permanent form: a total (44/152) filed beside its denominator (196) and its method (regex, not reading) is what stops it being quoted forward as "22% coverage" the way seven other numbers were quoted forward earlier this project.

**Still in flight:** owed items 5, 14, 34 unchanged; item 35 new.

**For next session:** whoever takes item 35 reads the files, not the regex — the deliverable is a list of files depending on an unasserted fixture property, expected smaller than 152.

**Sourcing (hard rule 8) — no shell this session.** The 196/44/152 figures, the two-file spot-check, and the "nothing else owed from the pdfcer-gui exchange" claim are all taken from the dispatching engineer's account and not independently re-run or re-checked this filing.

## 2026-09-13 (541st filing) — `pdfcer-gui` shipped its half of `objects_dropped` with no ask back; their §3 is a new `R162` instance, filed to the RAG

**Shipped:** nothing on this side. `pdfcer-gui` consumed
`notice_2026-09-13-recovery-now-names-the-objects-it-dropped-and-your-banner-may-want-the-field.md`
and wired `RecoveryReport::objects_dropped` into their Document-properties
panel unassisted (`f37598b` on their side). They asked one question — should
the CLI stderr sentence and the GUI panel sentence be worded identically —
and answered it themselves in the channel, declined on `R221` grounds: two
surfaces stating the same substance in different words is not the duplicate
`R221` warns about, since stderr has no numbers beside it and the panel does.

**Decisions made this session:** none.

**Findings + decisions:**
- **`R162` gains a corroborating instance**, filed in full to
  `D:\dev\rag\rust\absence_assertion_must_first_prove_the_container_could_have_held_it.md`
  (index updated in the same edit). The shape: a driven check asserting a
  disclosure panel does NOT draw needs its control to be a **recovered file
  with the two loss-causing traps removed**, not a sound file — a sound file
  leaves three independent things unexercised at once (panel never opened,
  document never recovered, block correctly empty) and "the block is absent"
  is satisfied by all three. Generalises this file's existing remedy with one
  refinement: the control must be a **minimal-diff sibling** of the positive
  fixture, not merely "a different, empty-shaped" one. Cross-project, no
  pdfcer commit — `pdfcer-gui` is a separate repository — so filed to the
  ecosystem RAG rather than to `ROADMAP.md`'s ledger.
- **Declined to merge with two other same-day "clean for the wrong reason"
  instances** (`Pass 301.1`'s completeness test carrying its own copy of the
  set it checked; `42b44ab`'s `R192` ninth instance), on the reporting
  engineer's own recommendation and on this librarian's read that the three
  have different mechanical causes (duplicated definition; unopened path;
  chained-precondition fixture) — a shared symptom is not a shared mechanism.
- **Flagged, not measured:** the correspondence notes pdfcer's own corpus
  fixtures may have assumed properties (page/object counts, which objects a
  redaction/recovery/scan path should touch) asserted nowhere — the same gap
  their fixture-through-engine test closes on the GUI side. No count given;
  worth a survey, not filed as an owed item on anyone's say-so but mine.

**Still in flight:** owed items 5, 14, 34 unchanged from the 540th filing.

**For next session:** consider whether a corpus-fixture-invariant survey
(previous bullet) belongs on the owed list; nothing else new.

**Sourcing (hard rule 8) — no shell this session.** `f37598b` is the GUI
side's own stated commit, relayed and not independently confirmed (external
repo, no shell access to it from here). Every quoted sentence and file path
in this entry was read directly via `Read`/`Grep` from
`D:\Dev\FeatureRequests\pdfce_FeatureRequests\open\done_recovery_dropped_objects_CONSUMED.md`
and the paired `reply_2026-09-13-keep-your-wording-and-your-control-fixture-is-the-finding.md`,
and from the two files this filing edited.

## 2026-09-13 (540th filing) — `Pass 302.1` (`919b0f0`): the CLI now prints the object recovery drops, closing the gap `Pass 302.0` opened an hour earlier

**Shipped:** `Pass 302.1` — `disclose_recovery` (`main.rs`) now prints `RecoveryReport::objects_dropped` as two separate counted notes (`IdMismatch` vs `Unparseable`, never summed), discharging the CLI half of owed item 33. New out-of-process test file `crates/pdfcer-cli/tests/recovery_names_what_it_dropped.rs` asserts the dropped object's number and reason, plus a clean-recovery control that asserts nothing is printed when nothing was dropped.

**Decisions made this session:** none — a CLI wiring fix to `Pass 302.0`'s own report, addended into `ARCHITECTURE.md` §10.5 rather than minted separately.

**Findings + decisions:**
- **This is a defect in `Pass 302.0`, not a follow-on feature.** The report field existed and nothing outside `recover.rs`'s own two unit tests ever printed it, so a terminal read of a recovered file was still silently shorter than the object it actually held — the same hour the field was authored.
- **`RecoveryReport`'s own doc comment already claimed the CLI/GUI surface every field ("none is rounded away") — false for this one field until now.** Filed as `R247`'s 4th dated instance.
- **The prior filing's own ledger claim was wrong.** `Pass 302.0`'s Ledger row said "`R245` gains a 9th dated instance (dated footer, no re-mint)"; the footer had never actually been appended to *Standing rules*' master `R245` entry. Appended in this filing, one filing late — a register claim outliving the write that was supposed to make it true, recurring in the register's own bookkeeping about itself.
- **Item 33 split, not closed outright.** Its CLI half is discharged; new owed item 34 carries the GUI half (`pdfcer-gui` is a separate, external repo, untouched this filing) — raised on the `pdfce_FeatureRequests` channel rather than assumed either way.

**Still in flight:** owed item 34 (GUI wiring question, cross-project) is new and unstarted; items 5, 14 unchanged.

**For next session:** owed items 5, 14, 34 remain open; item 33 is discharged (CLI half).

**Sourcing (hard rule 8) — no shell this session.** Commit hash `919b0f0` and the test-count/clippy/sabotage figures are taken from the dispatching engineer's account, not independently confirmed via `git log`/`git show`. Independently verified against the live tree via `Read`/`Grep`: `crates/pdfcer-cli/src/main.rs:12805-12904` (`disclose_recovery`'s new block), `crates/pdfcer-core/src/recover.rs:228-235` (the doc comment, quoted verbatim) and `:196-226` (`DropReason`'s `#[non_exhaustive]`), `crates/pdfcer-cli/tests/recovery_names_what_it_dropped.rs` in full (both tests, both fixtures), and the *Standing rules* master `R245` entry (confirmed it read EIGHTH, not NINTH, before this session's own edit).

## 2026-09-13 (539th filing) — `Pass 302.0` (`acf9234`): recovery names the object it drops now, not just the loader; the CLI print path does not yet say so

**Shipped:** `Pass 302.0` — `RecoveryReport::objects_dropped: Vec<DroppedObject>` (`DropReason::{Unparseable, IdMismatch}`) discharges owed item 18 (decision 145's recovery-path sibling gap): a scanned object recovery cannot keep (measured: the content stream of a PDFsharp-written file whose `startxref` undershoots its own `xref` by 134 bytes) is now named with a reason instead of vanishing with no record and a false "not in the file" description.

**Decisions made this session:** none — no new decision number; extends decision 145's existing disclose-never-silent kernel to the recovery subsystem's own pre-existing report (`R20`, decision 013), addended into `ARCHITECTURE.md` §10.5 rather than minted separately.

**Findings + decisions:**
- **The CLI does not print the new field.** `disclose_recovery` (`main.rs`) prints every other `RecoveryReport` field and not this one; a workspace grep found no consumer of `.objects_dropped` outside `recover.rs`'s own two tests. `RecoveryReport`'s own doc comment claims the CLI/GUI surface every field — now false for this one until wired. Filed as `R245`'s 9th dated instance and a new owed item (33), separate from item 18, which is closed at the report/data level (what decision 145 actually obliges).
- **Item-18 ambiguity resolved.** Two unrelated findings have carried the number 18 in this ledger: an earlier redaction gap, closed by `Pass 285.0` (`1366138`, confirmed via `FEATURES.md`'s own text), and this recovery-path gap, opened at the 494th filing. Both confirmed independently against the live document text.
- **Items 5 and 14 judged, per the engineer's request.** Both re-read at their source: item 5 is a standing methodological reminder (not undone work); item 14 is a cause correctly held at `n=2`, not minted, watching for a third (hard rule 11's own threshold discipline working as intended). Neither removed from the ledger unilaterally; the judgment is recorded in `ROADMAP.md`.

**Still in flight:** owed item 33 (CLI/GUI wiring for the new disclosure field) is new and unstarted.

**For next session:** owed items 5, 14, 33 remain open; item 18 is discharged.

**Sourcing (hard rule 8) — no shell this session.** Commit hash `acf9234` and its contents are taken from the dispatching engineer's account, not independently confirmed via `git log`/`git show`. Independently verified against the live tree via `Read`/`Grep`: `crates/pdfcer-core/src/recover.rs` (the new types, both tests, both `dropped.push` call sites), `crates/pdfcer-cli/src/main.rs`'s `disclose_recovery` (confirmed no reference to `objects_dropped`), and a workspace-wide grep for `.objects_dropped`.

## 2026-09-13 (538th filing) — owed item 13b discharged: already shipped by `Pass 287.0`/`291.0`/`292.0`, and so was the ask that superseded its own withdrawal

**Shipped:** nothing — a librarian-only correction to the owed ledger, no code commit.

**Decisions made this session:** none — a stale-record correction, not a crate-boundary or invariant change.

**Findings + decisions:**
- Owed item 13b (*"`resize_annotation` refuses a pdfcer-authored `/Stamp` as foreign … a re-bake has no record of the original derivation to redo"*) was stale in full, confirmed by reading `crates/pdfcer-core/src/edit.rs` and `annot_author.rs` directly: `Pass 287.0` added the third authorship arm (`recover_stamp_parameters`/`apply_stamp_parameters`) and moved the label size into `/DA`; 13 tests in `tests/stamp_text_size.rs` cover it, all passing.
- The companion request had **three states, not two**: defect → narrowed to a nice-to-have ("close it as you see fit") → **re-escalated to wanted** two hours later, in the operator's own words, once he'd used the fix that had just shipped. A dispatch proposing this discharge cited only the middle state and offered its residue for Backlog. Reading the archived request in full found the third state already fully shipped too — `Pass 287.0`'s `StampFit::GrowToText` grows the box while holding text size fixed (exactly the re-escalated ask), and `Pass 292.0`'s stamp-label recovery in `set_text_annot_style` made the "twin function" fallback unnecessary. Nothing filed to Backlog; there is no unshipped remainder.
- **R243 dated instance, no new rule.** The stale blocker sentence was copied forward unchanged across roughly a dozen filings after `Pass 287.0` removed it — the same shape `R243` already names ("a documented obligation on a future caller is not a control") one level up, here applied to the register's own memory rather than to a code warning. Instance appended to `D:\dev\rag\rust\a_documented_obligation_on_a_future_caller_is_not_a_control.md`.

**Still in flight:** nothing new.

**For next session:** owed items 5, 14, 18 remain open, unchanged; item 13b is discharged.

**Sourcing (hard rule 8) — no shell this session.** All claims verified via direct `Read`/`Grep` against the live tree (`edit.rs`, `annot_author.rs`, `tests/stamp_text_size.rs`, the archived request file in full) — none relayed from the dispatch that requested the discharge.

## 2026-09-13 (537th filing) — `42b44ab`: the gate shipped an hour ago could not see half the correspondence; R192's ninth instance

**Shipped:**
- `42b44ab` — `tools/check-requests-scoped.py` (built one commit earlier, `3d8c160`) gains an explicit two-channel list (`pdfce_FeatureRequests` + `iccce_FeatureRequests`) and a whole-filename answer regex, replacing a channel constant and a prefix-tuple matcher that both silently covered only the first channel.

**Decisions made this session:** none new — a same-day fix to an existing gate, not a crate-boundary or invariant change.

**Findings + decisions:**
- The gate this filing fixes was itself found blind by the exact blind spot the 536th filing (immediately below) had just paid for: a reply-citation audit reported 19, then 15, then 10 replies "missing" from `ROADMAP.md`, and the true number was 0 — four of the ten were sitting, correctly answered, in `iccce_FeatureRequests`, a channel the audit never opened. The gate built an hour later to make that kind of audit mechanical inherited the identical omission.
- **Filed as R192's ninth instance, not a new mint, and not R221.** `D:\dev\rag\rust\a_gate_states_what_it_cannot_see.md` already names this shape exactly — a tool's input set narrower than its obligation's subject set — across eight prior instances in this project; this is the ninth, and it is filed as **one** instance covering both manifestations (the audit and the gate), because both trace to the same uncatalogued fact (pdfcer answers two request channels, never enumerated as a pair anywhere) rather than two independent discoveries of the mechanism. `R221` was considered and declined: that rule's mechanism needs two descriptions of one question that can drift apart; here there was only ever one decider (first the audit, then the gate), and its domain was simply never written down as a set. Dated instance appended directly to the RAG file, per that rule's own single-file ledger discipline; `index.md` bullet updated in the same edit; no new standing-rule number.
- The two fixes: an explicit `DEFAULT_CHANNELS` tuple (never a glob over the parent directory, which also holds unrelated projects' correspondence — widening the set stays a deliberate act), and a whole-filename regex answer matcher (`reply|done|notice|note`) replacing a prefix tuple that matched only `pdfce_`'s naming convention and not `iccce_`'s date-prefixed one. The prefix tuple's failure mode is the sharper of the two: it still printed "clean" for every request, because some other file happened to cite each one — a matcher wrong about HOW it found something reports success identically to one that is actually complete.
- Judgment call, not acted on: the engineer's own follow-up commit (`29ba103`, docs-only) added a two-channel explanation to `docs/NEXT_SESSION.md`, which now duplicates this filing's own account. Left alone — `NEXT_SESSION.md` is engineer-owned and replaced each session, so it carries none of the drift risk a permanent register would.

**Still in flight:** nothing new.

**For next session:** items 5, 13b, 14, 18 remain open, unchanged (see `docs/ROADMAP.md`'s owed-work ledger).

**`FEATURES.md`**: unchanged — an internal CI/register control, no operator-facing capability.

**Sourcing (hard rule 8) — no shell this filing.** `.git/logs/HEAD` read directly (not relayed): `42b44ab` sits one commit after `873ff62` (the 535th filing's tip) and one before `29ba103` (the engineer's own follow-up docs commit). `42b44ab`'s own message is read from its reflog subject line; no retained `COMMIT_EDITMSG` for it (the tip's file now carries `29ba103`'s message). Independently verified against the live tree via `Read`: `tools/check-requests-scoped.py` in full, confirming the two-entry `DEFAULT_CHANNELS` tuple, the whole-filename `ANSWER_RE`, and the per-channel `SKIPPED` announcement all match the account above. Not checked this filing: `origin/main` state, backup-bundle currency, CI colour.

## 2026-09-13 (536th filing) — `reply_*.md` citation audit: owed item 11 discharged, two fabricated filenames named, four citations found in the wrong channel

**Shipped:** nothing — a librarian-only documentation correction, no code commit.

**Decisions made this session:** none — a citation-hygiene finding, not a crate-boundary or invariant change. Declined to mint a standing rule for it (see Findings).

**Findings + decisions:**
- Owed item 11 (`pdfcer-gui`'s fourth outbound reply, open since the 479th filing) is **discharged**: `Glob`-confirms as `archive/2026-09-09-reply-your-coverage-flag-was-REAL-and-it-was-the-first-name-on-the-list.md`. The channel renames `reply_<subject>` → `<date>-reply-<subject>` on archiving; any future check must normalise (strip date, fold `reply_`/`reply-`, compare stems) before calling anything missing.
- Two citations in this file, `reply_G010_renamed_to_EmptyNameSegment_SHIPPED.md` and `reply_G011_validate_partial_name_is_public_SHIPPED.md`, were never real filenames — they paraphrase the content of two real replies (`archive/2026-09-12-G010-period-in-partial-name-reply.md`, `…-G011-validate-partial-name-reply.md`). General shape: a citation describing a reply's content, rather than quoting its saved filename, reads as a filename and is not one.
- Four more citations resolve fine but in `D:\Dev\FeatureRequests\iccce_FeatureRequests\` (the ICC-colour-management partner channel), not `pdfce_FeatureRequests`: `reply_the_49_rows_and_the_black_end_is_where_i_am_weaker.md`, `reply_capability_status.md`, `reply_the_profile_census_and_your_33_node_constant.md`, `reply_cmyk_buffer_destination_and_width.md`. Not a defect in the files — a defect in the citation's own channel ambiguity, since only one prior citation in this file gives the full path.
- `reply_suite_render_harness.md` was a real, already-flagged stray duplicate (184th filing) and has since been deleted from `pdfce_FeatureRequests/open/` as recommended; its authoritative pair lives in `iccce_FeatureRequests/archive/`. `reply_insert_pages_orphaned_widgets.md` was a discharge-condition's planned filename, never written under that name — the actual reply landed as `2026-08-19-insert-pages-orphan-count-reply.md`, already on record two paragraphs later in the same `ROADMAP.md` entry.
- **No standing rule minted.** A mechanical gate would need to know which of two external, ungoverned channels a bare `reply_*.md` belongs to — exactly the ambiguity in question. Flagged to the engineer as a citation habit (name the channel directory when it could be confused; quote the saved filename, never a paraphrase) rather than built as a control.

**Still in flight:** nothing new.

**For next session:** items 5, 13b, 14, 18 remain open, unchanged (see `docs/ROADMAP.md`'s owed-work ledger). Do not re-open item 11.

**`FEATURES.md`**: unchanged — a citation-hygiene correction, no operator-facing capability.

**Sourcing (hard rule 8) — no shell this filing.** Every resolution above was produced by `Glob`/`Grep` reads against the live channel directories (`D:\Dev\FeatureRequests\pdfce_FeatureRequests\{open,archive}\`, `D:\Dev\FeatureRequests\iccce_FeatureRequests\{open,archive}\`) run this filing, not relayed or inferred from prose. Not checked: `origin/main` state, backup-bundle currency, CI colour — none are implicated by a docs-only correction.

## 2026-09-13 (535th filing) — `tools/check-requests-scoped.py` built, discharging the last item from the operator's "do everything but the token thing" batch; fourth `R243` dated instance, one level up from the first three

**Shipped:**
- `3d8c160` — `tools/check-requests-scoped.py`: red on exactly one state, a request scoped in `ROADMAP.md` with no answer in the channel; green otherwise (6 open, 6 answered, 0 scoped-unanswered at baseline). Registered in CI's `audits` job (23 → 24 checks) and `check-ci-parity.py`'s `LOCAL` table, so `run-gates.sh` picks it up automatically.
- `873ff62` — engineer-owned, no separate filing: strikes the corresponding owed-tool line in `docs/NEXT_SESSION.md`, struck rather than deleted.

**Decisions made this session:** none new — a control built to an already-minted rule (`R242`), not a crate-boundary or invariant change.

**Findings + decisions:**
- The rule chain is the point. `R242` (a request leaves `open/` when ANSWERED, not when SCOPED) is correct, and its correctness is what created the hazard this tool closes: a scoped request stays in the channel by design, so an audit reading `open/` counts it as outstanding. That produced two incidents in 35 hours (460th and 461st filings). `R242` was minted 2026-09-06 19:58; the tool was owed from that moment.
- **It sat owed for seven days while the register carried a written instruction to remember** — which is `R243`'s own shape (a documented obligation on a future caller is not a control), applied reflexively to a rule about another rule's enforcement. Filed as `R243`'s fourth dated instance.
- Design: matches on the exact request FILENAME (not a topic key — only one of six current requests carries a `G0NN` key); the channel lives outside the repository and is absent in CI, so the gate announces `SKIPPED — channel not present` by name rather than passing silently — `R255`'s shape, and it would otherwise have been this gate's own first defect.
- This discharges the last of the owed items from the operator's *"yes do everything but the token thing that breaks builds"* batch — the 534th filing's own verification had restated it as still owed to the engineer; it is now built.

**Still in flight:** nothing new — a complete, one-tool filing.

**For next session:** items 5, 11, 13b, 14, 18 remain open, unchanged (see `docs/ROADMAP.md`'s owed-work ledger).

**`FEATURES.md`**: unchanged — an internal CI/register control, no operator-facing capability.

**Sourcing (hard rule 8) — no shell this filing.** `.git/logs/HEAD` and `.git/COMMIT_EDITMSG` read directly (not relayed) to confirm `3d8c160` sits one commit before the tip `873ff62` and to quote `873ff62`'s own message verbatim. Independently verified against the live tree via `Read`/`Grep`, not relayed: `tools/check-requests-scoped.py` read in full (exit-code contract, filename-match logic, `SKIPPED` announcement all present as described); `.github/workflows/ci.yml`'s `audits` job named `"repository audits (24 checks)"` and listing this script as a step; `tools/check-ci-parity.py`'s `LOCAL` table carrying it. The 460th/461st-filing incident-interval figures and the sabotage/verification transcript are taken from the dispatching engineer's account as authoritative and not independently re-run. Not checked this filing: `origin/main` state, backup-bundle currency, CI colour.

## 2026-09-13 (534th filing) — `set_font` reuses an existing subset; the pre-flight promised to add one it would have reused; a text-fixture PROVENANCE backfill falsifies a legal-adjacent claim; `R221`'s instance ledger relocated out of ROADMAP prose

**Shipped:**
- `52a0ccd` (`Pass 301.2`) — measured owed item 8 in full: `set_font` DOES resolve to an existing subset-embedded resource sharing the target `/BaseFont` (via `resolve_target_resource`'s `subset_stem` match), and the feared consequence (a font-remedy refusal naming a face that then fails the subset floor) does NOT occur — the remedy list is built by asking the accepting code per face, so it already excludes the shadowed name. Fixed a real, adjacent defect: `survey_standard_14` (backing `font-preflight`) matched `/BaseFont` exactly instead of through `subset_stem`, so it reported a reused subset as "would-add."
- `408c93c` — docs-only: backfilled `fixtures/synthetic/text/PROVENANCE.md` for all twelve previously-undocumented fixtures, discharging owed item 4.

**Decisions made this session:**
- No new `ARCHITECTURE.md` decision — both commits are correctness fixes/documentation to existing internal mechanisms, not crate-boundary or invariant changes.
- `R221`'s instance-tracking mechanism changed: the ledger moves from inline ROADMAP prose (which had already produced a genuine numbering collision in the frozen historical record — two unrelated instances both labelled "fourth") to a single append-only RAG file, mirroring `R225`'s own successful discipline. Discharges owed item 10.

**Findings + decisions:**
- `Pass 301.2`'s doc-comment claim ("the answer here and the outcome of the later `set_font` cannot disagree") was false and unenforced — `R247`'s third dated instance, and simultaneously `R221`'s twelfth reconciled instance on the same line: one incident, two distinct reasons it went unnoticed.
- The `fixtures/synthetic/text/PROVENANCE.md` backfill found a blanket claim ("no embedded font programs... no attribution is owed") that had quietly gone false as composite/subset fixtures were added beside the original six — ten of twenty-nine PDFs embed a `/FontFile2`. The `LEGAL.md` §5 classification itself was never at risk (every outline is drawn by a committed generator); this was a description that stopped being true, corrected in place per this project's history discipline.
- Hard rule 10: the owed-item register's own figure was wrong. Item 4 said "21 of 38 files undocumented"; the file directly measured 39 entries / 29 PDFs / 12 undocumented — the register had been counting entries, not fixtures, and carrying the wrong total forward unverified since the 477th filing.
- Owed item 9 was found to be **already resolved** (489th filing, 2026-09-09, direct engineer ruling) — the framing that prompted this filing's dispatch was stale, citing a superseded reservation. Corrected without reopening anything.
- `tools/check-requests-scoped.py` (owed by `R242`) verified still unbuilt against the tree — restated as owed to the engineer, not built here (outside this role's remit).
- New RAG file: `D:\dev\rag\rust\a_capability_predicate_that_restates_its_accepting_function_will_drift_ask_the_function_instead.md` — `R221`'s full mechanism plus a reconciled, best-effort 11-instance chronological ledger built from the frozen history file and the live ROADMAP, explaining the collision and adopting `R225`'s single-file dated-footer discipline going forward. Indexed in `D:\dev\rag\rust\index.md`. A twelfth instance (this session's own `Pass 301.2`) is appended within the same file.

**Still in flight:** nothing new — both commits were complete, measured answers to pre-existing owed items.

**For next session:** items 5, 11, 13b, 14, 18 remain open, unchanged by this filing (see `docs/ROADMAP.md`'s owed-work ledger). `check-requests-scoped.py` remains unbuilt.

**`FEATURES.md`**: checked, no row changed — neither commit moves an operator-facing capability (a pre-flight correctness fix and a fixture-provenance backfill).

**Sourcing (hard rule 8) — no shell this filing.** `.git/logs/HEAD` and `.git/COMMIT_EDITMSG` read directly (not relayed) to confirm `408c93c` is the current tip and `52a0ccd` is its immediate parent, and to quote `408c93c`'s own message verbatim. Independently verified against the live tree via `Read`/`Grep`: `crates/pdfcer-core/src/text_edit/format.rs`'s `survey_standard_14` (now routed through `subset_stem`, doc comment matching the account above), `crates/pdfcer-core/tests/font_preflight.rs`'s two new tests, and `fixtures/synthetic/text/PROVENANCE.md`'s struck correction paragraph. The test-count/clippy/sabotage verification figures for `Pass 301.2` and the exact fixture counts for `408c93c` are taken from the dispatching engineer's report as authoritative and were not independently re-run or recounted file-by-file. Not checked this filing: `origin/main` state, backup-bundle currency, CI colour — the engineer should check these directly if they matter for the next act.

## 2026-09-13 (533rd filing) — a completeness guard is lost three different ways: `CheckStyle`/`DocInfoField` widened like `Unit`, `PermissionBit` deliberately not, `SnapKind` gains a rank-uniqueness check it never had

**Shipped:**
- `d378417` (`Pass 301.1`) — sweep of `pdfcer-core` for `Pass 301.0`'s finding. `CheckStyle::all()` and `DocInfoField::all()` widen `[Self; N]` → `&'static [Self]`, each gaining an exhaustive-match completeness test in place of the deleted array guard. `SnapKind::all()` is new (it had none); its ordering test and completeness test now read it instead of carrying a hand-written copy, and a new assertion checks ranks are unique and contiguous from 0. `PermissionBit::all()` is left `[Self; 8]`, deliberately.

**Decisions made this session:**
- Decision 156 minted: a completeness guard is lost three distinct ways — deleted by an improvement (`Unit`, `CheckStyle`, `DocInfoField`), never worked because it tested its own copy of the set rather than the source (`pdfcer-gui`'s `every_unit_is_named_distinctly`), or covers completeness while leaving an orthogonal property unchecked (`SnapKind`'s ranks — exhaustive on *names*, silent on *uniqueness*, and `snap_candidates` sorting by rank then distance makes a collision a non-deterministic pick under the cursor, which `R19` forbids).
- Standing rule `R256` minted for the first mode only (three within-project instances clear the two-occurrence bar). The other two modes are recorded as candidate patterns at `n=1`, not minted — a shared symptom is not a shared mechanism.

**Findings + decisions:**
- The trigger: `pdfcer-gui` read `Pass 301.0`'s reply, found the identical shape in its own code within the hour, and coined "a completeness test that carries its own copy of the set is testing the copy" — adopted verbatim into `R256`'s derivation and the new RAG file.
- `PermissionBit::all()` is the load-bearing counter-example: eight bits are ISO 32000-1 Table 22's closed enumeration in a published standard, not a set this project expects to grow, so its fixed-size return type is information rather than debt. The reasoning is written into the accessor's own doc comment specifically so a future sweep finds the exception instead of "fixing" it.
- New RAG file: `D:\dev\rag\rust\a_completeness_guard_can_be_lost_three_different_ways.md`, indexed in `D:\dev\rag\rust\index.md`. Distinct family from the same day's diagnosis/heuristic-tuning entries — this one is about guards, not diagnoses or tuned heuristics.

**Still in flight:** nothing — this was a complete, one-commit sweep requested alongside `Pass 301.0`'s own reply.

**For next session:** none owed by this Pass. A notice for `pdfcer-gui` is already filed in the shared channel (`notice_2026-09-13-three-more-all-accessors-are-slices-and-one-deliberately-is-not.md`); not archived here, that is the requester's own step.

**`FEATURES.md`**: checked, no row changed — every touched accessor is an internal Rust return-type signature, no operator-facing capability moved.

**Sourcing (hard rule 8) — no shell this filing.** Verified independently against the live tree (`Read`/`Grep`): `annot_author.rs`'s `CheckStyle::all`, `edit.rs`'s `DocInfoField::all`, `crypto/standard.rs`'s `PermissionBit::all` (still `[Self; 8]`, growth/closed-enumeration reasoning present in its own doc comment), and `vector/snap.rs`'s `SnapKind::all`/`priority` plus both tests all match the dispatch's account. Commit hash `d378417` and the test-count/clippy/sabotage verification were supplied by the dispatching engineer's report and not independently re-run.

## 2026-09-13 (532nd filing) — `Unit` gains km/yd/mi; `Unit::all()` widens to a slice; a proposed `/U`-vs-`/Measure` split declined on spec grounds

**Shipped:**
- `d4b5f00` (`Pass 301.0`) — `dimension::Unit` gains `Kilometer`, `Yard`, `Mile`; `Unit::all()` changes from `[Unit; 6]` to `&'static [Unit]` (now 9). Inbound from `pdfcer-gui` (`G013`): *"we need units (including km and miles) added as options to everything."*

**Decisions made this session:**
- `abbrev()`/`measure_u()` split **declined**: `pdfcer-spec-librarian` confirmed ISO 32000-1 §12.9 Table 263 (= ISO 32000-2 Table 268) defines `/U` as a label "for displaying the units... in a user interface" — one role, not two — and `/U` carries no arithmetic, so a wrong `/U` under a right `/C` would be undetectable by any reader or round trip. No `ARCHITECTURE.md` decision minted: a value-space widening on an existing type, not a crate-boundary or invariant change.

**Findings + decisions:**
- ISO 32000-2 §12.10.2 Table 269 `/PDU` **is** an enumerated vocabulary containing `KM`/`MI`, and does not govern `/U` (different key/dict/subtype/clause/type) — a trap the spec librarian flagged explicitly so it isn't "corrected" into the wrong table later. `/U` is a text string and therefore encrypted in an encrypted document.
- A stray six-member `/U` example list in the spec corpus's own `iso32000__s__12.9.md` (not present in the standard) is plausibly where the requester's doubt originated — corrected this session.
- **Widening `all()` to a slice removed a compile-time guard** (omitting a variant used to fail to compile at the array's own type) — replaced by an exhaustive-`match` test that fails to compile until a new variant is named, not a length assertion (which "fixes" itself by bumping a number). Filed as a candidate pattern, not a rule: this project's own two-occurrence bar means n=1 doesn't mint.
- Yard added though only km/mi were named — "including" opens a list rather than closing one (the requester's reasoning, adopted). A draft mile-factor comment (`2.1919192e-7`) was wrong; checked value `2.1920595e-7` (`1/4,561,920`) — caught by doing the division, since ISO prints no such constant.

**Still in flight:** nothing — this was a complete, one-commit inbound request.

**For next session:** none owed by this Pass. The reply to `pdfcer-gui` is already filed at `D:\Dev\FeatureRequests\pdfce_FeatureRequests\open\reply_G013_..._SHIPPED.md` (their own channel to archive, not touched here).

**`FEATURES.md`**: *ce dimensions* → new row, unit choice now nine units, citing `Pass 301.0`.

**Sourcing (hard rule 8) — no shell this filing.** Verified independently against the live tree (`Read`/`Grep`): `crates/pdfcer-core/src/dimension/units.rs`'s 9-variant enum, its updated `abbrev`/`baseline_per_point`/`default_format`/`all`/`parse`/`token`, and its four new/changed tests all match the dispatch's account, including the corrected mile-factor comment. The commit hash `d4b5f00` and the test-count/clippy/sabotage verification were supplied by the dispatching engineer's report and not independently re-run or read from the commit message itself.

## 2026-09-12 (531st filing) — `ContentToken`'s optional shrink filed to Backlog, gated on the operator's own stated approval requirement

**Shipped:** nothing — a register-only filing, no commit.

**Decisions made this session:**
- Ken, verbatim: *"Put the shrinking token type the list you use for this sort of thing and note that I must approve it being changed first."* Filed as an unscoped, no-Pass-ID Backlog entry (`docs/ROADMAP.md`) plus open operator question **(cd)** (ceiling `(cc)` → `(cd)`, next free `(ce)`). Default if unanswered: do not change `ContentToken`.

**Findings + decisions:**
- Cross-referenced from `Pass 300.3`'s own *Shipped* entry ("Still open, now correctly ranked" paragraph), so a reader arriving from the shipped work lands on the approval gate rather than re-deriving that the shrink is optional.
- No new Pass ID minted — an approval-gated idea does not get one until approved and scoped, per this role's own standing discipline for unscoped Backlog residue.

**Still in flight:** the `ContentToken` shrink itself remains completely unstarted, pending Ken's answer to (cd).

**For next session:** if Ken approves (cd), scope it into a real Pass (mint an ID, plan the 64-workspace-match-site + `pdfcer-gui` sweep, and measure the text-heavy-file cost before committing to boxing composite operands). If unanswered, leave `ContentToken` untouched — the default holds.

**`FEATURES.md`**: untouched — no capability change.

## 2026-09-12 (530th filing) — the token vector learns each stream's own density instead of doubling; the memory half of the Toronto-map arc closes without breaking `ContentToken`

**Shipped:**
- `865ed7b` (`Pass 300.3`) — `ContentStream`'s token vector no longer relies on `Vec`'s power-of-two doubling for large streams. Below 4,096 tokens it still doubles; above it, capacity is projected from `tokens × total_bytes ÷ consumed_bytes` and reserved exactly, with the safety margin growing (⅛, then ¼) if a projection proves low.

**Decisions made this session:**
- `ARCHITECTURE.md` §12 decision **155** minted — size a growing buffer from the input's own measured density, and prove the heuristic against a counter-sample before trusting it, not only against the file that motivated it. No body section changed: `ContentToken`'s layout, `docs/core-api/`'s description of it, and every call site are untouched — this is an internal allocation-strategy change, not an API or invariant change.

**Findings + decisions:**
- **The planned fix ("shrink `ContentToken`", named by the 527th filing) named the wrong quantity.** Measured first: peak after load 32.7 MB, peak after parsing every form (each dropped immediately) 301.7 MB. The largest single form is 2,291,669 tokens = 139.9 MB used, but `Vec`'s doubling reserved 4,194,304 slots = 256.0 MB — 116.1 MB slack. Only one form is ever live, so the peak is `32.7 + 256.0 + decoded buffer`, closing to within a megabyte. The 241.9 MB "sum of all forms' tokens" the 527th filing named is NOT the peak. It was one vector rounding up to a power of two, not token volume — shrinking `ContentToken` (a breaking change to a `docs/core-api/`-published type, 64 workspace match sites, unknown count in `pdfcer-gui`) would have addressed the smaller half of the wrong number.
- **The adaptivity answers the operator's own request** — *"a way to make these as modifications that get adjusted dependent on what is most optimal for each file that is opened"* — rather than a tuned constant: density (bytes/token) is a property of the CONTENT, not of PDF, so it is measured from the stream being parsed, not hard-coded.
- **Measured on seven files of deliberately different shapes** (slack before → after; worst reserved/used ratio before → after): 372 synthetic fixtures 0.2 MB→0.2 MB (1.00→1.00); Toronto street map 176.8 MB→32.7 MB (1.83→1.20); ncored CAD benchmark 103.9 MB→45.2 MB (1.58→1.36); Kubota concept 27.1 MB→7.1 MB (2.00→1.37); print-industry output suite 11.8 MB→4.5 MB (1.97→1.12); SW41177 10.0 MB→5.1 MB (2.00→1.25); banana-at-scale 7.8 MB→5.6 MB (1.95→1.29); 5518 construction pkg 2.8 MB→2.6 MB (1.97→1.46). Every file improves, none regresses. Parse time improves too (a reallocation copies the whole vector): Toronto ~0.40 s → ~0.23 s, ncored 0.24 s → 0.17 s, each over three runs.
- **The corpus caught what review did not.** The first draft set the minimum capacity to 64 "to save a series of small allocations" — reasoning with nothing behind it. The 372 synthetic fixtures, all small streams, priced it instantly: reserved went 0.7 MB → 1.6 MB, fixing 176 MB on the large file while making every small file worse — the exact failure this Pass set out to avoid. At 4 (what `Vec` would have done anyway), small streams are untouched. Judged a SIBLING finding to `D:\dev\rag\rust\a_plausible_explanation_that_predicts_the_right_order_of_magnitude_is_not_a_diagnosis.md`, not a further instance — that file is about a plausible cause turning out to be the wrong CAUSE; this one is about a plausible heuristic having no COUNTER-sample. New file: `D:\dev\rag\rust\a_heuristic_tuned_on_the_motivating_case_needs_a_counter_sample_before_it_ships.md` (index entry added same filing).

**Still in flight:** shrinking `ContentToken` remains open, now correctly ranked — worth a further ~70 MB on this file, still a breaking change, and the SECOND step rather than the first. Unscoped, no Pass ID.

**For next session:** the Toronto-map arc (`Pass 300.0`–`300.3`) is now closed in full — three time fixes plus this memory fix, with only the optional `ContentToken` shrink left open. `docs/NEXT_SESSION.md` (engineer-owned) still named "shrink `ContentToken`" as the fix in its STILL OWED section as of this filing — flagged, not edited: that framing is superseded by this Pass, though the SECOND-STEP `ContentToken` work it also owed is still real.

**Sourcing (hard rule 8) — no shell this filing.** `.git/refs/heads/main` reads `865ed7b998a9bc4594430f3544badc9badbfff3c`, matching `.git/logs/HEAD`'s final reflog line, one commit past `cd8dd37b61ddbd8b1fcf57d970496af4369617ea` — `.git/refs/remotes/origin/main` reads that same `cd8dd37`, confirming `865ed7b` is local and unpushed. `.git/COMMIT_EDITMSG` (the tip's own message, retained) carries this commit's message in full, read directly, not relayed. The seven-file table, the peak/slack figures and the parse-time deltas are taken from the commit message as authoritative. The "workspace suite green (250 test binaries, 0 failed) plus 183 doc-tests; clippy and fmt clean" verification was supplied by the dispatching engineer's report rather than independently re-run or found in the commit message itself.

## 2026-09-12 (529th filing) — a transparency group composites over its own bbox: 783.54 s → 8.02 s at 4×, same pixels

**Shipped:**
- `6ff57ab` (`Pass 300.2`) — `composite_group_result` blended the WHOLE PAGE back into the canvas for every transparency group. Narrowed to the group's own `/BBox`: `clone_rect` + an offset `draw_pixmap`, no coordinate translation, same page-sized paint buffer as always.

**Decisions made this session:**
- `ARCHITECTURE.md` §12 decision **154** minted — a forward pointer from decision 068 (whose page-sized-buffer reasoning is untouched: this Pass narrows the composite-BACK step only, not the paint buffer) recording the reusable mechanism: narrow a full-canvas composite to its provably-inert region, and prove "provably" with a `debug_assert!` of the exact predicate rather than trust in the spec table alone. §3's transparency-group body entry amended in place with the matching note.

**Findings + decisions:**
- **The measurement**: scale 1.0 `54.58 s → 2.45 s` (hash `b98327ee43aa5601`, identical); scale 4.0 **`783.54 s → 8.02 s`** (hash `0587ca929c10693b`, identical). The 4× figure answers the operator's own report — thirteen minutes to eight seconds.
- **This is the confirmed third occurrence of one root cause independently misdiagnosed**, named by the 528th filing immediately below: a per-group full-page buffer allocation was named as the cost by three separate sessions over a month, and never was. This filing's probe returned early from `composite_group_result` and TIMED the rest rather than reading the code: whole render 54.94 s; composite skipped 2.33 s; the per-group `Pixmap::new` all three prior diagnoses named, pooled instead, 53.18 s (essentially unchanged). 52.6 of 54.9 seconds was the composite call; the allocation was 1.8 s of it.
- **Correctness argument**: outside a group's `/BBox` the buffer is never painted into (alpha zero), and all sixteen of Table 136's blend modes composite as `B(Cb,Cs)` under source-over alpha, returning the backdrop unchanged at `αs = 0` — arithmetic whose answer is already in `dest`.
- **Excluded on purpose, not by oversight**: tiny-skia's destructive Porter-Duff modes (`Clear`/`Source`/`DestinationIn`/similar) are matched out explicitly, not defaulted past; soft masks, non-separable modes, and groups covering more than half the page keep the original path; knockout/CMYK groups composite through `compositor`/`CmykBuffer`, never `draw_pixmap`, so this Pass never touches them.
- **The safety net**: `nothing_painted_outside`, a `debug_assert!` after every group, checked across all 409 render unit tests and 364 synthetic fixture renders — never fired. Its rectangle is the SAME one the viewport cull already computes, hoisted rather than re-derived, so the composite's copy cannot silently be the larger of two disagreeing derivations.

**Still in flight:** the memory-side fix (shrinking `ContentToken`, 64 B today) named by the 527th filing remains unscoped to a Pass ID; unrelated to this Pass.

**For next session:** none — this closes the arc `Pass 300.0`/`300.1`/`300.2` opened this evening from the operator's Toronto-map investigation. `docs/NEXT_SESSION.md` is engineer-owned; flagged, not edited, that its "unstarted" note on per-bbox group buffers is now stale (the composite-back half shipped; only `ContentToken` shrinkage remains open).

**Sourcing (hard rule 8) — no shell this filing.** `.git/refs/heads/main` reads `6ff57abee11832a56dc2b0bf0d6eb1dee5d204b2`, matching `.git/logs/HEAD`'s final reflog line, one commit past `a66dd5f7bf7d370dd26e2f1ddf56ae3025d428f1` (the 528th filing's own commit) — `.git/refs/remotes/origin/main` reads that same `a66dd5f`, confirming `6ff57ab` is local and unpushed. `.git/COMMIT_EDITMSG` (the tip's own message, retained) carries this commit's message in full, read directly, not relayed. All timing figures, hashes, and the 364/409 test counts are taken from the commit message as authoritative, not independently re-run this filing.

## 2026-09-12 (528th filing) — a discarded full-page scan per transparency group: 4%, and the 4% is the finding

**Shipped:**
- `1295af1` (`Pass 300.1`) — `Canvas::group`'s `Paint` arm computed `backdrop_present` (a full-page pixmap scan) on every transparency group and discarded it on 6,173 of 6,174 (`||` short-circuit skips it whenever the group is isolated). Moved the expression inside the `if`; semantics provably identical, only WHEN it runs changed.

**Decisions made this session:** none new — judged an existing rust-RAG methodology finding recurring a third time, not a new pdfcer standing rule (see below).

**Findings + decisions:**
- **A doc-comment prediction was wrong by two orders of magnitude, and is kept visible on purpose.** "~20x" was written before measuring; the A/B (same binary, same machine, through a throwaway `pdfcer-render` release test) measured **4%** — scale 1.0 went 51.13 s → 49.23 s, scale 0.5 was unchanged at 12.78 s.
- **Second same-session, same-direction misattribution.** `Pass 300.0` (previous entry) attributed 307 MB peak RAM to image decoding and was also wrong when measured. Both times an `O(page)`-sized operation, read off the code rather than timed, was assumed to be the hot cost.
- **This is arguably a third occurrence of one specific root cause being independently rediscovered under a new wrong mechanism.** `D:\dev\rag\rust\a_plausible_explanation_that_predicts_the_right_order_of_magnitude_is_not_a_diagnosis.md` (2026-08-21, a per-pixel-merge-loop hypothesis) and `ablate_the_suspect_to_find_the_floor_before_optimizing_anything.md` (2026-08-07, a clip-machinery hypothesis) already record the same underlying fact about this codebase: a transparency group's full-page-sized buffer allocation dominates its render cost, not whatever loop looked suspicious that day. Appended a dated third instance to the first file rather than minting a new pdfcer standing rule — the methodology is already on the books twice.
- **Where the time actually is, now recorded in the code comment**: render time scales with page area; `Pixmap::new` + the full-canvas `draw_pixmap` in `composite_group_result` are the per-group, area-proportional costs, ~8 ms/group across ~2M pixel operations × 6,174 groups on this file. The real fix — a per-`/BBox`-sized buffer per group — is unstarted, needing the interior's CTM and clip masks translated with it.
- `pdfcer-cli` cannot be release-linked on this machine at all (four OOM-killed builds this session); the A/B was taken through a scratch `pdfcer-render` release test, deleted before the commit and not reproducible without rebuilding it.

**Still in flight:** both real fixes from `Pass 300.0`/`Pass 300.1` combined — shrinking `ContentToken` (memory) and per-`/BBox` group buffers (time) — remain unscoped to a Pass ID.

**For next session:** flag `docs/NEXT_SESSION.md` (engineer-owned) to add that this machine's cargo OOM now blocks release-linking `pdfcer-cli` outright, and that benchmarking should go through a `pdfcer-render` release test instead.

**Sourcing (hard rule 8) — no shell this filing.** Verified via `Read` on `.git` internals: `.git/refs/heads/main` and `.git/logs/HEAD`'s final reflog line both read `1295af1a94a69c6a85259ad36a6363fec1081a8a`, one commit past `ac65d41db1ef06614c69761f36e1dea1b927f049` (the 527th filing's own commit) — `.git/refs/remotes/origin/main` reads that same `ac65d41`, confirming `1295af1` is local and unpushed. `.git/COMMIT_EDITMSG` (the tip's own message, retained) carries this commit's message in full, read directly, not relayed. The timing figures, the ~8 ms/group estimate and the 364-fixture/409-test verification are taken from the commit message as authoritative, not independently re-run this filing.

## 2026-09-12 (527th filing) — an image off the viewport is skipped now; the investigation that asked for it was wrong about why the file is expensive

**Shipped:**
- `8d78770` (`Pass 300.0`) — an image `Do` whose unit square, mapped through the CTM, lands entirely outside the canvas/clip is now skipped before any sample byte is touched, the image-side twin of the `Pass 74.x` form-XObject cull (§8.9.5.2 vs §8.10.1). Measured on the operator's own "Toronto street map" PDF (7.9 MB, 25,246 objects): a 400×200 px region went from 1,182/1,183 images decoded to 92 decoded / 1,090 culled. `images_culled` reported beside `images` on the metrics line, kept as its own counter rather than folded into `forms_culled`.

**Decisions made this session:**
- No new `ARCHITECTURE.md` §12 decision. Checked the "keep the cull counter separate" reasoning against decision 115's `icc_managed_paints`/`icc_unmanaged_paints` pair and judged it an instance of that discipline (a merged counter loses information a reader needs), not a new invariant.

**Findings + decisions:**
- **The investigation that asked for this Pass was wrong about the memory number, and the correction matters more than the fix.** It attributed the file's 307 MB peak to image decoding. Measured after this Pass shipped: peak memory is UNCHANGED at 307 MB, and `extract-text` — which rasterises nothing — peaks at the identical 307 MB. The real cause is `ContentToken` volume: the file's form XObjects hold 20.0 MB of content parsing into 3,962,903 tokens at 64 bytes each = 241.9 MB (`ContentToken` = 64 B, `ContentTokenKind` = 48 B, `Object` = 40 B). One form alone is 2,291,669 tokens.
- **The 55 s full-page render is a third, separate cause**: 6,174 transparency groups each allocating a full 1224×792 canvas (3.88 MB) — ~24 GB of allocate-and-zero, a TIME cost (each buffer frees before the next), not a memory one.
- **Re-scoping for the next session picking this up**: the original investigation's finding (1) "image decode cache" is not the memory problem and should be re-aimed or dropped; the real memory work is shrinking `ContentToken`. Finding (3) stands, but as a time fix (size each transparency-group buffer to its own bounding box), not a memory fix. No new Pass ID minted for either — left for the engineer's scoping when picked up.
- **Methodology lesson, cost real work this session.** The first regression baseline rendered 114 fixtures and stopped at `fontinfo` alphabetically, missing `images`, `transparency`, `overprint` and `shading` — exactly the directories the change touches. A baseline that omits the dirs a change touches certifies nothing. Real verification: stash/rebuild/re-render of all 364 synthetic fixtures, byte-compared identical.
- `R225` gains a further instance: the gate was sabotaged three ways (never fires, always fires, clip intersection skipped), each turning a different assertion red.

**Still in flight:** unchanged from the 526th filing below — the fixture-path-vs-guard-clause judgement call, and `docs/NEXT_SESSION.md`'s stale skip-count wording, both still owed to the engineer.

**For next session:** shrink `ContentToken` (the actual memory fix); size transparency-group buffers to their own bbox (the actual 55 s fix) — neither scoped to a Pass ID yet.

**Sourcing (hard rule 8) — no shell this filing.** `.git/refs/heads/main` reads `8d7877090ae23732d4a9acae31a17009977b3a4a`, matching `.git/logs/HEAD`'s final reflog line, one commit past `fcdfff4f9819816f234b74a0fd5320556bdf2d0b` — `.git/refs/remotes/origin/main` also reads `fcdfff4f9819816f234b74a0fd5320556bdf2d0b`, confirming `8d78770` is local and unpushed. `.git/COMMIT_EDITMSG` (the tip's own message, retained) carries this commit's message in full, read directly, not relayed. The 364-fixture/409-test verification figures and the `canvas.rs:1626` citation are taken from the commit message as authoritative, not independently re-run or re-read from source this filing.

**★ AMENDMENT 2026-09-12 (530th filing).** This entry's own "the real memory work is shrinking `ContentToken`" (above) and its "For next session" line naming the same were the plan of record, stated before it was measured. Measuring first (`Pass 300.3`, `865ed7b`) found the peak was one `Vec` rounding a 2.29M-token count up to the next power of two, not token volume — the 241.9 MB figure two lines up was never the peak, because only one form is ever live. `ContentToken` was not touched; the fix was an adaptive capacity reservation instead. Kept legible above rather than rewritten, per this project's history discipline. Source: `865ed7b`'s commit message and `docs/ROADMAP.md`'s `Pass 300.3` entry.

## 2026-09-12 (526th filing) — the re-check the previous filing told itself to do, run within the hour, found a defect the previous filing itself had left

**Shipped:**
- `05b3a80` — re-checked the remaining 8 declared skips for the same shape (`d291a03`'s own closing suggestion). Two converted: `structure_inspect::an_object_inside_an_object_stream_is_reachable_and_located` was still SKIPping despite `d291a03` repointing its fixture path an hour earlier, because a second guard clause (`if l.object_streams.is_empty() { … return; }`) further down the function was the real gate, not the path — converted to `assert!`. `insert_pages_preserves_undo::inserting_a_form_page_reports_its_orphaned_widgets` needed five hand-authored objects (`doc_with_one_form_page()`), no corpus. `tools/skippable-tests-baseline.txt`: 8 → 6 (26 when the gate shipped).

**Decisions made this session:**
- No new standing rule. Left open for the engineer's judgement: is "fixture path fixed, but a second guard clause below it still declines" a new failure shape, an `R255` instance (test never ran for want of a corpus), or just this entry — `R255`'s mechanism doesn't quite fit, since the corpus was *present* and the test still declined.

**Findings + decisions:**
- **The transferable half, flagged for judgement rather than named:** "converted the fixture dependency, left the decline in place" is a defect invisible from the diff of the fixture-path change alone — the guard clause sits elsewhere in the same function.
- **The handoff note caught its own defect.** `d291a03`'s note said "before hunting any more fixtures, re-check the remaining declared skips" — writing that and then running it immediately, in the same session, is what surfaced a bug the note's own author had left an hour before. A delayed re-check would have carried the cost forward.
- **The remaining 6 are two different kinds of debt, not one pile.** Four (`widget_adoption` census/preview) are corpus-composition tests deliberately left — converting them would measure an invented fixture. Two (`stamp_collection`) read Adobe's own installed stamp files from `%APPDATA%` — the operator's machine, not a corpus at all, so they can never run in CI on any other machine. Flagging this split for `docs/NEXT_SESSION.md` (engineer-owned) so the remaining six aren't treated as one homogeneous debt.

**Still in flight:** `docs/NEXT_SESSION.md`'s OWED list still reads "8 tests silently SKIP" without the four/two split above — flagged for the engineer, not edited here.

**For next session:** the `docs/NEXT_SESSION.md` split-and-count flag above; whether the fixture-path-vs-guard-clause shape found here deserves a standing rule, an `R255` instance, or neither.

**Sourcing (hard rule 8) — no shell this filing.** `.git/refs/heads/main` reads `05b3a805f06f27a52ba742ea7308f8f3306a17eb`; `.git/logs/HEAD`'s final reflog line names this commit's subject verbatim, one step past `c37b63f` (the 525th filing's own commit). `.git/refs/remotes/origin/main` reads `c37b63fb2929d5f07729626b30f0d2175850f4f6`, one commit behind — confirming `05b3a80` is local and unpushed. `.git/COMMIT_EDITMSG` (the tip's own message, retained) carries this commit's message in full, read directly, not relayed. Independently verified against live source: `crates/pdfcer-core/tests/structure_inspect.rs:307-322` and `crates/pdfcer-core/tests/insert_pages_preserves_undo.rs:367-382` both match the commit message's account; `tools/skippable-tests-baseline.txt` counted directly at 6 entries (4 `widget_adoption`, 2 `stamp_collection`).

## 2026-09-12 (525th filing) — the last two of the 26-test skip debt's easy wins: a CC BY 4.0 veraPDF fixture, and a test that never needed a corpus at all

**Shipped:**
- `39759d2` — investigation, no code: the three remaining qpdf-gated skips (`editable_roundtrip.rs` ×2, `structure_inspect.rs` ×1) all need a document using object streams, which pdfcer's writer cannot produce (it only ever decompresses `/ObjStm`) and no synthetic fixture contains. Recorded the three measured facts in `docs/NEXT_SESSION.md` rather than hand-authoring a fixture under time pressure.
- `d291a03` — the operator asked whether a suitable PDF could be found online, which changed the answer. `LEGAL.md` §5 already approves veraPDF's open corpus as a fixture source; took the smallest valid file containing an `/ObjStm` (11,516 bytes, CC BY 4.0, attributed in new `fixtures/verapdf/PROVENANCE.md`) for two of the three. The third (`an_encrypted_document_is_refused_rather_than_decrypted`) needed only *an* encrypted document — `fixtures/synthetic/encryption/` already had eight — and never needed a corpus. `tools/skippable-tests-baseline.txt`: 10 → 8 (26 when the gate shipped).

**Decisions made this session:**
- No new standing rule. Whether "a test's declared corpus dependency can be narrower than the property it actually needs" deserves a check is left at n=1, per the operator's own suggestion to re-ask it against the remaining 8 declared skips before hunting more fixtures — not named here.

**Findings + decisions:**
- **First category-(b) tracked fixture in the repo.** `fixtures/verapdf/object-streams.pdf` is CC BY 4.0 inside an MIT repository — permitted under `LEGAL.md` §5, dev-time only, never shipped (`tools/package-portable.py` ships no fixtures). Flagged for the operator (not edited): whether `LEGAL.md` should gain an explicit line recording that a non-MIT file now exists in the tree, since §5 approved the *source* but no prior filing had actually landed one.
- **The PDF Association's own PDF 2.0 examples were checked first and had none** — worth recording that "the obvious cleanest source had nothing" before anyone looks there again.
- **The shape of the two-commit pair**: an investigation that stops with three recorded facts, made cheap by one operator question. Left as an observation, not named as a pattern.

**Still in flight:** `docs/NEXT_SESSION.md`'s OWED list still reads "10 tests silently SKIP" and still carries the now-resolved "budget it properly" paragraph for the qpdf-gated three — both stale as of `d291a03`, flagged for the engineer (engineer-owned file, not edited here). 8 skippable-test entries remain: 3 qpdf-gated (blocked, see above), 4 `widget_adoption` census/preview left deliberately, 1 other.

**For next session:** the `docs/NEXT_SESSION.md` staleness flag above; whether the "declared dependency vs. actual dependency" check is worth running against the remaining 8 skips before any further fixture work.

**Sourcing (hard rule 8) — no shell this filing.** `.git/refs/heads/main` reads `d291a03f4069f1af8e9519f92c21531216365296`; `.git/refs/remotes/origin/main` reads `cb5a0bec8aa4c696cddd139848d63b88e21de257` (the 524th filing's own commit), confirming both `39759d2` and `d291a03` are local and unpushed. `.git/logs/HEAD`'s final two reflog lines name both subjects verbatim. `.git/COMMIT_EDITMSG` (the tip's own message, retained) carries `d291a03`'s message in full, read directly. `39759d2`'s loose git object exists but is zlib-compressed and unreadable without a shell; its account above is reconstructed from `docs/NEXT_SESSION.md`'s live OWED-list text (the artifact that commit wrote) and from the operator's own relay, not from an independent read of the raw commit message — flagged as such rather than presented as a direct read. Independently verified against the live tree: `fixtures/verapdf/PROVENANCE.md` exists and states CC BY 4.0; `tools/skippable-tests-baseline.txt` counted directly at 8 entries (11 lines including its 3-line header comment).

## 2026-09-12 (524th filing) — eight more merge-document tests run now (18 → 10 skippable entries); a misaimed sabotage and a completion signal from `clippy::dead_code`

**Shipped:**
- `e41892a` — `crates/pdfcer-core/tests/merge_document.rs`'s 8 pdfbox-corpus SKIPs are gone; a new `synthetic_acroform()` (12 fields / 13 widgets, one two-widget radio group, `/NeedAppearances`, `/SigFlags`) supplies what all eight actually read. File now runs 16/16, zero skips; the corpus-path const is deleted. `tools/skippable-tests-baseline.txt`: 18 → 10 (26 sites when the gate shipped).

**Decisions made this session:**
- No new standing rule. The misaimed first sabotage (broke `named_destinations_renamed`, an unrelated destinations test went red instead of `fields_renamed`) is `a_sabotage_that_does_not_compile_or_change_behavior…md`'s existing cause-5 family ("the sabotage fired and the wrong oracle answered") — a dated instance appended to that rust-RAG file, not a new pdfcer `R225`/`R255` instance (neither mechanism matches: this test genuinely ran).
- Convertibility criterion — "are the asserted numbers a property of the FIXTURE or of the corpus?" — used a second time (first at `f0d1dc7`). Left as a decision heuristic, not minted; would need a third occurrence.

**Findings + decisions:**
- **New rust-RAG finding:** `clippy::dead_code` naming a corpus-path constant unused is a compiler-verified completion signal for a corpus-to-synthetic-fixture conversion, stronger than counting a diagnostic string (`SKIP`) by hand across test output. `D:\dev\rag\rust\a_dead_code_lint_is_a_compiler_verified_completion_signal_for_a_fixture_migration.md`, indexed.
- **A commit-message arithmetic slip caught against the file it describes, not propagated.** `e41892a`'s own message says "the remaining debt is 8: three qpdf, four `widget_adoption`, and one other" (3+4+1=8) in the same paragraph as "18 → 10." `tools/skippable-tests-baseline.txt`, read directly, has 10 entries — 4 `widget_adoption`, 6 others (3 qpdf-gated, 3 not). "One other" should read "three others." Corrected in the `ROADMAP.md` entry rather than relayed as-is (hard rule 10 — a total and a per-item breakdown are the same fact in two forms, and this pair disagreed).
- One test hard-codes the field name `TextField`/`TextField_2` because the test itself asserts that literal suffixing behaviour (`merging_a_document_into_itself_renames_every_collision`); the fixture was named to suit the assertion, not the reverse. Worth a sentence, not a rule.

**Still in flight:** `docs/NEXT_SESSION.md`'s owed-list line ("16 tests silently SKIP … down from 26") is now stale — it predates both `f0d1dc7` (18) and this filing (10) — flagged for the engineer, not edited here (engineer-owned file). Remaining 10 skippable-test entries: 3 qpdf-gated, 4 `widget_adoption` census/preview left deliberately (assert the real AcroForm's own composition), 3 others.

**For next session:** the `docs/NEXT_SESSION.md` stale-count flag above. Whether the fixture-subject-vs-corpus-setting criterion recurs a third time is worth watching before naming it.

**Sourcing (hard rule 8) — no shell this filing.** `.git/refs/heads/main` reads `e41892ac703a0c13e201ee5175b518e34dcb5191`; `.git/refs/remotes/origin/main` reads `7254e93e2cc1b11682c160d83ff7834d41678ee0`, one commit behind — confirming `e41892a` is local and unpushed. `.git/logs/HEAD`'s final line names this commit's subject verbatim, matching the account above. `.git/COMMIT_EDITMSG` (the tip's own message, retained) carries `e41892a`'s full message, quoted directly rather than relayed. Independently verified against the live tree: `crates/pdfcer-core/tests/merge_document.rs:74` (`synthetic_acroform`), no remaining reference to `fixtures/external/pdfbox` in that file; `tools/skippable-tests-baseline.txt` counted directly at 10 lines.

## 2026-09-12 (523rd filing) — the last owed off-page residuals are a classification gap, not an incomplete cut; two register documents corrected

**Shipped:**
- `7a22c523` — doc-comment-only: confirms the 12 `partial` off-page residuals owed since `Pass 297.0` are correct behaviour being counted as a finding — `covered_cells` snaps outward so their off-page ink is already gone, but `scan-offpage` classifies by geometry (bounding box still crosses the page edge) and re-detects its own successful cut. The analogous fix (count by ink, not geometry) is deliberately not taken — it needs the decoded samples, and decoding every image during a scan is what made `redact-offpage` take ten minutes on one file (`Pass 294.1`). Disclosed as a doc comment on `OffPageObject` pending a real measurement.

**Decisions made this session:**
- No standing rule minted for "a classifier counts geometry where the operator's question is about ink" (2nd instance, alongside `Pass 294.2`'s empty text husk) — flagged for a future filing's judgement rather than named here; the two agree on symptom but diverge on remedy (294.2's fix was cheap, this one is refused on measured decode cost).

**Findings + decisions:**
- **Register correction.** `Pass 297.0`'s `ROADMAP.md` entry and `FEATURES.md:331` both described the remaining 12 objects as "a cut that leaves a sliver," which reads as an incomplete cut — the cut is complete, the scan's classification is what remains. Both corrected in place, struck-and-visible.
- The obvious hypothesis — "the sliver is too thin to clear" — is the opposite of the truth; `covered_cells` rounds outward by construction. Checking the rounding direction rather than reasoning about it kept a false diagnosis out of the record.
- Housekeeping: a `grep.exe.stackdump` crash artifact left in the shared `FeatureRequests` channel (read by both `pdfcer` and `pdfcer-gui`) was removed.

**Still in flight:** unchanged from the 522nd filing — pre-push wording flag in `docs/NEXT_SESSION.md` still owed; corpus-gated tests remain declared-skip, not run.

**For next session:** correct `docs/NEXT_SESSION.md`'s "sliver" wording to match the `ROADMAP.md`/`FEATURES.md` correction above (engineer-owned, not done here). Whether the "classifier counts geometry, not ink" pattern (2 instances) is worth a standing rule is the engineer's call.

**Sourcing (hard rule 8) — no shell this filing.** `.git/refs/heads/main` reads `7a22c523cc74aed8495aa1ea69685d249b888048`; `.git/COMMIT_EDITMSG` (the tip's own message, verbatim) matches the account above and was read directly, not relayed. Not checked against `origin/main` — no shell available to this filing.

## 2026-09-12 (522nd filing) — a third baked-in string gap, same cause as the 511th filing's own note

**Shipped:**
- `2f67b63` — one-line fix: `check-string-gaps.sh` went red in CI on the previous push because a python heredoc ate a line-continuation backslash in `Pass 299.0`'s new test, leaving ten literal spaces mid-sentence in an assertion message.

**Decisions made this session:**
- No new rule minted. This is a dated recurrence of the working-method note already on record at the 511th filing (and `R243`'s 492nd-filing dated instance) — `tools/edit-source.py` exists for exactly this failure mode and was not reached for, three times in one session now.

**Findings + decisions:**
- **A gate run before the session's last edit is a gate that did not run.** `check-string-gaps.sh` was clean after the fix that closed the first two gaps (`f16e266`/`5917ece`), then more code was written and the gate was not re-run before pushing — which is exactly how the third instance of the identical defect reached CI. Flagged for `docs/NEXT_SESSION.md`'s pre-push wording (engineer-owned): it currently says sweep before pushing, not "after your last edit."

**Still in flight:** unchanged from the 521st filing.

**For next session:** the pre-push wording flag above, for the engineer to act on in `docs/NEXT_SESSION.md`.

**Sourcing (hard rule 8) — no shell this filing.** `.git/refs/heads/main` reads `951eba3216765c10c2a74323a7051ba61700c254`; `.git/refs/remotes/origin/main` reads `f8e54930d7f0915b59657583a08ff79bb409cad4`, four commits behind — confirming `2f67b63` is local and unpushed, not taken on the dispatch's word. `.git/logs/HEAD`'s reflog line names `2f67b63`'s subject verbatim, matching the account above; `.git/COMMIT_EDITMSG` retains only the tip (`951eba3`) and does not carry this commit's own message. The ten-space/`Pass 299.0` diagnosis is taken from the dispatch as authoritative, not independently re-run through `check-string-gaps.sh` from here.

## 2026-09-12 (521st filing) — two docs-only corrections, both from `pdfcer-gui`'s consumption notes on the 520th filing's own work

**Shipped:**
- `f756d60` — doc comment on `EditSession::adopt_preview` (`edit.rs:40985-41007`): `adopt_preview` shares `adopt_plan`'s whole body with the writes dropped, so a guard's PLACEMENT inside this crate decides which of a shell's surfaces has to explain a refusal — inside the plan, a hover before a click; outside it, a status line after one. `pdfcer-gui`'s `G011` note reported this as a gap and got agreement from the earlier reply; they measured the next morning and found `Pass 298.0`'s `reject_dotted_partial` had been inside `adopt_plan` since the day it shipped, so their tab-order name box had been greying on a dotted name the whole time with no wording for why.
- `82e988e` — `docs/core-api/03-capabilities.md`'s authority note (~line 3467, "`FEATURES.md` is authoritative") gains the clause it was missing: a correction landed in the mirror against a measurement is HALF-FINISHED until it lands in `FEATURES.md` too. Bit within six hours of being published (`G008`'s answer), on the exact row `4ba7202` (previous filing) fixed.

**Decisions made this session:**
- **Declined to unify the two findings under one named pattern.** Both are "a correct, local change whose consequence crossed a boundary nobody was watching," but the mechanisms differ — `f756d60` is disclosure-*placement* (which surface explains a refusal), `82e988e` is document-*mirror staleness* (a rule pointing at a source nobody re-read). Per the standing pattern-naming discipline ("would fixing one have prevented the other?") the answer is no, so they stay two entries rather than one rule.

**Findings + decisions:**
- **Reusable, attributed to `pdfcer-gui`, filed to `D:\dev\rag\rust\`:** a private predicate used at several call sites is several behaviours until something forces them to agree, and the thing that forces it is typically a request for a *public* function, not a test of the rule — `Pass 299.0` (`766c52a`+`7a0a9c2`, prior filing) is the worked instance. New file `D:\dev\rag\rust\a_private_predicate_with_several_callers_is_several_behaviours_until_a_public_wrapper_forces_them_to_agree.md`; `index.md` bulleted.
- **Channel-register note, not filed as a rule.** `pdfcer-gui` sent its `82e988e`-prompting quote as agreement ("we have the scar too"), not as advice — it named a shared failure mode from its own history (three documents quoting each other's counts, bitten seven times) rather than proposing policy for this project. Recorded here as a property of the channel worth preserving, not a mechanism to formalise.
- **Both corrections trace to the 520th filing's own work landing hours or days earlier** — `82e988e` bit the authority note the same session it was written; `f756d60` surfaced a guard that had been silently live since `Pass 298.0` (previous day). Neither is a new defect in shipped behaviour; both are the record catching up to what the code already did.

**Still in flight:** unchanged from the 520th filing — 16 corpus-gated tests remain (`merge_document` 8, `editable_roundtrip` 2, `insert_pages_preserves_undo` 1, `structure_inspect` 1, `widget_adoption` 4 declared), plus the backup-bundle and standing-rule-enforcement debt carried in `docs/NEXT_SESSION.md`.

**For next session:** `f756d60`'s finding names no owed pdfcer-core work — the guard is correct, only its documentation was missing. Whether the consuming shell wants a hover string for the newly-disclosed refusal is theirs to scope, not filed here. Push is pending on the operator's own go-ahead per this filing's dispatch instructions (three unpushed commits at `82e988e`: `f8e54930`, `f756d60`, `82e988e`).

**Sourcing (hard rule 8) — no shell tool this filing.** `.git/refs/heads/main` and `.git/logs/HEAD`'s final line both read `82e988eec3ed228c59d6d70336b98e5572b7d581`; the two prior reflog lines give `f8e54930…`→`f756d60d3792d568a952d5849d698d4f7c09812c`→`82e988e…`, subjects matching both accounts. `.git/COMMIT_EDITMSG` (tip only) carries `82e988e`'s message verbatim; `f756d60`'s account is taken from live source (`edit.rs:40985-41007`, read directly) since its own message is not retained anywhere this role can reach without a shell. Push state relative to `origin/main` not independently re-derivable without a shell — not asserted.

## 2026-09-12 (520th filing) — the partial-name rule made askable, and asking it found two of this crate's own bugs

**Shipped:**
- `4ba7202` — `FEATURES.md:331`'s off-page `gui` box ticked (`G012`), closing a same-day contradiction where `03-capabilities.md`'s appendix had been ticked to `x` for the same capability while its own authority note said `FEATURES.md` wins on disagreement — and `FEATURES.md` still read `[ ]`.
- `Pass 299.0` (`766c52a` + `7a0a9c2`) — `FormAuthorError::PeriodInPartialName` renamed to `EmptyNameSegment` (its actual trigger; **breaking**, taken because it costs nothing today) and `forms_author::validate_partial_name` made public, consolidating three private enforcement sites behind one predicate. Both from `pdfcer-gui` reports (`G010`, `G011`) against the `Pass 298.0` guard shipped the day before.

**Decisions made this session:**
- Renamed rather than re-documented `PeriodInPartialName` → `EmptyNameSegment`, because the doc comment was the third wrong thing and the name would still have lied; taken as a breaking `pdfcer-core` change now (zero matches in `pdfcer-gui`/`pdfcer-cli`) rather than deferred.
- No `CHANGELOG.md` exists in this project. Per the existing convention (`ARCHITECTURE.md`'s `ClipAnnotation::Markup` decision, §12), a breaking API change is recorded in its own `ROADMAP.md` Shipped entry and absorbed by the next Cargo 0.x minor bump — the workspace's breaking slot, not yet cut (still `0.53.0`).

**Findings + decisions:**
- **Consolidating three enforcement sites behind one predicate surfaced a live divergence nobody had reported:** `reject_dotted_partial` tested `contains('.')` only, so `adopt_widget` and `sign` accepted `"a..b"` where `rename_field` refused it. Fixed by construction (all three now share `single_segment`) and pinned by a test that drives the same strings through the validator and every verb, requiring agreement.
- **Judged not a new rule (n=1):** "make a rule askable, and enforcement sites converge or reveal they hadn't" — a good habit, not yet a second instance of anything already on the books. This session had already declined three unifications on the same distinction (a shared symptom is not a shared mechanism).
- **First draft of the consolidation test was wrong, on-topic:** it paired the validator with `add_text_field`, which takes a fully-qualified *name* rather than a *partial* one — failed immediately, an hour after `G010` was about exactly that distinction. Recorded in the test rather than quietly fixed.
- **The doc-block-splice gate caught its author within hours of shipping.** Inserting `validate_partial_name`/`single_segment` above `split_field_path` orphaned that function's doc block; `check-public-fns-documented.py` named it in one read, and it was reattached in the same commit. A further dated instance of the class `check-doc-block-spliced.py` (513th filing, `3334377`) exists to catch — this time working correctly on its own author within the same session.
- **A correction measured against source has to land IN the file it corrects, not merely point a reader there.** `4ba7202`'s finding: an authority note in `03-capabilities.md` said `FEATURES.md` wins on disagreement, while the fix that prompted writing that note never touched `FEATURES.md` itself. Not minted as a rule — restates an authority note already on the books.

**`FEATURES.md`**: `docs/FEATURES.md:331` (off-page) ticked `gui [x]`; three existing rows extended in place, no checkbox change — *Rename a field* (line 306), *Adopt an existing widget* (line 318), *Sign a document (APPROVAL, PAdES B-B)* (line 341) each gain a `Pass 299.0` clause.

**Sourcing note (hard rule 8):** no shell this filing. `.git/refs/heads/main` reads `4ba7202906481b0265a2ab1aa61577c1536b0392`; `.git/logs/HEAD`'s last three lines give the chain `766c52a5…`→`7a0a9c2e…`→`4ba72029…`, all local and unpushed (no change to `.git/refs/remotes/origin/main` this filing). Only the tip's own `COMMIT_EDITMSG` was read verbatim; the account of `766c52a`/`7a0a9c2` is taken from the two `pdfcer-engineer` reply files in the feature-request channel (`reply_G010_renamed_to_EmptyNameSegment_SHIPPED.md`, `reply_G011_validate_partial_name_is_public_SHIPPED.md`), cross-checked against live source. Independently verified: `FormAuthorError::EmptyNameSegment` at `crates/pdfcer-core/src/forms_author.rs:377` (no `PeriodInPartialName` remaining); `validate_partial_name`/`single_segment` at lines 465/475, doc comment naming `Pass 299.0`; `rename_field`'s use of `single_segment` at `crates/pdfcer-core/src/edit.rs:24753`; `docs/FEATURES.md:331` reads `[x] | [x] | [x] | ?`.

**Still in flight:** unchanged from the 519th filing — 16 corpus-gated tests remain (`merge_document` 8, `editable_roundtrip` 2, `insert_pages_preserves_undo` 1, `structure_inspect` 1, `widget_adoption` 4 declared), plus the backup-bundle and standing-rule-enforcement debt carried in `docs/NEXT_SESSION.md`.

**For next session:** flagging for `docs/NEXT_SESSION.md` (engineer-owned, not edited here) — the inbound channel is otherwise answered as of this filing (`G010`, `G011`, `G012` all closed); nothing new queued by this filing.

## 2026-09-12 (519th filing) — ten widget-adoption tests run now, paying down 10 of the debt two filings back

**Shipped:** `f0d1dc7` — `synthetic_orphaned_session()` byte-authors a source with four merged field-widgets (`/FT /Tx`×2, `/Btn`, `/Ch`) and two `/Parent`-ed bare kids in one radio group, inserted into a blank target exactly as `orphaned_session()` does. Ten `widget_adoption.rs` tests converted from the pdfbox-gated fixture to this synthetic one; the file now prints 4 declared skips, not 14. `tools/skippable-tests-baseline.txt` drops from 28 to 18 entries; of the 517th filing's 26-test debt (23 pdfbox / 3 qpdf), 16 remain, all outside this file.

**Findings + decisions:**
- **The shapes were not guessed.** Four tests failed on the first cut of the fixture, each naming the premise it needed; the fixture was grown to satisfy the assertions, not the reverse — the risk this conversion runs, and the failures are the evidence it didn't happen.
- **Sabotage confirms it.** Breaking `adopt_widget`'s rename report now turns three tests red; before this commit it turned none, because none ran.
- **Four tests deliberately NOT converted** — `the_fixture_carries_both_widget_shapes_and_the_counts_agree` and the three preview tests assert against the real AcroForm's own composition; converting them would measure an invented document rather than the verb, so they stay a declared skip.
- **The file's own stated caution answered by the file itself:** its header argues a hand-built fixture "exercises whichever shape the author thought of" — true, and `a_widget_with_ft_but_no_t_is_still_unrecoverable` is the file's own rebuttal, since the real corpus's bare radio kids can't distinguish `/FT` from `/T` either. Neither fixture source is automatically better.

**`FEATURES.md`**: unchanged — internal test-harness debt.

**Sourcing note (hard rule 8):** no shell this filing. `.git/refs/heads/main` reads `f0d1dc7bdcf75985ac9bfce89ecd184392f290cc`; the loose object exists at `.git/objects/f0/d1dc7b…`, confirming it is current. `.git/COMMIT_EDITMSG` carries this commit's message verbatim, the source for the account above. Independently verified against the live tree: the four named functions in `crates/pdfcer-core/tests/widget_adoption.rs` (lines 197/261/345/541); `tools/skippable-tests-baseline.txt`'s 18 entries, 4 of them naming this file.

**Still in flight:** 16 corpus-gated tests remain (the 517th filing's debt minus this paydown) — `merge_document` 8, `editable_roundtrip` 2, `insert_pages_preserves_undo` 1, `structure_inspect` 1, plus `widget_adoption`'s remaining 4 declared skips (not corpus-fixable; deliberately left on the real AcroForm).

**For next session:** flagging for `docs/NEXT_SESSION.md` (engineer-owned, not edited here) — the OWED line there naming 26 corpus-gated tests should be corrected to 16, with `widget_adoption` struck as closed to the extent a synthetic fixture can close it.

## 2026-09-12 (517th filing) — a test that can decline to run reports as a PASS, and it had never run anywhere

**Shipped:** `2d2e217` — `tools/check-skippable-tests-declared.py`, a gate requiring every occurrence of the skip idiom in `crates/*/tests/*.rs` to be declared in `tools/skippable-tests-baseline.txt` (28 sites). Found because `Pass 298.0`'s own guard was sabotaged (`R225` discipline) to prove its test could fail, and the test **stayed green**: its helper needed `fixtures/external/pdfbox/…`, absent on this machine, returned `None`, the test printed `SKIP`, and removing the guard entirely changed nothing.

**Findings + decisions:**
- **Measured:** 26 tests across five files report "passed" while actually skipping — `editable_roundtrip` 2 of 6, `insert_pages_preserves_undo` 1 of 7, `merge_document` 8 of 16, `structure_inspect` 1 of 12, `widget_adoption` 14 of 20. `fixtures/external/` is untracked in git and no CI step fetches it, so these tests are green everywhere and have run nowhere.
- **Judged, not `R225`:** the mechanism differs — `R225` is a fixture that runs against a wrong or unconsidered value; here nothing executes at all. Corroborated independently: `pdfcer-gui` reported this identical shape about its own harness four days earlier. Standing rule **`R255` minted** (*ROADMAP.md Standing rules*).
- **A dated recurrence, not a new rule.** Third same-session instance of a wrapped command's exit code mistaken for the command's own — `run-gates.sh | tail` (corrected at `21403ff`), `cargo clippy | grep | head` (`4608f7e`, 513th filing), and this gate's own first sabotage attempt (piped through `head`). The 513th filing already declined to mint a rule here (standard POSIX pipeline semantics, not a project defect); recorded as a recurrence-rate datum, not reopened.
- **Backfilled: `21403ff` was never cited.** It landed between the 513th filing and `e0019af` (514th filing's first commit) — a docs-only self-correction of a false claim in `docs/NEXT_SESSION.md` — and the 514th filing's entry did not name it. Cited now in `ROADMAP.md` to close the gap.
- The gate's own stated limit: it cannot see a test that returns early printing nothing — a worse version of the same defect, undetectable without running the tests. Widening it is owed only against a future measurement of that, not a guess.

**`FEATURES.md`**: unchanged — internal test-harness/tooling discipline, no operator-visible capability touched.

**Sourcing note (hard rule 8):** no shell this filing. `.git/refs/heads/main` and `.git/logs/HEAD` both read `2d2e21709bc6263500329ddf6f236a4ad4ae088b`, one commit past `.git/refs/remotes/origin/main` (`d5a501f6…`) — local, unpushed. The reflog's final two entries show a commit at `52b79ee…` immediately amended in place to `2d2e217`; `52b79ee` is cited nowhere here, per instruction. `.git/COMMIT_EDITMSG` carries the current tip's message verbatim, matching the account given. The skip-count table and 28-site baseline figure are taken from the commit message as authoritative per instruction, not independently re-run.

**Still in flight:** 26 corpus-gated tests (`widget_adoption` 14, `merge_document` 8, `editable_roundtrip` 2, `insert_pages_preserves_undo` 1, `structure_inspect` 1) need synthetic fixtures before they defend anything measurable; baseline is debt, direction is down.

**For next session:** flagging for `docs/NEXT_SESSION.md` (engineer-owned, not edited here) — (1) the 26-test synthetic-fixture debt above, by file; (2) whether `fixtures/external/` should be tracked or fetched in CI at all, given nothing currently supplies it; (3) the gate's stated blind spot (a silent-print skip) as a future widening condition, not present work.

**★ Amendment 2026-09-12 (`4086b35`), correction to this same filing, no code change:** the "Still in flight" line above and item (2) above both understated the item as an open preference. Checked what making the 26 tests run would actually take: **23 need `fixtures/external/pdfbox/…`**, which `fixtures/README.md` itself marks "NOT blanket-safe … may be copyrighted to third parties … license may not allow redistribution … never bulk-import" (`LEGAL.md` §5 / project rule 7); **3 need `fixtures/external/qpdf`**, absent from `fetch-corpora.sh` entirely (it fetches only veraPDF, pdf20examples, and a corpora index — omitting pdfbox is deliberate). **No licence-compliant route exists for the 23** — item (2) above is answered for those: fetching in CI is REFUSED, not merely un-chosen, and a synthetic fixture is the only fix. The 3 qpdf-gated tests are not covered by this refusal, only by the corpus's current absence from the fetch script. **Sourcing, no shell:** `.git/refs/heads/main` reads `4086b35…`, one commit past `.git/refs/remotes/origin/main` (still `d7eabb3…`, this session's own prior filing commit) — local, unpushed; `.git/COMMIT_EDITMSG` carries `4086b35`'s message verbatim, the source for the figures above.

## 2026-09-11 (516th filing) — a fix authored one Pass, and a coordinator's own withdrawal turned out to be the more durable finding

**Shipped:** `Pass 298.0` (`93f329b`) — `adopt_widget` and `sign` now refuse an operator-typed name containing a dot before writing it into a top-level `/T`. Reported by `pdfcer-gui`, which had just independently re-verified that the 2026-08-29 dotted-name guard at `place_new_field_deferred` really is complete for the six `add_*`/`paste_field` verbs, then asked whether that choke point is the ONLY way a name reaches `/T`. It is not. `adopt_widget`/`sign` write an operator-typed name verbatim; a dotted one (`Text.2`) collides with §12.7.3.2's own FQN convention, so every resolver splits on `.`, finds the real terminal `Text`, and the new field renders and clicks but is reachable by nothing — `fill_text_field`, FDF/XFDF import, `/CO`, reset-form. Not data loss (no `/Kids` append). Fixed by reusing `FormAuthorError::DottedPartialName` (generalised wording — it used to describe only a rename). `sign`'s guard fires on the CREATE path only, deliberately: an existing nested signature field's FQN legitimately contains a period (`Approvals.Engineer`), so a guard at the top of the verb would have refused every such placeholder.

**Findings + decisions:**
- **★★★ A sabotage came back green.** The first `adopt_widget` test needed `fixtures/external/pdfbox/…`, absent on this machine — the helper returns `None`, the test prints `SKIP` and **passes**, so deleting the guard entirely stayed green. Sixteen tests in `crates/pdfcer-core/tests/widget_adoption.rs` are in the same position — `pdfcer-gui` reported this exact shape about its own harness four days ago, reproduced within the hour here. New tests use a synthetic fixture instead; owed item flagged below for the corpus-dependent sixteen.
- **A dispatch was corrected mid-filing, and the correction is kept as evidence, not scrubbed.** The coordinator's first brief for this filing named an owed ask — a coarse `EditError::kind()` discriminant — and then withdrew it before this filing landed, on the requester's own re-measurement: the claim it had been drafted from ("the generic floor has nothing to switch on") is true of the floor and false of a call site, since every `FormAuthorError` variant is already reachable by name. **Not filed as Backlog work.** Kept instead as `R220`'s third dated instance (*ROADMAP.md Standing rules*): a negative capability claim landed in a request draft unchecked against source, the same mechanism as the other two instances on a different document type. Their own line, worth keeping: *"a limitation sentence is a citation, and it goes stale faster than the code it describes."*
- **A second, related deletion from the same revision**: `pdfcer-gui` removed its own `group_is_a_field` pre-check for `sign` after finding it had silently become a wrong parallel model of pdfcer's own guard (refusing any name prefix rather than only a true terminal). Noted against `R221`'s mechanism, ordinal not incremented (the count is already flagged owed for reconciliation in `docs/NEXT_SESSION.md`).
- **Not minted as a rule (n=1):** the audit move that produced this Pass — verify a prior fix independently, then ask what its own premise excludes — is a good habit with only one instance on record so far.
- A doc-comment claim on `add_text_field` ("a refusal costs nothing and nothing partial is staged") was narrowed in the same commit — true of the check it described, not of a later guard where `alloc_number`/`stage_bytes` have already moved (neither reaches `state` or the file, so still no leak).

**`FEATURES.md`**: two rows extended in place, no checkbox changed — *Adopt an existing widget into an `/AcroForm` field* and *Sign a document (APPROVAL signature, PAdES B-B)* both gain a sentence on the new guard.

**Sourcing note (hard rule 8):** no shell this filing. `.git/refs/heads/main` and `.git/logs/HEAD` both read `93f329b2b2a1d0acada969618e3c326fd796fdee`, one commit past `.git/refs/remotes/origin/main` (`2faca2e5…`, confirmed `HEAD~1`) — local and unpushed, matching the dispatch. `.git/COMMIT_EDITMSG` carries `93f329b`'s message verbatim, matching the account given. The coordinator's mid-task correction (withdrawing the `EditError::kind()` ask, supplying the `group_is_a_field` account) is taken as authoritative per instruction, arriving after the commit and not checkable against it.

**Still in flight:** the sixteen `widget_adoption.rs` tests gated on an absent `fixtures/external/pdfbox/…` corpus — real coverage, currently unmeasurable on this machine, no amber signal.

**For next session:** flagging for `docs/NEXT_SESSION.md` (engineer-owned, not edited here) — (1) the sixteen corpus-gated tests above; (2) the `EditError::kind()` ask is CLOSED, withdrawn by its own requester, remove it if it was staged anywhere as pending; (3) `R221`'s instance-count reconciliation remains owed and now has one more shape to fold in when it happens.

## 2026-09-11 (515th filing) — a fully off-page image straddling two bands was blanked, not removed; and Pass 294 never reached ROADMAP.md

**Shipped:** `Pass 297.0` (`536ef3b`) — closes the larger half of `Pass 294.0`'s recorded limit. `wholly_covered` (`redact_image.rs`) tested one region only; a placement covered by the UNION of two off-page bands (the common case — the bands ring the page) was cleared cell-by-cell instead of removed, so `scan-offpage` re-run on the cleaned file still reported it. Now tested by coordinate compression (cut the AABB along every region edge, require every sub-rectangle's centre inside some region). Measured on all 17 affected drawings, re-cleaned from source: 17 files/20 pages/11 fully-off/12 partial → 7 files/9 pages/0 fully-off/12 partial. **Owed figure corrected: 12 objects, not 23** — all `partial` (edge slivers), a distinct, untouched sub-case.

**Gap found and closed the same filing:** `Pass 294.0`/`294.1`/`294.2` (`04d0099`/`d41be61`/`1230c1f`, 500th/502nd/505th filings) were recorded in `SESSION_LOG.md` but never reached `ROADMAP.md`'s Shipped section — the same class of gap `Pass 295.0` hit at the 507th filing. Backfilled retroactively. `FEATURES.md` also had no row at all for scan/redact-offpage until this filing — added.

**Findings + decisions:**
- **`R225`, 20th dated instance, new sub-shape.** `wholly_covered_needs_one_region_to_contain_the_placement` asserted the union case was NOT covered, with a matching doc comment — assertion, comment and code all agreed, and none agreed with a re-scan of the saved file's content. Distinguished from the 18th instance (`minimal.pdf`, 2026-09-10): that test *leaned on* a defect elsewhere as an incidental precondition; this one *asserted* the defect directly as its own stated expectation, corroborated in three places rather than one.
- **Not minted as a rule: a second instance of "counters can all report success while the artifact is wrong."** `Pass 294.2`'s `TJ`-corruption regression (505th filing) was caught the same way — by reading the saved page back, not by trusting the report's own counters — and no standing rule was filed from it at the time. This is the second occurrence of the same mechanism (a verification path built from the same code it is meant to check cannot see that code's own defect). Flagged for the engineer's judgement rather than minted unilaterally — this role does not mint standing rules on its own authority.
- Kept as a practice note: `Pass 294.0`'s 17/23 figure being written down as a known limit rather than left implicit is why closing it was a measurement against a stated number, not a fresh investigation.

**`FEATURES.md`**: added the scan/redact-offpage row (*Redaction & security*), core/cli `[x]`, gui `[ ]`; owed figure stated as 12, not 23.

**Sourcing note (hard rule 8):** no shell this filing. `.git/refs/heads/main` reads `536ef3b77e27fa7a64b1fbaf8d27a83e089cb15f`; `.git/COMMIT_EDITMSG` (the tip's own message) matches the account above verbatim — both read via `Read`, not a shell command. `Pass 294.x` facts relayed from `SESSION_LOG.md`'s own prior entries (history, not a live-tree claim). Independently verified against live source: `wholly_covered`'s coordinate-compression body and doc comment (`crates/pdfcer-core/src/redact_image.rs:198-296`); the renamed/inverted test (`redact_image.rs:1680-1730`); `scan-offpage`/`redact-offpage` subcommands live in `crates/pdfcer-cli/src/main.rs` (lines 1167, 1202, 10077, 10092, 40597-41008).

**Still in flight:** the 12 remaining `partial` off-page residuals (edge-sliver sub-case) — untouched by this Pass, stated as owed.

**For next session:** `docs/NEXT_SESSION.md`'s OWED list should have its off-page line's number corrected from "17 of 174 / 23 objects" to 12 (engineer-owned file, flagged not edited here).

## 2026-09-11 (514th filing) — a mirror table disagreed with the thing it mirrors, and an accuracy-only doc comment was reassuring readers into the wrong conclusion

**Shipped:** `e0019af` + `297dc19` — not Passes, two docs-only fixes. `e0019af` corrects six rows of `docs/core-api/03-capabilities.md`'s Appendix (capability → module → `FEATURES.md` state): two contradicted `FEATURES.md` outright (forms flatten, move a widget — already `x`/`x`/`x` there, wrongly `[ ]` in the appendix), four more claimed `[ ]` for capabilities a consuming shell measurably calls; fixed, plus a glyph legend (`x`/`[ ]`/`⊘`/`—`) and a stated rule that `FEATURES.md` wins when the two disagree. `297dc19` adds a section to `cmyk_to_srgb`'s doc comment (`crates/pdfcer-core/src/color/mod.rs`) stating the conversion is lossy and one-way — its existing calibration/clamping/pdfium-agreement sections are all about accuracy, none about direction, "a reader who checks the accuracy is reassured into exactly the wrong conclusion." Rule: display-only conversion is fine, a control whose value is read back is not.

**Findings + decisions:**
- **Dated instance, `R220`** (*Standing rules*): the appendix's own pre-fix `gui [ ]` row for offpage, written from assumption in `bfa981b` hours before this filing's correction, is the same mechanism `R220` names on a different axis — "a shell has no caller" rather than "core has no verb," a negative capability claim sent into a document unchecked against source.
- **Verification-methodology finding, `D:/dev/rag/rust`**: verifying a shell's call-site claims by grepping the qualified receiver form (`Module::function(`) reported 3 of 4 claimed sites as absent — all three imported the verb through a grouped `use` and called it bare, so the qualified grep was the wrong instrument, not the shell's report wrong. Filed as `verifying_call_sites_by_qualified_path_misses_calls_made_bare_after_a_grouped_use_import.md`, the mirror image of this RAG's existing bare-callback-reference finding.
- **API-design finding, `D:/dev/rag/rust`**: the rejected `DisplayOnly(Rgb)` wrapper, and the reusable argument against it — "a type encoding the caller's intention is a type the caller can lie to," because it differs from its unwrapped form in what the caller MEANT, not in what the value CONTAINS. Filed as `a_type_that_encodes_the_callers_intention_is_a_type_the_caller_can_lie_to.md`.
- **No new mint for "an audit that only reports hits is not an audit."** Already this role's own hard rule 11 clause (e) — report the surviving-correct hits, not only the fixed ones. `e0019af`'s dispatch named the two rows it checked and found already correct, held up as the standard rather than recorded as a new finding.

**`FEATURES.md`**: confirmed unchanged, correctly. Neither commit is a capability change — `e0019af` fixed the mirror, not the mirrored rows (`FEATURES.md` was already right); `297dc19` is a doc-comment clarification, no new verb. Verified by reading `FEATURES.md`'s Forms-flatten row and grepping the six corrected capability names against it.

**Sourcing note (hard rule 8):** no shell this filing. `297dc19`'s commit message read verbatim from `.git/COMMIT_EDITMSG` (the current tip); `e0019af`'s subject and parent chain confirmed from `.git/logs/HEAD`'s plain-text reflog (entries 327-328) — its full body was not recoverable without a shell (`COMMIT_EDITMSG` only retains the most recent commit) and is taken from the dispatch's account, cross-checked against live source rather than re-derived. Independently verified against the live tree: the appendix's six corrected rows and glyph legend (`docs/core-api/03-capabilities.md:3432-3469`); `FEATURES.md`'s Forms-flatten row; `cmyk_to_srgb`'s doc comment verbatim (`crates/pdfcer-core/src/color/mod.rs:240-274`), including the rejected-`DisplayOnly` paragraph.

**Still in flight:** nothing new opened by this filing.

**For next session:** none owed by this filing specifically; `docs/NEXT_SESSION.md`'s existing OWED list is unchanged (engineer-owned).

## 2026-09-11 (513th filing) — a doc-block splice detector, and the pipeline exit code that hid its own repair's defect

**Shipped:** `3334377` — `tools/check-doc-block-spliced.py` (a contiguous `///` run must not repeat a rustdoc heading; CI's `audits` job now 22 checks). Found that a `tools/public-fns-undocumented-baseline.txt` row can hide a splice rather than an omission: `delete_subpath`'s doc block was welded thirty lines up onto `delete_node`'s; five of 57 baseline rows were recoverable text this way, baseline now 28 (confirmed independently by count). Widening the doc-coverage gate to private functions was measured (1,837 undocumented) and rejected as a baseline nobody reads. `4608f7e` (local, unpushed until this filing unblocks the pre-push hook) removes a duplicated `#[must_use]` the splice repair left on one function — reached `origin` because clippy was piped through `grep | head` and the exit code read afterward was the pipeline's (`head`'s, always 0), not clippy's.

**Findings + decisions:**
- **Scepticism applied to the engineer's proposed shared mechanism — declined.** Asked whether this pipeline-exit-code defect is the same failure, a third time this session, as `run-gates.sh` exiting 0 while printing `FAILED — N of 31`, and `check-string-gaps.sh` truncating its own excerpt past a second defect. All three share a description (a glanced-at signal wasn't the real one) but not a mechanism: `run-gates.sh`'s is a script deliberately exiting 0 regardless of internal failure (already known, already recorded in `docs/NEXT_SESSION.md`, not new); `check-string-gaps.sh`'s is a truncated *display* over a still-correctly-red gate (already filed as a further truncated-read-hazard instance at the `f16e266`+`5917ece` `ROADMAP.md` entry); the pipeline's is standard POSIX multi-command exit-status semantics, not a defect in any tool this project wrote, and trivially derivable from the shell's own manual — no RAG entry earned on its own account. No new standing rule minted; a habit recommendation (check a command's own exit status, don't trust a pipeline's) left for the engineer's `docs/NEXT_SESSION.md`, not written here.
- Continues this session's run of declined unifications (`R151`/`R251`/`R253`/`R254` boundary-checks, 508th–512nd filings): a shared symptom is not a shared mechanism, checked again rather than assumed.

**`FEATURES.md`**: unchanged — both commits are internal tooling/hygiene, no operator-visible capability touched.

**Sourcing note (hard rule 8):** no shell this filing. Verified via `Read`/`Grep` against `.git/refs/heads/main` (`4608f7e`), `.git/refs/remotes/origin/main` (`3334377`), and `.git/logs/HEAD`'s plain-text reflog (entries 323–324), confirming `origin/main` is one commit behind local `HEAD`/`main` exactly as described, and the parent chain plus commit-subject text match the dispatch's account. The two commit messages themselves are authoritative per instruction and were not re-read verbatim from the object store. Independently verified against live source, not merely relayed: the new gate's existence/wiring/header text, the 28-row baseline count, `delete_subpath`'s repaired doc block (`crates/pdfcer-core/src/edit.rs:13893-13896`), the CI job's `(22 checks)` label, and the absence of any remaining duplicated `#[must_use]` pair anywhere under `crates/`.

**Still in flight:** nothing new opened by this filing.

**For next session:** none owed by this filing specifically; `docs/NEXT_SESSION.md`'s existing OWED list is unchanged (engineer-owned) — flagging for that file, not editing it here: (1) consider fixing `run-gates.sh` to propagate a real exit code instead of perpetuating a read-the-text habit; (2) avoid piping a check whose exit code will be read (`cargo clippy | grep | head` reads as clippy's status but is `head`'s).

## 2026-09-11 (512th filing) — a misreading of R151 was licensing a different failure entirely, and it took three instances in a day to see the boundary

**Shipped:** `Pass 296.8` (`f392b19`) — `BlendSpaceFrom::token()` is `pub` now. `Pass 296.4` made the enum `pub` but kept this enum→string mapping `pub(crate)`, reasoning nothing had asked for it directly — even though the mapping already ran, unconditionally, on `pdfcer`'s own metrics line. Within the hour a consuming shell's `Debug`-derived trace wrote `PageGroup` where the metrics line writes `page_group`: two stable spellings of one fact across a boundary whose purpose is that both sides agree. The shell declined to hand-copy the mapping (`R74`) and filed instead. A test now pins the three tokens and asserts they differ from the `Debug` derive.

**Findings + decisions:**
- **Decision 153 authored, standing rule `R254` minted** (`ARCHITECTURE.md` §8.2/§12; `ROADMAP.md` *Standing rules*): a value a crate already computes for its own use does not earn `pub` by demand — it already earned it by existing. Keeping it `pub(crate)` "until something asks" puts the discovery cost on the party structurally unable to see the gap, who then reaches for `Debug` or prose instead, and that reach becomes an unintended contract.
- **The engineer's own question, checked rather than accepted.** The dispatch asked whether this is `R151` needing a narrowing, an `R253`/decision-152 instance, or neither, and asked this role to apply real scepticism rather than take the framing on faith — matching the last three filings' declines. Both were checked against their actual mechanisms and declined: `R151` audits whether an *already-published* capability is *called* before crediting a Pass; `token()` had a caller throughout (the metrics line, inside the crate), so it was never uncalled in `R151`'s sense — the fault was an inference drawn FROM `R151`, not a defect IN it, so `R151`'s text is untouched. `R253`/decision 152 restricts *unsafe* content OUT of a safe default (`Display`); this is the opposite failure, a *safe*, wanted accessor withheld entirely, forcing the caller onto an unsafe substitute (`Debug`). Filed as a new rule instead.
- **Three instances, one session, and a note this discharges.** `Refusal::remedy_faces` (`Pass 296.1`) and `impl Display for Object`/`Name` (`Pass 296.2`) share `token()`'s exact mechanism — a computation the crate already had, kept in its unpublished/debug form until asked. This role flagged that pair at the 509th filing as "the same observation as `R251` but not the same mechanism" and left the boundary unnamed, pending a fix that would generalise. `Pass 296.8`'s fix (publish, don't gatekeep) is that generalisation, so `R254` names it now. **Checked and kept separate, not folded in:** `PassedOver`'s re-export gap (`Pass 295.1`, `R251` — a compile-visibility accident, nobody reasoned "wait for demand") and `preview_style_ladder` (`Pass 295.0` — a gate structurally couldn't see a rung, closer to `R151`'s territory than this one).

**`FEATURES.md`**: unchanged — dev-facing API-surface fix inside `Pass 296.4`'s existing row (already correctly states `BlendSpaceFrom` is `pub`), not an operator-visible capability change. Verified by reading the row directly.

**Sourcing note (hard rule 8):** no shell this filing. `Read`/`Grep` against the live tree, plus `.git/HEAD`, `.git/refs/heads/main` and `.git/logs/HEAD` (all plain text, readable without a shell) confirmed `main` is at `f392b19` and its own one-line reflog message matches the dispatch's account before anything was filed. The commit message at `f392b19` itself is treated as authoritative per instruction and was not read verbatim (no `git show`). Independently verified against live source: `token()` is `pub const fn` at `crates/pdfcer-render/src/interpret.rs:2165` with a doc comment narrating this exact history; `crates/pdfcer-render/tests/ink_answered_before_rendering.rs:74–97` pins the three tokens and the Debug-inequality assertion. Not independently verified: any test-count delta (none was stated in the dispatch to check against).

**Still in flight:** nothing new opened by this filing. Pass IDs `296.6`/`296.7` were not found in either register by grep and are recorded as possibly reserved outside this role's visibility — not a collision, not investigated further.

**For next session:** none owed by this filing specifically; see `docs/NEXT_SESSION.md`'s existing OWED list (unchanged by this filing, engineer-owned).

## 2026-09-11 (511th filing) — a diagnostic's excerpt is not its finding, and the tool that would have prevented the other defect already existed

**Shipped:** `f16e266` + `5917ece` — not Passes. `tools/run-gates.sh`, run
before pushing the day's `Pass 296.x` batch, came back red on three
self-inflicted defects: `preview_style_resolution` lost its 38-line doc
comment to a splice caused by `Pass 295.0`'s literal `str.replace` on the
function signature; two string literals carried a baked-in double-space
from a lost heredoc line-continuation; the `audits` CI job said
`(20 checks)` while running 21 (`Pass 295.1` added a check without
updating the count). `f16e266` fixed all three; `5917ece` fixed a second
gap in the same literal that `check-string-gaps.sh`'s own ~100-character
printed excerpt had hidden from the first fix.

**Findings + decisions:**
- No new architectural decision, no new standing rule minted.
- `5917ece`'s cause is filed as a further instance of the existing
  cross-project finding at
  `C:\personal_rag\claude_code\lesson_20260807_truncated_read_of_wrapped_sentence.md`
  (`LEGAL.md` §6.5.5) — a diagnostic tool's own truncated printed excerpt
  is the same trap as a reader's own `head -5`, just moved to the tool's
  side of the pipe. Flagged for `troubleshooting-librarian` to add the
  dated instance there; not written directly, matching the 492nd
  filing's handling of the CRLF `str.replace` hazard.
- The doc-splice defect recurring after `tools/edit-source.py` shipped
  (`aeeecb5`) is recorded as a working-method note, not stretched into an
  `R243` instance — `R243`'s mechanism is a *documented* obligation
  failing as a control, and here the *machinery* already existed; it
  simply wasn't reached for on this edit.

**`FEATURES.md`**: unchanged — no operator-visible capability touched.

**Sourcing note (hard rule 8):** no shell this filing. Commit hashes and
the three-defect description relayed from the dispatching agent's
account (`f16e266`/`5917ece` themselves unread here). Independently
verified by `Grep`/`Read` against the live tree: the 38-line doc block is
reattached at `crates/pdfcer-core/src/text_edit/format.rs:3651-3666`;
`.github/workflows/ci.yml:318` reads `name: repository audits (21
checks)`. Not independently verified: the two string-literal gap fixes
(no shell to re-run `check-string-gaps.sh`).

**Still in flight:** nothing new opened by this filing.

**For next session:** consider adding "run `tools/run-gates.sh` before
every push, not only before a release" to `docs/NEXT_SESSION.md` —
flagged to the engineer; that file is engineer-owned and not edited
here.

## 2026-09-11 (510th filing) — `Pass 296.0`'s own argument arrived from the other side

**Shipped:** `Pass 296.5` (`4f6f5a5`) — `RenderError::RasterizerLimit`'s
`Display` no longer embeds the rasteriser's raw third-party panic text.
`Pass 296.0` had put it in the `#[error(...)]` format string with a doc
comment telling callers not to match on it; a consuming shell's generic
error-display arm — written deliberately so a structured diagnostic beats
"an error occurred" — routed exactly that string onto an operator's page.
The consumer had already written a named arm to avoid it and reported it
as a workaround, not a request (decision 058: a workaround is a finding
about pdfcer's own boundary, not a favour). `Display` now reads only the
scale; `panic_message` is unchanged and still reachable by name.

**Findings + decisions:**
- **Decision 152 minted.** `Pass 296.2`, same session, gave `Object`/`Name`
  a `Display` on exactly the reasoning that a consumer's catch-all arm
  decides what an operator sees, so the engine must own the safe default.
  `Pass 296.0` shipped with the identical fact true of it and chose the
  opposite default. General rule: a variant safe only for a consumer who
  has read its doc comment is unsafe for every consumer who has not — the
  safe rendering must be the default one, not an opt-in via source-reading.
- **`R253` minted** (not filed as a further `R251` instance — checked
  against `R251`'s actual mechanism, a re-export/reachability gap, and this
  is a different failure: a runtime string inside a `Display` impl, nothing
  to do with compile-time reachability. Shared moral, not shared mechanism,
  per this role's own standing discipline).
- **Noted, not separately filed:** this is the second time in one day
  decision 058's "workaround, not request" framing caught a real defect —
  the other being a `search_text` double-extraction the engineer reports
  surfacing during `Pass 296.3`'s CLI fix. Two in one day is a frequency
  worth watching for a third.

**`FEATURES.md`**: no row changed — error-message correctness inside an
existing capability, not a new or extended one. Said explicitly rather than
inventing a row.

**Sourcing note (hard rule 8):** no shell this filing. Commit hash and
reasoning relayed from the dispatching engineer's account (`4f6f5a5`
itself unread here). Independently verified by `Grep`/`Read` against the
live tree: `RenderError::RasterizerLimit`'s format string omits
`panic_message`, and the field's doc comment states it is "Deliberately
absent from `Display`" (`crates/pdfcer-render/src/lib.rs:553-564`). Not
independently verified: the consuming shell's own named-arm workaround,
and the `search_text` double-extraction claim from `Pass 296.3`.

**Still in flight:** nothing new opened by this filing.

**For next session:** if a third same-day "workaround, not request"
defect turns up in a future batch, decision 058's framing is earning
enough repeat hits to be worth a dedicated sweep, not just a note.

## 2026-09-11 (509th filing) — five `pdfcer-gui` requests answered in five Passes; a measurement that refused to become a constant

**Shipped**, all replying to `pdfcer-gui`'s inbound batch (`G002`–`G006`,
`D:\Dev\FeatureRequests\pdfce_FeatureRequests\open\`):
- `Pass 296.0` (`69d4d67`) — a deep-zoom region render refuses
  (`RenderError::RasterizerLimit`) instead of panicking a worker thread
  inside tiny-skia. `MAX_GUARANTEED_REGION_SCALE` published as a **floor**,
  not an exact ceiling — see finding below.
- `Pass 296.1` (`141c989`) — `Refusal::remedy_faces: Vec<String>`, the
  font-coverage remedy as data, not only prose; `std14_faces_covering`
  gains a warning; `Refusal::new`'s signature changed.
- `Pass 296.2` (`90576a8`) — `impl Display for Object`/`Name`: scalars
  exact, containers named, never dumped.
- `Pass 296.3` (`5943beb`) — `search_and_mark_redactions_by_pattern{,_styled}`
  reports unreadable text, matching the literal-search route; pdfcer's own
  `pdfcer-cli --pattern` branch had the identical silence and is fixed too.
- `Pass 296.4` (`8d2f6bb`) — `page_composites_in_ink`: ask whether a page
  composites in ink without rendering it; `BlendSpaceFrom` made `pub`.

**Findings + decisions:**
- **`Pass 296.0`, decision 151, `R252` minted**: measuring the panic
  boundary across six page geometries found three values that order with
  NOTHING — not width, area or device extent; the largest sheet is the
  most fragile. Publishing one as an exact constant would have been an
  invented number wearing a measurement's clothes. Shipped instead: the
  constant as a floor below the lowest failure, and the guarantee as the
  caught, named refusal itself — which cannot be wrong because it *is*
  the failure, caught. `ARCHITECTURE.md` §10.7 records the invariant.
- **`R245`, 8th dated instance** (`Pass 296.3`): the literal-search-vs-
  pattern-search redaction-disclosure pair has now produced this exact
  shape twice.
- **Declined to file `Pass 296.1`/`296.2` as further `R251` instances.**
  The dispatching engineer's own framing called this "R251's shape
  recurring across all five" (four consumer-invisible defects in two
  days: `295.0`, `295.1`, `296.1`, `296.2`). Checked against `R251`'s
  actual mechanism (a re-export gap) and it doesn't match either of
  these two — neither is a re-export gap, and `check-reexport-closure.py`
  would not have caught either. Filed as a cross-cutting *observation*
  in `ROADMAP.md`'s *Standing rules* instead of a mechanical instance
  count, per this role's own prior "shared mechanism, not shared moral"
  finding. Worth a proper mint if a fix for one would plausibly have
  caught the others — not yet true here.

**`FEATURES.md`** updated in this filing: font-coverage remedy row (data
not prose), redaction pattern-route row (disclosure + CLI fix), region-
render row (RasterizerLimit + floor), and a new row for
`page_composites_in_ink` (core `[x]`, cli `[ ]`, gui `[ ]`).

**Housekeeping:** `docs/core-api/02-editing-and-saving.md` + `index.md`
were already updated (225→227 verbs) ahead of this filing — verified
current against the live tree, not re-touched. No `docs/core-api/` entry
was owed by this batch beyond that; the `offpage`-module entry owed since
`Pass 294.0` is **not** touched by this filing and remains open (see the
495th-and-earlier filings) — carried forward, not this session's to close.

**Sourcing note (hard rule 8):** this role had no shell this filing.
Commit hashes and reasoning are relayed from the dispatching engineer's
own summary (explicitly framed as an index, not the record — the commit
messages are authoritative and were not read directly here). Five claims
were independently checked against the live tree by `Grep` instead:
`RasterizerLimit`/`MAX_GUARANTEED_REGION_SCALE`, `remedy_faces`,
`impl fmt::Display for Object`/`Name`, `search_and_mark_redactions_by_pattern`,
and `page_composites_in_ink`/`pub enum BlendSpaceFrom` — all present as
described. The six-geometry measurement, exact test counts and
`run-gates.sh` result are relayed, not re-run.

**For next session:** the `R251`-vs-`R151` boundary observation above is
worth a look once a third or fourth instance shares an actual fix, not
just a symptom.

## 2026-09-11 (508th filing) — a re-export gap with no local signal, gated; a missing Shipped row closed

**Shipped:** `Pass 295.1` (`e360e11`) — `pdfcer_core::text_edit::PassedOver` was
unreachable from a consuming crate (`error[E0432]`) because `Pass 295.0`
re-exported `StyleLadder` but not the `PassedOver` type its own field names.
Fixed, and turned into `tools/check-reexport-closure.py` (196 re-exported
types checked; wired into CI's `audits` job and `check-ci-parity.py`), which
found two more live instances nobody had reported: `AddTextRequest::face:
NewTextFace`, `PageObjects::leaves: Vec<FormLeaf>`. Also removed a dead
`pub use decompose::{};`.

**Decisions made this session:** No new architectural decision — a
surface/tooling fix within the existing crate-boundary contract. Standing
rule `R251` minted: a type reachable only through a re-exported item's own
public field, but not itself re-exported, compiles and clippy-passes clean
*inside its defining crate* — the only observer is a downstream consumer.
Stated limit: the gate checks fields, not method return types.

**Findings + decisions:** Second instance in this project of a
consuming-crate bug report exposing a defect with no local signal (first:
`R151`'s uncalled-capability family — a different mechanism, same shape of
blindness: nothing inside `pdfcer-core` itself can fail on it).

**Still in flight:** Nothing new opened by this filing.

**For next session:** None specific to this filing.

**Housekeeping:** `Pass 295.0` (`7160932`) was recorded in the 507th filing
below but never reached `ROADMAP.md`'s Shipped section — added there
retroactively in this filing so the contract and the log agree. `FEATURES.md`
unchanged: this is a surface/tooling fix, not a capability change, so no row
qualifies.

Verified: `cargo fmt --all --check` clean; `cargo clippy --workspace
--all-targets -- -D warnings` exit 0; `check-core-api-verbs.py` PASS (225
verbs); `check-ci-parity.py` clean.

## 2026-09-11 (507th filing) — five shell requests, answered in one Pass

**Shipped:** `Pass 295.0` (`7160932`) — `preview_style_ladder` (read-only
twin of the ladder: the R90 gate cannot see rung 2, so the shell's tooltip
predicted synthesis while the commit bound a real `Helvetica-Bold`);
`StyleLadder::same_family`; `passed_over` typed as `Vec<PassedOver>` with
the `Refusal` carried through the survey path; `Refusal::new`, making
`FormatError::CoverageFailure` constructible — a public variant that was
untestable by construction; and `SynthesisRefusedByPosture`'s clause,
which read *"X was used"* where it meant *"X was tried and rejected"*.

★ **The shell's sentence worth keeping:** *"the missing shape did not
cost a workaround, it cost a feature."* A consumer disciplined about not
re-deriving engine facts stays silent rather than parse, so a prose-only
field reads as *"this information is not available"* even though it was
computed.

★★ **`R225`, 19th instance, caught by sabotage:** the first
`same_family` test passed a hard-coded `Some(true)` because its fixture
only ever bound `Helvetica` → `Helvetica-Bold`. A cross-family fixture
now exists and the same sabotage is red. It is the requesters' own
`CoverageFailure` argument pointed the other way — **a case you cannot
construct is a case you cannot defend.**

## 2026-09-11 (506th filing) — `v0.53.0` released

**Released:** tag at `8a65e3f`, CI green at the tagged commit. Zip
`pdfcer-v0.53.0-windows-x64.zip`, **19,140,999 bytes**, SHA-256
`bc2bff7aacf09e32d6d39d83a6d1454e5fcc62df0930d73d11bde5eda7e44fea`, on
the release page with its checksum. Portable folder
`builds/pdfcer-20260911-0859-8a65e3f` on D:. OneDrive slot **`pdfcer2`**;
`pdfcer1` keeps `0.52.0`. `verify-release.py` nine of nine.

**Contents:** `Pass 294.2` — the `TJ`-number corruption, both performance
fixes, and the paint-nothing scan rule.

★ The release notes say plainly that the corruption **affects ordinary
redaction, not only the off-page command**: any document whose text uses
`TJ` arrays could be damaged when the redacted text was numeric. A note
that buried that under the new feature would be a note written for the
feature rather than for the operator.

## 2026-09-11 (505th filing) — the precaution was corrupting the page it protected

**Shipped:** `Pass 294.2` (`1230c1f`). Running `redact-offpage` over 176
real drawings produced one file with **three pages neither pdfcer nor its
renderer could read**, from clean input. Cause: the residual sweep filled
matched bytes across a whole `TJ` operand, and `TJ` is an array of
strings **and numbers** (§9.4.3) — a redacted DIMENSION is digits, so a
kerning number became `-53XXXX00221014025`.

★ The glyph surgery was correct throughout. The belt-and-braces pass was
the one destroying content, which is why the new test reads the page
BACK rather than trusting the report's counters — every one of them said
success.

Also: the off-page bands now carry the scan's tolerance (they cut what
the scan called clean, and made every full-bleed image decode); the
residual sweep no longer decodes image samples; the scan ignores objects
that paint nothing. **>10 min → 0.74 s** on the file that exposed it,
**3 → 0** unreadable pages across 174 outputs.

**Owed:** 17 of 174 outputs still carry 23 off-page objects — fully-off
images straddling two bands, where `wholly_covered` is per-region and
the union is what matters. Disclosed by the scan, not silent.

## 2026-09-11 (504th filing) — the scan said "exit 1" and was read as "stops"

**Shipped:** `8129dc1` — `scan-offpage`'s help, and the published
`v0.52.0` notes, now say that every file and every page is scanned
always, that an unreadable file is reported and the walk continues, and
that the exit code is a verdict at the END of the run. The one-shot form
is spelled out beside it.

**Why:** the operator read *"Exits 1 when something is off-canvas"* as an
early stop and asked how to make it scan to the end. The code was right,
a test would have passed, and the defect was entirely in the sentence.

★ **Nothing in this project's gates reads prose for what it will be
UNDERSTOOD to mean.** Worth remembering the next time a release note
describes an exit code.

## 2026-09-11 (503rd filing) — `v0.52.0` released, with the portable build

**Released:** tag at `cb7727e`, CI **green** at the tagged commit. Zip
`pdfcer-v0.52.0-windows-x64.zip`, **19,139,784 bytes**, SHA-256
`6ebe3671afb5b7700189292cc3c6c2e8a6d770a0f318a878e8eb2df0d1dd95eb`,
published on the release page with its checksum file. Portable folder
`builds/pdfcer-20260911-0005-cb7727e` on D:, 8 files, 35,384,512 bytes.
OneDrive slot **`pdfcer1`**; `pdfcer2` keeps `0.51.0`.
`verify-release.py` **nine of nine**.

**Contents:** `Pass 294.0` and `294.1` — `scan-offpage` and
`redact-offpage`, both taking files, folders and `--recursive`.

★ A local `v0.52.0` tag from an interrupted first attempt pointed at the
BUMP commit, one commit behind the batch feature. Caught by reading the
tag before building, confirmed unpublished with `git ls-remote --tags`,
then moved. **An unpushed tag is a local note; a pushed one is a
published claim** — the check that separates them costs one command.

## 2026-09-10 (502nd filing) — redact-offpage goes batch

**Shipped:** `Pass 294.1` (`d41be61`) — `redact-offpage` takes files,
folders and `--recursive`, matching the scan. `-o FILE` for one input;
`--out-dir DIR` for a batch, **mirroring the input tree** rather than
flattening, because two product folders can hold drawings with the same
file name — a collision this feature's own test copy hit on the first
try. An existing output is skipped and counted unless `--force`, so an
interrupted batch resumes.

**Owed:** still no `docs/core-api/` entry for the `offpage` module.

## 2026-09-10 (501st filing) — `v0.52.0` bumped for the off-canvas Pass

**Shipped:** the version bump to `0.52.0` (`4d5b226`) — `Cargo.toml` and `fuzz/`'s own
lockfile, which is a separate cargo workspace — `v0.51.0` learned that
the hard way, from a release binary whose banner read `-dirty`).

Carries `Pass 294.0`: `scan-offpage` and `redact-offpage`. Release notes
lead with the measurement on the operator's own drawings — 176 of 341
files draw outside the sheet.

## 2026-09-10 (500th filing) — off-canvas content: found, and cut away

**Shipped:** `Pass 294.0` (`04d0099`) — `pdfcer scan-offpage` (files, folders,
`--recursive`) and `pdfcer redact-offpage`. Asked for as ASAP work.

Content drawn outside the page box is still in the file: it prints on a
larger sheet, survives a page-box change, and its text is extractable.
The scan is a read-only census; the removal authors `/Redact` marks over
the four bands around the page box and applies them, so a PARTIAL object
is cut at the page edge by the same code that cuts it at the edge of an
operator's redaction box. **"Outside the page" is a region like any
other** — that observation is the whole Pass; no new geometry surgery
was written.

**Measured on `R:/Products`, 341 files:** 176 affected, 554 pages,
471,840 fully-off objects, 1,152 partial, 0 unreadable. One sheet had
15,927 fully-off objects — a second drawing at x = -600. The 176 are
copied to the operator's test folder.

**Verified end to end** on one: 234 paths dropped, 247 cut, 4,642
off-page glyphs removed; the output re-scans clean and page 1 renders
pixel-identical to the input.

**Owed:** no `docs/core-api/` entry yet (the new module is `pdfcer-core`
public surface); the full workspace suite was not re-run, deliberately,
at the operator's request for speed.

## 2026-09-10 (499th filing) — `v0.51.0` released

**Released:** tag at `1ccd31e`, CI **green** at the tagged commit. Zip
`pdfcer-v0.51.0-windows-x64.zip`, **19,117,523 bytes**, SHA-256
`b8e5741075e17c6c02b159249fb4de77a84d50df2d51f5f6251386a21274ed48`.
Portable folder `D:/builds/pdfcer-20260910-1611-1ccd31e`, 8 files,
35,300,188 bytes. OneDrive slot **`pdfcer2`**; `pdfcer1` keeps `0.50.0`
as the previous version. `verify-release.py` **nine of nine**.
`run-gates.sh` PASS, 30 commands.

**Contents:** six Passes — `290.0`/`290.1`, `291.0`, `292.0`, `293.0`,
plus `289.0`. Headline for an operator: a PDF Acrobat wrote could not
be opened, and now it can.

★ **Two builds were discarded before one was shippable**, both caught by
the binary's own version banner reading `-dirty`: the first was built
before the version bump was committed, the second while `fuzz/Cargo.lock`
— its own cargo workspace, its own lockfile — still carried `0.50.0`.
A release binary that says *"this is not the commit it names"* is not a
release binary. The banner did the work no checklist item would have.

## 2026-09-10 (498th filing) — the queue was a fifth finished work, and the rules now say who enforces them

**Shipped:** `93b7bbf` — 19 of `ROADMAP.md`'s 99 *Next up* items described a
Pass that had already shipped; moved verbatim to
`docs/history/roadmap-nextup-already-shipped.md`. `ROADMAP.md` 30,324 →
26,542; queue 99 → 80 items; register-size debt 136 → 117. Found by script
(a Next-up `Pass N.M` matched against every Shipped heading carrying a hash),
so it will find the next batch too.

Plus the rule-enforcement column (`8e426e9`): `tools/annotate-rule-gates.py` marks every
standing rule whose own text names a script in `tools/`. **44 of 187 do.** The
header records the other 143 as a backlog and explicitly declines to call them
judgment calls, which is a reading nobody has done.

**Owed:** `Backlog` (9,566 lines) is the remaining bulk and needs editorial
judgment. The 143 unenforced rules want a per-rule verdict: gate it, or bin
it.

## 2026-09-10 (497th filing) — the registers were the bottleneck

**Shipped:** `a12dca6`, `233a9ef` — the register trim. `ROADMAP.md`
168,036 → 30,277 lines, `SESSION_LOG.md` 99,597 → 1,446,
`ARCHITECTURE.md` 34,341 → 10,317; history moved verbatim to
`docs/history/`. New gate `tools/check-register-entry-size.py` caps
new entries (150/80/200 lines, 1,200 chars) with 136 pre-existing
entries carried as DEBT. Three filing gates taught to read the
archive. Full reasoning: the two commit messages.

**Why:** the operator said the project *"has slowed to a crawl"*, and
the measurement agreed — 372,011 lines of docs against 455,626 of
code, 2,722 register lines written that day against 5,844 code lines,
the same paragraph landing four times.

**Owed:** `ROADMAP.md`'s *Next up* (12,900 lines) and *Backlog* (9,566)
are the remaining bulk and need editorial judgment, not a script. The
136-entry baseline should shrink. The 231 standing rules still have no
"what enforces this" column — the operator's *"script it or bin it"*
applies there next.

**Note the shape of this entry:** it is 20 lines. That is the point.

## 2026-09-10 (496th filing)

**Shipped:**
- Pass 293.0 (`56c5e55`) — a custom stamp can now be PLACED: one page's
  artwork onto another, as vector. `Pass 288.0` gave pdfcer stamp
  COLLECTIONS (container, names, category) but nothing could draw one
  page onto another, so pdfcer could read the operator's own signature
  stamps and could not stamp anything with them. New
  `EditSession::place_page_artwork(&source_view, source_page, page_index,
  rect) -> Result<PlacedArtwork, EditError>` imports the source page's
  content and resources as a **form XObject** behind a `/Stamp`
  annotation's `/AP /N`. New CLI verb `place-stamp <in> --from
  <collection.pdf> (--stamp NAME | --stamp-page N) --page N (--at X,Y |
  --rect x0,y0,x1,y1) -o <out>`. Closes the last of the five
  `pdfcer-gui` requests filed 2026-09-10.
  **Why a form XObject, not a raster**: what Acrobat writes, and
  architecturally forced by §12.5.5 + §8.10 — keeps the artwork vector,
  keeps it selectable/movable/deletable, never touches the page's own
  content stream (R47). A raster alternative (render the stamp page,
  place via `add_image`) was considered and rejected: not
  Acrobat-compatible, inflates a 5.6 MB CAD drawing per stamp, does not
  survive zooming, picks a resolution nobody asked for — the consuming
  shell had already declined the same alternative for the same reasons
  (reported through the existing decision-058 channel).
  **Disclosures on `PlacedArtwork`** (project rule 4, since the CLI
  invocation is the commit): `scale_x`/`scale_y`/`distorted` (§12.5.5
  maps `/BBox` onto `/Rect` with independent factors — the stretch is
  normative behaviour, not a pdfcer shortcut); `objects_imported`;
  `resources_renamed` (always `0` by construction, per §8.10, reported
  anyway); `source_annotations_ignored`; `source_widgets_ignored` (the
  dynamic-stamp caveat — a dynamic stamp places its design-time text,
  correct as a picture, wrong as a promise); `transparency_group_carried`.
  No `/Name` is written (Table 181 leaves it optional); whether Acrobat
  records one for a custom stamp is an open gap, flagged by name (`R250`).
  New `EditError::SourcePageOutOfRange` (135 variants now).
  Verified on the operator's own
  `%APPDATA%\Adobe\Acrobat\DC\Stamps\YTV_yyfVN1TzJ0_6oei-GB.pdf` — both
  signatures place and render, the same file `Pass 290.0` had to fix
  page-tree-resource handling for just to open. Nine new tests in
  `crates/pdfcer-core/tests/place_artwork.rs`, including an R47
  byte-comparison with an explicit fixture-can-fail assertion (`R225`);
  sabotaged twice, each turning exactly the expected test red.
  `cargo test --workspace`: **5,309 pass over 9 new tests this Pass
  (5,300 prior + 9)**; fmt/clippy `--all-features` clean;
  `check-core-api-verbs` PASS at **224 verbs (223 prior + 1)**.
  `FEATURES.md` gains a new row in the same filing — `core [x] / cli [x]
  / gui [ ]` — the requesting shell asked for the core API only.

**Decisions made this session:**
- None. The form-XObject-behind-an-annotation shape is §12.5.5/§8.10,
  already established elsewhere in `ARCHITECTURE.md` (annotation
  appearance placement); the rejected-raster-alternative rationale is
  reported through the existing decision-058 channel (the external
  `pdfce-gui` consumer had already declined the same alternative for the
  same reasons) rather than argued fresh here. No new crate boundary,
  library choice or invariant.

**Findings + decisions:**
- **A `pdfcer-acrobat-librarian` dispatch this session corrected a
  standing project premise.** Acrobat **Reader** — not only Pro — can
  place an existing custom stamp; it can only NOT author a new stamp
  category. The project memory that reads "Acrobat Reader is available;
  Pro is not" had been carried as implying no stamp-placement artifact
  is obtainable in this environment, and that inference does not follow
  from the fact. One Reader-placed-and-saved PDF would settle the
  `/Name` round-trip gap flagged above, open since `Pass 288.0`. New
  corpus file:
  `Acrobat_Features/markup__custom_stamp_placement_and_appearance_authoring.md`;
  `markup__stamp_text_size_and_resize_behavior.md` upgraded (d)→(a) in
  place — §12.5.5's anisotropic stretch turned an inference into a
  documented fact. This project's own memory entry on the Reader/Pro
  split is corrected accordingly (see this filing's memory update).
- **An empirical PDF-domain finding**, written to `C:\personal_rag\pdf\`:
  the operator's own Acrobat-authored stamp collection contains a page
  whose only content is a `/DCTDecode` RGB image with a **black
  background and no `/SMask`** — a faithful placement puts a black box
  on the page. pdfcer reproduces it exactly (pixel-identical against a
  direct render of the source page), so a future "the stamp looks wrong"
  report would be about the source file, not pdfcer. New lesson:
  `lesson_20260910_stamp_collection_page_with_black_background_dct_image_and_no_smask_places_as_a_black_box.md`,
  indexed in both `pdf\index.md` and the master `personal_rag\index.md`.

**Still in flight:**
- All five `pdfcer-gui` requests filed 2026-09-10 are now closed
  (`place_page_artwork` was the last).
- Owed items 4, 5, 10, 11, 13b, 14, 18 all carried forward, unchanged —
  this Pass does not touch the owed ledger.
- The `/Name`-on-a-custom-stamp round-trip gap (`R250`) is still open,
  but is now potentially answerable via an Acrobat-Reader-placed
  artifact — see the finding above.

**For next session:**
- If an Acrobat-Reader-placed custom stamp PDF becomes available, check
  it for a `/Name` entry to close the `R250` gap.
- This filing had no shell; all commit-message detail beyond what
  `Read`/`Grep`/`Glob` could confirm against the live tree (byte-level
  test/verb counts, the two sabotage mechanisms, the exact new-test
  count) is relayed, not independently verified — see `ROADMAP.md`'s
  sourcing paragraph for `Pass 293.0` for the full list.

## 2026-09-10 (495th filing)

**Shipped:**
- Pass 291.0 (`0173a95`) — a shrunk or clipped stamp label now says so.
  `Pass 287.0` gave `StampFit` three values (`GrowToText`/`ShrinkToBox`/
  `ClipToBox`) but the consuming shell could offer only one of them,
  because the other two decide something the operator did not ask for
  with no channel back to report it. The disclosure channel that should
  have carried this, `AuthoredTextAnnot::applied_autosize`, is `None` for
  every stamp, always — it signals variable-text auto-size (`/DA 0 Tf`),
  and a stamp's fitted size is written as an explicit `/DA` size, so the
  field never fires for the case it was needed for. New
  `AuthoredTextAnnot::stamp_label_fit: Option<StampLabelFit>`, an enum
  (`AsRequested`/`BoxGrown`/`LabelShrunk`/`LabelClipped`,
  `#[non_exhaustive]`) rather than a bare size, since the same number
  means opposite things depending on whether it was requested or forced.
  New `EditSession::add_text_annotation_reporting` returns
  `TextAnnotOutcome`, added alongside the existing verb rather than
  widening its return type. CLI prints all three inference cases,
  nothing for `AsRequested`. Five new tests, `cargo test --workspace`:
  5,292 pass.
- Pass 292.0 (`c11c1aa`) — a placed stamp's label size can now be read and
  written. New read half: `annot::stamp_label_parameters_in` /
  `EditSession::stamp_label_parameters`, returning `{label, size,
  size_source}` with `size_source` one of `DeclaredInDa` /
  `RecoveredFromAppearance` / `DaUnreadable`. New write half:
  `TextAnnotStyle::font_size` (+ `stamp_fit`) on `set_text_annot_style`,
  refused by name on `/Text`. **A live data-loss defect was found and
  fixed on the way in**: the restyle route rebuilds a stamp's appearance
  from a spec read back out of the file, and `text_spec_from_dict`
  deliberately reports `label: None` for a `/Stamp` (correct in
  isolation — `/Contents` must not drive the face) — but the restyle's
  rebuild read that `None` as "no custom label" instead of calling the
  same recovery `resize_annotation` already used, so changing a stamp's
  COLOUR silently replaced its own custom text with the stamp name's
  default. Fixed by calling the shared recovery from both routes. CLI
  `set-text-annot-style --font-size POINTS [--stamp-fit grow|shrink|clip]`;
  `list-annotations` gains `stamp_label=`/`stamp_size=`/
  `stamp_size_from=`. Eight new tests, `cargo test --workspace`: 5,300
  pass; fmt/clippy clean; `check-core-api-verbs` PASS at 223 verbs.
- Both close two of the three `pdfcer-gui`-channel requests still open as
  of the 494th filing (the shrink/clip-fit disclosure gap and the
  placed-stamp label size read/write gap); a reply is on file
  (`reply_2026-09-10-stamp-label-size-both-halves-and-the-fit-disclosure-
  SHIPPED.md`, relayed — not independently confirmed present, outside
  this session's accessible directories).

**Decisions made this session:**
- None. Both Passes add API surface inside decision 147's existing
  `/DA`-storage reading for a stamp's label — no new crate boundary,
  library choice or invariant.

**Findings + decisions:**
- **`R245` gains a seventh dated instance, in two parts.** Part (a):
  `TextAnnotOutcome::applied_autosize` was wired for one member of a
  size-inference family (`/FreeText` auto-size) and silently `None` for
  a sibling added later (`/Stamp` fit) — the rule arriving
  *retroactively*, through a later Pass growing a new family member onto
  an existing disclosure surface without re-checking coverage, rather
  than being incomplete from day one. Part (b): the stamp-label recovery
  was called by one of two rebuild routes and not its sibling — the
  founding shape exactly, and the more serious of the two instances,
  since the gap silently destroyed operator data rather than merely
  under-reporting. Named `R245` by the engineer's own commit message.
  No amendment to the rule's text; ceiling unaffected, `R245`, next free
  `R246`.
- **A related design principle flagged at n=1, not minted**: the
  requester's own framing for `Pass 292.0` — "a read with no write, and a
  write with no read, are both unbuildable surfaces, so an inspect/modify
  pair for the same property ships together" — is a real stated
  discipline for this Pass but not yet an independently-observed
  cross-Pass pattern. Worth a standing rule at a second, independently-
  arrived-at instance.
- This filing had no shell; all commit-message detail beyond what
  `Read`/`Grep` could confirm against the live tree (see `ROADMAP.md`'s
  sourcing paragraph for the full list) is relayed, not independently
  verified.

**Still in flight:**
- Owed items 4, 5, 10, 11, 13b, 14, 18 all carried forward, unchanged —
  neither Pass this filing touches the owed ledger.
- Only **one** of the five `pdfcer-gui` requests filed 2026-09-10 remains
  open and unscoped: `place_page_artwork` (no verb draws one page's
  artwork onto another page as a form XObject); the requester ranked it
  lowest, and its `as_annotation` shaping question is undecided.
- The two flagged `personal_rag/pdf` findings from the 494th filing
  (Acrobat's own spacer-page habit; the population-scale empirical claim)
  are still unwritten — carried forward again.

**For next session:**
- Scope `place_page_artwork` into a Pass ID.
- Write the two flagged `personal_rag/pdf` findings.

## 2026-09-10 (494th filing)

**Shipped:**
- Pass 290.0 (`556878e`) — a page with no `/Resources` (on itself or any
  ancestor, or resolving to a dangling reference) now opens as an empty
  resource dictionary instead of refusing the whole page tree. Before this,
  `page_tree::resolve_page`'s `?` sat on a walk returning ONE `Result` for
  every page, so one blank spacer page cost the whole document on
  `render-page`, `extract-pages`, `set-page-size` and `extract-text` alike.
  Measured on the operator's own Acrobat-written signature-stamp collection
  (`YTV_yyfVN1TzJ0_6oei-GB.pdf` — a blank spacer page 1, then two perfect
  signature pages) and on pdfcer's own `fixtures/synthetic/minimal.pdf`,
  which has the identical shape. Sourced from Table 30's own `/Resources`
  row ("shall be an empty dictionary" when the page needs none) rather than
  invented; `/MediaBox` deliberately does NOT get the same treatment (no
  clause names a default box), pinned by a test. Disclosed via `Page::
  resources_defaulted`, appended to `render-page`'s and `extract-text`'s
  metrics lines. **Decision 150 minted** — extends decision 145/`R248`'s
  fail-clean kernel to a case where the "other reading" comes from the
  standard rather than from the file; new `ARCHITECTURE.md` §10.6.
- Pass 290.1 (`bce4703`) — `stamp_file::read` used to build its page list
  via `page_tree::pages(doc).map(..).unwrap_or_default()`, so a page-tree
  failure produced an EMPTY list — indistinguishable from `page_index:
  None`'s existing meaning ("this stamp's name points at a page that does
  not exist"). On the operator's real stamp file `pdfcer stamp-list`
  printed `page=MISSING` beside both of his genuine signatures.
  `StampCollection::page_tree_error: Option<String>` now carries the real
  cause; the CLI prints `page=UNKNOWN`, distinct from `page=MISSING`, and
  names the cause on stderr.
- Both close inbound `pdfcer-gui`-channel requests filed 2026-09-10
  (`request_one_resourceless_page_makes_the_whole_document_unopenable_
  and_acrobat_writes_those.md`,
  `request_a_page_tree_failure_is_reported_as_every_stamp_pointing_at_
  nothing.md`); a reply is on file
  (`reply_2026-09-10-a-resourceless-page-no-longer-costs-the-document-
  SHIPPED.md`). Channel state relayed, not independently `Glob`-confirmed
  this filing (no shell in this invocation).

**Decisions made this session:**
- **Decision 150** — a required page-tree attribute that is absent (or
  dangles) defaults to the value the standard itself names for that key,
  when one exists (Table 30's `/Resources` row), rather than refusing the
  page tree; a key with no stated default (`/MediaBox`) is unaffected and
  stays fatal. `ARCHITECTURE.md` §12 + new §10.6, sibling to §10.5
  (decision 145). Explicitly does NOT restate `R248` — the file supplies
  no reading here at all; the standard does, once, for a named key — which
  is why this is its own decision rather than a dated `R248` instance.

**Findings + decisions:**
- **A test that measured exactly what it claimed, reached through a
  defect it was fixing, not through the condition it named — filed as
  `R225`'s 18th dated instance, a new sub-shape.** `fontinfo`'s
  `an_unwalkable_page_tree_is_reported_not_rendered_as_no_fonts` and the
  CLI's `an_unwalkable_page_tree_is_flagged_rather_than_reported_as_empty`
  both obtained "an unwalkable page tree" by relying on `minimal.pdf`'s
  now-fixed `/Resources` defect, and both went RED when the defect was
  fixed. Every prior `R225` instance is a test that measured LESS than its
  name/doc comment claimed; this is the inverse. Both repointed at a new
  fixture, `fixtures/synthetic/xref-recover/page-tree-cycle.pdf` (a
  `/Pages` node listing itself in its own `/Kids`).
- **A first-draft justification was replaced mid-Pass, not merely
  reworded, by the dispatched spec librarian.** The claim "a page with no
  `/Contents` can never name a resource" is false — §7.8.3's third bullet
  lets a form XObject or Type 3 font inherit the page's `/Resources`, and
  the ISO 32000-2 erratum extends that to annotation appearance streams,
  which is exactly the stamp-page shape in play. The decision to default
  survived; the reason given for it did not.
- **A candidate finding at n=2, flagged rather than minted**: a value
  computed and discarded via `.unwrap_or_default()`, whose ABSENCE is then
  read as a content fact, is the same shape as `Pass 285.0`'s whole-buffer
  blank (a different subsystem — redaction, not stamp reading). Worth a
  standing rule if a third instance surfaces; not yet.
- **`docs/NEXT_SESSION.md` is now stale** — it still states "the queue is
  empty of inbound work" as of `Pass 288.0`; two more requests have since
  arrived and closed. Flagged for the engineer, not edited (that file is
  engineer-owned).
- Spec-librarian corpus additions from this session (relayed, not
  independently confirmed by this filing): `D:\Dev\Rag-Specialized\
  PDF_Spec\iso32000\iso32000__ref__page_required_attributes_absent.md`,
  amendments to `iso32000__s__7.7.3.md` and the ambiguity register
  (`PR-N1`/`PR-N2`), noting `/MediaBox` absent is a separate case
  (register `PB-A5`, not §7.7.3.4) — worth a `personal_rag/pdf` finding
  for Acrobat's own habit of writing a contentless, resourceless spacer
  page inside a user stamp collection, and for the PDF Association CTO's
  quoted empirical population claim ("a lot of PDFs out there fail this
  simple validation") — **not written this filing** (budget; flagged for
  next librarian session, not forgotten).

**Still in flight:**
- Owed items 4, 5, 10, 11, 13b, 14, 18 all carried forward, unchanged.
- Three of this morning's five `pdfcer-gui` requests remain open, in the
  requester's stated priority: placed-stamp label size read/write gap,
  the shrink/clip-fit disclosure gap (`applied_autosize` is `None` on
  every stamp), and `place_page_artwork` (no verb draws one page's
  artwork onto another). Not yet scoped to Pass IDs.
- The candidate `personal_rag/pdf` findings named above (Acrobat's own
  spacer-page habit; the population-scale empirical claim) are named but
  not yet written — carry forward as a small owed task, not a numbered
  owed-ledger item (a documentation debt, not a defect).

**For next session:**
- Scope the three remaining open `pdfcer-gui` requests into Pass IDs.
- Write the two flagged `personal_rag/pdf` findings.
- Treat `docs/NEXT_SESSION.md` as stale until the engineer refreshes it —
  do not carry forward its "queue is empty" claim.

## 2026-09-10 (493rd filing)

**Shipped:**
- Pass 289.0 (`9b7bc6c`) — a `/Text` sticky note or `/Stamp` naming a
  standard icon, with no `/AP`, is now painted from pdfcer's own icon
  artwork rather than left blank. Triggered by the operator's
  `Annotations_output.pdf` (PDFsharp 1.3): three annotations, no `/AP` on
  any of them, `R43` correctly left the page blank while Acrobat Reader
  showed content. §12.5.6.4 Table 172 and §12.5.6.12 Table 181 put a
  `shall` on **conforming readers**, not on the annotation, to provide
  predefined icon appearances — so drawing the icon discharges a duty the
  standard assigned to pdfcer, not synthesis. `R43` is narrowed, not
  repealed: `/Square`, `/Circle`, `/Line`, `/Ink`, `/Caret` stay governed
  by its original text (their `shall`, where one exists, addresses the
  annotation, not the reader). New counter `annots_icon_painted`,
  appended to `render-page`'s metrics line; old "not painted" stderr note
  rewritten rather than left to contradict the new pixels. 6 tests
  (`crates/pdfcer-render/tests/named_icon_without_ap.rs`) — a sabotage of
  the subtype restriction survived on the original 5 because a `/Square`
  control failed for the wrong reason (a different guard caught it first);
  a `/FreeText` fixture, the 6th test, separates the two failure modes.
  `tools/run-gates.sh` PASS 29/29 (relayed, no shell this filing).

**Decisions made this session:**
- **Decision 149** — the grammatical subject of a spec `shall` clause
  ("conforming readers shall…" vs "the annotation shall…") is the
  discriminator for whether a no-`/AP` look is forbidden synthesis or an
  obligation `R43` does not reach. `ARCHITECTURE.md` §12; `R43`'s own
  `ROADMAP.md` entry carries a matching narrowing note. No body-section
  edit — a rendering-policy change inside `pdfcer-render`'s existing
  annotation-paint loop, not a crate-boundary or invariant change.

**Findings + decisions:**
- **A corpus sentence sourced 2026-07-31 sat unused until 2026-09-10.**
  §12.5.2's "individual annotation handlers may ignore this entry and
  provide their own appearances" was filed into the spec RAG the same
  session `R43` was written, and never reached the decision it governed
  for five weeks / 492 filings. Second recorded instance of this shape —
  the first is the XFA-deprecation finding in `CLAUDE.md`'s Outstanding
  open items. Cross-referenced from decision 149; not yet a standing rule
  (n=2), flagged for `pdfcer-spec-librarian` if a third instance appears.
- A metrics-line gate (`check-metrics-line-contract.py`) used to locate
  the format string's end by naming the last key — a maintenance trap its
  own comment says had already gone stale before, failing loudly and
  unread across several Passes. Fixed to scan to the closing quote
  instead; caught a real omission (`annots_icon_painted` missing from the
  first draft of the published template) immediately. Filed as an `R243`
  dated instance.

**Still in flight:**
- Owed item 18 opened: `decision 145`'s disclosure obligation has a gap
  in the **recovery** path — the same `Annotations_output.pdf`'s
  `startxref` points 134 bytes short of its own `xref` keyword, recovery
  drops the resulting corrupted content-stream object with no anomaly
  recorded, and mis-describes it as "not in the file" when it is present
  and simply declined. Not scoped to a Pass yet.
- Items 4, 5, 10, 11, 13b, 14 carried forward unchanged.
- `docs/FEATURES.md`'s new row for this capability is `gui [ ]` — the fix
  lives in the shared `pdfcer-render` annotation-paint loop, which
  `pdfcer-gui`'s canvas calls directly, but that repo pins `pdfcer-render`
  as a **git dependency** (`branch = "main"`, not a path dependency), so
  it needs `cargo update -p pdfcer-render` in `D:\dev\pdfcer-gui` before
  the fix is actually reachable there. Flagged, not performed by this
  role.

**For next session:**
- Scope owed item 18 (decision-145 recovery-path gap) into a Pass.
- Confirm `pdfcer-gui` has pulled the updated `pdfcer-render` revision
  before treating the FEATURES row's `gui` box as answered either way.
- Consider whether `/FileAttachment`/`/Sound` icon artwork is worth
  building, now that the reader-`shall` pattern is established for them
  too (currently deliberately excluded — no artwork exists).

## 2026-09-10 (492nd filing)

**Shipped:**
- Pass 288.1 (`4b45a96`, committed, not yet pushed) — `stamp-pack` gains
  `--stamps-from <FILE>`, a name-list file (one name per line, `#`
  comments and blank lines skipped) so a 113-page stamp sheet doesn't need
  113 repeated `--stamp` flags. Names are not auto-derived from the
  artwork (the supplied sheets are pure vector, no text layer) — asking is
  better than a picker full of `Stamp001`. Verified against two real
  third-party files: a non-Adobe dynamic-stamp collection with generated
  internal names, and a 113-page pure-vector sheet round-tripping through
  `stamp-pack` → `stamp-list`.
- `tools/edit-source.py` (`aeeecb5`, committed, not yet pushed — the
  pre-push gate was blocking on this until it was filed) — a
  line-ending-agnostic exact-replacement tool. A multi-line `str.replace`
  against a CRLF file with an `\n`-typed pattern matches zero times
  **silently**; this happened three times in one session despite an
  existing written warning in `docs/NEXT_SESSION.md`. The tool refuses a
  non-exactly-one match and writes nothing unless every replacement
  succeeds; patterns are passed as file paths (not shell arguments) after
  this project separately lost content out of a pushed commit message to
  shell-argument mangling.

**Decisions made this session:**
- No new architectural decision — neither filed item touches
  `pdfcer-core`/`pdfcer-render`'s public surface or the object model.
  Decision ledger stays at `148`.

**Findings + decisions:**
- **A hazard written down and hit anyway is a missing tool, not a missing
  warning.** Filed as a dated instance of `R243` (not a new mint) —
  `R243`'s own text already covers "a documented obligation … is not a
  control"; this instance is the same mechanism one layer out (a warning
  failing to stop a repeated *manual* action, not two call sites failing
  to agree on a value). The remedy differs from `R243`'s usual one
  (extract into a shared function) because there is no function to
  extract from a human/agent re-typing an edit by hand — the remedy here
  is a tool that refuses the silent failure mode outright.
- `R250` (minted last filing, 491st, in the Shipped-section banner only)
  was owed its *Standing rules* master-list entry — discharged this
  filing.
- The CRLF/`str.replace` gotcha itself is flagged for
  `troubleshooting-librarian` (`personal_rag/claude_code` or
  `personal_rag/python`) rather than written by this role — it's a
  general Python-scripting-under-Claude-Code finding, not PDF-domain and
  not Rust/egui-ecosystem, so it sits outside every tier this role owns.

**Still in flight:**
- Owed items 4, 5, 10, 11, 13b, 14 all carried forward, unchanged.
- Both `4b45a96` and `aeeecb5` are committed but not yet pushed; the
  push is gated on this filing landing (per the coordinator's mid-dispatch
  note) and was not performed by this role — no shell this filing to
  confirm push/CI state either way.
- The `Annotations_output.pdf` (PDFsharp) `/AP`-less annotation question
  — whether `R43` is being applied outside its territory for a *named
  standard icon* stamp with no `/AP` — is under investigation by the
  engineer via `pdfcer-spec-librarian`, explicitly **not resolved**; do
  not treat as closed.

**For next session:**
- Push `4b45a96` + `aeeecb5`, confirm CI colour with a shell.
- Dispatch `troubleshooting-librarian` for the CRLF `str.replace` lesson,
  if judged worth a personal_rag entry.
- Follow up on the `Annotations_output.pdf` `/AP`-less-stamp investigation
  once `pdfcer-spec-librarian` reports back.

## 2026-09-10 (491st filing)

**Shipped:**
- Pass 288.0 (`554897e`, pushed to `origin/main` per the dispatch — not
  independently verified, no shell this filing) — custom stamp
  collections, readable and authorable, compatible with Adobe's:
  `pdfcer_core::stamp_file`, `EditSession::set_named_pages` (verb 221),
  CLI `stamp-list`/`stamp-pack`. A collection is an ordinary PDF, one
  page per stamp; category = `/Info` `/Title`; stamp names live in the
  catalog's `/Names`→`/Pages` name tree; `#` marks a dynamic stamp
  (read, never authored). There is no separate interchange format —
  "export" is handing someone the PDF, and that is Acrobat's own
  answer too.

**Decisions made this session:**
- Decision **148** minted (`ARCHITECTURE.md` §12): the stamp-collection
  format — category in `/Info` `/Title`, stamp names in the catalog's
  name tree written in **lexicographic** order per §7.9.6 (not page
  order — Adobe's own `StandardBusiness.pdf` proves the two differ),
  `#` prefix for a dynamic stamp, `/PieceInfo` rejected as a red
  herring (present only alongside `/Illustrator` data, never as the
  naming mechanism).
- Standing rule **R250** minted, this role's own synthesis of a finding
  the engineer offered without a number: a Feature-RAG entry labelled
  `(c)` convergent-secondary is a pointer at what to go verify against
  a primary artifact when one is on disk (here, Adobe's own shipped
  stamp files), not a license to build from unchecked. Full text:
  `ROADMAP.md` *Standing rules*.

**Findings + decisions:**
- **The methodological point is the more durable finding.**
  `pdfcer-acrobat-librarian` reached the correct capability shape from
  convergent community sources and correctly flagged two gaps by name
  (where the category name is stored; whether `#` was real) rather
  than guessing. Both were closed by reading Adobe's own shipped stamp
  files directly — two file opens, not a research session — which is
  exactly what the RAG's own `(c)` label pointed at doing.
- `pdfcer-acrobat-librarian`'s two RAG files
  (`markup__stamp_text_size_and_resize_behavior.md`,
  `markup__custom_stamp_file_format.md`) are flagged for that role to
  consider upgrading the two now-closed gaps from `(c)` to `(b)
  observed` — not this role's corpus to edit (hard rule 6's sibling
  boundary, applied to a confidence label rather than content).
- §7.9.6 name-tree order is lexicographic, not page order — Adobe's
  own file proves it (`SBApproved` names page 0, `SBCompleted` names
  page 4). A test deliberately gives page 0 the alphabetically-last
  name so a page-order-emitting implementation fails it.
- A doc-comment orphan (splicing `set_named_pages` above
  `set_info_field` stranded its doc block) — the same shape as earlier
  the same session (`Pass 287.0`).
- A CRLF/LF `str.replace` matched zero times silently for the third
  time this session; a small line-ending-agnostic edit helper was
  built in a temp dir but **not added to the repo** — flagged for the
  engineer to judge whether it belongs in `tools/`.

**Still in flight:**
- Owed items 4, 5, 10, 11, 13b, 14 all carried forward, unchanged.
- Whether `554897e` has actually reached `origin/main`, and current CI
  colour, are relayed from the dispatch only — not independently
  confirmed, no shell this filing.

**For next session:**
- Confirm `554897e`'s push/CI state with a shell.
- Flag `pdfcer-acrobat-librarian` for the `(c)`→`(b)` label-upgrade
  consideration above.
- Judge whether the line-ending-agnostic edit helper belongs in
  `tools/`.

## 2026-09-10 (490th filing)

**Shipped:**
- Pass 287.0 (`1bbb7c1`, committed, not yet pushed) — a stamp's text
  size is now a property (`StampStyle`/`StampFit`, `/DA` storage,
  `--stamp-font-size`/`--stamp-fit`), the box follows the text by
  default, and both size and a custom label are recovered from every
  pre-existing stamp's baked appearance — which also closes a standing
  defect where `resize_annotation` refused pdfcer's own stamps as
  foreign.
- Chore commit `e774a41` (lockfile bumps + `pdfcer-acrobat-librarian`
  agent-memory notes) — filed alongside Pass 287.0, same push gate.
- `v0.50.0` released (relayed from the dispatch — no shell this
  filing): ten Passes since `v0.49.0` (`277.0`→`286.0`), tagged,
  pushed, GitHub release published latest with zip+sha256,
  `verify-release.py` clean on every check, fresh-folder smoke test
  run, OneDrive slot `pdfcer1` (alternating scheme, `0.49.0` preserved
  on `pdfcer2`). 159 GB reclaimed from `target/` in the same window
  (154 GB of it in `target/debug/deps` alone, on a disk at 90% full).

**Decisions made this session:**
- Decision **147** minted (`ARCHITECTURE.md` §12): where the spec
  defines no key for a subtype's derived parameter, pdfcer borrows the
  key the standard already defines for the identical problem on a
  sibling subtype rather than inventing a private sidecar — `/DA` on
  `/Stamp`, sourced from §12.7.3.3's `/FreeText` entry, not
  `/PieceInfo`. Sourced via `pdfcer-acrobat-librarian` before the
  choice was made. A `StampFit` policy is settable but deliberately
  never recovered from an existing file — nothing stored records an
  author's intent, and inferring one from geometry would invent a
  decision nobody made.
- No new standing rule minted. The Pass's fake-test finding (reverting
  the fix left `stretching_a_stamp_keeps_its_text_size` green because
  the fixture stretched only width, and the old formula depended only
  on height) is filed as `R225`'s **17th** dated instance, not a new
  rule or an `R247` instance — it is a fixture that could not
  discriminate two implementations on the axis they actually disagree
  on, squarely `R225`'s family, not a doc comment (`R247`'s shape).

**Findings + decisions:**
- **Two compounding defects, which is why this shipped as a
  `StampStyle`, not a one-line bug fix.** A stamp's label was clipped
  to `/BBox` (right for a form field's box, wrong for a stamp's
  drawing gesture), and the repair scaled the text because the font
  size was `(rect_height * 0.42).clamp(8, 28)`, derived from the box
  and stored nowhere. Either alone is an annoyance; together the first
  mistake is unfixable — you cannot escape the clip by resizing,
  because resizing rescales the text with it.
- **The more valuable finding is the second one the first uncovered: a
  stamp's custom label was stored nowhere either.** `/Contents` is a
  comment *about* a stamp, not its words, so a rebuild always produced
  the stamp name's default label — which is why `resize_annotation`
  refused pdfcer's own stamps as foreign. The long-open
  `request_resize_annotation_refuses_a_pdfcer_authored_stamp_as_foreign.md`
  had been read as a geometry bug; it was a spec-completeness gap. The
  authorship test was correct; the spec it tested against was lossy.
- Both values are now recovered from the appearance itself
  (`EditSession::recover_stamp_parameters`) — the same both-ways trick
  `Pass 276.0` used for `/FreeText`'s `multiline`. Recovery is
  mandatory, not optional: every stamp already in every document has
  no `/DA`, so a stored property alone would silently change all of
  them on first touch.
- `resize_annotation` gained a **third** authorship arm (markup,
  `/FreeText`, now `/Stamp`) — `R245`'s shape on a family of three
  routes, closed rather than merely counted again.
- A dead function (`stamp_font_size_from_appearance`, superseded) was
  deleted rather than kept with a justifying comment — the second time
  this session clippy caught a function kept alive only by its own
  doc comment (the first was `Pass 286.0`'s "the honest raw record"
  field, filed at the 488th filing). `tf_size_in` unified into one
  shared implementation instead of two token scanners that could
  disagree.
- The fuzz target now drives the new `style` field from fuzz input
  rather than `..Default::default()`, on the reasoning that a new
  field satisfied only by its default is a new field nothing fuzzes.

**Still in flight:**
- Owed items 4, 5, 10, 11, 13b, 14 all carried forward, unchanged.
- `docs/NEXT_SESSION.md`'s queue item 2 (the stamp-resize request) is
  now closed by `Pass 287.0` — flagged for the engineer to update
  directly; that file is engineer-owned and was not edited here.
  Its "look for a fourth authoring family" note is answered as far as
  this filing can tell (three families now recognised, closed) but
  whether a fourth was actually searched for and not found, versus
  simply not searched, was not stated in the dispatch and is not
  asserted here either way.
- `e774a41` and `1bbb7c1` are committed but **not pushed** as of this
  filing (per the dispatch); pushing both, and confirming
  `v0.50.0`'s tag/push/release state independently, needs a shell —
  not available to this role this filing.

**For next session:**
- Push `e774a41` and `1bbb7c1`; the operator's own ordered plan
  (`Pass 142.0`, resize-page-contents, `Pass 259.0`, `Pass 10.11`) is
  otherwise untouched, per `docs/NEXT_SESSION.md`.

## 2026-09-09 (489th filing)

**Shipped:**
- Nothing — a librarian reconciliation filing, dispatched and ruled on by
  the engineer directly.

**Decisions made this session:**
- The engineer resolved the `R247` reservation directly (his own explicit
  call, not derived by this role): standing rule `R247` — *a doc comment
  stating a behavioural guarantee is an unenforced claim until a test
  exists that would fail if it were violated* — claims the number. The
  competing "alternate route" sabotage-fixture cause (`n=3`) is
  **withdrawn**, not deferred and not given its own number: its instances
  are already correctly filed inside `R225`'s own family, and a second
  number for one family would only hand a future reader two rules to
  reconcile mid-defect.
- No `ARCHITECTURE.md` §12 decision minted — a standing-rule numbering
  resolution is a librarian/engineer process call, not a crate-boundary/
  library/invariant redefinition, consistent with `R225`'s and `R246`'s own
  precedent (neither carries a §12 decision either).

**Findings + decisions:**
- `R247`'s founding instance is `Pass 285.0`'s `blank_show_strings` doc
  comment (already on record as `R225`'s 16th instance): a stated
  span-scoping safety guarantee that a scope-widening sabotage would have
  violated, caught only because the fixture was widened to separate the
  two implementations' output.
- `R247`'s second, lesser instance is `Pass 286.0`'s "the honest raw
  record" doc comment on a dead field, caught by `clippy` rather than a
  person — recorded to show the rule catches low-severity cases too.
- The two rules are distinguished on the record rather than merged:
  `R225` is a **test** whose own name over-claims relative to its fixture;
  `R247` is a **doc comment** publishing a guarantee no test enforces at
  all. A reader re-derives behaviour from a test only once they distrust
  it; they trust a doc comment *instead of* re-deriving — the entire point
  of documentation-first discipline — which makes the doc-comment case the
  more dangerous of the two.
- Cross-project derivation filed as its own new file (not a section of the
  existing sabotage-fixture file), because the two findings are found and
  repaired differently:
  `D:\dev\rag\rust\a_doc_comment_stating_a_behavioural_guarantee_is_unenforced_until_a_test_would_fail_without_it.md`.
  Dated footer also added to the existing sabotage-fixture file pointing
  forward to it, so a reader who lands there via the 16th-instance note
  does not read `R247` as still unclaimed.

**Still in flight:**
- Owed items 4, 5, 10, 11, 13b, 14 all carried forward, unchanged. Item 9
  (the `R247` reservation) is CLOSED this filing — resolved, not deferred.
- `docs/NEXT_SESSION.md`'s own OWED bullet on `R247` is now stale (it still
  reads "reserved-but-unclaimed... resolve it before a fourth candidate
  lands") — flagged for the engineer to update directly; that file is
  engineer-owned and was not edited here.

**For next session:**
- Nothing `R247`-specific remains. Next items are whatever `NEXT_SESSION.md`
  and the carried-forward owed list (above) already name.

## 2026-09-09 (488th filing)

**Shipped:**
- Pass 286.0 (`369d4de`, pushed to `origin/main` per the dispatch — not
  independently verified, no shell this filing) — closes owed item 17: a
  per-glyph producer's redacted text is no longer single characters.
  `RedactionReport::redacted_text` is now grouped per `/Redact` mark, not
  per show operator — `Surgeon::glyph` returns the region index a glyph
  landed in (was a bare `bool`), and `box_marks` folds a mark's characters
  into one string. `SW41177-obselete.pdf` (GPL Ghostscript 8.15) drew one
  glyph per show operator, so marking `3.5 TYP` used to yield
  `["3", ".", "5", " ", "T", "Y", "P"]` and a consuming absence proof
  refused a correct redaction on finding `"3"` on every page. Now yields
  `["3.5 TYP"]`.

**Decisions made this session:**
- None minted. Not a crate-boundary/library/invariant change — a bug fix
  in a shared field's granularity.

**Findings + decisions:**
- **The three-consumer analysis.** `redacted_text` has three readers (the
  absence proof, `carrier_info`, `residual_sweep`'s `redaction_evidence`)
  and joining characters into per-mark strings is safe for all three only
  because it strictly LENGTHENS entries — never shortens them — so every
  reader's match floor (`MIN_MATCH_LEN` = 4 characters) is cleared more
  reliably, not less. A change that split entries instead would not carry
  the same guarantee, and that asymmetry is the only reason a field with
  three readers could be changed in one Pass. Flagged as a trap in the
  handoff before the Pass was built; resolved in the safe direction.
- **Nothing new was inferred to make the fix** — `Surgeon::glyph` already
  computed which region a glyph landed in; it was discarding that as a
  bare `bool` one line before the caller needed it.
- **A kept, justified, unread field was deleted, not excused.** The old
  per-operator `removed_text: Vec<String>` field was first kept beside the
  new map with a doc comment calling it "the honest raw record"; `clippy`
  flagged it as unread and it was deleted. Recorded as a recurring
  self-deception shape: an unread field with a justification attached is
  not a record, it is dead weight with an excuse.
- **The fixture is the finding, again.** A producer drawing the run in a
  single `Tj` cannot distinguish old grouping from new (both report
  `["3.5 TYP"]`), so a test written on an ordinary producer would have
  been green before and after this Pass, measuring nothing. The new
  fixture emits one `Tm … (c) Tj` per character.
- `docs/FEATURES.md`'s *Apply redaction* row amended in place: owed item
  17's sentence replaced with the fix and the per-mark grouping named.
- `C:\personal_rag\pdf\`: a second dated footer added to the existing
  2026-09-09 lesson on this producer (the "joining half" is no longer
  unbuilt); subject-index and master-index bullets corrected in place.

**Still in flight:**
- Owed items 4, 5, 9 (`n=3`, `R247` reservation unreconciled), 10, 11,
  13b, 14 all carried forward. Item 17 discharged this filing.
- **`R247` reservation flagged for a FOURTH consecutive filing** (475th,
  483rd, 486th, 487th, now 488th) — still unreconciled, two candidates
  contesting the slot (a second `///`-guarantee-with-no-enforcing-code
  instance; the "alternate route" sabotage-fixture cause at `n=3`). `R248`
  and `R249` were both minted past it deliberately. Nobody has yet sat
  down with time to resolve it.
- Whether `369d4de` has been pushed or released is relayed from the
  dispatch only, not independently checked — the engineer should verify
  directly.

**For next session:**
- Resolve the `R247` reservation — this is now a fourth consecutive
  filing carrying the flag forward unresolved, the longest it has run.

## 2026-09-09 (487th filing)

**Shipped:**
- Pass 285.0 (`1366138`, pushed to `origin/main` per the dispatch — not
  independently verified, no shell this filing) — closes owed item 18
  (`Pass 284.0`): an abandoned content stream's drawn text is now blanked,
  not merely named. `blank_show_strings` parses the stream and touches only
  the operand spans of `Tj`/`TJ`/`'`/`"`, never the whole buffer, so a
  resource name sharing bytes with the redacted evidence is never
  corrupted into one that resolves to nothing. New counter
  `residual_content_streams_blanked`. Still declines and discloses (the
  sweep's existing floor): a non-parsing stream, and glyph-code text on a
  subset font.

**Decisions made this session:**
- None minted. `R225` gains a 16th dated instance (severity escalation,
  not a new cause) to its RAG file; `R249`/`R247` untouched.

**Findings + decisions:**
- **A scope-widening sabotage survived all eight tests in
  `redaction_residual_sweep.rs`.** The wrong implementation (blank every
  byte-occurrence of the evidence in the whole buffer, not only the
  show-operator spans) agreed exactly with the correct one on the shipped
  fixture, because the fixture's only occurrence of the word was inside a
  string operand — the one place both implementations blank. Outside that
  string the wrong implementation also corrupts resource names, which is
  precisely the failure the function's own doc comment names as the
  reason for the narrower scope. Fixed by widening the fixture (the word
  now also appears in a resource name) rather than by strengthening the
  assertion, which could not have discriminated on the old fixture no
  matter how it was written.
- **Escalation recorded explicitly, at the engineer's request**: every
  prior instance of this project's `R225` sabotage-fixture family is a
  test measuring less than its own *name* claimed. This is the first
  where the survived sabotage would have shipped a defect the project's
  own *documentation* claimed was impossible — judged a severity clause
  on the existing "scope or filtering" degenerate-value row, not a new
  cause and not a rule amendment. Filed as the RAG file's 16th dated
  instance; no mint, `R225`'s founding text unchanged, `R249` remains the
  standing-rule ceiling and `R247` remains reserved-but-unclaimed.
- **Test amended, not deleted, honest half preserved as a new control**:
  `Pass 284.0`'s `an_unreachable_content_stream_is_named_not_silently_left`
  became `an_abandoned_content_streams_drawn_text_is_blanked` (old
  assertions kept struck through in place); the disclosure half it used
  to carry survives as a new, separate test,
  `a_stream_that_cannot_be_blanked_is_still_named`, over a fixture the
  blanking function structurally cannot reach — without it, a future
  silent regression in the disclosure would pass unnoticed.
- `docs/FEATURES.md`'s *Apply redaction* row amended in place: owed item
  18's sentence replaced with the fix, the new counter named, and the two
  remaining declining cases named where item 18 used to be.
- `D:\dev\rag\rust\a_sabotage_can_only_be_as_discriminating_as_the_fixture_it_runs_on.md`
  gains a 16th dated footer (the escalation above); its `index.md` bullet
  extended in the same edit, along with a compact catch-up note for
  instances 12–15 which the index bullet had fallen behind on.

**Still in flight:**
- Owed items 4, 5, 9 (`n=3`, `R247` reservation unreconciled), 10, 11,
  13b, 14, 17 all carried forward. Item 18 discharged this filing.
- The `R247` reservation is still unreconciled — flagged again, three
  filings running now.
- Whether `1366138` has been pushed or released is relayed from the
  dispatch only, not independently checked — the engineer should verify
  directly.

**For next session:**
- Resolve the `R247` reservation before a fourth unrelated candidate
  makes the gap harder to reconcile — this is now the third consecutive
  filing carrying that flag forward unresolved.
- `D:\dev\rag\rust\index.md`'s summary bullet for the sabotage-fixture
  file had drifted behind its own source file (missing instances 12–15
  before this filing added a catch-up note) — worth a proper backfill
  next time an `D:\dev\rag\rust\` index-check runs, rather than leaving
  the catch-up note as the permanent form.

## 2026-09-09 (486th filing)

**Shipped:**
- Pass 284.0 (`ea4acb3`, pushed to `origin/main` per the dispatch — not
  independently verified, no shell this filing) — the queue's head owed
  item ("an orphaned `/Info`-shaped object survives a redaction") turned
  out to be one instance of a class: every redaction carrier finds its
  target by navigating the document graph, while the writer emits objects
  by enumerating the cross-reference table, and every object in the
  difference was re-emitted verbatim into a redacted file, never offered
  to a carrier. New fourteenth carrier `residual_sweep` closes it,
  scoping the sweep to the xref table's own listing rather than to a
  computed reachability walk. Closes owed item 16; also closes three
  carriers nobody had filed (a thread's own information dictionary, and
  two further XMP routes, §14.3.2 B/C) as a byproduct of sweeping instead
  of enumerating. New owed item 18: a non-metadata content stream
  carrying redacted text is named, not removed.

**Decisions made this session:**
- **Decision 146** (`ARCHITECTURE.md` §12, body §5.9): a destructive
  sweep obliged by an outcome-shaped requirement ("remove all traces of
  X") is scoped by the evidence the requirement itself names, never by a
  computed reachability walk — because reachability computations on a
  graph-shaped format fail silently rather than loudly, and the census
  probe built to measure this very fix reproduced that failure shape
  twice within the hour it was written.
- **Standing rule `R249` minted** from the engineer's own generalisation,
  which was offered unnumbered and left for this filing to judge. Minted
  past the still-reserved-but-unclaimed `R247`, same precedent `R248`
  itself set one filing ago. Full text in `ROADMAP.md` *Standing rules*.

**Findings + decisions:**
- **A census probe made the exact mistake it was written to catch,
  twice, within an hour** — `examples/unreachable_census.rs`, written
  immediately after reading §12.5.6.23, first counted every object
  stream as an orphan, then every cross-reference stream, before landing
  on the correct figure. Both wrong numbers (21% and an intermediate
  figure) were caught only by measurement, never by re-reading the
  clause — the final reported figure moved 21% → 12%. Recorded because a
  filing that keeps only the final 12% loses the lesson that a careful,
  purpose-built reachability computation, written by someone who had just
  argued against trusting reachability computations, made the trap-shaped
  error anyway.
- **Two implementation bugs in the new sweep, both caught by tests
  written for earlier Passes**, an argument against deleting a test whose
  subject you are changing: a staged span not indexing the base buffer
  (`stage()` allocates at `base_len + staging.len()`, so slicing the
  original bytes with a replaced stream's span read the wrong region);
  and two different empty answers collapsed into one report value ("no
  text redacted at all" vs. "text redacted but all of it below the match
  floor" both reported the same way in the first cut).
- **A PDF-domain empirical finding**, filed to `C:\personal_rag\pdf\`:
  measured over the operator's own 57-file drawing set, 12 files (21%)
  carry objects the cross-reference table lists but the document graph
  never reaches, 20 such objects total, 7 of which could carry drawn
  text — merge outputs are the worst offenders. Distinct from the spec
  text half (§14.3, filed by `pdfcer-spec-librarian`).
- **Two new `D:\dev\rag\rust\` findings**: the staged-span base-offset
  indexing bug (generalises to any base-plus-staging-buffer pattern), and
  the census-probe-reproduces-the-trap-it-measures finding (generalises
  as a caution about trusting a freshly-written verification computation
  more than the mechanism it is checking, when both share a structural
  blind spot).
- `docs/FEATURES.md`'s *Apply redaction* row amended in place: owed item
  16's sentence replaced with the fix (fourteenth carrier, two new
  counters, the three incidentally-closed carriers, the evidence-not-
  reachability argument in brief), and the new non-metadata-stream gap
  named where item 16 used to be.

**Still in flight:**
- Owed items 4, 5, 9 (`n=3`, `R247` reservation unreconciled), 10, 11,
  13b, 14, 17, 18 (new this filing) all carried forward.
- The `R247` reservation is now flanked by three unrelated, already-
  decided-past-it rules (`R248`, `R249`) — worth resolving soon so the
  numbering gap does not become confusing in its own right.
- Whether `ea4acb3` has been pushed or released is relayed from the
  dispatch only, not independently checked — the engineer should verify
  directly.

**For next session:**
- Item 18 (non-metadata content stream carrying redacted text, named not
  removed) is queued, unstarted — a destructive act on a new object
  class, deliberately not folded into `Pass 284.0`.
- Resolve the `R247` reservation before a fourth unrelated candidate
  makes the gap harder to reconcile.

## 2026-09-09 (485th filing)

**Shipped:**
- Pass 283.1 (`d8fcb68`) — addendum to Pass 283.0: the disclosed,
  overridable malformed-PDF policy reached only
  `Document::from_bytes_with_options`; every real shell opens a **file**,
  not bytes. New `Document::load_with_options(path, password, options)`,
  and `pdfcer`'s own `open_document` now uses it instead of duplicating
  `Document::load`'s `std::fs::read`. The CLI's `--on-malformed` override
  is now reachable from a real invocation, not only from the bytes-based
  test harness.

**Decisions made this session:**
- None minted. This is a completeness fix inside decision 145's own
  mechanism (`ARCHITECTURE.md` §10.5, §12), addended in place rather than
  re-argued.

**Findings + decisions:**
- Sixth dated instance of standing rule `R245` (`ROADMAP.md` *Standing
  rules*): the rule's shape (a guard/key/disclosure shipped on one member
  of a parallel family, untested on the rest) recurs over a family of TWO
  ENTRY POINTS rather than verbs, and the withheld item is an *affordance*
  rather than a restriction — `docs/core-api/01-reading-and-model.md`
  §3.6b already names this explicitly. No amendment to `R245`'s text; the
  fix (a test that opens the same path twice, once directly and once
  through `load_with_options`) is exactly the family-wide test the rule
  asks for.
- Also `R151`-adjacent, noted rather than merged: the affordance had test
  callers all along, just not its intended production caller — `R151`'s
  canonical shape is zero callers, so this stays a distinct observation
  under `R245` rather than folding into `R151`'s text.
- Filed as a dated footer on the existing `D:\dev\rag\rust\` finding
  (`a_guard_or_key_added_to_one_sibling_verb_is_untested_until_a_family_wide_test_exists.md`),
  not a new file — the underlying mechanism is unchanged, only the family
  shape. `index.md` line updated in the same edit.
- `docs/FEATURES.md` row 169 (*Document & pages*) sentence amended to name
  `Pass 283.1`'s fix; no checkbox moved — the row was already correctly
  ticked for the capability, which now, post-fix, actually reaches a real
  invocation rather than only the bytes-based test harness.

**Still in flight:**
- Owed items 4, 5, 9 (`n=3`, `R247` reservation unreconciled), 10, 11, 13b,
  14, 16, 17 all carried forward unchanged.
- Whether `d8fcb68` has been pushed or released is not asserted here (no
  shell) — the engineer should check directly.

**For next session:**
- Same open items as the 484th filing's own "For next session" entry —
  nothing new surfaced by this addendum beyond the `R245` instance
  captured above.

## 2026-09-09 (484th filing)

**Shipped:**
- Pass 283.0 (`dce2223`) — a PDF with structural errors now opens instead
  of refusing: six defect classes (duplicate dictionary key, missing/
  unusable `/Length`, missing `endobj`, an unparseable object, an xref/body
  id disagreement, an unreadable object stream) are each recorded as a
  `LoadAnomaly` — what pdfcer decided, and what it discarded — with a new
  CLI `--on-malformed keep-last|keep-first|refuse` letting the operator
  take the other decision. Prompted by the operator's own file (a real
  drawing Acrobat opens and pdfcer refused, on a duplicate `/PageMode`
  key) and his own ruling that this must generalize to "all defects where
  it is possible to continue and open the file" — not a one-defect patch.

**Decisions made this session:**
- **Decision 145** (`ARCHITECTURE.md` §12, body section §10.5): a
  structural defect that leaves the object graph AMBIGUOUS rather than
  UNDEFINABLE is opened under a disclosed, overridable default; only a
  defect requiring pdfcer to INVENT a reading (no `/Root`; encryption with
  no working password) stays fatal. Argued as an extension of `R27`'s
  fail-clean kernel from the decoder layer to the loader layer, not a
  relaxation of it — `R27` was always about silence, never about refusal.
- **Standing rule `R248` minted**, argued rather than deferred, directly
  from the operator's own general-scope ruling (matching this project's
  `R35`/`R58`/`R67` precedent for minting from a decisive ruling rather
  than waiting for a second occurrence). Numbered past the still
  reserved-but-unclaimed `R247` deliberately, to avoid entangling this
  claim with that unrelated, unreconciled reservation (two other
  candidate triggers, neither decided).

**Findings + decisions:**
- **An analogy dressed as a citation, caught before shipping.** The first
  draft justified keeping the LAST value on a duplicate key by citing
  §7.5.6 (incremental-update object ordering) — an ordering the standard
  makes meaningful, borrowed to justify a decision about §7.3.7's
  dictionary-entry order, which the standard's own preceding sentence says
  "shall be ignored." `pdfcer-spec-librarian`'s answer corrected it before
  the code shipped: the real support is observed behaviour (qpdf, pdf.js,
  pdfium all keep-last, none refuses), not an internal spec analogy. Filed
  as a new `D:\dev\rag\rust\` methodology finding — close in spirit to
  `R246`'s reference-corpus reinfection finding, but about analogical
  reasoning rather than a stale figure.
- **A flag named for one member of the class it governed.** The first cut
  called the new CLI flag `--duplicate-keys` while its `strict` value also
  silently disabled two unrelated recoveries (`/Length`, `endobj`).
  Renamed to `--on-malformed` before shipping. Filed as a second new
  `D:\dev\rag\rust\` methodology finding.
- **A PDF-domain empirical finding, filed to `C:\personal_rag\pdf\`
  rather than duplicated into the spec RAG:** real-world readers (qpdf,
  pdf.js, pdfium) converge on keep-last for duplicate dictionary keys,
  where ISO 32000 itself leaves reader behaviour explicitly out of scope
  (pdf-issues #199). The spec-text half (the `shall not`, the erratum
  #3 precedent) is `pdfcer-spec-librarian`'s territory and is already
  filed there (new `iso32000__s__7.3.7.md`, supersession redirect,
  register entries `DK-A1`/`DK-A2`).
- `docs/FEATURES.md` gains a **new** row under *Document & pages*, kept
  deliberately separate from the existing xref-recovery row (different
  mechanism: object-level ambiguity resolution with disclosure/override,
  vs. xref-table rebuild-by-scan) — `[x]` core, `[x]` cli, `[ ]` gui,
  `[x]` Acrobat, with the exceed named explicitly: Acrobat opens such
  files too but does not disclose which value it kept or offer the
  alternative.

**Still in flight:**
- Items 4, 5, 9 (`n=3`, `R247` reservation unreconciled), 10, 11, 13b, 14,
  16, 17 all carried forward unchanged from the 483rd filing — this Pass
  originated from a fresh operator report, not from a prior owed item, and
  discharged none of them.
- Whether `dce2223` has been pushed or released is not asserted here (no
  shell) — the engineer should check directly.

**For next session:**
- The `R247` reservation is now flanked on both sides by unrelated,
  already-decided-past-it work (`R246` below it, `R248` above it) — worth
  resolving soon so the numbering gap does not become confusing in its own
  right.
- Owed items 16 and 17 (orphan `/Info`-shaped object; per-glyph
  absence-proof joining) remain queued, unstarted.
- Item 13b's measured blocker is now on record in `ROADMAP.md`: a stamp's
  label size (`(h * 0.42).clamp(8.0, 28.0)`) is derived at bake time and
  stored nowhere, so a re-bake has nothing to recompute from without a new
  stored field.

## 2026-09-09 (483rd filing)

**Shipped:**
- Pass 282.0 (`a83c6e6`) — `redact::carrier_info` (the `/Info`
  metadata-carrier redaction-diligence check) had two opposite
  defects, both reporting `scrubbed`: a one-directional match (a
  redacted run *longer* than the `/Info` string could never match) and
  no length floor (a single-character redacted piece on a per-glyph
  producer matched almost any string). Found while smoke-testing Pass
  281.0 on a file from the private corpus. Fixed by `redaction_evidence`
  matching whole runs **and** their whitespace-delimited tokens at a
  4-character floor, and a new `CarrierAction::CheckedClean` that
  distinguishes a present-and-clean `/Info` from no `/Info` at all
  (previously both reported `Absent`). Discharges the 482nd filing's
  owed item 15.

**Decisions made this session:**
- No new decision minted (`ARCHITECTURE.md` §12 untouched) — a bug fix
  and a numeric-constant alignment with a consuming project, not a
  crate-boundary/library/invariant redefinition.
- Declined to mint a standing rule for the "alternate route"
  sabotage-fixture cause, despite it reaching its third occurrence in
  one calendar day (past this project's own `n=2` minting precedent).
  Argued in the roadmap entry: mint/decline decisions for this family
  belong to the engineer inside the finding Pass, not to a
  roadmap-filing pass; the next free rule number (`R247`) is already
  contested by an unrelated trigger and should be reconciled before a
  second cause is folded in; the RAG file's dated-footer mechanism
  already captures every occurrence at low cost. Flagged for the next
  session with time to reconcile `R247`.

**Findings + decisions:**
- The floor (`MIN_MATCH_LEN = 4`) was chosen to match the consuming
  project's own `MIN_VERIFIABLE_LEN`, deliberately — two independent
  redaction-evidence checks disagreeing on the floor would produce a
  file one project calls scrubbed and the other calls unverified.
- Third instance in one session (and third occurrence specifically of
  the "alternate route" sabotage cause) of a test whose fixture could
  not exhibit the defect its name claimed: the first version marked by
  search for a single word contained outright by the metadata string,
  so the *old, unchanged* containment rule decided the case
  regardless of the new token-split logic under test. Repaired by
  redacting a multi-word region instead. New dated footer (instance
  15) in `D:\dev\rag\rust\a_sabotage_can_only_be_as_discriminating_as_the_fixture_it_runs_on.md`.
- **★ A different, new redaction-diligence gap measured and NOT
  fixed:** the file that started this still has one survivor — its
  `/Keywords` lives in an `/Info`-shaped object (140) superseded by
  another (145) that the current trailer now names, but object 140 is
  still listed in the cross-reference table and is therefore
  re-emitted verbatim by the forced full rewrite. `carrier_info` only
  inspects the trailer's own `/Info`; no carrier covers an orphan.
  `prior_revisions action=dropped_by_rewrite` remains true and
  accurate — it is about superseded byte ranges, not objects the xref
  table still names. New PDF-domain lesson,
  `C:\personal_rag\pdf\lesson_20260909_a_superseded_info_shaped_object_still_xref_listed_survives_a_full_rewrite_untouched_by_a_trailer_scoped_scrub.md`.
  Filed as owed item 16; wants its own Pass and a reading of
  §12.5.6.23's "all content" against an xref-listed, trailer-orphaned
  object.
- The length-floor idea behind this Pass's fix is the same one named
  in the same-day Ghostscript lesson
  (`lesson_20260909_ghostscript_8_emits_one_glyph_per_show_operator_so_string_level_checks_see_single_characters.md`)
  for the GUI's content-stream absence proof; applied here to a second,
  independent consumer. That lesson's "joining" half (words from
  adjacent single-glyph shows) remains unbuilt — dated footer added,
  filed as new owed item 17
  (`request_redacted_text_carries_single_characters_on_a_per_glyph_producer_so_the_absence_proof_is_blind.md`,
  confirmed at the source, replied to, not built).

**Still in flight:**
- Items 4, 5, 10, 11, 13b, 14 carried forward unchanged.
- Item 9 (the "alternate route" sabotage cause) strengthened from
  `n=2` to `n=3`; still not minted, `R247` reservation still
  unreconciled.
- Item 15 (the `carrier_info` diligence gap) discharged by this Pass.
- Items 16 and 17 (new): the orphan `/Info`-shaped object, and the
  per-glyph absence-proof "joining" request, both above.
- Whether `a83c6e6` has been pushed or released is not asserted here
  (no shell) — the engineer should check directly.

**For next session:**
- Resolve the `R247` reservation conflict (the "alternate route"
  sabotage cause, now at `n=3`, vs. the unrelated `clap`-derive
  doc-guarantee trigger named 2026-09-08) with time to reconcile both
  properly, rather than guessing one onto the number.
- Owed item 16 (orphan `/Info`-shaped object surviving a full rewrite)
  is redaction-area and reachable on a real file — worth scoping into
  its own Pass before the next redaction-adjacent change.
- Owed item 17 (per-glyph absence-proof joining) has been open since
  earlier the same day; still unbuilt.

## 2026-09-09 (482nd filing)

**Shipped:**
- Pass 281.0 (`1177221`) — a hybrid-reference file (ISO 32000-1
  §7.5.8.4, classic xref table + `/XRefStm`) can now be fully rewritten,
  so redaction — which is forced to a full rewrite by `R35` — finally
  reaches it. The old refusal's own named remedy ("use incremental
  save") was the one thing a redaction is forbidden to take, so
  redaction was unreachable on every such file. Reported by
  `pdfcer-gui` against the operator's own SolidWorks-drawing-set file,
  asked about three times; discharges the 481st filing's owed item 13
  (hybrid half).

**Decisions made this session:**
- No new decision minted (`ARCHITECTURE.md` §12 untouched). This was an
  engineering fix and a correction to a refusal's own stated
  reasoning, not a crate-boundary/library/invariant redefinition.
- `R33` ("the writer never normalizes") gains a dated clarifying note
  in the Standing Rules body: it is UPHELD, not waived, by this Pass —
  the rewrite reproduces the file's own two-part partition rather than
  collapsing it to one section.

**Findings + decisions:**
- The old refusal's doc comment gave two true reasons and reached a
  false conclusion: the "merged view" it said would need re-deriving
  was pdfcer's OWN load-time merge (`merge_first_wins`), discarding a
  fact (which objects the `/XRefStm` established) that only needed to
  be *retained*, not re-derived. Re-deriving would in fact have been
  wrong — §7.5.8.4 states what a producer MAY hide, not what this file
  DID hide.
- A corpus-harness check (`tools/roundtrip`'s `is_section_object`) was
  complete on the day it was written only because the hybrid case
  could not arise; the moment the writer stopped refusing it, every
  hybrid file's xref-stream object was misreported as an unexplained
  change. Confirmed by probing stream-object numbers before fixing.
  Generalizable — new file in `D:\dev\rag\rust\`
  (`an_enumerating_check_complete_today_goes_silently_incomplete_the_day_a_refused_case_becomes_possible.md`).
- A test fixture corrupting an offset-bearing structure (the broken
  `/XRefStm`'s `/W` array) must corrupt to the SAME total width — the
  first attempt widened `/Length` too, shifted every byte offset,
  triggered the loader's rebuild-by-scan fallback, and silently tested
  a different failure. PDF-domain finding — new lesson in
  `C:\personal_rag\pdf\`
  (`lesson_20260909_same_length_corruption_is_the_only_honest_way_to_corrupt_an_offset_bearing_fixture.md`);
  the earlier same-day lesson recording the refusal
  (`lesson_20260909_excel_365_exports_hybrid_reference_pdfs_that_refuse_a_full_rewrite.md`)
  corrected in place with a dated footer, not deleted.
- Corpus measured before/after on the private corpus (name withheld
  per the operator's standing ruling): full-rewrite per-object-verbatim
  225/237 → 237/237; hybrid refusals 12/237 → 0/237; mutation-gate
  denominator 225 → 237; raster oracle 456/456 → 468/468. No
  shortfalls either direction. End-to-end proof through the binary on
  a real hybrid file: `redact-apply` went from refusing verbatim to
  `pages_redacted=4 marks_applied=12 glyphs_removed=170`.
- **★ Redaction diligence gap measured, NOT fixed this Pass:**
  `redact::carrier_info` drops an `/Info` string containing a redacted
  run only when the run is no LONGER than the string; a longer run is
  not detected, yet the carrier report still says `action=scrubbed`.
  Observed live on the smoke-test file (`/Keywords` kept a string
  sharing the redacted word while the report read "scrubbed").
  Pre-existing; this Pass makes it reachable on more files. Filed as
  owed item 15, flagged for priority attention as a redaction-area
  finding.

**Still in flight:**
- Items 4, 5, 9, 10, 11 carried forward unchanged.
- Item 14 (second instance of the file-channel-blindness cause,
  flagged not minted) carried forward unchanged.
- Owed item 13 split: the hybrid half (13a) is discharged by this
  Pass — a reply closing the request to `pdfcer-gui` is owed but not
  written by this filing (no shell); the `/Stamp`-resize half (13b)
  remains queued and unstarted.
- Item 15 (new): the `carrier_info` redaction diligence gap above.
- Whether `1177221` (or `26ef381`) has been pushed or released is not
  asserted — no shell this filing.

**For next session:**
- Send the reply to `pdfcer-gui` closing the hybrid-reference request.
- Scope `resize_annotation`'s `/Stamp`-as-foreign refusal (item 13b)
  into a Pass.
- Fix the `carrier_info` redaction diligence gap (item 15) — a
  correctness gap in the redaction area, not merely a report-wording
  issue.
- Reconcile `R221`'s instance count (item 10, long-carried).
- Watch for a third instance of the file-channel-blindness cause.

**★ Amendment, 2026-09-09 (483rd filing):** `ROADMAP.md`'s owed-item-13
text (mirrored into this entry's own **Shipped** bullet above, which was
already correctly worded) said the hybrid file was "measured on the
operator's own SolidWorks sheet." That is wrong about the producer: the
file is `SW41177 MATERIAL REQUIREMENTS.pdf`, exported by
`Microsoft® Excel® for Microsoft 365` (`/Producer` and `/Creator` both),
sitting inside a SolidWorks drawing set alongside two genuinely
SolidWorks-exported sheets (`SOLIDWORKS PDF Publisher`, 2022/2024) that
are **not** hybrid and rewrite cleanly — recorded the same day in
`C:\personal_rag\pdf\lesson_20260909_excel_365_exports_hybrid_reference_pdfs_that_refuse_a_full_rewrite.md`.
**Mechanism, not just the fix:** an inbound request's description of a
file ("SolidWorks-exported") is a claim, not a fact, and this project's
own empirical corpus already held the measured answer one grep away —
the error propagated from the dispatch that filed owed item 13, which
repeated the request's wording without checking it. `ROADMAP.md` line
~527 corrected in place with this same dated note; this entry's own
Shipped bullet needed no change. The `Pass 281.0` commit message
(`1177221`) also says "the operator's own SolidWorks drawing" and is
published history that cannot be corrected — read it with this
amendment attached.

## 2026-09-09 (481st filing)

**Shipped:**
- Pass 280.0 (`26ef381`) — a verb that answers "which characters will
  this text run accept?" before the first keystroke
  (`EditSession::run_repertoire`, `pdfcer run-repertoire`), so a shell
  can grey a key instead of a caller typing a whole word and losing it
  at commit. Acceptance is decided by calling the same accepting code
  `edit_text` calls (`R221`), never a parallel description of it.
  Discharges the 480th filing's owed item 12 — `pdfcer-gui`'s standing
  ask, offered twice.

**Decisions made this session:**
- No new decision minted (`ARCHITECTURE.md` §12 untouched).
- Declined to mint a standing rule for the file-channel-blindness cause
  at its second recorded instance (see below) — flagged for the
  engineer's judgment rather than decided here.

**Findings + decisions:**
- An existing gate (`route_enumeration.rs`) caught this Pass's brand
  new verb as a fourth route needing find-resolution, within the hour
  of the verb being written — the first instance of this gate catching
  code that postdates it rather than rediscovering old code. Discharged
  by resolving the find and reporting which run was resolved
  (`RunRepertoire::text`), not by exemption. Dated footer + table row
  added to `D:\dev\rag\rust\a_behaviour_test_over_an_enumerated_list_cannot_fail_on_a_route_added_later_but_a_source_scan_can.md`.
- A sabotage of the `encode_char` call on the simple-font branch
  survived for a checked, honest reason (every candidate on that branch
  can only be refused under one rule no fixture reaches) rather than
  because the call is dead — documented at the call site and in
  `docs/core-api` instead of deleted or forced red. New RAG file:
  `D:\dev\rag\rust\a_sabotaged_call_can_survive_for_an_honest_reason_document_it_as_a_third_option.md`.
- `R221` gains another instance; its true current instance count
  remains unreconciled (480th filing's owed item 10, untouched here).

**Still in flight:**
- Items 4, 5, 9, 10, 11 carried forward unchanged (see `ROADMAP.md`'s
  owed-work ledger).
- Two new inbound requests from `pdfcer-gui`, read and queued, neither
  started: a hybrid-reference file's forced full-rewrite refusal makes
  redaction unreachable on such files; `resize_annotation` refuses a
  pdfcer-authored `/Stamp` as foreign (third such family, after
  `/FreeText` and `/Text`).
- A second instance, in two days, of the file-channel-blindness cause
  (a reply asserted two requests were unanswered when they had been
  answered 67 minutes earlier) — flagged, not minted; the
  `stat`-before-replying remedy was already written down and not
  applied twice now.
- Whether `26ef381` has been pushed or released is not asserted — no
  shell this filing.

**For next session:**
- Confirm `pdfcer-gui`'s fourth outbound reply by `Glob` (carried from
  the 480th filing).
- Reconcile `R221`'s instance count before its Standing Rules body
  gains another dated note.
- Scope the two new `pdfcer-gui` requests (hybrid-reference redaction,
  `/Stamp` resize) into Passes.
- Watch for a third instance of the file-channel-blindness cause before
  deciding whether it earns a standing rule.

## 2026-09-09 (480th filing)

**Shipped:**
- Pass 279.0 (`5b8ec61`) — a font-coverage refusal's named remedy could
  lead in a circle: `format-text --set-font Helvetica` on
  `ABCDEF+Helvetica` resolved back to the very subset that had just
  refused the character, reported success, and changed nothing, so the
  repeated edit refused word-for-word. Fixed by running every candidate
  face through the same resolution and acceptance path `set_font`
  itself uses (`R221`), rather than describing it separately. Discharges
  the 479th filing's owed item 8.

**Decisions made this session:**
- No new decision minted (`ARCHITECTURE.md` §12 untouched).
- Declined to guess a reconciled instance ordinal for `R221` — the
  commit calls this "the third recorded instance," but `docs/ROADMAP.md`'s
  own Standing Rules `R221` entry already shows numbers well past three,
  with a documented history of prior mis-tracking (300th filing). Filed
  as owed work rather than asserted.

**Findings + decisions:**
- Two dated footers written to `D:\dev\rag\rust\`: a 14th instance of
  the sabotage/fixture-discrimination family — the first of the day on
  an *ordinary, pre-existing, correct* regression test rather than a
  deliberate sabotage, because the test's fixture could not collide
  with the defect's precondition — and a fresh instance of the
  non-unique-string sabotage-anchor cause, where a generic Rust idiom
  (`None => true`) matched an unrelated match arm before the intended
  one.
- The named-remedy-leads-in-a-circle defect is exactly the risk
  `pdfcer-gui` flagged this morning in the abstract ("we have not seen
  that happen and are not claiming it") — it happens, and it is the
  first name on the list. Confirmed and replied to.

**Still in flight:**
- `R221`'s true current instance count is unreconciled — needs research
  before a dated note can be added to its Standing Rules body.
- `pdfcer-gui`'s fourth outbound reply is relayed, not independently
  `Glob`-confirmed this filing (unlike the 479th filing's three).
- `pdfcer-gui`'s standing ask for a pre-keystroke "which characters can
  this run accept?" verb remains open, offered twice, unanswered.
- Items 4, 5 and 9 (PROVENANCE.md backfill, the `origin/main..HEAD`
  filing-boundary note, and the "alternate route" `R247` reservation)
  carried forward unchanged.
- Whether `5b8ec61` has been pushed or released is not asserted — no
  shell this filing.

**For next session:**
- Confirm `pdfcer-gui`'s fourth reply by `Glob` against
  `D:\Dev\FeatureRequests\pdfce_FeatureRequests\open\`.
- Reconcile `R221`'s instance count before its Standing Rules body gains
  another dated note.
- Resolve the `R247` reservation and mint (or fold) the "alternate
  route" cause properly (carried from the 479th filing).

## 2026-09-09 (479th filing)

**Shipped:**
- Pass 277.0 (`fccd6cd`) — a sticky note (`/Text`) no longer refuses a
  resize by falsely claiming pdfcer had not drawn it; the refusal is
  now permanent and correctly argued (§12.5.6.4 + §12.5.3: a `/Text`
  annotation behaves as `NoZoom`/`NoRotate`, so `/Rect` is an anchor,
  not a size, and no scale factor has anything to act on), refused as
  a class (the `/Text` rule OR a `NoZoom` flag on any subtype), with
  no override. pdfcer's own `TextAnnotSpec::Sticky` doc comment named
  the wrong anchor corner (lower-left; spec says upper-left) and is
  corrected in place.
- Pass 278.0 (`c8a6697`) — a freehand `/Ink` stroke's nodes are
  editable now, per point (`move_ink_point`/`insert_ink_point`/
  `remove_ink_point`) and per whole stroke (`replace_ink_stroke`/
  `move_ink_stroke`/`remove_ink_stroke`), overturning a refusal that
  had argued from Acrobat's own lack of per-point ink editing at any
  version — a decision, not a not-yet, per the requester's own
  framing, overturned under the standing "parity is the floor, not
  the ceiling" ruling. `reshape_annotation` still refuses `/Ink` (its
  single-index shape cannot address a stroke) but now names these
  verbs. pdfcer's own polyline-only authoring (`m`/`l`, no curve
  operators) makes the point-drag preview exact, not approximate —
  §12.5.6.13 leaves the join style implementation-dependent, so both
  readings conform.

**Decisions made this session:**
- No new decision minted (`ARCHITECTURE.md` §12 untouched). Neither
  Pass redraws a crate boundary, picks a library, or redefines an
  invariant; `Pass 278.0` overturns a capability ruling made inside a
  Pass, not an architectural decision.
- A standing-rule candidate ("a refusal must name the property that
  makes the operation impossible, not the nearest fact that happens
  to be true") was considered for `Pass 277.0` and **declined at
  n=1**, consistent with this project's practice of waiting for a
  genuine second instance before minting.
- A second candidate was drafted and then **corrected before filing**:
  what first looked like a novel n=1 shape (an alternate,
  independently-justified guard masking the removal of the guard
  under test, `Pass 277.0`) turned out on checking to be the
  **second** instance of an "alternate route" sabotage-survival cause
  already recorded in `D:\dev\rag\rust\` from `Pass 155.1`
  (2026-09-07) — there between two derivation rules, here between two
  refusal guards. Now at `n=2`, this project's own stated minting
  threshold, but not minted this filing: the next free standing-rule
  number (`R247`) is already reserved for an unrelated trigger, and
  resolving that reservation needs more time than this filing had.
  Filed as owed work.

**Findings + decisions:**
- Two PDF-domain lessons written to `C:\personal_rag\pdf\`, checked
  against the index first and confirmed not already covered:
  `/InkList`'s join style is implementation-dependent (§12.5.6.13),
  and a `NoZoom` annotation's anchor is `/Rect`'s upper-left corner,
  not lower-left (§12.5.3) — the second lesson exists because pdfcer's
  own doc comment had this backwards.
- `R225` (sabotage survives on a non-discriminating fixture) gains a
  twelfth dated instance from `Pass 278.0`: removing the *last*
  element of a list made `.get(i)` return `None` under both the
  correct and the sabotaged code, so a naive report's default answer
  happened to be right by accident. Re-pointed at index 0 instead.
  New degenerate-value-table row: the last index of a collection is a
  degenerate fixture choice for any bounds-checked/`Option`-returning
  access.
- The consuming shell's own question on `Pass 278.0` — *"is this a
  decision or a not-yet, because from here they look identical?"* —
  is recorded as a suggested convention for `docs/core-api/`'s
  refusal documentation (engineer-owned, not edited here), not minted
  as a standing rule.
- Reply debt discharged and independently confirmed: the 478th
  filing's owed item 7 (three unconfirmed outbound replies) and this
  filing's own three replies are the same three files, confirmed to
  exist by `Glob` directly against
  `D:\Dev\FeatureRequests\pdfce_FeatureRequests\open\` rather than
  relayed.

**Still in flight:**
- The `format_text --set-font` subset-resolution question promised to
  the requesting project is still unmeasured: whether `set_font`
  resolves to an existing subset-embedded resource sharing the target
  `/BaseFont` before authoring a fresh standard-14 one. If it does,
  `Pass 274.0`'s font-remedy refusal can name a face that then fails
  the `R-INV-1` subset floor. One fixture, one test; flagged, not
  built.
- The "alternate route" sabotage cause's proper standing-rule number
  is unresolved (see *Decisions*, above) — needs the `R247`
  reservation checked before minting.
- 477th filing's owed items 4 (21 of 38 `fixtures/synthetic/text/`
  files undocumented in `PROVENANCE.md`) and 5 (`origin/main..HEAD` is
  not a filing boundary once a release has been pushed) remain open,
  carried forward unchanged.
- Whether `fccd6cd`/`c8a6697` have been pushed or released is **not
  asserted** — this filing had no shell. Check
  `git rev-parse origin/main` / `git describe --tags --abbrev=0`
  directly.

**For next session:**
- Resolve the `R247` reservation and mint (or fold) the "alternate
  route" cause properly.
- Build the `format_text --set-font` subset-resolution measurement
  (owed item 8, `docs/ROADMAP.md`).
- Backfill `fixtures/synthetic/text/PROVENANCE.md` (477th filing's
  item 4, still open).


> **Entries before 2026-09-09 are in [`history/session-log-before-2026-09-09.md`](history/session-log-before-2026-09-09.md)** — verbatim, still citation-valid, still read by the filing gates.
> Moved there 2026-09-10, when this file had reached 99,597 lines.
