"""Append a 2x nearest upsample to the 512 encoder.

Why: the mask decoder must stay at 64x64 (it degrades badly at 32x32), so the
512 encoder is exported with a 64x64 embedding output. The app then keeps the
shipped decoder and `mask_input` untouched - only SAM_INPUT_SIZE changes.

nearest / bilinear / bicubic were measured equivalent (IoU 0.764 / 0.761 / 0.755
vs the 1024 pipeline), so nearest is used: no runtime cost, exact repeat.

Usage:
    python make_encoder_512_up.py <encoder_512.onnx> <encoder_512_up.onnx>
"""
import sys
import numpy as np
import onnx
from onnx import helper, numpy_helper, TensorProto

SRC, DST = sys.argv[1], sys.argv[2]

m = onnx.load(SRC)
g = m.graph
del g.value_info[:]

old = g.output[0]
old_name = old.name
inner = "embedding32"
new_out = "output"

nodes = []
for n in g.node:
    for i, o in enumerate(n.output):
        if o == old_name:
            n.output[i] = inner
nodes.append(helper.make_node(
    "Resize", [inner, "", "up_scales"], [new_out], name="embedding_upsample",
    mode="nearest", coordinate_transformation_mode="asymmetric", nearest_mode="floor"))
g.node.extend(nodes)
g.initializer.append(numpy_helper.from_array(np.array([1, 1, 2, 2], np.float32), "up_scales"))

old.name = new_out
old.type.tensor_type.shape.dim[2].dim_value = 64
old.type.tensor_type.shape.dim[3].dim_value = 64

m = onnx.shape_inference.infer_shapes(m, strict_mode=True)
onnx.save(m, DST)

check = onnx.load(DST)
out = check.graph.output[0]
print("saved", DST, "output", out.name, [d.dim_value for d in out.type.tensor_type.shape.dim])
