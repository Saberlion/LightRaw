"""Quantize the 512x512 SAM encoder with ONNX Runtime.

Two recipes:

  static  QDQ int8, calibration on 8 synthetic views of a real 512 crop.
          `--ops MatMul,Conv` keeps element-wise/residual tensors in fp32,
          which is both more accurate and faster than quantizing everything.
  dynamic weight-only int8 (MatMul only - the Conv variant needs ConvInteger,
          which the on-device ORT build does not ship).

Peak RSS of the static path is ~11.5 GB on the 512 model (the 1024 model needs
> 24 GB and kills the WSL VM, see docs/HARMONYOS_PORTING.md §6.29-4).

Usage:
    python quantize_512.py static  <encoder_512_fp32.onnx> <out.onnx> <calib_512.bin> [MatMul,Conv]
    python quantize_512.py dynamic <encoder_512_fp32.onnx> <out.onnx> [MatMul]
"""
import sys
import numpy as np
import onnxruntime as ort
from onnxruntime.quantization import (
    quantize_static, quantize_dynamic, CalibrationDataReader,
    QuantType, QuantFormat,
)

SZ = 512


class Views(CalibrationDataReader):
    """One real 512x512 uint8 crop plus flips and four quadrants."""

    def __init__(self, path):
        img = np.fromfile(path, np.uint8)[: 3 * SZ * SZ].reshape(1, 3, SZ, SZ)
        rng = np.random.default_rng(7)
        views = [img, img[:, :, :, ::-1].copy(), img[:, :, ::-1, :].copy()]
        for _ in range(5):
            y, x = rng.integers(0, SZ // 2, 2)
            views.append(img[:, :, y:y + SZ // 2, x:x + SZ // 2].copy())
        self.views = views
        self.i = 0

    def get_next(self):
        if self.i >= len(self.views):
            return None
        a = self.views[self.i]
        if a.shape[2:] != (SZ, SZ):
            pad = np.zeros((1, 3, SZ, SZ), np.uint8)
            pad[:, :, :a.shape[2], :a.shape[3]] = a
            a = pad
        self.i += 1
        print(f"  calibrating {self.i}/{len(self.views)}", flush=True)
        return {"input": a}


def main():
    if len(sys.argv) < 5:
        raise SystemExit(__doc__)
    mode, src, dst, calib = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
    ops = sys.argv[5].split(",") if len(sys.argv) > 5 and sys.argv[5] else None

    if mode == "static":
        quantize_static(
            src, dst, Views(calib), quant_format=QuantFormat.QDQ,
            activation_type=QuantType.QInt8, weight_type=QuantType.QInt8,
            per_channel=True, op_types_to_quantize=ops,
        )
    elif mode == "dynamic":
        quantize_dynamic(src, dst, weight_type=QuantType.QInt8,
                         per_channel=True, op_types_to_quantize=ops)
    else:
        raise SystemExit(f"unknown mode {mode}")
    print("done")

    s = ort.InferenceSession(dst, providers=["CPUExecutionProvider"])
    print("providers", ort.get_available_providers(), "inputs", [i.name for i in s.get_inputs()])


if __name__ == "__main__":
    main()
