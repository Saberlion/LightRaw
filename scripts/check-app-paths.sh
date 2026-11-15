#!/usr/bin/env bash
# check-app-paths.sh - enforce the AGENTS.md OHOS path rule mechanically:
#
#   "Never call app.path().app_data_dir() / app_cache_dir() / app_config_dir()
#    directly in port code - use crate::app_paths::*."
#
# On OHOS the tauri fork resolves these with desktop XDG semantics, i.e. onto
# the FUSE user storage where writes fail with EPERM (see docs/HARMONYOS_PORTING.md
# 6.13). The upstream module split of 2026-11-15 reintroduced a direct call in
# library/thumbnails.rs and silently stalled the whole thumbnail worker on
# device (D-TA-03) - this gate exists so that class of regression cannot land
# unnoticed again.
#
# Usage: scripts/check-app-paths.sh   (from anywhere; exits 1 on violations)

set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
src="$repo/src-tauri/src"

# app_paths.rs is the adaptation layer and legitimately calls the tauri
# resolver on the other platforms; everything else must go through it.
violations=$(grep -rnE '\.(app_data_dir|app_cache_dir|app_config_dir|app_local_data_dir)\(\)' \
  "$src" --include='*.rs' | grep -v '/app_paths.rs:' || true)

if [ -n "$violations" ]; then
  echo "ERROR: direct tauri path-resolver calls found (AGENTS.md OHOS rule)."
  echo "Use crate::app_paths::{app_data_dir, app_cache_dir, app_config_dir} instead:"
  echo
  echo "$violations"
  exit 1
fi

echo "check-app-paths: OK (no direct .path().app_*_dir() calls outside app_paths.rs)"
