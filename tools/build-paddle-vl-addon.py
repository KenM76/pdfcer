#!/usr/bin/env python3
"""Build a PaddleOCR-VL OCR add-on folder (decision 182, engine decision 183).

Input: a local copy of the onnx-community/PaddleOCR-VL-1.5-ONNX repository
(Apache-2.0), already on disk. This script downloads nothing.

    SOURCE/README.md                    licence front matter (apache-2.0)
    SOURCE/config.json                  token ids, hidden size
    SOURCE/processor_config.json        image-processor constants
    SOURCE/tokenizer.json
    SOURCE/onnx/vision_encoder_q8.onnx
    SOURCE/onnx/decoder_q8.onnx
    SOURCE/onnx/embedding.onnx (+ embedding.onnx.data)

Optional: --layout DIR, a local copy of PaddlePaddle/PP-DocLayoutV3_safetensors's
ONNX export (Apache-2.0): DIR/README.md and DIR/inference.onnx. It is copied
as layout.onnx, which `pdfcer ocr --layout` needs; without it the add-on
reads whole pages only.

Output: OUT/ holding the five files the engine reads (six with --layout) plus
pdfcer-ocr-model.txt with a sha256 line per file. Select it with
`pdfcer ocr --ocr-model NAME --ocr-folder PARENT_OF_OUT`.

The vision encoder is rewritten for rten 0.24, which the q8 export does not
run correctly as shipped:
  1. MatMulInteger static uint8 weights and zero points become int8 by
     subtracting 128 from both (exact: b - zb is unchanged); rten's
     MatMulInteger takes u8 x i8 only.
  2. Each MatMulInteger weight is requantised to symmetric int8 in [-63, 63]
     with the int32 output rescaled in float: rten-gemm's AVX2 u8 x i8 kernel
     (vpmaddubsw) saturates at i16 otherwise (the offline form of
     quantising with reduce_range).
  3. The ConvInteger patch embedding becomes a float Conv on dequantised
     weights: rten 0.24's ConvInteger result is wrong for this graph.
The residual against the float reference is in docs/paddleocr-vl-feasibility.md.

Requires: onnx, numpy.
"""

import argparse
import hashlib
import json
import shutil
import sys
from pathlib import Path

ENGINE = "paddle-vl"
MANIFEST = "pdfcer-ocr-model.txt"
# Must match crates/pdfcer-core/src/ocr/engine_paddle_vl.rs REQUIRED_FILES.
VISION, DECODER, EMBED, EMBED_DATA, TOKENIZER = (
    "vision_encoder.onnx",
    "decoder.onnx",
    "embedding.onnx",
    "embedding.onnx.data",
    "tokenizer.json",
)
# Must match crates/pdfcer-core/src/ocr/vl_pre.rs and the engine's prompt.
PROCESSOR = {
    "patch_size": 14,
    "merge_size": 2,
    "min_pixels": 112896,
    "max_pixels": 1003520,
    "image_mean": [0.5, 0.5, 0.5],
    "image_std": [0.5, 0.5, 0.5],
    "resample": 3,
    "temporal_patch_size": 1,
}
LAYOUT = "layout.onnx"  # crates/pdfcer-core/src/ocr/engine_layout.rs LAYOUT_MODEL
CONFIG = {"eos_token_id": 2, "image_token_id": 100295}
TOKENS = {
    "</s>": 2,
    "<|IMAGE_PLACEHOLDER|>": 100295,
    "<|begin_of_sentence|>": 100273,
    "<|IMAGE_START|>": 101305,
    "<|IMAGE_END|>": 101306,
}


def fail(msg):
    sys.exit(f"build-paddle-vl-addon: {msg}")


def source_files(src):
    files = {
        "readme": src / "README.md",
        "config": src / "config.json",
        "processor": src / "processor_config.json",
        "tokenizer": src / "tokenizer.json",
        "vision": src / "onnx" / "vision_encoder_q8.onnx",
        "decoder": src / "onnx" / "decoder_q8.onnx",
        "embed": src / "onnx" / "embedding.onnx",
        "embed_data": src / "onnx" / "embedding.onnx.data",
    }
    missing = [str(p) for p in files.values() if not p.is_file()]
    if missing:
        fail("missing source files:\n  " + "\n  ".join(missing))
    return files


def check_licence(readme):
    text = readme.read_text(encoding="utf-8")
    head = text.split("---")[1] if text.startswith("---") else ""
    if "license: apache-2.0" not in head:
        fail(f"{readme} front matter does not say `license: apache-2.0`; refusing")


def check_constants(files):
    proc = json.loads(files["processor"].read_text(encoding="utf-8"))
    proc = proc.get("image_processor", proc)
    cfg = json.loads(files["config"].read_text(encoding="utf-8"))
    tok = json.loads(files["tokenizer"].read_text(encoding="utf-8"))
    added = {t["content"]: t["id"] for t in tok.get("added_tokens", [])}
    wrong = [f"processor {k} = {proc.get(k)!r}, expected {v!r}"
             for k, v in PROCESSOR.items() if proc.get(k) != v]
    wrong += [f"config {k} = {cfg.get(k)!r}, expected {v!r}"
              for k, v in CONFIG.items() if cfg.get(k) != v]
    wrong += [f"tokenizer {k} = {added.get(k)!r}, expected {v!r}"
              for k, v in TOKENS.items() if added.get(k) != v]
    if wrong:
        fail("the source is not the export this engine drives:\n  " + "\n  ".join(wrong))


def u8_weights_to_s8(graph, nh, onnx):
    """Rewrite 1: uint8 MatMulInteger weights and zero points to int8."""
    import numpy as np

    init = {i.name: i for i in graph.initializer}
    done = set()
    for n in graph.node:
        if n.op_type != "MatMulInteger":
            continue
        for k in (1, 3):
            name = n.input[k] if len(n.input) > k else ""
            if name in done or name not in init:
                continue
            if init[name].data_type != onnx.TensorProto.UINT8:
                continue
            a = nh.to_array(init[name]).astype(np.int16) - 128
            init[name].CopyFrom(nh.from_array(a.astype(np.int8), name))
            done.add(name)
    for n in graph.node:
        for k, x in enumerate(n.input):
            if x in done and not (n.op_type == "MatMulInteger" and k in (1, 3)):
                fail(f"{x} also feeds {n.op_type} input {k}; the u8->i8 rewrite is unsafe")
    return len(done)


def requantise_63(graph, nh, helper, onnx):
    """Rewrite 2: MatMulInteger weights to symmetric int8 in [-63, 63]."""
    import numpy as np

    init = {i.name: i for i in graph.initializer}
    consumers = {}
    for n in graph.node:
        for x in n.input:
            consumers.setdefault(x, []).append(n)
    nodes, count = [], 0
    for n in graph.node:
        nodes.append(n)
        if n.op_type != "MatMulInteger" or n.input[1] not in init:
            continue
        b = nh.to_array(init[n.input[1]]).astype(np.int32)
        has_zp = len(n.input) > 3 and n.input[3]
        zb = nh.to_array(init[n.input[3]]).astype(np.int32) if has_zp else 0
        d = b - zb
        s = max(float(np.abs(d).max()) / 63.0, 1.0)
        q = np.clip(np.round(d / s), -63, 63).astype(np.int8)
        wn, zn = n.input[1] + "_rr", n.input[1] + "_rr_zp"
        graph.initializer.extend([nh.from_array(q, wn), nh.from_array(np.array(0, np.int8), zn)])
        ins = list(n.input)
        ins[1] = wn
        ins = (ins + [""] * 4)[:4]
        ins[3] = zn
        del n.input[:]
        n.input.extend(ins)
        if s != 1.0:
            out = n.output[0]
            n.output[0] = out + "_i32"
            sn = out + "_rr_scale"
            graph.initializer.append(nh.from_array(np.array(s, np.float32), sn))
            nodes.append(helper.make_node("Cast", [out + "_i32"], [out + "_f"], to=onnx.TensorProto.FLOAT))
            nodes.append(helper.make_node("Mul", [out + "_f", sn], [out]))
            for c in consumers.get(out, []):
                if c.op_type != "Cast":
                    fail(f"{out} feeds {c.op_type}, not Cast; the rescale would change its type")
        count += 1
    del graph.node[:]
    graph.node.extend(nodes)
    return count


def conv_integer_to_float(graph, nh, helper):
    """Rewrite 3: ConvInteger -> Cast -> Mul(scale) becomes a float Conv."""
    import numpy as np

    init = {i.name: i for i in graph.initializer}
    by_out = {o: n for n in graph.node for o in n.output}
    count = 0
    for n in [n for n in graph.node if n.op_type == "ConvInteger"]:
        w = nh.to_array(init[n.input[1]]).astype(np.float32)
        has_zp = len(n.input) > 3 and n.input[3]
        zw = nh.to_array(init[n.input[3]]).astype(np.float32) if has_zp else 0.0
        cast = [c for c in graph.node if n.output[0] in c.input]
        mul = [c for c in graph.node if cast and cast[0].output[0] in c.input]
        if len(cast) != 1 or cast[0].op_type != "Cast" or len(mul) != 1 or mul[0].op_type != "Mul":
            fail(f"ConvInteger {n.name} is not followed by Cast then Mul")
        cast, mul = cast[0], mul[0]
        scale_node = by_out[mul.input[1]]
        w_scale = nh.to_array(init[scale_node.input[1]]).astype(np.float32)
        x_float = by_out[n.input[0]].input[0]
        wf_name = n.input[1] + "_f32"
        graph.initializer.append(nh.from_array(((w - zw) * w_scale).astype(np.float32), wf_name))
        attrs = {a.name: helper.get_attribute_value(a) for a in n.attribute}
        conv = helper.make_node("Conv", [x_float, wf_name], [mul.output[0]], **attrs)
        at = list(graph.node).index(n)
        for dead in (n, cast, mul):
            graph.node.remove(dead)
        graph.node.insert(at, conv)
        count += 1
    return count


def rewrite_vision(src, dst):
    import onnx
    from onnx import helper
    from onnx import numpy_helper as nh

    model = onnx.load(str(src))
    g = model.graph
    s8 = u8_weights_to_s8(g, nh, onnx)
    rr = requantise_63(g, nh, helper, onnx)
    conv = conv_integer_to_float(g, nh, helper)
    used = {x for n in g.node for x in n.input}
    keep = [i for i in g.initializer if i.name in used]
    del g.initializer[:]
    g.initializer.extend(keep)
    onnx.checker.check_model(model)
    onnx.save(model, str(dst))
    print(f"vision encoder: {s8} u8->i8 initialisers, {rr} MatMulInteger requantised, "
          f"{conv} ConvInteger -> Conv")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def write_manifest(out, name, version):
    lines = [
        "# PaddleOCR-VL add-on, built by tools/build-paddle-vl-addon.py",
        f"name = {name}",
        f"engine = {ENGINE}",
        'label = "PaddleOCR-VL 1.5 (q8, rewritten for rten)"',
        "languages = mul",
        f"version = {version}",
        "licence = Apache-2.0",
    ]
    names = [VISION, DECODER, EMBED, EMBED_DATA, TOKENIZER]
    if (out / LAYOUT).is_file():
        names.append(LAYOUT)
    lines += [f"sha256 = {f} {sha256(out / f)}" for f in names]
    (out / MANIFEST).write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("source", type=Path, help="local onnx-community/PaddleOCR-VL-1.5-ONNX folder")
    ap.add_argument("out", type=Path, help="add-on folder to create (must not exist or be empty)")
    ap.add_argument("--name", default="paddle-vl", help="add-on name (default: paddle-vl)")
    ap.add_argument("--version", default="1.5-q8rr", help="add-on version string")
    ap.add_argument("--layout", type=Path, metavar="DIR",
                    help="local PP-DocLayoutV3 ONNX folder (README.md, inference.onnx)")
    args = ap.parse_args()
    files = source_files(args.source)
    layout = None
    if args.layout:
        layout = args.layout / "inference.onnx"
        if not layout.is_file():
            fail(f"missing {layout}")
        check_licence(args.layout / "README.md")
    check_licence(files["readme"])
    check_constants(files)
    out = args.out
    if out.exists() and any(out.iterdir()):
        fail(f"{out} exists and is not empty")
    out.mkdir(parents=True, exist_ok=True)
    rewrite_vision(files["vision"], out / VISION)
    for key, dest in (("decoder", DECODER), ("embed", EMBED), ("embed_data", EMBED_DATA),
                      ("tokenizer", TOKENIZER)):
        shutil.copyfile(files[key], out / dest)
    if layout:
        shutil.copyfile(layout, out / LAYOUT)
    write_manifest(out, args.name, args.version)
    print(f"wrote {out / MANIFEST}")


if __name__ == "__main__":
    main()
