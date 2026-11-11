# measure-3d

Corpus counts and ray speed for Pass 705.0 (decision 224: indirect light and
reflections). Never shipped; excluded from the root workspace.

```
cargo build --release                                   # native
measure-3d <model.prc>...                               # counts + native rays/s
MEASURE_FINISHES=1 measure-3d <model.prc>...            # plus each distinct finish
cargo build --release --lib --target wasm32-unknown-unknown
node wasm/time.mjs <target>/wasm32-unknown-unknown/release/measure_3d.wasm <model.prc>...
```

Per model it prints:

- meshes and triangles;
- occurrence and view lights by kind;
- finished meshes, how many have white specular >= 0.9, and how many have a
  non-zero emissive term;
- texture count and notes;
- closest-hit `PickIndex` rays per second and the fraction of rays that hit.

The rays come from a fixed LCG, so both targets trace the same 200,000 rays
and must report the same hit fraction. `.prc` input only: pdfcer has no U3D
reader.

## WebGL 2 checks (`webgl/`)

Run on SwiftShader with headless Edge (recipe:
`D:\dev\rag\egui\headless_edge_swiftshader_runs_webgl2_...md`), or open the
page in any WebGL 2 browser on the device being measured.

- `rgba32f_texelfetch.html`: uploads RGBA32F 2D, 3D and 2D-array textures
  (640×640, 16 layers) and compares every `texelFetch` result's float bits
  with an RGBA32UI copy. It also prints the texture limits. Any non-zero
  `mismatched_bits` fails. Flipping one reference bit per 4,000 words
  reports exactly that many mismatches, so a zero is not vacuous.
- `probe_lookup_bench.html`: decision 224's bounce lookup per pixel. That
  is 8 probes, each a manual-bilinear octahedral irradiance read plus one
  depth-moment read: 40 `texelFetch` per pixel from a 4,096-probe RGBA32F
  atlas. It reports ms per full-canvas pass, with and without the lookup,
  at device-pixel size.
