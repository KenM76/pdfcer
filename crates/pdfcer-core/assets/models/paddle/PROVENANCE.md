# PROVENANCE — PaddleOCR (PP-OCRv4) model weights

Two ONNX model files that pdfcer redistributes in its portable folder as
`models/paddle/`, where `--ocr-engine paddle` finds them. Like the `ocrs`
weights, they are not a Cargo dependency, so the attribution is written by
hand in `about.hbs`.

- **Creator:** the PaddleOCR authors (PaddlePaddle,
  <https://github.com/PaddlePaddle/PaddleOCR>), who trained PP-OCRv4. The ONNX
  conversion is by the RapidOCR authors (<https://github.com/RapidAI/RapidOCR>).
- **Source:** the PyPI wheel `rapidocr_onnxruntime-1.4.4-py3-none-any.whl`
  (SHA-256 `971d7d5f223a7a808662229df1ef69893809d8457d834e6373d3854bc1782cbf`),
  files `rapidocr_onnxruntime/models/ch_PP-OCRv4_det_infer.onnx` and
  `ch_PP-OCRv4_rec_infer.onnx`. Retrieved 2026-09-28.
- **Licence: `Apache-2.0`**, for both upstream projects. It is declared in the
  `LICENSE` files of both repositories and in the wheel's metadata, and
  confirmed through GitHub's licence API on 2026-09-28. Neither repository
  has a `NOTICE` file. `LICENSE` in this directory is PaddleOCR's copy.
- **Changes made by pdfcer: none.** The files are byte-identical to the wheel's
  and were renamed only.
- **Languages:** Chinese (simplified) and English, with 6,623 characters. The
  dictionary is embedded in `rec.onnx` as ONNX metadata (`character`), so no
  `dict.txt` ships.

| Shipped as | Upstream filename | Bytes | SHA-256 |
|---|---|---:|---|
| `det.onnx` | `ch_PP-OCRv4_det_infer.onnx` | 4,745,517 | `d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9` |
| `rec.onnx` | `ch_PP-OCRv4_rec_infer.onnx` | 10,857,958 | `48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b` |

Total 15,603,475 bytes.

Shipping was ruled by the operator on 2026-09-28 ("Add it to the package").
To read another language, replace `rec.onnx` with that language's PP-OCR
recognition export and add its `dict.txt` if the model does not embed one.
