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

## Plan

1. Proof spike (in progress): load the q8 graphs in rten, OCR one synthetic
   line crop, compare to the reference, time it.
2. Then, if it passes: engine + decode loop, layout stage, CLI/disclosure,
   add-on zip (Pass 442.1 tooling). Estimate 4–6 Passes.
