"""Derive a 512x512 SAM ViT-B mask decoder from the 1024x1024 export.

Changes vs the 1024 model:
  * image_embeddings  1x256x64x64  -> 1x256x32x32
  * mask_input        1x1x256x256  -> 1x1x128x128
  * four baked shape constants 64 -> 32
  * dense positional encoding (1,1,256,64,64) -> (1,1,256,32,32)

The positional encoding is exact, not interpolated: SAM samples it on a
half-pixel grid, so the 32x32 grid is the 64x64 grid at odd indices
(x=(j+0.5)/32 == (2j+1)/64).

The final mask Resize keeps its baked 1024x1024 target, so `masks` still comes
out at the original image size and the caller does not change.

Usage:
    python make_decoder_512.py <decoder.onnx> <decoder_512.onnx>
"""
import sys
import numpy as np
import onnx
from onnx import numpy_helper

SHAPES = {
    "image_embeddings": [1, 256, 32, 32],
    "mask_input": [1, 1, 128, 128],
}

# point_coords / point_labels keep their original symbolic `num_points` axis -
# the app feeds 1 point (or 2 box corners) into these, a literal 0 would make
# the model reject every real prompt.

PATCH = {
    "/Constant_27_output_0": [256, 32, 32],
    "/Constant_35_output_0": [256, 32, 32],
    "/Constant_29_output_0": [-1, 256, 32, 32],
    "/Constant_37_output_0": [-1, 256, 32, 32],
}

# Baked `input_image_size`: the prompt encoder normalizes point coordinates as
# (p + 0.5) / input_image_size and point labels as (label + 1) / input_image_size.
# Leaving it at 1024 maps every 512-grid prompt into the top-left quadrant.
SCALARS = {"ortshared_13_1_0": 512.0}


def main(src, dst):
    m = onnx.load(src)
    g = m.graph
    del g.value_info[:]

    for i in g.input:
        if i.name in SHAPES:
            for d, v in zip(i.type.tensor_type.shape.dim, SHAPES[i.name]):
                d.dim_value = v

    seen = 0
    for j, t in enumerate(g.initializer):
        if t.name in PATCH:
            g.initializer[j].CopyFrom(
                numpy_helper.from_array(np.array(PATCH[t.name], np.int64), t.name))
            seen += 1
    for n in g.node:
        if n.op_type == "Constant" and n.output[0] in PATCH:
            for a in n.attribute:
                if a.name == "value":
                    a.t.CopyFrom(numpy_helper.from_array(
                        np.array(PATCH[n.output[0]], np.int64), a.t.name))
                    seen += 1
    if seen != len(PATCH):
        raise SystemExit(f"patched {seen}/{len(PATCH)} shape constants")

    pe = 0
    for j, t in enumerate(g.initializer):
        if t.name == "/Unsqueeze_4_output_0":
            a = numpy_helper.to_array(t)
            if a.shape != (1, 1, 256, 64, 64):
                raise SystemExit(f"unexpected positional encoding shape {a.shape}")
            # SAM samples the PE at (i+0.5)/size, so the 32-grid point (j+0.5)/32
            # sits exactly between 64-grid samples 2j and 2j+1 -> bilinear midpoint.
            b = (a[:, :, :, 0::2, 0::2] + a[:, :, :, 0::2, 1::2]
                 + a[:, :, :, 1::2, 0::2] + a[:, :, :, 1::2, 1::2]) * 0.25
            g.initializer[j].CopyFrom(numpy_helper.from_array(b.astype(np.float32), t.name))
            pe += 1
    if pe != 1:
        raise SystemExit("dense positional encoding not found")

    sc = 0
    for j, t in enumerate(g.initializer):
        if t.name in SCALARS:
            old = numpy_helper.to_array(t)
            new = np.array(SCALARS[t.name], dtype=old.dtype)
            g.initializer[j].CopyFrom(numpy_helper.from_array(new, t.name))
            print(f"input_image_size {old.ravel().tolist()} -> {new.ravel().tolist()}")
            sc += 1
    if sc != len(SCALARS):
        raise SystemExit(f"patched {sc}/{len(SCALARS)} scale constants")

    m = onnx.shape_inference.infer_shapes(m, strict_mode=True)
    onnx.save(m, dst)
    print("saved", dst)


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    main(sys.argv[1], sys.argv[2])
