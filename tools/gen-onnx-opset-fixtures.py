#!/usr/bin/env python3
"""Regenerate fixtures/synthetic/onnx/ -- a tiny opset-7 ONNX model and its
opset-13 conversion, for `ocr::onnx_upgrade`.

The model reads the operator forms that changed before opset 13, in the order
PaddlePaddle's opset-7 PP-OCR recogniser exports use them:

    X[2,3,4,5] -> Slice(axes=[3], starts=[1], ends=[4])   attribute form
               -> BatchNormalization(spatial=1)           dropped in opset 9
               -> Softmax(axis=2)                         flatten semantics
               -> LogSoftmax()                            default axis 1
               -> Y[2,3,4,3]

Both softmaxes sit on a NON-last axis, where opset-7 and opset-13 semantics
differ; a last-axis one would pass whether or not the rewrite ran.

`opset7-as13.onnx` is the same model after `onnx.version_converter` -- the
reference the upgraded bytes must compute identically to.

Needs the `onnx` Python package (MIT): `pip install onnx`.
"""
import pathlib

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper, version_converter

OUT = pathlib.Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "onnx"

rng = np.random.default_rng(7)
C = 3
init = [
    numpy_helper.from_array(rng.uniform(0.5, 1.5, C).astype(np.float32), "scale"),
    numpy_helper.from_array(rng.uniform(-0.5, 0.5, C).astype(np.float32), "bias"),
    numpy_helper.from_array(rng.uniform(-0.2, 0.2, C).astype(np.float32), "mean"),
    numpy_helper.from_array(rng.uniform(0.5, 2.0, C).astype(np.float32), "var"),
]
nodes = [
    helper.make_node("Slice", ["X"], ["s"], axes=[3], starts=[1], ends=[4]),
    helper.make_node("BatchNormalization", ["s", "scale", "bias", "mean", "var"],
                     ["b"], spatial=1, epsilon=1e-5),
    helper.make_node("Softmax", ["b"], ["m"], axis=2),
    helper.make_node("LogSoftmax", ["m"], ["Y"]),
]
graph = helper.make_graph(
    nodes, "opset7_upgrade_fixture",
    # IR 3 (the opset-7 era) requires every initializer to be a graph input.
    [helper.make_tensor_value_info("X", TensorProto.FLOAT, [2, C, 4, 5])]
    + [helper.make_tensor_value_info(t.name, TensorProto.FLOAT, [C]) for t in init],
    [helper.make_tensor_value_info("Y", TensorProto.FLOAT, [2, C, 4, 3])],
    init,
)
model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 7)],
                          producer_name="pdfcer-fixture")
model.ir_version = 3
onnx.checker.check_model(model)

OUT.mkdir(parents=True, exist_ok=True)
onnx.save(model, OUT / "opset7.onnx")
onnx.save(version_converter.convert_version(model, 13), OUT / "opset7-as13.onnx")
print("wrote", OUT)
