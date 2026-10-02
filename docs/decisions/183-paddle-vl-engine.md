# Decision 183 — PaddleOCR-VL engine (`paddle-vl`), first rung

- **Status:** DECIDED; first engine rung of `Pass 442.2`.
- **Authored by:** `pdfcer-engineer`.
- **Builds on:** decision 182 (add-on folders), `docs/paddleocr-vl-feasibility.md`
  (licence, spike measurements, the three graph rewrites).

## 1. What ships

| Piece | Where | Feature |
|---|---|---|
| Engine `PaddleVlEngine` | `crates/pdfcer-core/src/ocr/engine_paddle_vl.rs` | `ocr-vl`, default on (core and CLI) |
| Preprocessing, prompt, line placement | `ocr/vl_pre.rs` | always |
| Greedy decode loop | `ocr/vl_decode.rs` | always |
| `tokenizer.json` reader (BPE) | `ocr/vl_tokenizer.rs`, `ocr/json_lite.rs` (`#[doc(hidden)]`) | always |
| Add-on builder | `tools/build-paddle-vl-addon.py` | — |
| CLI | `pdfcer ocr --ocr-model <name>` / `--ocr-engine paddle-vl`; `ocr-models` lists it | `ocr-vl` |

The model files are **never** in the repository or the portable package. The
operator builds the add-on folder from a local copy of
`onnx-community/PaddleOCR-VL-1.5-ONNX` (Apache-2.0, model card front matter
`license: apache-2.0`; the builder refuses any other value). The builder
downloads nothing. Its manifest names `engine = paddle-vl`,
`licence = Apache-2.0` and a `sha256` line per file.

## 2. No `tokenizers` crate

`tokenizers` (HF) with `default-features = false` still pulls `rayon`,
`rayon-cond`, `esaxx-rs`, `getrandom` 0.3 (no wasm32 without a cfg flag) and
`rand` 0.9. That breaks the no-threads-in-core rule and the wasm32 build. The
model's tokenizer is a plain byte-fallback BPE with a `Replace` normaliser and
no pre-tokenizer, so `vl_tokenizer` implements exactly that subset and
**refuses** anything outside it (a pre-tokenizer, dropout, regex patterns,
non-BPE models, `ignore_merges`, prefix/suffix, added-token stripping). A
refusal is an error at load, never a silent mis-tokenisation. Net new crates:
**zero**.

## 3. Reading model: one region per page

- This rung has no layout stage. The page's ink bounding box, plus a 16 px
  margin, is cropped and read as **one region**. The layout model
  (PP-DocLayoutV2) is the next rung.
- The model returns text without coordinates. Line boxes are **inferred**:
  - when the number of non-empty text lines equals the number of inked row
    bands, each line goes on its own band (`LinePlacement::Bands`);
  - otherwise the lines split the ink box evenly (`Even`).
- One `RecognizedWord` per **line**. Words are not individually placed.
- Confidence is the mean softmax probability of the chosen tokens. It is the
  same for every line of the region.
- Rule 4: the CLI prints that the layer is region-aligned, one box per line,
  and inferred. It also prints the placement mode, the token count, and a
  warning when decoding hit the token ceiling.

## 4. Ceilings (ARCHITECTURE §10)

| Bound | Value | Where |
|---|---|---|
| Input image pixels | 2^26 | `vl_pre::MAX_INPUT_PIXELS` |
| Aspect ratio | 200 | `vl_pre::MAX_ASPECT` |
| Model pixels | 112,896 – 1,003,520 (`smart_resize`, HF processor) | `vl_pre` |
| New tokens per region | 2,048 by default, hard cap 8,192 | `vl_decode` |
| `tokenizer.json` size | 64 MiB | `vl_tokenizer::MAX_TOKENIZER_BYTES` |
| Vocabulary | 2^20 entries | `vl_tokenizer::MAX_VOCAB` |
| Text per `encode` | 16 KiB | `vl_tokenizer::MAX_ENCODE_BYTES` |
| JSON nesting | 64 | `json_lite::MAX_DEPTH` |

The decoder runs at most `max_new_tokens + 1` steps. The KV cache grows by
one position per step and is bounded by that count.

## 5. Matching the reference processor

- `smart_resize` rounds as Python's `round` does (ties to even).
- Resampling is PIL bicubic (a = −0.5), with support widened when
  downsampling: horizontal pass first, then vertical, each clamped to u8.
- Patches are flattened in `(gh, gw, c, py, px)` order.
- Unit-test values were computed with Python/PIL. They are not hand-derived.

## 6. The graphs

- The vision encoder is rewritten offline by the builder; the three rewrites
  are in the feasibility doc.
- The decoder and the embedding graph ship as exported (`decoder_q8.onnx`,
  renamed).
- At load the engine checks node names, requires fixed cache dimensions
  and pairs every `present.*` output with its `past_key_values.*` input. An
  export of a different shape is refused with `PaddleVlError::Interface`.
