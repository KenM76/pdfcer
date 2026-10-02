# PaddleOCR-VL as an add-on OCR engine — feasibility (Pass 442.2)

Research of 2026-10-02 for Ken's request to offer the PaddleOCR vision-language
model as a downloadable add-on folder (decision 182 layout), not in the
portable package. SOURCED = checked against the cited page; INFERRED = not
measured.

## Licence — clear

- Weights, tokenizer, config, processor code: Apache-2.0, no use restriction,
  revenue cap or acceptable-use clause (SOURCED,
  huggingface.co/PaddlePaddle/PaddleOCR-VL `LICENSE`; versions 1.5 and 1.6
  likewise).
- Components: ERNIE-4.5-0.3B decoder Apache-2.0; vision encoder initialised
  from Keye-VL (Apache-2.0); layout model PP-DocLayoutV2 inside the same repo.
- Community ONNX exports `onnx-community/PaddleOCR-VL-1.5-ONNX` and
  `hb-dev/PaddleOCR-VL-1.6-ONNX`: Apache-2.0.

## Model

- ~0.9B parameters; BF16 safetensors 1.92 GB. Vision: 27 layers, patch 14,
  dynamic resolution. Decoder: 18 layers, GQA 16/2, 3D M-RoPE, vocab 103,424.
- The onnx-community export is three graphs (vision encoder, embed_tokens,
  decoder with KV cache); q8 ≈ 1.2 GB.
- Full-page use also needs PP-DocLayoutV2 (212 MB, Paddle format — needs
  paddle2onnx).

## Runtime

- rten 0.24 (pdfcer's pin) supports every op in the onnx-community graphs
  (SOURCED by diffing the graphs' op set against rten's op list). Attention
  and RoPE are exported as plain MatMul/Softmax/Sin/Cos.
- `rten-generate` takes token ids only; this decoder takes `inputs_embeds`, so
  pdfcer needs its own greedy decode loop.
- Risk: the decoder graph has no `position_ids` input; image-token 3D
  positions may collapse to 1D (INFERRED, unverified).
- Tokenizer: `tokenizers` crate (Apache-2.0) with the pure-Rust regex
  backend, not onig.
- Alternative: candle ships a PaddleOCR-VL implementation reading the
  official safetensors (MIT OR Apache-2.0), at the cost of a second ML runtime.
  `ort` links native onnxruntime — out for the pure-Rust/wasm story.

## Product fit

- Output is markdown per layout region, not word boxes: a text layer from it
  is region-aligned, not word-aligned. Must be disclosed (CLAUDE.md rule 4).
- CPU speed: ~72 s/page measured by the hb-dev card on onnxruntime int8;
  expect 1.5–3 min/page on rten (INFERRED).

## Proof spike — PASS (MEASURED, 2026-10-02)

Spike code (not committed; local only): `D:\Dev\pdfTests\vl-spike\`
(`src/main.rs` greedy decode, `src/bin/dump.rs` + `py/dump_cmp.py` bisection,
`py/reduce_range.py` graph rewrite).

- `onnx-community/PaddleOCR-VL-1.5-ONNX` q8 on rten 0.24 reads two synthetic
  images exactly (10 and 50 tokens); onnxruntime 1.20 gives identical token ids.
- i9-10900KF (AVX2, no VNNI), release: load 1.7 s, vision encode 2.5 s for
  165 image tokens, prefill 0.5 s, 70–85 ms/token (onnxruntime 32–70 ms).
- Add-on folder ≈ 1.24 GB: `vision_encoder_q8rr.onnx`, `decoder_q8.onnx`,
  `embedding.onnx` + `.onnx.data` (fp32, 424 MB), `tokenizer.json`.

Graph rewrites the vision encoder needs, done offline (ship the rewritten
file):

1. rten rejects u8 `MatMulInteger` weights (u8×i8 only) → shift to i8.
2. rten-gemm's AVX2 kernel saturates unless weights are within ±63 →
   requantise each `MatMulInteger` weight to symmetric int8 ±63, rescale in
   float (`reduce_range.py`). Without this the text is hallucinated.
3. rten `ConvInteger` is wrong (cosine 0.93 vs onnxruntime exact) → replace
   the patch-embedding conv with float `Conv` on dequantised weights.

Residual: vision cosine 0.975 vs the original, the same under onnxruntime on
the rewritten graph, so the loss is requantisation, not rten; per-channel
requantisation from the fp32 graph should recover most of it (INFERRED). The
int8 decoder runs unmodified but 0.08% of its weights exceed ±63
(saturation risk on long pages, INFERRED). The M-RoPE position risk did not
show on single lines.

Constants (no config read needed): prompt `<|begin_of_sentence|>User:
<|IMAGE_START|>` + image tokens (id 100295) + `<|IMAGE_END|>OCR:\nAssistant:\n`,
EOS id 2; patch 14, merge 2, pixels 112,896–1,003,520, mean/std 0.5. The
processor config file is `processor_config.json`. `tokenizers` 0.22 builds
pure-Rust with `default-features = false, features = ["fancy-regex"]`.

Upstream reports worth filing (rten): `ConvInteger` defect; u8×u8
`MatMulInteger`.

## Plan

Recommendation: rten (already shipped, pure Rust). Engine Passes, each
behind the decision-182 add-on folder:

1. Engine + decode loop + preprocessing in `pdfcer_core::ocr`, line/region
   crops; tool to build the add-on folder (rewrites included).
2. Layout stage (PP-DocLayoutV2 via paddle2onnx) and region text layer, with
   region-level placement disclosed.
3. CLI/disclosure, add-on zip release asset.
4. Optional: per-channel requantisation and decoder reduce-range.
