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
