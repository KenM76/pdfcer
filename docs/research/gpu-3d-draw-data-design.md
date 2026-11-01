# GPU draw data for 3D models (Pass 654.2, request G253 ask 1)

Design for handing a host's OpenGL / GLES 3.0 context (eframe glow) everything
it needs to draw an `AssembledModel` so it looks like the CPU path
(`PreparedModel::render`). `pdfcer-3d` stays GPU-free: it produces plain
buffers, matrices and shader source text; the host owns every GL call.

## What the CPU path does (the behaviour to match)

| Stage | CPU behaviour | GPU equivalent |
|---|---|---|
| Projection | Perspective: `focal = 1/tan(fov_y/2)`, near = `distance·1e-4`, no far clip. Orthographic: half-height `height/2`, no clip. | `projection_matrix(aspect, bounds)`; far plane from the model bounds (CPU has none, so far must enclose every drawn point). |
| Lighting | Per corner, in model space: `Headlight` `0.3 + 0.7·|n·v|`; `Unlit` 1; `Set` `ambient + Σ colourᵢ·|n·dᵢ|`, a zero direction meaning `v`. Two-sided (`abs`). `v` = unit (eye − p) in perspective, −forward in orthographic. Camera-frame lights turned to model space through the camera basis once per frame. | Vertex shader, same formula; the lights resolved per frame by a public function mirroring `Shader::new`, sent as uniforms (max 6 lights: Cube). |
| Normals | Stored corner normals when every corner has a usable one; else the face normal, and the whole triangle takes ONE shade computed at corner 0. | Unshared vertices per triangle. Flat triangles carry the face normal and a `flat` shade: GLES 3.0 takes a `flat` varying from the **last** vertex, so emit corner 0 last (no culling anywhere, so winding is free). |
| Colour | Mesh colour or `options.colour`, straight RGBA; RGB × shade, saturating at 255. | Uniform per draw; `clamp` in the fragment shader. |
| Texture | `uv_matrix` (affine) then origin flip, bilinear, wrap Repeat/Clamp/MirroredRepeat, texel centres at `+0.5`; function Modulate/Replace/Blend/Decal; then shaded. | UVs pre-transformed per vertex on the CPU (affine, so exact); `GL_LINEAR`, `GL_REPEAT`/`GL_CLAMP_TO_EDGE`/`GL_MIRRORED_REPEAT`; rows uploaded top first so `t = v` after the flip. Function as a uniform switch. |
| Order | Opaque meshes depth-tested and depth-writing; translucent meshes (alpha < 255 or a texture with any non-opaque texel) after, in mesh order, tested against opaque depth, not writing it. | Two passes in the same order; `glDepthMask(false)` for the second. |
| Blend | Straight-alpha over, keeping destination alpha. | `glBlendFuncSeparate(SRC_ALPHA, ONE_MINUS_SRC_ALPHA, ONE, ONE_MINUS_SRC_ALPHA)`; over an opaque background this equals the CPU exactly. |

## Proposed public surface (`pdfcer-3d`)

- `PreparedModel::draw_data(&self, model: &AssembledModel, options: &RenderOptions) -> DrawData`
  - `DrawData { vertices: Vec<f32>, draws: Vec<Draw>, textures: Vec<TextureUpload>, bounds: Option<Bounds> }`
  - vertex layout, interleaved, 9 floats: position 3, normal 3, uv 2, flat 1 (1.0 = flat triangle).
  - `Draw { first: u32, count: u32, colour: [f32; 4], texture: Option<usize>, translucent: bool }`, in draw order (opaque first, then translucent in mesh order).
  - `TextureUpload { width, height, rgba: &[u8] or index into model.textures, wrap, function }`.
  - Camera-independent: built once per model/paint change, kept by the host.
- `Camera::view_matrix(&self) -> Result<[f32; 16], RenderError>` and
  `Camera::projection_matrix(&self, aspect: f64, bounds: &Bounds) -> Result<[f32; 16], RenderError>`, column-major as GL expects.
- `FrameLights::new(camera, options) -> FrameLights { mode, ambient, directions, colours, count, eye, forward, orthographic }`: the per-frame uniforms.
- `GLSL_ES_300_VERTEX: &str`, `GLSL_ES_300_FRAGMENT: &str`: the reference shader, so the host need not re-derive the lighting formula.

## Tests

- Matrices: a point projected by `projection × view` lands where the CPU rasterizer puts it (same pixel, ±0.5).
- `FrameLights` shade equals `Shader::shade` for every scheme and both frames.
- A software evaluation of the shader formula over `DrawData` agrees with `PreparedModel::render` per pixel within ±1 on the fixture models (shared tests, as G253 asks).
- Draw order: an opaque-after-translucent model lists the opaque draw first.

## Not in scope

GL calls, a GL dependency, mipmaps (the CPU path has none), backface culling.
