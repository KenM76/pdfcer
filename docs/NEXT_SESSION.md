# NEXT_SESSION.md — engineer handoff

**Read this first on resume**, then the newest `docs/SESSION_LOG.md` entry.
Engineer-owned; replaced each session. The previous long-form handoff (mesh
failure classes, probe recipes, release recipe detail) is in git:
`git show 40e7d1d7:docs/NEXT_SESSION.md`. Grep it; do not re-adopt it wholesale.

**2026-10-31 update (latest, late night).** **v0.112.0 is the latest release** (annotated tag on `0a19909f`, CI green at the tag, 8 assets, OneDrive `pdfcer1`, `pdfcer2` keeps 0.111.0, crates color/model/pkix 0.112.0 published, verify-release ok but for the known dirty tree; publication filed 1359th). **The next release writes pdfcer2.** It carried only Pass 647.4b (X027, `96af5ff3`): XFA rectangle/line/arc draws, dashed edges, `rotate` (`LayoutItem.rotate`, `OddRotate`, `DashLengths`, `DashCaps`, `ArcCircle`, `ArcFill`, paint notes Shape/Rotate; CLI label `stroke-not-drawn` renamed `stroke-drawn-solid`). X027 reply renamed SHIPPED, notice `notice_2026-10-31_v0.112.0_released.md`. **Next: Pass 648.6 (G245 part 2, `/MA` measurements; spec RAG first).** 647.3f and 647.4 stay in the Backlog. Loop still running, slow pace (~1800s).

**2026-10-31 update (night).** **v0.111.0 is the latest release** (annotated tag on `f7b50b50`, CI green at the tag, 8 assets, OneDrive `pdfcer2`, `pdfcer1` keeps 0.110.0, crates color/model/pkix 0.111.0 published, verify-release ok but for the known dirty tree; publication filed 1355th). **The next release writes pdfcer1.** It carried Pass 647.3e (G248 items 2-3, `53f4e938` + `d4ffa8fd`) and 647.2c (X026, `ead2642a`); the G248 and X026 replies are renamed SHIPPED, notice `notice_2026-10-31_v0.111.0_released.md`. **Next: Pass 648.6 (G245 part 2, `/MA` measurements; spec RAG first).** 647.3f (dataRef/subform-set instances) and 647.4 (image natural size) stay in the Backlog. Loop still running, slow pace (~1800s).

**2026-10-31 update (evening).** **Pass 647.2c shipped, unreleased** (`ead2642a`, 1353rd): a field or draw with no `w`/`h` grows to its plain-text value via one measurement path shared with paint (`paint/size.rs` `content_size`, `paint/text.rs` `text_extent`), clamped to min/max; `max*`=0 is omitted. Measured at paint defaults (G-T1 normal, CaptionFont::Own): a non-default paint setting can draw text wider than it was sized. Image natural size stays on 647.4. Owed to the librarian: FEATURES row ~739 still says "field/draw splitting not yet" (647.2b shipped it). **Next: release (writes pdfcer2) carrying 647.3e + 647.2c; rename the G248 and X026 replies SHIPPED; then 648.6.**

**2026-10-31 update (afternoon).** **Pass 647.3e shipped, unreleased** (`53f4e938`): XFA subforms with `occur` repeat once per matching data group (by-name only; dataRef and subform-set instances are Pass 647.3f), `ExcessData`/`InstanceLimit` warnings, `MergedForm.unfilled`/`unbound_values`, `xfa-layout` prints `data unfilled= unbound-values=`. G248's own synthetic shape gives tenants One/Two, no warnings; reply `reply_G248_..._SHIPPED.md` names the by-name limit. **Next: Pass 647.2c (X026, content-sized fields and draws; reply PLANNED written; spec RAG Ch.8 pp.275-278 first), then a release (writes pdfcer2) carrying 647.3e + 647.2c, then 648.6.** D: filled to 19 MB mid-session; `target/debug/incremental` (63 GB) cleared it.

**2026-10-31 update (morning).** **v0.110.0 is the latest release** (annotated tag on `5c296530`, 8 assets, OneDrive `pdfcer1`, `pdfcer2` keeps 0.109.0, crates color/model/pkix 0.110.0 published, verify-release ok but for the known dirty tree). **The next release writes pdfcer2.** It carried only Pass 647.2b (X025, `fe0135a1`): growable XFA field/draw splitting between lines, `LayoutItem.fragment`, `xfa-layout fragment=`. X025 is closed SHIPPED. Std-14 weights share line metrics, so layout has no font-weight option: do not re-add it. **Next: Pass 647.3e (G248 items 2-3; Ken's private lease = structure only, never copy), then 648.6 (G245 part 2, spec RAG first).** Loop still running, slow pace (~1800s).

**2026-10-31 update (earlier).** v0.109.0 was the release then (annotated tag on `91924926`, CI green at the tag, 8 assets, OneDrive `pdfcer2`, `pdfcer1` keeps 0.108.0, crates color/model/pkix 0.109.0 published). **The next release writes pdfcer1.** It carried 648.4, 648.5, 647.3b, 647.3c, 647.3d, 647.3d2 (XFA data-node creation, `3ce0017e` + doc fix `732fb530`) and 652.0. X022, X023, X024 and G247 are closed SHIPPED; G248 item 1 released, items 2-3 = 647.3e; G245 part 2 = 648.6. **Next: Pass 647.2b (X025, growable field/draw splitting) in its own release**, then 647.3e, then 648.6 (spec RAG first). Loop still running, slow pace (~1800s). **Before any push, run every `python tools/*.py` and `bash tools/*.sh` gate named in `.github/workflows/ci.yml`**: change-scoped gates missed `check-public-fns-documented.py` on 2a9f04ae (CI red, fixed by 732fb530).

**2026-10-30 update (late night).** Pass 647.3b shipped (`f5c1f27e`, normal name matching; X022 Form F and X023 Forms A/B read side pass), filed 1338th. **X023 arrived** (pdfcraft: `match="none"` subforms are not data scopes); read side done, write side added to 647.3d; replied `reply_X023_*_PLANNED.md`. **Next: 647.3c** (global fallback + its two settings), then 647.3d (fill verb + X022 Form G + X023 save round trip), then a release (pdfcer2), then 648.6.

**2026-10-30 update (night).** Unreleased on main after v0.108.0: Pass 648.4 (G247, `set_3d_opening_view`, `8d414a9f`) and 648.5 (G245 part 1, `pdfcer_3d::pick` + `Camera::ray_through`, `3d-render --pick`, `d8e7ac87`); both replied (`reply_G247_*`, `reply_G245_part1_*`). **X022 arrived** (pdfcraft: XFA `bind match="global"`, shared value per name, edits reach every copy, one data node on save, then a release). It needs, in order: normal `once` name matching (647.3b; our merge still passes `once` scope through, so X022's "Form F already works" premise is wrong and the reply must say so), then global matching, then an XFA fill verb writing `xfa:datasets` at Save (new write path; decision 204's commit-at-Save rule). Filed as Passes 647.3b/c/d (`c7993cae`), replied `reply_X022_*_PLANNED.md`. Spec: `adobe_ext__xfa_data_binding_global_writeback.md` (G-B7..G-B16); strictly, `global` is a no-op without config `record`, so 647.3c ships scope `Document` (default, PDFium-like) vs `Records`, and exclGroup `Honour` (default, p.348) vs `AsOnce`. Then 648.6 (G245 `/MA`, spec check first). X022 asks for a release when it lands; the next release writes pdfcer2.

**2026-10-30 update (evening).** **v0.108.0 is the latest release** (annotated tag on `fa9af179`, 8 assets, OneDrive `pdfcer1`, `pdfcer2` keeps 0.107.0, crates color/model/pkix 0.108.0 published). **CI is red at the tag on one docs-size gate** (`check-register-entry-size.py`: the 1331st filing grew a FEATURES row to 1,262 chars); fixed on main by `212f0ce5`, tag not moved. **Before tagging, run the repo-audit doc gates on the filing being tagged.** **The next release writes pdfcer2.** It carried Pass 647.3a (XFA `dataRef` merge, `4319af9f`: `xfa::merge_data`, `xfa-layout value=` and `warning kind=data-ref-*`), 647.4a (3-D edges, round radio buttons, `56ef7bcd`) and 648.3 (`threed::saved_3d_views`, `30bd10a6`). **X020 and G246 are replied** (`reply_*_SHIPPED.md`, notice `notice_2026-10-30_v0.108.0_released.md`). X019 and X021 are addressed to pdfcer-gui; no engine work. **Next:** Pass 648.4 (G247, lit opening view at insert), then 648.5/648.6 (G245 3D measuring; 648.6 needs a spec-RAG check first); XFA next cut is normal name-matching binding and `occur` instances (647.3b), slow pace. The paragraph below is the previous update (v0.107.0).

**2026-10-30 update (afternoon).** **v0.107.0 was the release then** (annotated tag on `14774b1f`, CI green, 8 assets, OneDrive `pdfcer2`, `pdfcer1` keeps 0.106.0, crates color/model/pkix 0.107.0 published). **The next release writes pdfcer1.** It carried `G241` (print vector-or-raster decision public, `75a3dc58`), `G242` (separation plates public, `c17bb8ef`) and `X018` (PRC trees read past markups, views, lights, clipping planes, `65dd718c` + help `3b53a789`); all three replied with the tag. **Open inbound:** `X020` (XFA `dataRef` data merge + round checkButtons in exclGroups, then a release), filed as Passes 647.4a (paint, first) and 647.3a (merge, after the spec librarian's SOM slice lands in `PDF_Spec/adobe_ext/`). `X019` is addressed to pdfcer-gui (its own paste-chord hook), not to the engine; no engine work. Gotchas this stretch: `cargo fuzz run` after `fuzz build` with a changed PATH recompiles pdfcer-core under ASan (~15 min); run the built `fuzz/target/x86_64-pc-windows-msvc/release/<target>.exe corpus/<target> -max_total_time=60` directly with the ASan DLL dir on PATH. `gh-release.py` must run from the repo (it calls git). Python scripts with backslashes go in a file, never a Bash heredoc. The paragraph below is the previous update (v0.106.0).

**2026-10-30 update (morning).** **v0.106.0 was the release then** (annotated tag on `e1236835`, CI green, 8 assets, OneDrive `pdfcer1`, `pdfcer2` keeps 0.105.0, crates color/model/pkix 0.106.0 published). **The next release writes pdfcer2.** It carried `X017`: XFA image values drawn, inline and linked through `/Names /XFAImages` (`0851e68b`, 647.4 cut, decision 213: placement/overflow/dpi settings). **X017 is replied to**; its one pending item is that layout does not size growable containers from an image's natural size (no content measurement yet). Release zips: build with Python zipfile, never PowerShell `Compress-Archive` (backslash entry names). Keep checking the FeatureRequests folder for pdfcraft follow-ups. Next: 647.4's remaining cuts, then 647.3, slow pace. The paragraph below is the previous update (v0.105.0).

**2026-10-29 update.** **v0.105.0 was the release then** (annotated tag on `e53e9634`, CI green, 8 assets, OneDrive `pdfcer2`, `pdfcer1` keeps 0.104.0, crates color/model/pkix 0.105.0 published). **The next release writes pdfcer1.** It carried only `X016` item 2 (a subformSet test, no behaviour change). **X016 is replied to:** item 2 done; items 1 and 3 are pending on 647.3 and 647.7 and get a reply when those ship (X016 stays open; pdfcraft expects back-and-forth). **The operator asked me to check the FeatureRequests folder regularly for pdfcraft's XFA follow-ups.** Next work is the XFA arc at a slow pace: 647.4's remaining render cuts, then 647.3. The paragraph below is the earlier same-day update (v0.104.0).

**2026-10-29 update (earlier, v0.104.0).** **v0.104.0 was the release then** (annotated tag on `c9382561`, CI green, 8 assets, OneDrive `pdfcer1`, `pdfcer2` keeps 0.103.0, crates color/model/pkix 0.104.0 published). **The next release writes pdfcer2.** The print arc 55.9-55.14 is shipped and released. **XFA is next:** Pass 647.4 remaining render cuts, then 647.3; slow pace. The paragraph below is the earlier same-day update (v0.103.0).

**2026-10-29 update (earlier).** **v0.103.0 was the release then** (annotated tag on `aee919a1`, CI green at the tag, 8 assets, OneDrive `pdfcer2`, `pdfcer1` keeps 0.102.0; crates color/model/pkix 0.103.0 published). **The next release writes pdfcer1.** It carried Pass 648.0 (glTF to PRC; views Acrobat activates: perspective `/P` with `/N` and `/PS /W`, `/LS /CAD`; PRC sons before father; click-to-activate read back as `ThreeDArtwork.activation`, `3d-list activate=`), plus XFA 647.4 cuts. Ken confirmed the v4 Bancroft PDF works in Reader. X014 filed and replied. The tag push can hang silently after main pushes: kill git and re-push the tag, then check with `git ls-remote origin 'refs/tags/vX^{}'`. **XFA next:** Pass 647.4's remaining render cuts (checkButton drawing next), then 647.3 normal data merge. Keep the pace slow (operator).

**2026-10-28 update.** Loop running: `/loop check your featurerequest folder and implement ... dynamic xfa ... a little slower` (self-paced, ScheduleWakeup ~1800s). Ken reopened dynamic XFA: decision 204, arc Pass 647.0-647.7 (Next up head 647.0, operator-paced: one step per few ticks, keep token use low). Pass 646.25 shipped `5f401310` (G237, `xfa::form_kind`), filed 1271st, replied. Unreleased: 646.24 + 646.25. XFA spec first slice is in `PDF_Spec/adobe_ext/adobe_ext__xfa_*.md` (5 files; gap IDs G-X/S/T/L/B are the default-decisions list); owed next slice: SOM, then table/row. pdfcraft (`D:/Dev/pdfcraft/upstream/crates/xfa`, MIT OR Apache-2.0) is reference/oracle/fixture source only, never a dependency, no copied code; its open-time materialization is rejected (invariant 2). Survivors of "dynamic XFA out of scope" still to fix in 647.0: `forms.rs` XfaPresence doc (~line 954), `docs/xfa-implementation-survey.md` 17/29/40, `PRIOR_ART.md` 95, decision 020 section 3.2.1 forward pointer. Untracked file `-` at repo root (the lease placeholder text, not mine): never commit it.

**Written:** 2026-10-27. **Work resumed** ("Continue", 2026-10-26). An active loop is running, `/loop 8 minutes Check feature requests folder and implement any changes.`, re-armed with ScheduleWakeup. **v0.102.0 is the latest release** (tag on `47140cdb`, CI green at the tag, 8 assets, OneDrive `pdfcer1`, `pdfcer2` keeps 0.101.0, crates published). **The next release writes pdfcer2.** It carried Passes 646.19 (G235) and 646.20-646.23 (G236, complete): `/AP`-less markup is drawn at render time from its own geometry (decision 203), and `EditSession::bake_missing_appearances` / `pdfcer bake-appearances` (`757a9385`) writes that look as `/AP` on request. The derivation is core's (`annot_author::derive_appearance`, `DerivedAppearance::place`); render paints through `place()` with a `/BBox` clip, so bake and render are pixel-identical by construction. The CLI unit-test large-stack helper is 64 MiB (16 overflowed with one more subcommand). A change to a `pub` struct/variant's fields needs `cargo +nightly fuzz build` before push: fuzz targets construct public types, and CI's fuzz jobs were red for three pushes on exactly that (`b3b9023a`). **Unreleased: Pass 646.24** (`0403197c`, filed 1270th): `Settings::free_text_c` persists the FreeText `/C` reading (R169), and the CLI's `--free-text-c` is optional, falling back to it; reply `reply_G236_free_text_c_setting_SHIPPED.md`. Next Pass 646.25, decision 204, R263; next filing 1271st. Decision 191 governs the size-reduction arc.

**crates.io: pdfcer-color, pdfcer-model and pdfcer-pkix 0.99.0 are published** (Pass 637.0, settings `19df5115`, X001 replied `reply_X001_crates_io_publishing_PUBLISHED.md`). Every release now publishes them after the tag, in that order (ROADMAP release recipe). **Pass 646.10 shipped** (X005, decision 202, `c33bd6b1`): RGB into a DeviceCMYK blending space separates through baked inverse grids of the calibrated table (`srgb_to_cmyk_fast`, `BlackGeneration::Medium`); regenerate the grids with `cargo run --release -p pdfcer-color --example inverse_grid_gen` after any change to the forward table or the solver. Out-of-gamut colours clip in sRGB, not through a profile, so they differ from a reference viewer by ~15 levels (stated in `reply_X005_rgb_into_a_cmyk_blending_space_FIXED.md`). **Passes 646.11/646.12 shipped** (`f3843690`): `mark_comparison` fails with `Box<MarkComparisonError>` whose `authored` flag is measured from the undo top serial; `PageDecorations::forms_unread` and `RichTextOutcome::decoration_forms_unread` disclose forms whose underline/strikethrough was not read. Replies `reply_G231_mark_comparison_error_says_what_was_authored_FIXED.md` and `reply_G232_page_decorations_counts_forms_it_skipped_FIXED.md`.

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
  `cargo publish` is allowed for the agent (`19df5115`).
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
- **Release:** v0.100.0 is published (tag on `d3996c09`, 8 assets, CI green at
  the tag; OneDrive `pdfcer1`; `pdfcer2` keeps 0.99.0; crates published). Assets: the build
  writes all three OCR add-on zips beside its folder
  (`<build>-ocr-addon-<name>.zip`). Copy them in as `ocr-addon-<name>.zip`,
  and zip the build under a top folder `pdfcer-vX.Y.Z/`. **The next release
  writes `pdfcer2`.** **The tag push can print nothing.** Check it with
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
