# Build the on-device CPU benchmark (ort_probe) for OpenHarmony aarch64.
#
# Uses the ONNX Runtime shared library that the app already builds for OHOS
# (src-tauri/libs/ohos/arm64-v8a/libonnxruntime.so) and the OHOS NDK clang.
#
# Output: scripts/ort-probe/ort_probe
#
# Deploy (device):
#   mkdir -p /data/local/tmp/ort
#   hdc file send scripts/ort-probe/ort_probe            /data/local/tmp/ort/ort_probe
#   hdc file send src-tauri/libs/ohos/arm64-v8a/libonnxruntime.so /data/local/tmp/ort/libonnxruntime.so
#   hdc shell "cd /data/local/tmp/ort && cp libonnxruntime.so libonnxruntime.so.1"
#   # libc++_shared.so comes from the OHOS build tree:
#   #   src-tauri/gen/ohos/entry/build/default/intermediates/libs/default/arm64-v8a/libc++_shared.so
#   hdc shell "chmod 777 /data/local/tmp/ort/*"
# Run:
#   hdc shell "cd /data/local/tmp/ort && LD_LIBRARY_PATH=/data/local/tmp/ort \
#       ./ort_probe /data/local/tmp/onnx/model.onnx 5 6 /data/local/tmp/in.bin"
param(
  [string]$Sdk = "C:\Program Files\Huawei\DevEco Studio\sdk\default\openharmony\native",
  [string]$Repo = (Resolve-Path "$PSScriptRoot\..\..").Path
)

$clang   = Join-Path $Sdk "llvm\bin\clang.exe"
$sysroot = Join-Path $Sdk "sysroot"
$ortInc  = Join-Path $Repo "src-tauri\vendor\onnxruntime\include"
$ortLib  = Join-Path $Repo "src-tauri\libs\ohos\arm64-v8a"
$src     = Join-Path $PSScriptRoot "ort_probe.c"
$out     = Join-Path $PSScriptRoot "ort_probe"

foreach ($p in @($clang, $src, (Join-Path $ortLib "libonnxruntime.so"))) {
  if (-not (Test-Path $p)) { throw "missing: $p" }
}

& $clang --target=aarch64-linux-ohos "--sysroot=$sysroot" -O2 -fPIC `
  "-I$ortInc" "-I$ortInc\onnxruntime\core\session" `
  $src "-L$ortLib" -lonnxruntime "-Wl,-rpath,/data/local/tmp/ort" -o $out

if ($LASTEXITCODE -ne 0) { throw "clang failed ($LASTEXITCODE)" }
Write-Output "built: $out"
Get-Item $out | Select-Object Name, Length | Format-Table -AutoSize
