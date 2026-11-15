"""Derive a 512x512 SAM ViT-B image encoder from the 1024x1024 export.

The HuggingFace export used by the app (sam_vit_b_01ec64_encoder.onnx) has a
fixed 1024x1024 input and one baked 64x64 absolute positional embedding. The
rest of the graph derives every window/padding/reshape shape from Shape() at
runtime, so only that embedding has to be resized.

Usage:
    python make_encoder_512.py <encoder_fp32.onnx> <encoder_512.onnx>

`encoder_fp32.onnx` is the dequantized fp32 model (see python/ort_quantize.py
"dequantize" or quantize_dynamic's counterpart).  If you only have the shipped
int8/dynamic model, dequantize it first with onnxruntime.quantization.
"""
import sys
import numpy as np
import onnx
from onnx import numpy_helper

NEW = 512


def cubic(x, a=-0.75):
    ax = np.abs(x)
    w1 = (a + 2) * ax ** 3 - (a + 3) * ax ** 2 + 1.0
    w2 = a * ax ** 3 - 5 * a * ax ** 2 + 8 * a * ax - 4 * a
    return np.where(ax <= 1, w1, np.where(ax < 2, w2, 0.0))


def resize_bicubic(x, out_h, out_w):
    """(1,H,W,C) -> (1,out_h,out_w,C), matching F.interpolate(align_corners=False)."""
    def taps(src, dst):
        scale = src / dst
        idx = (np.arange(dst) + 0.5) * scale - 0.5
        base = np.floor(idx).astype(np.int64)
        frac = idx - base
        t = np.stack([base - 1, base, base + 1, base + 2], axis=1)
        w = cubic(frac[:, None] + np.array([1, 0, -1, -2])[None, :])
        return np.clip(t, 0, src - 1), w

    H, W, _ = x.shape[1:]
    ht, hw = taps(H, out_h)
    tmp = sum(x[:, ht[:, k], :, :] * hw[:, k][None, :, None, None] for k in range(4))
    wt, ww = taps(W, out_w)
    out = sum(tmp[:, :, wt[:, k], :] * ww[:, k][None, None, :, None] for k in range(4))
    return out.astype(np.float32)


def main(src, dst):
    m = onnx.load(src)
    g = m.graph

    dims = g.input[0].type.tensor_type.shape.dim
    dims[2].dim_value = NEW
    dims[3].dim_value = NEW

    hit = 0
    for i, t in enumerate(g.initializer):
        if t.name == "sam.image_encoder.pos_embed":
            a = numpy_helper.to_array(t)
            b = resize_bicubic(a, NEW // 16, NEW // 16)
            g.initializer[i].CopyFrom(numpy_helper.from_array(b, t.name))
            print(f"pos_embed {a.shape} -> {b.shape}")
            hit += 1
    if hit != 1:
        raise SystemExit("pos_embed initializer not found")

    # Drop stale 1024-era value_info, otherwise shape inference merges the old
    # dimensions back in and the graph no longer loads.
    del g.value_info[:]
    for o in g.output:
        d = o.type.tensor_type.shape.dim
        if len(d) == 4 and d[2].dim_value == 64:
            d[2].dim_value = NEW // 16
            d[3].dim_value = NEW // 16

    m = onnx.shape_inference.infer_shapes(m, strict_mode=True)
    onnx.save(m, dst)
    print("saved", dst)


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    main(sys.argv[1], sys.argv[2])
