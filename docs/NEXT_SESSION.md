# NEXT_SESSION.md — engineer handoff

**Read this first on resume**, then the newest `docs/SESSION_LOG.md` entry.
Engineer-owned; replaced each session. The previous long-form handoff (mesh
failure classes, probe recipes, release recipe detail) is in git:
`git show 40e7d1d7:docs/NEXT_SESSION.md`. Grep it; do not re-adopt it wholesale.

**2026-11-09 update (release v0.140.0, newest).** **v0.140.0 is the latest release** (tag on `20d559b8`, CI green, 8 assets, OneDrive `pdfcer2`; `pdfcer1` keeps 0.139.0; all 14 crates at 0.140.0). **The next release writes pdfcer1.** It carries Pass 698.0 (`957aab8d`): `pdfcer_render::white_margin_crop(view, page, margin_pt)`, the shared remove-white-margins verb, answering pdfcer-gui's `request_remove_white_margins_core.md` (reply + notice in the channel). The channel gate `check-requests-scoped.py` counts only a `reply_*`/`done_*`/`notice_*` file naming the request; an inline reply appended to the request does not count. Ken's open question on double-sided printing (offer: enable the PrintService Operational log) still awaits his yes.

**2026-11-09 update (release v0.139.0, previous).** **v0.139.0 was the latest release until v0.140.0** (tag on `bbc01732`, CI green, 8 assets, OneDrive `pdfcer1`; `pdfcer2` keeps 0.138.0; all 14 crates at 0.139.0). **The next release writes pdfcer2.** It carries Passes 693.0, 694.0, 694.1, 694.2, 695.0, 696.0 and 697.0 (X062: printer's marks clear the registration targets, `335e3e2c`). Notice `open/notice_2026-11-09_v0.139.0_released.md` sent to pdfcer-gui; nothing open in the channels at this update. **Run the CI yml's python/bash repository audits before EVERY push, release-scoped or not.** Change-scoped crate tests skip them, and CI went red twice this round (ci-parity, ci-job-names, code-structure and public-fns-documented; fixed in `ec1278fc`). The list comes from `python tools/check-ci-parity.py --list`.

**2026-11-09 update (release v0.138.0, previous).** **v0.138.0 is the latest release** (tag on `d5061fff`, CI green, 8 assets, OneDrive `pdfcer2`; `pdfcer1` keeps 0.137.0; all 14 crates at 0.138.0). **The next release writes pdfcer1.** It carries 691.0 and JPEG XL read/import (692.0/692.1, decision 220). **In the tree, uncommitted:** Passes 693.0 (copy-as-vector keeps highlights vector), 694.0 (`object-delete --rect`), 694.1 (`redact-mark --outside`), 695.0 (highlighter shapes). **Pass 694.2 (crop parity) shipped** in `c9468b0c` (unpushed; `set-crop-box --box/--odd/--even/--remove-white-margins`, core `set_page_boxes`; gui is with pdfcer-gui). **Pass 696.0 (JPEG XL export) shipped** in `6ed962dc` (encoder `jixel`; MSRV now 1.97, decision 221, gated by `tools/check-msrv-floor.py`). Its unshipped half (Optimize re-encode to `/JXLDecode`, reversible JPEG transcode) is Backlog 696.1. The GUI halves (marquee as a region selection, highlight modes, crop UX) and the dynamic-XFA canvas placeholder are with pdfcer-gui in `open/request_2026-11-09_*`.

**2026-11-08 update (release v0.137.0, previous).** **v0.137.0 was the latest release until v0.138.0** (tag on `82796a5f`, CI green at the tag, 8 assets, OneDrive `pdfcer1`; `pdfcer2` keeps 0.136.0). **The next release writes pdfcer2.** **Pass 690.1 SHIPPED: all 14 engine crates are on crates.io at 0.137.0** (model, color, pkix, vec3, json, jpeg-remux, 3d, 3d-nav, image-codec, fonts, function, text, core, render; publish in that order). Every release now publishes all 14 at the workspace version. crates.io rate-limits NEW crate names (the 14th got HTTP 429 with a retry time); version bumps of existing names did not trip it. zune-jpeg is stable 0.5.15 from crates.io; `pdfcer-jpeg-remux` rewrites the T.81 A.2.2 non-interleaved layout 0.5.15 mis-reads (`719d2127`, hardened `d3c44948`). X061 and X001 are SHIPPED; notice `open/notice_2026-11-08_v0.137.0_released.md` sent to pdfcer-gui. **A new engine crate needs the clippy panic deny block and doc comments on every fn from its first commit**, and the pre-push scoped gate list comes from `python tools/check-ci-parity.py --list`, never by hand. Nothing open in the channels at this update.

**2026-11-07 update (release v0.136.0).** **v0.136.0 is the latest release** (tag on `0ac810f8`, CI green at the tag, 8 assets, OneDrive `pdfcer2`; `pdfcer1` keeps 0.135.0; crates color/model/pkix/vec3/json/3d/3d-nav 0.136.0). **The next release writes pdfcer1.** It carries X060 / 689.0 (page `/UserUnit` on every render route and print sizing), the RSA-PSS saltLength overflow fix, and X061 part 1 / 690.0 (lean `pdfcer-render --no-default-features` build, CI-gated by its doctests and `tools/check-lean-render-graph.py`; `pub use pdfcer_core`). **Pass 690.1 (publish pdfcer-render to crates.io) is BLOCKED** on three upstream crates: icccer 0.4 (git dep), zune-jpeg 0.5.16 stable (git pin `c1c7f497`), ocrcer-core (vendored path dep); requests sit in the icccer and OCRcer channels; recheck crates.io each round. pdfcer-gui notice `open/notice_2026-11-07_v0.136.0_released.md`. Any cfg(windows) CLI change needs the Linux-target clippy (`cargo clippy -p pdfcer-cli --all-targets --target x86_64-unknown-linux-gnu -- -D warnings`) before the bump: a Windows-only helper cost a CI round here (`30feb478`). Next filing 1460th; next Pass 691.0.

**2026-11-06 update (release v0.135.0, previous).** **v0.135.0 was the latest release until v0.136.0** (tag on `2ee0a6bd`, CI green at the tag, 8 assets, OneDrive `pdfcer1`; `pdfcer2` keeps 0.134.1; crates color/model/pkix/vec3/json/3d/3d-nav 0.135.0). **The next release writes pdfcer2.** It carries G273, G274, G275, Pass 688.0 and the `pdfcer print` flag fix. No pdfcer-owned request is open from this round; pdfcer-gui was told to release on it. Build the local audit list from the CI yml: a hand list omitted the shell gates and cost a CI round on this release (`8030fb34`). Next filing 1456th; next Pass 689.0.

**2026-11-06 update (release v0.134.1, previous).** **v0.134.1 was the latest release until v0.135.0** (tag on `c2dca228`, CI green on the docs-only descendant `6c848ffc`, 8 assets, OneDrive `pdfcer2`; `pdfcer1` keeps 0.133.0; crates color/model/pkix/vec3/json/3d/3d-nav 0.134.1 published). **The next release writes pdfcer1.** It carries X055-X059 (Passes 679.0-683.0). **The `v0.134.0` tag at `fc3f7e1d` stays but was never published**: `check-clap-help` misread a two-line attribute on `3d-views` (fixed `7a4c6d52`), and the tag was already pushed. Tag only after CI is green on the bump, and run the audit list from `python tools/check-ci-parity.py --list` first. Notices sent to pdfcer-gui: `open/notice_2026-11-06_v0.134.1_released.md` and `open/notice_android_print_network_permission.md` (the Android print permission Ken raised; the app is theirs, and no engine work is owed). pdfcer-gui consumed X055, X058 and X059; X056 and X057 are next in its measure tool. No inbound request is open.

**2026-11-06 update (release v0.133.0).** **v0.133.0 was the latest release until v0.134.1** (tag on `50d0135f`, CI green at the tag, 8 assets, OneDrive `pdfcer1`; `pdfcer2` keeps 0.132.0; crates color/model/pkix/vec3/json/3d/3d-nav 0.133.0 published; filed 1445th). **The next release writes pdfcer2.** It carried X051-X053 / 677.0 (`3db3e31c`) and X054 / 678.0 (`209b61e5`, 360-degree photo sphere: `FisheyeMapping::Equirectangular`, `photo_sphere_xmp`, `insert_jpeg_xmp`, `3d-render --360`). Notice `open/notice_2026-11-06_v0.133.0_released.md` asks pdfcer-gui for the "360° (photo sphere)" screenshot option. v0.132.0 went out with CI red on `check-commits-filed.py` only (X050 filed one commit late), so **run that gate before the version bump as well as before the push**. No open Pass at the head. **Next: wait for the next request round.**

**2026-11-06 update (1438th filing, previous).** **Pass 676.0 (X050) SHIPPED** in `9b668449` (non-finite pick rays/hits refused). *(Superseded: released in v0.132.0.)* X049 belongs to pdfcer-gui, not us.

**2026-11-06 update (release v0.131.0, previous).** **v0.131.0 is the latest release** (tag on `ec81929c`, 8 assets, CI green on parent `c13ce318`, OneDrive `pdfcer1`; `pdfcer2` keeps 0.130.0; crates color/model/pkix/vec3/json/3d/3d-nav 0.131.0 published, Pass 675.1 complete). **The next release writes pdfcer2.** It carried X044 / 673.0, X046 / 674.0, X047 / 675.0. Notice `open/notice_2026-11-06_v0.131.0_released.md`; pdfcer-gui X048 item 1 is unblocked. **In progress: Pass 676.0 (X050, non-finite pick rays/hits; requester's code ported verbatim).** X049 is pdfcer-gui's.

**2026-11-05 update (release v0.130.0, previous).** **v0.130.0 was the release then** (tag on `5f9657e7`, 8 assets, OneDrive `pdfcer2`; `pdfcer1` keeps 0.129.0; crates color/model/pkix 0.130.0). *(Superseded: the next release wrote pdfcer1 as v0.131.0.)* It carries 667.0 (G269 light gain), 668.0 (G270 aimed, coloured sun), 669.0 (G271 lamp culling and counts), 670.0 (G272 shadow-caster choice), 671.0 (X043 glTF memory bounds) and 672.0 (G268 GPU fisheye fix). **CI was red at the tag on the doc audit only** (`check-public-fns-documented.py`: four undocumented helper fns); fixed forward by `30d93f57` (comment-only), tag not moved. Notice `open/notice_2026-11-05_v0.130.0_released.md`. **Open follow-ups:** Pass 669.1 (tiled per-tile light lists; GPU pixel agreement for the cull is unmeasured) and real-hardware checks of the GPU lens and lamps. **No open request addressed to pdfcer remains** (X041 and X038 are addressed to pdfcer-gui). **Next: wait for the next request round.** `gh run list --commit` needs the FULL sha; a short one returns `[]` forever.

**2026-11-05 update (release v0.129.0).** v0.129.0 was the release then (tag on `56ee4f85`, 8 assets, CI green, OneDrive `pdfcer1`; `pdfcer2` kept 0.128.0). It carried X042 / 665.0 (`94e4eaad`, rich copy keeps spaces) and G268 / 666.0 (`336f7c76`, `Projection::Fisheye`, `3d-render --lens`).

**2026-11-04 update (release v0.128.0).** **v0.128.0 was released** (tag on `eba71f2f`, 8 assets, OneDrive `pdfcer2`; `pdfcer1` keeps 0.127.0). **The next release writes pdfcer1.** It carries X039 / 661.0 (`a6a1ed44`), 662.0 (`f8da53ad`, the prepass silhouette outline Ken reported, measured headless in `ccf24788`), G266 / 660.0 (`d77620b6`, metadata), X040 / 663.0 (`7760a3bf`, rich-copy hardening) and G267 / 664.0 (`517f96d9`, plain text drops controls). The full gate run caught two stale tests (`eba71f2f`). **Standing rule R263 (decision 216, Ken):** test only what changed when it is safe to; rerun only the failed test binaries when a fix touched only tests; the cheap filing, doc, Linux and feature-variant gates always run. **Ken lifted the build load cap** ("build this as fast as you can"). X041 is pdfcer-gui's (clipboard cap and NUL); the engine side is in this tag. Notice `open/notice_2026-11-04_v0.128.0_released.md`. **Next: wait for the next request round.**

**2026-11-04 update (release v0.127.0).** **v0.127.0 was released** (tag on `7d8aedb8`, 8 assets, OneDrive `pdfcer1`; `pdfcer2` keeps 0.126.0; crates color/model/pkix 0.127.0). **The next release writes pdfcer2.** It carried X035 / 658.20, X036 / 658.21 (`91665b7f` plus `9248293f`, a CLI framing fallback for an all-transparent model that run-gates caught) and X037 / 659.0 (`c102b748`). **Decision 215:** X-requests come from PdfCraft; check its fork for a KenM76 fix first and adopt it, else write fresh and say so in the reply so PdfCraft can port it. **A local hook** (`.claude/hooks/feature-request-watch.py`, wired in `.claude/settings.local.json`, never committed) wakes the session when a new request lands; work it, then release when the round is complete. **Since the release:** X039 / Pass 661.0 shipped as `a6a1ed44` (a failed object stream is decoded once per load). **Open:** G266 / Pass 660.0 (metadata: custom `/Info` entries, `/Creator`, XMP kept in step; the spec mapping is `PDF_Spec/xmp/xmp__ref__info_to_xmp_mapping.md`). **Ken is working on the PC:** build at low load (`CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_TEST_THREADS=4`); `target/debug/incremental` is safe to delete when D: fills.

**2026-11-04 update (release v0.126.0).** **v0.126.0 is the latest release** (tag on `b5cb78eb`, 8 assets, OneDrive `pdfcer2`, `pdfcer1` keeps 0.125.0, crates color/model/pkix 0.126.0). **The next release writes pdfcer1.** It carries the whole 3D round Ken asked for in one release: X033 / 658.17 (`fb98413e`, reader + raster bounds), G265 / 658.18 (`21800243`, a damaged PRC model draws what survives: `damaged` / `damaged_why` / `damaged_parts`, memory ceilings stay refusals, `MeshFit::Unique` strict), X034 / 658.19 (`d481d708`, `OutlineClipLoss`). v0.124.0 (G264 depth prepass) was tag-only; v0.125.0 carried it plus X029-X032. **CI was red at the v0.126.0 tag on `cargo doc -D rustdoc::broken_intra_doc_links`** (a `Tessellation::Compressed` link in `pdfcer-3d/src/model.rs`); fixed on main by `1035130f`, tag not moved. **Second time this exact job hit a tag (v0.117.0, v0.126.0); v0.123.0 was the no-default-features class: run `RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links" cargo doc --workspace --no-deps` before every tag, or just `tools/run-gates.sh`.** Notice `open/notice_2026-11-04_v0.126.0_released.md`. `imm1344e.pdf` in the repo root is NOT rights-cleared: never commit it. **Next: wait for pdfcer-gui's next round.**

**2026-11-03 update (release v0.123.0).** **v0.123.0 is the latest release** (annotated tag on `912437c3`, 8 assets, OneDrive `pdfcer2`, `pdfcer1` keeps 0.122.0, crates color/model/pkix 0.123.0). **The next release writes pdfcer1.** It carries G263 / Pass 658.11 (`ece1c15e`): one fused GPU lamp loop, lamp rows reject on reach/cone after 2 fetches, `RenderOptions::lamp_cut: LampCut` (`Auto` per model, invisible / `Threshold(h)` / `Off`; Ken: navigation and screenshot looks get separate settings). Not timed on an Adreno; overdraw (`gl_FragDepth` kills early-Z) is Backlog 658.12, awaiting pdfcer-gui's choice of depth prepass vs non-log-depth. **CI is RED at the tag**: `--lamp-cut` named `pdfcer_3d::LampCut` in the CLI's clap struct, so every CLI `--no-default-features` build failed; fixed on main by `ba33466d` (CLI-local `LampCutArg`), tag not moved. **Fourth time this class hit a tag: run `tools/run-gates.sh`, or every `--no-default-features` step in `.github/workflows/ci.yml`, before tagging.** New `tools/clean-target-cache.py` (`3c679967`, Ken's request) runs at the end of every `package-portable.py`: sweeps build outputs superseded since the second-newest version bump, plus dead agent-worktree caches; first run freed 85.7 GiB (D: 87%). Notice `open/notice_2026-11-03_v0.123.0_released.md`. **Next: wait for pdfcer-gui's next round.**

**2026-11-03 update (release v0.122.0).** **v0.122.0 is the latest release** (annotated tag on `c7226cd8`, 8 assets, OneDrive `pdfcer1`, `pdfcer2` keeps 0.121.0, crates color/model/pkix 0.122.0). **The next release writes pdfcer2.** It carries G262 / Pass 658.9 (`2e6847ee`, filed 1401st): `GltfLights::Exposure` is the new `read_gltf` default, metering one light scale over the model's surfaces (area-weighted median of up to 20,000 sampled triangles at 80%; inverse square past the 5%-diagonal floor, spot cones, no occlusion or facing; falls back to `Normalise` with a note when nothing is lit). Bancroft office (64 lamps): scale 0.00113 to 0.0090, floors and desks visibly lit. Render-time exposure is Backlog 658.10. Notice `open/notice_2026-11-03_v0.122.0_released.md`. **Next: wait for pdfcer-gui's next round.**

**2026-11-03 update (release v0.120.0).** **v0.120.0 is the latest release** (annotated tag on `a4d9b0cf`, 8 assets, OneDrive `pdfcer1`, `pdfcer2` keeps 0.119.0, crates color/model/pkix 0.120.0 published; prep filed 1396th). **The next release writes pdfcer2.** It carried the whole lighting/shading/shadow arc Ken waited on: lamp shadows (658.2), 16 lights (658.3), material finishes (658.4), every directional light casts + see-through casters + highlights shadowed by their own light (658.5), all 15 `/RM` render modes and PRC wires (658.6), each GPU half pixel-compared against the CPU in headless Edge + SwiftShader. Also `2a78909d`: the release sweep caught `cli_help_ships_no_internal_markup` red (backticks in a `ValueEnum` variant doc; `--test all` does not run the `--bin pdfcer` unit tests, so the scoped gate must include `cargo test -p pdfcer-cli --bin pdfcer`). Notice `open/notice_2026-11-03_v0.120.0_released.md`. Open: U3D reader; format limits recorded by the 1395th filing. **Next: wait for pdfcer-gui's next round** (it must bind the new uniforms). **Since the tag:** Pass 658.7 (`ef0f64e8`, filed 1397th) puts lines wider than 1 px on the GPU (`GLSL_ES_300_WIDE_LINE_VERTEX`, instanced quads, `DrawData::line_width`); WebGL 2 vs CPU widths 1–5 mean ≤ 0.12 levels; GUI notice `open/notice_2026-11-03_wide_lines_on_the_gpu.md`. Unreleased. Harness: scratchpad `658.6-modes` (`main.rs wire <width>`, `wide<w>_1.html`). Pass 658.8 (`12345f8d`, filed 1399th) lifts the GPU light cap from 16 to `MAX_LIGHTS` = 1,024 per kind: lights travel in one RGBA32F texture `u_lights` (`FrameLights::light_table()`, 5 texels a row), the uniform arrays are removed, `FrameLights` is no longer `Copy` (BREAKING for the GUI; notice `open/notice_2026-11-03_a_thousand_lights_on_the_gpu.md`). Shadows stay at `MAX_SHADOW_CASTERS` = 16 (memory); lamps past 16 light unshadowed on both paths, `FrameLampShadows::dropped`. WebGL 2 vs CPU, 40+40 lights: mean 0.018 levels. Harness: scratchpad `658.8-lights` (`main.rs <n>`, `run40.html`). **Released in v0.121.0** (tag on `3435f01a`, CI green, OneDrive `pdfcer2`; the next release writes `pdfcer1`; filed 1400th). The 1399th filing's owed sweep for stale "16 lights" wording in `crates/pdfcer-3d` and `crates/pdfcer-cli` was done: no survivors. Operator priority relayed to pdfcer-gui (`notice_2026-11-03_operator_gpu_for_every_3d_pass.md`): every software-drawn 3D pass onto the GPU; the engine side is complete. D: was full (1 GB): clear `target/debug/incremental` before big builds.

**2026-11-03 update.** **v0.118.0 is the released engine** (tag on `c6baca00`, OneDrive `pdfcer1`, `pdfcer2` keeps 0.117.0). **The next release (v0.119.0) writes pdfcer2.** Since the tag, on main: the XFA arc (Ken: "continue until all forms work is done"): Passes 647.3f (dataRef instances, subform sets), 647.4 cuts (check-box outlines, display pictures, typed values, comb, inline captions, relevant views, fills, bindItems, two-tone strokes, image natural size, tiled root, picture rounding), 647.5 (`render-page` draws the computed page), 647.6 (`xfa-materialize`, now `--in-place` capable), 647.7 (tables) and **real fonts** (`5a0a9665`, decision 214: shell-supplied `XfaFaceSource`, embed by default, `--font-dir`/`--font-embedding`, std-14 fallback disclosed). **Open, awaiting Ken:** (1) barcode encoders built on permissive crates instead of the paywalled ISO symbology standards; (2) bundling an OFL Myriad-metric-compatible fallback (Source Sans 3). Running XFA JavaScript stays out of scope. The real test form is NOT rights-cleared: scratch use only, never named in repo docs.

**2026-11-02 update (release v0.117.0).** **v0.117.0 is the latest release** (annotated tag on `e12dd3b3`, 8 assets, OneDrive `pdfcer2`, `pdfcer1` keeps 0.116.0, crates color/model/pkix 0.117.0 published; filed 1377th-1379th). **The next release writes pdfcer1.** It carried G257 cast shadows + G258 ambient occlusion, CPU and GPU (Passes 657.0/657.1, `97db158e`; shaders compiled AND pixel-compared against the CPU in headless Edge + SwiftShader, recipe in `D:/dev/rag/egui/headless_edge_swiftshader_*.md`) and Pass 647.8 (`0486e1ba`, XFA `TiledSplit`, spec gap G-L35; done because Ken asked about `imm1344e.pdf`, NOT rights-cleared, never a fixture). **CI was red at the tag on `cargo doc -D rustdoc::broken_intra_doc_links`**; fixed on main by `7d0ef09d`, tag not moved. **Release gates now include CI's `cargo doc` step.** Open: Pass 657.2 (glTF `occlusionTexture`); `note_E064` asks pdfcer-gui to surface the computed XFA pages on open (the canvas shows the "Please wait..." placeholder); XFA 647.4 captions still paused. Worktree `.claude/worktrees/agent-a7aad6e39bd2e0461` can be removed. **Next: wait for pdfcer-gui's next round.**

**2026-11-01 update (release v0.116.0).** **Standing direction unchanged: the sole focus is pdfcer-gui's requests; XFA (647.3f, 647.4) stays paused.** **v0.116.0 is the latest release** (annotated tag on `56f0a7fb`, CI green at the tag, 8 assets, OneDrive `pdfcer1`, `pdfcer2` keeps 0.115.0, crates color/model/pkix 0.116.0 published; filed 1375th). **The next release writes pdfcer2.** It carried G256 (Pass 654.3, `b3c1bb4e`): log depth in the GPU reference fragment shader (`gl_FragDepth = log2(w/near)/log2(far/near)`, new `Camera::depth_range` -> `u_depth_range`, plus `u_orthographic`); hyperbolic depth into a 24-bit buffer at near = distance*1e-4 z-fought on every platform. Also carried `986a7d2f`. Reply `open/reply_G256_gpu_log_depth_SHIPPED.md`, notice `open/notice_2026-11-01_v0.116.0_released.md`. The shader is still uncompiled by us; the GUI confirms on hardware. **Inbound check: list `open/` with `ls -t`; `find -newer INDEX.md` missed G256 (same-minute write).** **Next: wait for pdfcer-gui's next round.** Loop still running, slow pace (~1800s).

**2026-11-01 update (release v0.115.0).** **Standing direction unchanged: the sole focus is pdfcer-gui's requests; XFA (647.3f, 647.4) stays paused.** **v0.115.0 is the latest release** (annotated tag on `89f3ee58`, 8 assets, OneDrive `pdfcer2`, `pdfcer1` keeps 0.114.0, crates color/model/pkix 0.115.0 published; filed 1373rd/1374th). **The next release writes pdfcer1.** It carried G253's GPU half (Pass 654.2: `PreparedModel::draw_data`, `Camera::view_matrix`/`projection_matrix`, `FrameLights`, GLSL ES 3.00 shaders; `98c2560e` `5b33a93c` `37c27e60` `75edce63`) and G255 (Pass 656.0, `62c4946a`). **The G252-G255 round is fully released**; notice `open/notice_2026-11-01_v0.115.0_released.md`. **CI was red at the tag in `cargo test -p pdfcer-3d --no-default-features`**: the new `gpu*` modules were not gated on `render`. Fixed on main by `986a7d2f`, tag not moved. **Third time this class has hit a tag (v0.113.0, v0.115.0): the change-scoped release gate must run CI's `--no-default-features` steps for every changed crate (`grep -n no-default-features .github/workflows/ci.yml`).** The GPU shader text is uncompiled by us; the GUI may file a driver error. **Next: wait for pdfcer-gui's next round.** Loop still running, slow pace (~1800s).

**2026-11-01 update (release v0.114.0).** **Standing direction unchanged: the sole focus is pdfcer-gui's requests; XFA (647.3f, 647.4) stays paused.** **v0.114.0 is the latest release** (annotated tag on `c0cc5465`, 8 assets, OneDrive `pdfcer1`, `pdfcer2` keeps 0.113.0, crates color/model/pkix 0.114.0 published). **The next release writes pdfcer2.** It carried G252 (lighting schemes, Pass 654.0) and G253's CPU half (`PreparedModel`, `RenderOptions::threads`, Pass 654.1) on `78f16dc2`, and G254 (`settings::set_store_dir`, `StoreKind::HostSupplied`, Pass 655.0) on `2a10522f`. Replies and `open/notice_2026-11-01_v0.114.0_released.md` are written. **CI was red at the tag on `check-register-entry-size.py` only**: the 1368th filing put the `Pass 654.2` item as the first `###` under `## Next up`, above the untitled filing-note paragraphs, so it measured 1,926 lines. Fixed by the 1370th filing (moved the item); tag not moved. **Lesson, second time (v0.108.0 was the first): re-run the filing gates (`check-register-entry-size.py`, `check-commits-filed.py`) on the prep-filing commit itself, just before tagging, not only before it.** **After the tag, unreleased:** G255 (Pass 656.0, `62c4946a`: `PreparedModel` owned, `render(&model, &camera, &options)`; replied) and `9a929245` (check-tests-harnessed exempts own-`[[test]]` files; it was a SECOND red audit at the tag, masked by the first in the same CI job). **Next: Pass 654.2** (G253 GPU half: GPU-ready vertex/index buffers plus view/projection matrices matching `Camera`; plain buffers only, no GPU crate in `pdfcer-3d`), unless the GUI's next round says otherwise. Read `D:\dev\rag\egui\` for glow paint callbacks before scoping it.

**2026-10-31 update (release v0.113.0).** **Standing direction (Ken): the sole focus for each release is pdfcer-gui's requests; it says when a round is complete. XFA (647.3f, 647.4) is paused until told otherwise.** **v0.113.0 is the latest release** (annotated tag on `ca9aa1dd`, 8 assets, OneDrive `pdfcer2`, `pdfcer1` keeps 0.112.0, crates color/model/pkix 0.113.0 published). **The next release writes pdfcer1.** It carried the whole G249-G251 round (G249 black point compensation `121eb7d9`, G250 `82aee9d6`, G251 `edb2d1ae`) plus G243 `396c7c70`, G244 `5c16e092`, G245 part 2 `01c5f515`; notice `notice_2026-10-31_v0.113.0_released.md`. **CI was red at the tag on two feature-off steps** (pick_index ungated on `render`; an unconditional CLI import used only under `3d`); fixed on main by `5587fca6`, tag not moved; `PickIndex` now needs pdfcer-3d's `render` feature. **The scoped release set must always include CI's five `--no-default-features` steps** (`grep -n no-default-features .github/workflows/*.yml`). Test patch 17.3 (JBIG2) is not filed by the GUI yet; G249's reply asked for it separately. **Next: wait for pdfcer-gui's next round.** Loop still running, slow pace (~1800s).

**2026-10-31 update (after midnight).** **Pass 648.6 shipped, unreleased** (`01c5f515`, filed 1360th by `2fb73814`): 3D measurements on saved views (`/MA`, ISO 32000-2 Tables 326-331): `ThreeDMeasurement`/`ThreeDMeasureKind`/`MAX_3D_MEASUREMENTS`, `ThreeDSavedView.measurements` + `with_measurement`, `set_3d_views` writes and validates them, two new disclosures; CLI `3d-views --measure A:B --units U` (linear only, plane facing each camera), `3d-list` prints `measure` lines. G245 part 2 replied `reply_G245_part2_measurements_SHIPPED.md`. Backlog: Pass 648.7 (`/3DU`, CLI for the other kinds, comment links). **The next release writes pdfcer2.** 647.3f and 647.4 stay in the Backlog. Loop still running, slow pace (~1800s).

**2026-10-31 update (late night).** **v0.112.0 is the latest release** (annotated tag on `0a19909f`, CI green at the tag, 8 assets, OneDrive `pdfcer1`, `pdfcer2` keeps 0.111.0, crates color/model/pkix 0.112.0 published, verify-release ok but for the known dirty tree; publication filed 1359th). **The next release writes pdfcer2.** It carried only Pass 647.4b (X027, `96af5ff3`): XFA rectangle/line/arc draws, dashed edges, `rotate` (`LayoutItem.rotate`, `OddRotate`, `DashLengths`, `DashCaps`, `ArcCircle`, `ArcFill`, paint notes Shape/Rotate; CLI label `stroke-not-drawn` renamed `stroke-drawn-solid`). X027 reply renamed SHIPPED, notice `notice_2026-10-31_v0.112.0_released.md`. **Next: Pass 648.6 (G245 part 2, `/MA` measurements; spec RAG first).** 647.3f and 647.4 stay in the Backlog. Loop still running, slow pace (~1800s).

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
  the git dep in pdfcer-render. **zune-jpeg is stable crates.io 0.5.15** behind `pdfcer-jpeg-remux`
  (Pass 690.1); the git rc3 pin of decisions 197/198 is gone. `tools/*/Cargo.lock` were refreshed
  to the git rc3 graph (0145632a); refresh them again after any engine dependency change.** pkix full verify waits for model on a registry; decision 196 split precedes
  pkix's first publish. All 14 engine crates are `publish = true` (Ken approved, 2026-11-08).
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
  Recipe: bump `Cargo.toml` + `cargo update -w` (root and `fuzz/`), run every
  CI `python tools/` audit (each `run: python tools/...` line in
  `.github/workflows/*.yml`) and check the round's CI colour before tagging
  (v0.130.0 went out red on a doc audit the scoped list lacked), librarian
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
