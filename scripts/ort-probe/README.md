# ort_probe — 设备端 CPU 基准探针

在同一台 OpenHarmony 设备上测 **ONNX Runtime CPU** 与 **NPU(`.om`)** 的时延对比。
用的是 App 已经构建好的 OHOS 版 `libonnxruntime.so`，不需要重编 ORT。

## 构建

```powershell
powershell -ExecutionPolicy Bypass -File scripts/ort-probe/build-ort-probe.ps1
```

产物：`scripts/ort-probe/ort_probe`（aarch64-linux-ohos）。

> 注意：必须用 Windows 路径调用 NDK 的 `clang.exe`（它不认 WSL 的 `/mnt/c/...`）。

## 部署

```powershell
hdc shell "mkdir -p /data/local/tmp/ort; chmod 777 /data/local/tmp/ort"
hdc file send scripts/ort-probe/ort_probe /data/local/tmp/ort/ort_probe
hdc file send src-tauri/libs/ohos/arm64-v8a/libonnxruntime.so /data/local/tmp/ort/libonnxruntime.so
# SONAME 是 libonnxruntime.so.1，需要同名副本
hdc shell "cd /data/local/tmp/ort && cp libonnxruntime.so libonnxruntime.so.1"
# libc++_shared.so 来自 OHOS 构建树
hdc file send src-tauri/gen/ohos/entry/build/default/intermediates/libs/default/arm64-v8a/libc++_shared.so /data/local/tmp/ort/libc++_shared.so
hdc shell "chmod 777 /data/local/tmp/ort/*"
```

## 运行

```powershell
hdc shell "cd /data/local/tmp/ort && LD_LIBRARY_PATH=/data/local/tmp/ort \
  ./ort_probe /data/local/tmp/onnx/model.onnx 5 6 /data/local/tmp/in.bin"
```

- `<iters>` 计时次数（前面自动 warmup 一次）
- `<threads>` `intra_op` 线程数（本机 MOR-M1 实测 **6** 最优）
- 动态维度可用 `<inN.bin>.shape`（逗号分隔）覆盖
- 每个输出会 dump 到 `<model.onnx>.outN.bin`（float32），用于 golden 对比

## 实测参考（MOR-M1 / Kirin XE90）

| 模型 | 设备 CPU(4T) | NPU | NPU 加速比 |
|---|---|---|---|
| NIND 降噪 | 2674.9 ms | 160.0 ms | 16.72× |
| MediaPipe face | 6.2 ms | 4.8 ms | 1.29× |
| MediaPipe selfie | 101.9 ms | 13.4 ms | 7.60× |
| ESAM3 image encoder | 2107.0 ms | 223.0 ms | 9.45× |
| ESAM3 text encoder | 5.8 ms | 8.0 ms | 0.72×（CPU 更快） |
| ESAM3 decoder（23 段） | 48859.7 ms | 5832.1 ms | 8.38× |
| SAM decoder | 37.0 ms | 35.6 ms | 1.04× |
| SAM encoder | 4931.5 ms | 18134.9 ms | 0.27×（CPU 更快） |
| DepthAnything | 992.1 ms | 1309.0 ms | 0.76×（CPU 更快） |
| BiRefNet | 9761.2 ms | 10745.0 ms | 0.91×（CPU 更快） |

结论与调优（perfmode、线程数、为什么不要反量化）见
`docs/HARMONYOS_PORTING.md` §6.29 与 `docs/reports/TEST_REPORT_2026-11-07.md` §12.28–§12.30。
