# NEXT_SESSION.md — engineer handoff

**Read this first on resume**, then the newest `docs/SESSION_LOG.md` entry.
Engineer-owned; replaced each session. The previous long-form handoff (mesh
failure classes, probe recipes, release recipe detail) is in git:
`git show 40e7d1d7:docs/NEXT_SESSION.md`. Grep it; do not re-adopt it wholesale.

**Written:** 2026-10-26. **Work resumed** ("Continue", 2026-10-26). An active loop is running, `/loop 8 minutes Check feature requests folder and implement any changes.`, re-armed with ScheduleWakeup. **v0.99.0 is the latest release** (tag on `14cc5ee6`, slot pdfcer2, GUI noticed in `open/notice_2026-10-26_v0.99.0_released.md`). It ships Passes 646.2–646.8 and 637.0. **The next release writes pdfcer1.** Next filing is the 1249th, next Pass 646.9, decision 201, R263. Decision 191 governs the size-reduction arc.

**Owed: crates.io publish.** Ken authorized it ("you can now publish to crates.io"), but `.claude/settings.json` denies `Bash(cargo publish:*)`. Do not edit the settings or work around them. Ken runs `cargo publish -p pdfcer-color`, then `-p pdfcer-model`, then `-p pdfcer-pkix` (they would publish as 0.99.0), or he lifts the rule. After the publish, write the X001 reply to the crates consumer.

## State

- **Crate hardening (Passes 618.0, 618.1) shipped and replied to**
  (`reply_operator_harden_and_ready_crates_for_publishing.md`). Engine crates
  deny every panic lint; exemptions are in `tools/panic-allows-baseline.txt`
  (shrink-only, `check-panic-allows.py`). color/model/pkix `cargo package` in
  CI. **Bump the `pdfcer-model` version in `[workspace.dependencies]` with
  every release.** The colour engine is now `icccer-*` at rev `7002353e` (v0.4.1,
  Pass 623.0, the rename only); if Ken publishes, add `version = "0.4"` beside
  the git dep in pdfcer-render. **zune-jpeg rc3 comes from git at rev `c1c7f497` (decision 198,
  amending 197) so a downstream `^0.5` resolves; moving to stable crates.io
  0.5.16 still blocks any crates.io publish. `tools/*/Cargo.lock` were refreshed
  to the git rc3 graph (0145632a); refresh them again after any engine dependency change.** pkix full verify waits for model on a registry; decision 196 split precedes
  pkix's first publish. Never flip `publish = true` (Ken's call).
  `cargo publish --dry-run` is permission-denied; use `cargo package`.
- **Prepress readers (Pass 631.0) shipped and replied to** (`reply_document_uses_overprint_FIXED.md`, `reply_detect_pdfx_claim_FIXED.md`): `prepress::overprint_use`, `pdfx_claim`, `pdfx_output_intents`; `inspect` prints them. Open: which reading Acrobat's "Only for PDF/X files" uses (intent alone or a claim), unmeasured.
- **Inserted/pasted pages carry their form fields (Pass 102.1, `3f180dd3`, filed `56105737`)**:
  wholly covered fields carried, partly covered groups pruned, split terminals counted
  (`fields_split`); `orphaned_widgets` narrowed. Two `merge_document` field-tree bugs
  fixed. GUI notified. Released in v0.94.0 with 633.0-636.0.
- **Redaction absence proof is in core (Pass 632.0, closes 72.0)**: `redact::verify_absence`; `redact-apply` refuses (exit 9, no file) on a drawn survivor and discloses (exit 10) on any other residual. GUI told it can delete its `proof.rs`. Fuzz `redact_proof`: 882,398 runs / 121 s, clean.
- **Redaction text surgeon fixed (Pass 635.0, `f51b10bb`, v0.94.0)**: a kerned
  `TJ` / consecutive `Tj` double-advanced `Tm`, so a mark could remove earlier text
  and leave the marked text; `q`/`Q` now restore the font. **Pass 635.1
  (`c50a3790`, `576dadb7`, v0.94.0) shipped:** redaction removes text, vectors,
  shadings and image samples inside form XObjects (`redact_form.rs`, copy-on-write
  `pdfceRdF{num}_{k}`); verified on the operator's out-of-repo file, standing
  request done. `635.2` (extraction-level absence proof) shipped `202e7b13`;
  the 635.x track is complete.
- G214-G217 replies are already on the channel (the earlier "owed" note was stale).
- **Release:** v0.99.0 is published (tag on `14cc5ee6`, 8 assets, CI green at
  the tag; OneDrive `pdfcer2`; `pdfcer1` keeps 0.98.0). Assets: the build
  writes all three OCR add-on zips beside its folder
  (`<build>-ocr-addon-<name>.zip`). Copy them in as `ocr-addon-<name>.zip`,
  and zip the build under a top folder `pdfcer-vX.Y.Z/`. **The next release
  writes `pdfcer1`.** **The tag push can print nothing.** Check it with
  `git ls-remote origin 'refs/tags/vX^{}'`, which must show the filing commit. Before a push that adds a field or
  item read only under a feature, also run `cargo clippy -p pdfcer-cli
  --no-default-features --all-targets -- -D warnings` (CI runs it; v0.95.0
  went out red on it). pdfium render-parity
  A/B v0.90.0 (zune 0.5.15) vs v0.92.0 (git rc3), 70 DCTDecode corpus files:
  0 pages changed; control (same files re-encoded `image(encode=jpeg,
  quality=70,chroma=420)`): 11 pages improved (worst 99.7% -> 0.2% over
  d>32), 6 `disclosed-gap` -> small, 0 worse. Disk D: filled once this session: `target/debug/incremental`
  (48 GB) is safe to delete; `.claude/worktrees` (22 GB) was left alone. Release gates are change-scoped (Ken, 2026-10-16): test
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
  images, decision 194) shipped; COMPLETE: Reader 25.1 draws RunLength/LZW/CCITT/JBIG2 identically, and
  draws any page with a Brotli stream BLANK, silently (the note now says so). **616.6 fonts: slices 1-5
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
  streams) slice 1 shipped** (`9f9e7f4b`): lossless `content(...)`
  StreamAction (whitespace, shortest numbers, redundant operators) in
  `optimize::content`, its own tokenizer + re-read check, not
  `reemit_canonical`. **Slices 2-5 shipped** (`b7938d8c`, `23f92d9e`,
  `041b358a`, `2c4099e4`): `round=T`, lossy: path operators, numbers inside
  arrays and dicts, `Tm`/`Td` line starts (Td error carried); every page
  content stream rounds at the scale its entry matrix stretches by
  (§7.7.3.3: `/Contents` streams run concatenated), a shared stream at its
  pages' largest; `Unrounded::UnknownStart` otherwise. Forms and tiling patterns round at
  their drawn scale (`281ac8c7`); appearance streams, soft masks and Type 3
  procedures do not (Backlog `628.0`, ~7 KB corpus-wide). `TD` excluded by design.
  Parity reference:
  `Acrobat_Features/optimize__page_content_and_content_stream_rewriting.md`.
  **616.8 linearization shipped** (`daa36707`): `optimize --linearize`;
  classification mirrors qpdf 12's check (pages beat outlines/struct
  tree/thumbs; a Pages node walks only from `/Pages`; nulls uncounted;
  hint columns byte-aligned). Oracle: `qpdf --check-linearization` over
  all 3,340 fixtures -- 3,290 clean, 6 source-inherited Resources
  warnings, 0 FAIL, 44 expected refusals. Re-run it after touching
  `linearize/classify.rs`. **616.9 COMPLETE** (ssim target, byte budget
  `--target-size`, visual-diff `--visual-diff`) and **616.10 COMPLETE**
  (`--compat`); core + cli, GUI not built. ssim sizes, 1,603,435 B photo: q90 805,853; ssim .99 -> q53
  234,628; .98 -> q24 147,835; .95 -> q8 85,268 (~7 s, 8 trials).
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

1. The size-reduction arc (616.x) is complete and its residue is not worth
   a Pass (short loca retired, 1205th; CFF merge has no instance; Type 1
   re-subset ~1.5% of font bytes). The table DPI sensitivity is fixed
   (640.0). Candidate next: the prose-answered table (Follow-ups).
2. Each loop tick: the three inbound channels (GitHub issues, both
   FeatureRequests folders).

## Follow-ups worth a Pass

- **A 14 pt unruled 4-column table is answered in prose at every dpi**
  (100-300, with or without 640.0's downscale). Reproduce with the
  scratchpad `p635_tables.py` / `p635_run.py` (table `t14_plain`); not
  committed. Cause measured: PP-DocLayoutV3 classes it `content` (text,
  score 0.58), not `Table`, so no table prompt runs; every line's text is
  read correctly. A fix would re-test a text region for column gutters
  (inferred structure; disclose it). Low value; the ROADMAP Backlog item
  still says "prose" and needs that cause added at the next filing.

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
