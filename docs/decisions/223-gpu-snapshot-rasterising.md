# 223 — GPU rasterising for PDF snapshots

Operator ask (2026-11-10): snapshots should render on the GPU, with a
GPU-acceleration switch for each part in the snapshot settings, as 3D
navigation has. Operator ruling: GPU snapshots "should match the CPU picture
pixel for pixel if possible". Consultant: autonomous-builder. The ruling
amends its gate and its go/no-go (both marked below).

## Today

Snapshots are drawn by the CPU (tiny_skia plus pdfcer's clause-11
compositor), via `render_page_with_view`. They run synchronously on
pdfcer-gui's UI thread.

Measured on a dense A3 CAD sheet with the release CLI (wall clock, including
load and PNG write):

| Scale | Time |
|---|---|
| floor (load + interpretation) | 1.33 s |
| 1.0 MP | 1.39 s |
| 300 DPI (17.4 MP) | 2.2 s |
| 49 MP (snapshot cap) | 3.43 s |

A GPU can only remove the resolution-dependent share: about 0.85 s at
300 DPI, which still includes PNG encode. Text pages floor at about 3 ms.

## Decision

1. **Pass 704.0, engine, measurement and seam.**
   - `tools/render-profile` splits cost per document and scale: load, record,
     replay (with clip/fill/blend ablations), encode, and 1 vs N row bands on
     host threads. Threads live in the tool only; the engine stays
     thread-free.
   - `DisplayList` is `Sync`, enforced by a static-assert test.
   - Bands cut at integer device rows stitch byte-identically to the whole
     replay.
   - Results are recorded in `docs/render-region-measurements.md`.
2. **pdfcer-gui** moves the snapshot off the UI thread and replays row bands
   on worker threads it owns. This is a GUI notice.
3. **GPU arc (704.1+).**
   - It is a `gpu` module in `pdfcer-render` behind a non-default `gpu`
     feature.
   - It reads the `DisplayList` and emits GLSL ES 3.00 shader text plus plain
     buffers. No GL, windowing or thread dependency, and no new crate
     dependency; it builds for wasm32.
   - The GUI hosts it on its glow context, as model3d does.
   - *Amended by the ruling:* the 704.0 numbers go to the operator; they do
     not cancel the arc silently.
4. **Parts, one switch each,** plus a whole-picture switch, "Graphics card
   draws snapshots":
   - **Fills & strokes:** paths, stroke outlines, clips and glyph outlines.
     The stroker stays CPU.
   - **Images:** sampling, `/Interpolate`, masks and tiling cells. Decode
     stays CPU.
   - **Shadings:** types 1–7. Functions go into a CPU-built lookup texture,
     and meshes are tessellated on the CPU.
   - **Transparency:** alpha, blend modes, soft masks and groups.

   Colour conversion, CMYK, overprint and separations are always CPU. If any
   part is needed but switched off or unsupported, the whole snapshot is
   drawn by the CPU, and the status note gives the reason (as 3D does).
   Build order: Images, Shadings, Fills, Transparency.
5. **Exports and defaults.**
   - A snapshot may use only parts that passed the gate. The 3D rule "saved
     pictures are always CPU" stays as written for 3D.
   - A switch appears only once its part has passed.
   - Defaults: the whole picture is on, and each passed part is on.
   - Turning the whole picture off gives the CPU picture, identical on every
     machine and to `pdfcer render-page`.
   - The CLI is always CPU. Export Image and Print stay CPU.
   - The status note names which path drew the snapshot.
6. **Gate (amended by the ruling).**
   - **The bar is byte-identical** RGBA8 against CPU replay of the same
     `DisplayList`, at scale 1, 300 DPI and the 50 MP cap. The corpus is
     synthetic per-part fixtures plus the render-parity pages that use the
     part. It runs on SwiftShader in CI, plus one recorded real-GPU run per
     part.
   - **A tolerance exists only for a part shown unable to be exact.** The
     record states why, and the limits are fixed before measuring (W14):
     - mean |Δ| ≤ 0.05;
     - ≤ 0.1% of pixels > 2 levels;
     - ≤ 0.01% of pixels > 8 levels, with no 3×3 block wholly > 8;
     - max |Δ| ≤ 64;
     - no page moves to a worse render-parity bucket.
   - That part's switch tooltip states that it differs slightly from the
     processor's picture. It is never silent (CLAUDE.md rule 4).
   - **Speed and bounds:** the part must beat parallel-band CPU end to end
     (including upload and readback) at 300 DPI, or its switch does not
     ship. It must stay within `page_budget`, with tiles ≤
     min(GL_MAX_TEXTURE_SIZE, MAX_PIXMAP_EDGE).

## Rejected

- **vello:** wgpu-centred while the GUI is glow-first. It is alpha
  (PRIOR_ART: re-evaluate around 2027), and it would add a third
  antialiasing and compositing model.
- **GUI-only rasterising:** puts rendering semantics out of reach of
  render-parity, the CLI and the web fork.

## Risks

- tiny_skia's supersampled coverage may never be exact on a GPU, so Fills
  may only ever reach the tolerance tier.
- GL readback can hitch the UI thread: tile across frames and use PBO
  readback.
- Bands must start on integer device rows.
- Check `DisplayList` memory at 50 MP against §10.
- The lean-graph and `--no-default-features` CI steps must stay green, and
  feature-on steps must be added.
