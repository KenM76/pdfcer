# 3D-in-PDF dependency survey (U3D / PRC / mesh I/O / CPU poster)

Date: 2026-09-30. Licence data verified from each crate's crates.io version
record (`license` field) or the repo's LICENSE / file headers, fetched this
session. Sizes marked **measured** come from a scratch probe crate
(a scratch probe crate, release, `lto="fat"`, `codegen-units=1`,
`strip`, `panic=abort`, Windows x64 and `wasm32-unknown-unknown`, no
wasm-opt). Everything else is an estimate and says so.

Rule set applied (LEGAL.md §6.1/§6.2): permissive (MIT/Apache/BSD/Zlib/0BSD)
= OK; LGPL/MPL = flag to operator; GPL/AGPL = forbidden, including as a
code-copying reference. Non-OSI "non-commercial" licences are treated as
unusable (not listed in §6.1, and incompatible with MIT redistribution).

Existing pdfcer state (repo grep): `/3D` authoring is refused by name in
`Pass 261.6` (ROADMAP line ~23010; FEATURES line 535) — "read/round-trip
recommended, unmeasured". `pdfcer-render/src/annot.rs` already classifies
`/3D` as a subtype and renders its `/AP` like any annotation; `edit.rs`
rejects edits on media annots (`Sound|Movie|Screen|RichMedia|3D` → `R::Media`).
The spec RAG has the sources (PDF32000_2008.pdf, ISO_32000-2 sponsored,
Adobe ExtensionLevel3 supplement) but only `iso32000__s__12.5.6.3.md` /
`7.3.7` touch 3D — **§13.6 (3D artwork) is not extracted yet**; dispatch
`pdfcer-spec-librarian` before any implementation Pass.

---

## 0. Strategic finding that changes the scoping

**PDF 2.0 now has glTF as a 3D format.** ISO/TS 32007:2024 (published
2024-04-01) adds glTF 2.0 (ISO/IEC 12113:2022) as a valid 3D artwork format
inside a **RichMedia** annotation. PDF Association states PDF 2.0 extensions
now cover four 3D formats: U3D, PRC, STEP AP 242, glTF.
Sources: https://pdfa.org/pdf-2-0-adds-gltf-model-support/ ,
https://pdfa.org/resource/iso-ts-32007/

Consequence: tier (d) "author PRC from glTF/STL/OBJ" may be the wrong
target. Authoring **glTF-in-RichMedia** needs no 3D-format encoder at all
(pass the glTF/GLB through, or convert STL/OBJ→glTF, which is trivial), is
royalty-free, and matches the modern standard. PRC/U3D authoring only
matters if the goal is display in Acrobat Reader today (Acrobat's glTF
support status was **not verified** this session — ask
`pdfcer-acrobat-librarian`). Viewer support for ISO/TS 32007 is the real
open question, not the encoder.

---

## 1. U3D (ECMA-363) and PRC (ISO 14739) implementations

### 1.1 Rust

**crates.io: none.** Searched `u3d`, `prc`, `ecma-363`, `iso 14739`,
`universal 3d`, `product representation compact`, `3dpdf`, `pdf 3d`,
`idtf`. Every `prc*` hit is the Smash Ultimate "param" format (`prc-rs`
1.6.1 on crates.io is that, unrelated). No U3D crate exists anywhere.

GitHub (Rust):

| Repo | Version | Licence (verified) | Last activity | State | Pure Rust | wasm32 | Weight | Fitness |
|---|---|---|---|---|---|---|---|---|
| **ralovich/prc-rs** (crate name `prc`, **not published**) | 0.2.0 (HEAD `bcbd0012`, 2026-09-21) | **MIT** (Cargo.toml + LICENSE) | created 2026-03-09; 113 commits; 0 stars; single author | Active, young, one-person | **No as shipped**: depends on `libdeflater` (C libdeflate). The code path using it is dead (`use_slow = true` → `inflate` crate). | **Fails** as shipped (libdeflate-sys needs clang). **Builds** after deleting the one `libdeflater` dep + dead branch (verified this session). Caveat: 3 `Instant::now` calls + 19 `measure_time` macros — `Instant::now()` panics at runtime on `wasm32-unknown-unknown`, so those must be removed/feature-gated too. | Deps: inflate, deflate, libdeflater, log, byteorder, modular-bitfield, static_assertions, num_enum, bitstream-io, measure_time, serde, serde_json. 35.6k lines of Rust (905 KB generated `prc_gen.rs`). **Measured: +0.97 MB native, 1.24 MB wasm** for `prc_describe`. | **Reader only.** README status: schema evaluator, Huffman, compressed arrays, compressed NURBS, C API = done; **PRC_TYPE_TESS_3D_Compressed vertices/triangles/normals/colours = NOT done**; B-rep tessellation = not done; **PRC write = not done**. So it cannot yet produce triangles from the compressed-tessellation files most CAD exporters emit. Public API is a describe/JSON-dump oriented `common::prc_describe(...)` with nine bool flags, not a mesh API. Best use: a **behavioural reference** (MIT, safe to read and copy with attribution) and possibly a future dependency once it stabilises and publishes; do not depend on it now. |
| cantudo/pdf3d_latex_extractor | 0.1.0, 2023-04-21 | **none** (no LICENSE) | 1 day of commits, dead | dead | yes | untested | lopdf+pdf | Extracts PRC/.vws streams from PDFs. No licence = all rights reserved → **not usable, not even as copy source**. Trivial anyway (tier a). |
| MysticFrog/FBX2U3D | 0.1.0, 2026-05-28 | **custom non-commercial** ("Commercial use is not permitted without prior written agreement") | 2 days of commits | dead-ish | wraps bundled Intel C++ SDK | no | — | **Not usable** (non-OSI, non-commercial). |
| user5522/b3d | — | Apache-2.0 | 2025-07 | — | — | — | — | Unrelated (Unity "u3d" in Bevy). |

### 1.2 Non-Rust behavioural references

| Project | Licence (verified) | State | What it covers | Usable as reference? |
|---|---|---|---|---|
| **Intel Universal 3D Sample Software** (IDTF ↔ U3D converter, RTL codec) — mirrors `ningfei/u3d` (v1.4.5 fork, last push 2017), `alemuntoni/u3d` (MeshLab's fork, tag 1.5.2, pushed 2025-09-09) | **Apache-2.0** (COPYING; file headers "Copyright (c) 1999-2006 Intel Corporation, Licensed under the Apache License 2.0") | Maintained only as MeshLab's build dep | The reference U3D encoder **and** decoder, incl. CLOD mesh arithmetic coding, IDTF text format. Large C++ (RTL tree). | **YES** — permissive; reading, porting, copying with NOTICE attribution all allowed. Apache-2.0 includes a patent grant from Intel. |
| **mradugin/u3d-to-stl** | **Apache-2.0** (LICENSE + Intel headers) | pushed 2025-11-20, 5 stars | Small C extraction of Intel's sample *u3d-parser*: bitdecoder, bitencoder, dynamic histogram, parser. 5.4k lines .c / 11.8k with headers. Limitation: only `CLOD_Mesh_Continuation (0xFFFFFF3B)` blocks, no materials. | **YES — the best starting reference for a Rust U3D mesh decoder.** Right-sized. |
| **Asymptote PRC writer** (`vectorgraphics/asymptote/prc/`: oPRCFile.cc, writePRC.cc, PRCbitStream.cc, PRCdouble.cc) | Files in `prc/`: **LGPL-3.0-or-later** (headers, © 2008 Orest Shardt & Michail Vidiassov). Rest of Asymptote: **GPL-3.0** (repo LICENSE). | Active (pushed 2026-09-30) | The most battle-tested open PRC *writer* (uncompressed tessellation, NURBS, materials). | **FLAG — LGPL.** Not a dependency. Reading it as a reference needs an operator decision per §6.2 step 4. Nothing outside `prc/` may be read (GPL). Recommendation: do not read it; work from the spec. |
| **XenonofArcticus/libPRC** | Repo LICENSE: MIT for Chris Hanson's code, **but `src/asymptote/` is an LGPL-3 fork of Asymptote's PRC code** (LICENSE says so) | dead since 2015-01-01 | Standalone PRC writer + OSG plugin + prctopdf | **Mixed.** MIT parts usable; the `src/asymptote/` tree is **FLAG — LGPL**. The MIT "libPRC" alternate writer was unfinished. Low value. |
| **libHaru** (`libharu/libharu`) | **Zlib** (LICENSE); `hpdf_u3d.c` header is an HPND-style permissive notice | Active, v2.4.6 released 2026-03-26 | **Does not parse or write U3D/PRC.** It sniffs the `U3D`/`PRC` magic, embeds the bytes as a `/3D` stream, and builds `/3DV` view / node / lighting / background dicts. | **YES**, but only for the PDF-side dictionaries — which the spec covers anyway. Zero help with the formats. |
| alexcool-project/mesh2u3d (Python) | pyproject says **MIT**; **no LICENSE file** in repo | created 2026-09-15, 0 stars, 3 days of commits | Pure-Python U3D writer (13 KB), PRC writer (9 KB), PDF embedder, validator. | **Weak YES** — declared MIT, but no licence text and zero track record; treat as low-trust. Interesting mainly as proof a minimal U3D/PRC mesh writer is ~500 lines. |
| johnyf/fig2u3d (MATLAB) | BSD-2-Clause | 2026-08 | Shells out to Intel IDTFConverter | Not useful beyond the IDTF route. |

Spec access: ECMA-363 (U3D, 4th ed.) is a free download from Ecma. ISO
14739-1:2014 (PRC) is paywalled; the prc-rs README lists public
alternatives (Acrobat 9 SDK "PRC Format Specification" archive, 2009
working draft SC2N570) and the pdf-association/pdf-issues tracker of known
spec errors.

**Patent note (PRC compressed tessellation):** prc-rs cites US 8,207,965 B2
("Rewritable compression of triangulated data", priority 2006-07-11,
filed 2007-03-21). Google Patents status: **"Expired – Fee Related"**
(fetched this session). Reported fact, not a legal conclusion; flag to the
operator if compressed tessellation is ever implemented.

---

## 2. Mesh I/O crates (authoring input)

| Crate | Version / date | Licence (crates.io) | Pure Rust | wasm32 | Deps | Size | Notes |
|---|---|---|---|---|---|---|---|
| **stl_io** | 0.11.0 / 2026-03-15 | MIT | yes | yes (built in probe) | **none** | 16 KB crate | Binary + ASCII read/write. Maintained. |
| **tobj** | 4.0.5 / 2026-08-02 | MIT | yes | yes (built, `default-features=false`) | all optional (ahash, log, tokio, futures-lite) | 26 KB crate | OBJ+MTL. Use `load_obj_buf` with a no-op material loader (no filesystem). Maintained. |
| — stl_io + tobj together | | | | | | **measured +92 KB native** | |
| **gltf** | 1.4.1 / 2024-05-10 | MIT OR Apache-2.0 | yes | yes (built, `default-features=false`) | gltf-json, serde, serde_json, byteorder, lazy_static; optional base64/image/urlencoding | **measured +389 KB native** (serde_json dominates) | Standard reader; last release 2024 but 2.1M recent downloads. Keep `image` and `import` features off (no filesystem/network). |
| wavefront_obj | 11.0.0 / 2025-01-08 | MIT | yes | likely | none | 27 KB | Alternative OBJ reader. |
| obj-rs | 0.7.4 / 2024-09-28 | Apache-2.0 OR MIT | yes | likely | num-traits | 754 KB crate (test assets) | No advantage over tobj. |
| ply-rs | 0.1.3 / 2020-08-27 | MIT | yes | likely | byteorder, linked-hash-map, peg | 27 KB | **Unmaintained.** |
| **ply-rs-bw** (maintained fork) | 4.0.1 / 2026-07-24 | MIT | yes | likely | byteorder, indexmap, peg | 53 KB | Use this if PLY is wanted. |
| **threemf** | 0.8.0 / 2026-03-05 | **0BSD** | yes | likely (zip w/o default features) | quick-xml, serde, thiserror, zip | 8 KB | 3MF read/write. Small user base (44k dl). 0BSD is permissive. |
| mesh-loader | 0.1.13 / 2024-10-15 | Apache-2.0 | yes | likely | roxmltree? | 88 KB | STL/COLLADA/OBJ in one; less used. |
| **truck-stepio** | 0.3.0 / 2024-09-20 | Apache-2.0 | yes | likely | truck-geometry/modeling/polymesh/topology, chrono, ruststep? | heavy (whole truck B-rep kernel; truck-meshalgo adds rayon, spade) | STEP import → B-rep → tessellate via truck-meshalgo. Repo active (pushed 2026-09-28, 1.5k stars) but crates lag. Estimated **+2–4 MB**. Only worth it if STEP input is a real requirement. |
| ruststep | 0.4.0 / 2024-09-20 | Apache-2.0 | yes | likely | nom, serde, derive_more, itertools, Inflector, thiserror | 100 KB crate | STEP Part 21 parser + EXPRESS codegen; no tessellation by itself. |
| oxideav-mesh3d / -gltf / -stl / -obj | 0.0.4–0.0.6 / 2026-06..09 | MIT | yes | claimed | serde, serde_json | 226–457 KB crates | Very young (0.0.x) unified mesh model. Not recommended yet. |

"likely" = pure Rust, no C, no threads required, not built this session.

Recommendation: **stl_io + tobj** (tiny, zero-dep) always; **gltf** behind
its own sub-feature because serde_json is ~0.4 MB; PLY/3MF on demand;
STEP out of scope unless explicitly requested.

---

## 3. CPU poster rasterization

A poster is only needed when **authoring** (the `/3D` annotation's `/AP`
must show something when inactive) or for a `/3D` annot with no `/AP`.
Reading existing 3D PDFs needs no 3D rendering: pdfcer-render already paints
the `/AP`.

| Option | Licence | State | wasm32 | Effort | Notes |
|---|---|---|---|---|---|
| **A. Painter's algorithm on tiny-skia** (already a dep, 0.11.4 in lock; BSD-3-Clause) | BSD-3 | in tree | yes | **1–2 days** | Transform by the /3DV camera, flat/Lambert shade per triangle, sort back-to-front, `fill_path` each triangle (or batch by colour). Anti-aliased, zero new deps, ~200–300 lines. Fails on intersecting/cyclic triangles (rare in CAD posters; visible as speckle). Seam artefacts between AA-filled adjacent triangles — fill without AA or with a small outset. |
| **B. Own z-buffer triangle rasterizer, composited through tiny-skia's Pixmap** | ours (MIT) | — | yes | **3–5 days** incl. tests | Edge-function / scanline fill with an f32 depth buffer, perspective + orthographic, back-face cull, Lambert + ambient, optional 2–4× supersample for AA, silhouette/edge lines optional. ~500–900 lines. Correct for every mesh. **Recommended.** Estimated +30–60 KB. |
| C. **euc** | crates.io 0.5.3: **"Apache-2.0 AND MIT"** (both apply — both notices required; still permissive) | Last release **2021-03-09**; repo HEAD is an unreleased 0.6.0, last push 2024-12-31; 355 stars. **Dormant.** | 0.5.3 **builds** for wasm32 with default `std` (verified); `default-features=false` (libm mode) **fails to compile** (vek 0.14 `num_traits::real` import) | 2–3 days of integration | Deps: vek 0.14, approx, num-traits, num-integer. Shader-trait API, no AA. Saves little over B and adds a dormant dependency. **Not recommended.** |

No other maintained, permissive, pure-Rust software rasterizer with depth
buffering was found that beats option B on weight.

---

## 4. Binary-size cost per tier (the optional crate, e.g. `pdfcer-3d` behind a `3d` feature)

| Tier | What | New deps | Estimated size cost | Basis |
|---|---|---|---|---|
| **(a)** list / extract 3D streams | Walk `/3D` & `/RichMedia` annots, read `/3DD` (stream or 3D reference dict), `/Subtype /U3D | /PRC` (+ glTF via RichMedia per ISO/TS 32007), sniff magic, dump bytes, list `/VA` views | **none** (core parser + existing flate) | **~10–30 KB**; ~1–2 days | estimate. This is the "read/round-trip" half Pass 261.6 already recommends; could live in core without a feature. |
| **(b)** parse U3D/PRC meshes | **U3D**: own port from Apache-2.0 Intel/u3d-to-stl decoder (CLOD base + continuation, arithmetic coder, histograms) — est. 3–5k Rust lines, **2–3 weeks**. **PRC**: either vendor/port prc-rs (MIT) or write own; compressed tessellation is the hard, under-documented part that nobody open has finished. | none if own code; prc-rs if adopted | U3D decoder **~150–300 KB** (estimate). PRC via prc-rs **~1.0 MB native / 1.2 MB wasm (measured)**; an own mesh-only PRC reader maybe 200–400 KB (estimate). | measured + estimate |
| **(c)** CPU poster | Option B (z-buffer) or A (painter's) | none | **~30–60 KB** | estimate |
| **(d)** author 3D from glTF/STL/OBJ | Inputs: stl_io+tobj (**+92 KB measured**), gltf (**+389 KB measured**). Output: **glTF-in-RichMedia** = pass-through, ~0 extra; **U3D writer** (port of Apache Intel encoder, CLOD with no LOD reduction) ~100–200 KB, 1–2 weeks; **PRC writer** (uncompressed `PRC_TYPE_TESS_3D`, from spec — not from LGPL Asymptote) ~100–200 KB, 2–3 weeks. Plus tier (c) for the poster. | stl_io, tobj, gltf | **~0.2 MB (STL/OBJ→U3D or PRC) to ~0.6 MB (with glTF input)**, + poster | measured + estimate |

All numbers are optimised, stripped, LTO; debug/unoptimised builds are
several times larger. wasm numbers before wasm-opt.

---

## 5. Verdicts

1. **No Rust U3D implementation exists. No published Rust PRC crate exists.**
   The only real Rust PRC code, prc-rs (MIT, unpublished, 7 months old, one
   author), is a partial reader: no compressed-tessellation meshes, no writer.
2. **Clean permissive references exist for U3D** (Intel sample software and
   the compact u3d-to-stl extraction, both Apache-2.0). **For PRC the only
   mature open writer is Asymptote's — LGPL-3.0-or-later, flag it**; libPRC
   embeds the same LGPL code. FBX2U3D (non-commercial) and
   pdf3d_latex_extractor (no licence) are unusable.
3. **Tier (a) is cheap and dependency-free**; do it first.
4. **Poster: write our own z-buffer rasterizer on tiny-skia's Pixmap**
   (3–5 days, ~50 KB). euc is dormant and its no_std mode is broken.
5. **Before scoping tier (d), check ISO/TS 32007 (glTF in RichMedia) viewer
   support.** If acceptable, authoring collapses to "embed glTF + poster",
   with no U3D/PRC encoder.
6. Mesh input: stl_io + tobj (~90 KB, zero deps) always; gltf (~390 KB)
   behind its own sub-feature; STEP (truck) out of scope.

Open items for the operator: (i) whether LGPL Asymptote PRC code may even
be *read* (recommend no); (ii) target viewer (Acrobat Reader: U3D/PRC;
glTF support unverified); (iii) the PRC compressed-tessellation patent is
listed as expired, which is information, not clearance.
