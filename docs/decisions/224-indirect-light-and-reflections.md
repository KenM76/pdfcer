# 224 — Indirect light and reflections in 3D

Operator ask (2026-11-10): "I think we are missing reflectivity or diffusion
or something so lights reflect off walls and things like that? Need that
fully implemented for all things 3D." Consultant: autonomous-builder. Today
pdfcer-3d draws direct light only: Blinn-Phong finishes, model, scheme and
sun lights, shadows and SSAO. Nothing bounces or reflects.

## Decision

Anything expensive in indirect light is computed once, by the engine on the
CPU, and published as data. Both paths sample it with the same arithmetic,
using `texelFetch` and manual interpolation, never hardware filtering. This
keeps the ±1 CPU/GPU gate structural: the GPU mirrors a short lookup, which is
the same shape as `render/ao.rs` and `gpu/ao.rs`.

1. **Diffuse bounce and colour bleed.** A probe irradiance volume in the DDGI
   style: per-probe octahedral irradiance plus depth-moment visibility. It is
   built by tracing rays through `PickIndex`, with multiple bounces by
   iteration. On by default for lights fixed in the model.
2. **Specular reflection.** Reflection captures rendered by the CPU (a cube,
   prefiltered per shininess, box-projected). Driven by the material finish or
   a PRC environment map. Off by default until 705.0 measures what producers
   write. Planar mirrors come later, on both paths.
3. **Traced quality.** A deterministic, progressive CPU path trace, for saved
   pictures only and chosen by name. It is never the live view, and the
   picture note names the quality.

**Restated invariant:** every quality that has a GPU path matches the CPU
path of the same quality to ±1. A quality without one is never the live view,
and it is always disclosed.

## Why

- **Lenses:** world-space data is seamless under fisheye and 360 cube-face
  frames; screen-space data is not.
- **No leaks:** probe visibility stops sunlit exterior walls bouncing into
  interiors. This matches the existing thin-ceiling shadow rule.
- **GLSL ES 3.00:** it has no compute shaders, but an RGBA32F texture read
  with `texelFetch` is core. The main shader binds 10 of the 16 guaranteed
  samplers.
- **Bounded and wasm32-safe:** constant ceilings, threads only via
  `render::parallel` (decision 080), and identical bytes at any thread count.

## Rejected

| Option | Rejected because |
|---|---|
| Reflective shadow maps | No indirect visibility, so light leaks through walls; and it is a second GPU implementation |
| Screen-space GI | Depends on the view, vanishes off-screen, and seams under fisheye/360 |
| Voxel cone tracing | Needs compute or image store, and leaks more than probes do |
| Screen-space reflections | Holes off-screen; a binary hit test breaks ±1; lens seams |
| Vertex radiosity or lightmaps | CAD meshes are thin slivers, and they carry no UVs |
| Live path tracing | Too slow; and a GPU version cannot hold ±1 under noise |

## Settings

New `RenderOptions` fields:

- `indirect: IndirectOptions`, in its own module:
  - `bounce`: `Off`, `ModelLights` (the default) or `EveryLight`;
  - `bounce_strength`: 0–4, default 1;
  - `bounce_sky`: `Dark` (the default) or `Background`;
  - `reflections`: `Off` (the default), `Finishes` or `EverySurface`;
  - `reflection_strength`: 0–1, default 1;
  - `producer_environment_maps`: whether a PRC environment map is drawn as a
    reflection.
- `quality`: `Live` (the default, for the screen and saved pictures) or
  `Traced { samples, max_bounces }`, with defaults 256 and 4 and ceilings
  4096 and 8.

Which lights bounce:

| Lights | When they bounce |
|---|---|
| The model's own directional, point and spot lights, the sun, and scheme lights in the model frame | `ModelLights` |
| The headlight and camera-following scheme lights | `EveryLight` only; the volume rebuilds once the view comes to rest |
| Ambient lights | Never; ambient already stands in for indirect light |

Glow-driven bounce is a separate layer, so the gain slider never forces a
rebuild.

Other rules:

- Bounce always has an albedo to work with: the colour as drawn. A reflection
  needs a finish or an environment map.
- `EverySurface` is a look the operator chooses; it is never inferred.
- Nothing is written into the PDF. The GUI holds the per-model and per-view
  settings.
- Every Pass ships its `3d-render` flags. The GUI's `CardPart` gains
  `Bounce`, `Reflections` and `Mirrors`, appended at the end, because
  switches persist by index.
- Disclosure is off-canvas only: which lights bounced and why, any coarsened
  grid, an "updating" state, and the quality used.

**Default flips, with criteria fixed now (W14):**

- Reflections become `Finishes` by default if fewer than 25% of finished
  meshes carry white specular of 0.9 or more.
- Bounce becomes `EveryLight` by default if a 4,096-probe rebuild takes
  100 ms or less on wasm32.

## Scope ("all things 3D")

- **Live and saved pictures:** CPU render, GPU live view and saved pictures,
  in every shaded mode.
- **Posters and thumbnails:** inherit through `render_default_view`. With no
  model-fixed lights they stay byte-identical.
- **glTF:** pdfcer only imports it. Metallic-roughness becomes an inferred,
  disclosed finish, with `--gltf-finish drop`. There is no exporter.
- **PRC writer:** writes nothing new by default. An opt-in bake is in the
  Backlog, with a double-lighting caveat.
- **U3D:** pdfcer has no reader or writer for it. Its material reflectivity
  needs spec-RAG sourcing first.
- **Lamps:** every lamp type injects, with ray-traced visibility, the
  far-side rule and reach culling. `LampCost::NearestPerFrame` limits direct
  light only.

## Passes (dependency order)

| Pass | Scope | Gate |
|---|---|---|
| 705.0 | **Measure.** Corpus counts: models with model-fixed lights, finish distribution, glow, and the `texture_mapping_attributes` values seen. Also `PickIndex` rays/s natively and on wasm32, the SwiftShader sampler and RGBA32F check, the Adreno 643 lookup timing from the GUI, and Acrobat's behaviour from the acrobat-librarian. | No digest changes. |
| 705.1 | **`PickIndex` ray queries** (closest-hit and any-hit with transmission and cut-outs), checked against brute-force Möller-Trumbore. | Picks unchanged. |
| 705.2 | **Bounce, CPU path,** with an incremental `step(budget)` build. Tests: a Cornell-box bleed ratio, a sealed box adding ≤ 2/255, a thin-wall leak bound, albedo 0 adding 0, linearity in gain, and thread-count invariance. Off, or no model-fixed lights, is byte-identical. Probes inside solids are found by a short-hit plus crossing-parity vote, because winding is untrusted. CLI flags and disclosure notes ship. | All existing digests unchanged; new digests pinned and sabotage-checked. |
| 705.3 | **Bounce, GPU path:** the `FrameBounce` atlas plus a GLSL lookup in its own shader chunk. | Software formula, then SwiftShader, ±1 including fisheye and equirectangular. |
| 705.4 | **`EveryLight`:** the live view keeps the last complete volume and swaps whole volumes only; a saved picture always builds a fresh one. | Live equals saved to ±1 once current. |
| 705.5 | **Reflection inputs:** glTF metallic-roughness becomes a finish, and PRC environment maps are identified. | glTF imports without metallic-roughness byte-identical. |
| 705.6 | **Reflections, CPU path:** at most 4 captures, faces ≤ 256, box-projected. | Defaults byte-identical. |
| 705.7 | **Reflections, GPU path,** with manual cube-face addressing. | ±1. |
| 705.8 | **Traced quality,** progressive and cancellable, with deterministic hashed sampling. On the Cornell box, Live and Traced must differ by a mean of ≤ 8/255 at 1,024 samples per pixel; this validates the probes. | Not a parity gate: Traced has no GPU path. |
| 705.9 | **Planar mirrors** on both paths, detected and disclosed. Adds `CardPart::Mirrors`. | ±1; off byte-identical. |

## Risks

- The GPU has been validated on SwiftShader only. Separate shader chunks mean
  a driver failure turns off one part, not the whole picture.
- Hardware filtering and cube-seam behaviour vary by vendor, so use
  `texelFetch` only.
- Camera-lit CAD files get no bounce by default. That has to be disclosed
  plainly, or it reads as a missing feature.
- Probes are coarse in small rooms, so measure on real building plans.
- Saved pictures and posters get slower.
- The per-channel clamp can saturate under strong bounce.
- The values of `texture_mapping_attributes` are a gap in the spec.

## Backlog

- Opt-in bake of bounce into the written PRC.
- U3D reflectivity, once a U3D reader or writer exists.
- Adaptive or per-room probe placement, if 705.2's leak bounds fail on real
  buildings.

## References

- CLAUDE.md rules 1, 4, 11 and 12.
- ARCHITECTURE.md §10.
- Decision 080.
- PRC Material 702 [WD 7.5.4] and TextureDefinition 712 [WD 7.5.7];
  `prc__8137__graphics_materials.md`.
