// Times measure-3d's wasm32 ray tracer: node wasm/time.mjs <measure_3d.wasm> <model.prc>...
import { readFileSync } from "node:fs";
const [wasmPath, ...models] = process.argv.slice(2);
const n = 200000;
for (const path of models) {
  const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
  const x = instance.exports;
  const bytes = readFileSync(path);
  const ptr = x.alloc(bytes.length);
  new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
  if (!x.load(ptr, bytes.length)) { console.log(`${path}\tnot assembled`); continue; }
  const t0 = performance.now();
  const hits = x.rays(n);
  const secs = (performance.now() - t0) / 1000;
  console.log(`${path}\twasm32 rays_per_s=${(n / secs).toFixed(0)} hit_frac=${(hits / n).toFixed(2)}`);
}
