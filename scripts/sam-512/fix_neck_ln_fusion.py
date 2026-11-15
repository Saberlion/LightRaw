"""Block omg's (broken) LayerNorm fusion in the SAM neck.

omg fuses the neck's explicit LayerNorm
    ReduceMean -> Sub -> Pow -> ReduceMean -> Add(eps) -> Sqrt -> Div -> Mul -> Add
into a single op and computes it incorrectly: the converted OM then disagrees with
the ONNX by corr 0.29. Exposing any intermediate as a graph output, or inserting
a no-op `Mul(x, 1.0)` between Sqrt and Div, disables the fusion and restores
corr 1.0000 / 69.97 dB.

This script inserts the identity multiply in every `/neck/` Div whose denominator
is a Sqrt output. Cost at runtime is nil (one elementwise multiply per pixel).

Usage:
    python fix_neck_ln_fusion.py <in.onnx> <out.onnx>
"""
import sys
import numpy as np
import onnx
from onnx import helper, numpy_helper

m = onnx.load(sys.argv[1])
m = onnx.shape_inference.infer_shapes(m)
g = m.graph

sqrt_out = {n.output[0] for n in g.node if n.op_type == "Sqrt" and "/neck/" in n.name}
new_nodes, blocks = [], 0
for n in g.node:
    if n.op_type == "Div" and n.input[1] in sqrt_out and "/neck/" in n.name:
        blocked = n.input[1] + "_lnblock"
        new_nodes.append(helper.make_node("Mul", [n.input[1], "ln_one"], [blocked], name=n.name + "_lnblock"))
        n.input[1] = blocked
        blocks += 1
    new_nodes.append(n)
del g.node[:]
g.node.extend(new_nodes)
if not any(i.name == "ln_one" for i in g.initializer):
    g.initializer.append(numpy_helper.from_array(np.array(1.0, np.float32), "ln_one"))

m = onnx.shape_inference.infer_shapes(m, strict_mode=True)
onnx.save(m, sys.argv[2])
print(f"inserted {blocks} LayerNorm fusion blockers -> {sys.argv[2]}")
if blocks == 0:
    raise SystemExit("no neck LayerNorm Div found - check the graph")
