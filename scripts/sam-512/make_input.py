"""Rebuild SAM inputs exactly the way the app does it (ai_processing.rs:2414-2441).

resize so the LONGEST side equals N (top-left anchored, no letterbox),
pad the remainder with zeros, RGB planar uint8 -> [1,3,N,N].
"""
import sys
import numpy as np
from PIL import Image

src, dst, size = sys.argv[1], sys.argv[2], int(sys.argv[3])
img = Image.open(src).convert("RGB")
w, h = img.size
scale = size / max(w, h)
nw, nh = max(1, round(w * scale)), max(1, round(h * scale))
img = img.resize((nw, nh), Image.BILINEAR)
arr = np.asarray(img, dtype=np.uint8)          # H W 3
out = np.zeros((1, 3, size, size), np.uint8)
out[0, :, :nh, :nw] = arr.transpose(2, 0, 1)
out.tofile(dst)
print(f"{src.split('/')[-1]}: {w}x{h} -> {nw}x{nh} in {size}  ({out.nbytes} bytes)")
