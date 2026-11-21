"""Regenerate ONLY the tablet assets at 16:9 1920x1080 (AGC tablet slot).

Imports the shared generator so layout/copy stays single-sourced:
  - assets/tablet/promo-1..4.jpg          (framed promo, 1920x1080 canvas)
  - assets/plain/tablet169/tablet-1..4.jpg (pure screenshot letterboxed)
"""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import make_assets as M

for i, (n, h, s, b) in enumerate(M.LAND_TAB, 1):
    print(M.land_promo("tablet", i, n, h, s, b, 1920, 1080))
    print(M.tablet_plain169(i, n))
print("tablet 16:9 regeneration done")
