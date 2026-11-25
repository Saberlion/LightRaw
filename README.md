<p align="center">
  <img src="https://raw.githubusercontent.com/CyberTimon/RapidRAW/assets/.github/assets/editor.jpg" alt="LightRaw Editor">
</p>

<div align="center">

[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![wgpu](https://img.shields.io/badge/wgpu-%23282C34.svg?style=for-the-badge&logo=webgpu&logoColor=white)](https://wgpu.rs/)
[![React](https://img.shields.io/badge/react-%2320232a.svg?style=for-the-badge&logo=react&logoColor=%2361DAFB)](https://react.dev/)
[![Tauri](https://img.shields.io/badge/Tauri-24C8DB?style=for-the-badge&logo=tauri&logoColor=white)](https://tauri.app/)
[![HarmonyOS](https://img.shields.io/badge/HarmonyOS-24C8DB?style=for-the-badge&logoColor=white)](https://developer.huawei.com/consumer/cn/harmonyos/)
[![AGPL-3.0](https://img.shields.io/badge/License-AGPL_v3-blue.svg?style=for-the-badge)](https://opensource.org/licenses/AGPL-3.0)

</div>

# LightRaw

> A beautiful, non-destructive, GPU-accelerated RAW image editor for HarmonyOS — powered by Kirin NPU for on-device AI.

**LightRaw is a derivative work of [RapidRAW](https://github.com/CyberTimon/RapidRAW)** by Timon Käch, ported to HarmonyOS/OpenHarmony with device-side NPU acceleration for AI workloads. All credit for the original editor, processing pipeline, and AI models goes to the RapidRAW project and its author.

## What is this fork?

| | |
|---|---|
| **Base project** | [RapidRAW](https://github.com/CyberTimon/RapidRAW) — a modern, high-performance alternative to Adobe Lightroom® for Windows, macOS, Linux, and Android |
| **This fork (LightRaw)** | Ports RapidRAW to **HarmonyOS / OpenHarmony** (2in1, tablet, and phone form factors) and adds **on-device NPU acceleration** for AI features using Huawei's CANN (Compute Architecture for Neural Networks) |
| **Upstream compatibility** | Tracks upstream releases; the `main` branch mirrors upstream, port work lives on `feature/harmonyos-port`, merged into `master` |
| **License** | AGPL-3.0 (inherited from upstream) |

## HarmonyOS Port

LightRaw runs natively on HarmonyOS devices via the [Tauri `feat/open-harmony`](https://github.com/tauri-apps/tauri/tree/feat/open-harmony) branch:

- **GPU rendering**: Vulkan-only (wgpu compute + readback + IPC bitmap path), with a record-based fallback for driver crashes
- **File access**: FileKit system picker (SAF) + folder persistence grants, no direct media library permissions required
- **Window management**: Custom titlebar on desktop (2in1), floating tab shell on phone, safe-area double-protection
- **Internationalization**: 18 locales with full Chinese (Simplified & Traditional) support
- **Build tooling**: `scripts/build-ohos.ps1` for self-contained HAP builds (ORT injection, CANN library injection, model bundling, release strip)

## Kirin NPU Acceleration

LightRaw uses Huawei's **CANN (Compute Architecture for Neural Networks)** and the **HiAI Foundation Model Runtime** to run AI models on Kirin NPU hardware for significant speedups over CPU inference:

| Feature | CPU baseline | NPU (CANN `.om`) | Speedup |
|---|---|---|---|
| AI Denoise (NIND) | ~1.2s/tile | ~189ms/tile | **6.4×** |
| RapidRes 2× upscale | CPU tile loop | NPU tile loop | **10×** |
| Quick Erase (LaMa) | ~44s/image | ~10.3s/image | **4.3×** |

**How it works:**
1. ONNX models are converted offline to CANN `.om` format (per-SoC artifacts)
2. At runtime, the app dlopens vendor libraries (`libai_fmk_hcl_model_runtime.so`, `libai_fmk_tensor.so`, `libai_fmk_model_runtime_core.so`) from the device
3. The HIAI_MR C API is used to restore the model, create ION-backed buffers, and execute inference on the NPU
4. Any failure (missing model, unsupported SoC, driver error) gracefully falls back to ONNX Runtime CPU inference

**Supported SoCs**: Kirin X90, Kirin 9030 (Mate 80/90 series, MateBook Pro). Other SoCs automatically use CPU fallback.

## AI Models

All AI models run entirely on-device — no cloud, no upload:

- **AI Denoise** (NIND): Non-destructive RAW denoising with tile-based blending
- **AI Masks**: Subject (SAM), Sky/Face/Text (EfficientSAM3), Foreground (BiRefNet), Depth (DepthAnything V2)
- **Quick Erase** (LaMa): Local inpainting for object removal
- **AI Tagging** (CLIP): Optional auto-tagging for library search
- **RapidRes**: 2× AI super-resolution

## Features

- **Non-destructive RAW editing**: all adjustments saved to `.rrdata` sidecar files; originals are never modified
- **Broad format support**: mainstream camera RAW (DNG, CR2, CR3, NEF, ARW, RAF, ORF, RW2, …) plus JPEG, PNG, WebP, TIFF, JXL, EXR
- **Professional adjustments**: exposure, AgX tone mapping, contrast, parametric curves, color grading, sharpening, dehaze, lens blur, vignette, glow, film grain, analog film emulations
- **Masks & retouching**: brush, linear, radial, color, luminance, AI depth masks; clone, heal, liquify, retouch
- **Lens correction**: Lensfun-based distortion, TCA, and vignetting with auto-detection
- **Merging**: HDR bracket merge, panorama stitching, focus stacking, collage
- **Library & batch**: recursive folders, ratings, color labels, filtering, virtual copies, batch apply/export
- **Export**: JPEG, PNG, WebP, AVIF, TIFF, JXL, LUT with watermarking and EXIF retention

## Supported Platforms

| Platform | Status |
|---|---|
| **HarmonyOS** (2in1 / tablet / phone) | ✅ Primary target of this fork |
| Windows | ✅ Inherited from upstream |
| macOS | ✅ Inherited from upstream |
| Linux | ✅ Inherited from upstream |
| Android | ✅ Inherited from upstream |

## Building for HarmonyOS

Prerequisites: DevEco Studio, OHOS SDK (API 26+), Rust with `aarch64-unknown-linux-ohos` target.

```powershell
# Debug build (device iteration)
scripts\build-ohos.ps1

# Release build (AppGallery submission, includes bundled AI models)
scripts\build-ohos.ps1 -Release
```

See `docs/HARMONYOS_PORTING.md` for the full porting guide, environment setup, and verification recipes.

## Acknowledgments

- **[Timon Käch](https://github.com/CyberTimon)** — creator of RapidRAW, the upstream project
- **[RapidRAW contributors](https://github.com/CyberTimon/RapidRAW/graphs/contributors)** — all upstream features, models, and processing pipeline
- **[rawler](https://github.com/dnglab/dnglab)** / dnglab — RAW decoding foundation
- **[wgpu](https://wgpu.rs/)** — GPU compute shaders
- **[Tauri](https://tauri.app/)** — application framework

## License

AGPL-3.0 — see [LICENSE](LICENSE). LightRaw is a derivative work of RapidRAW and complies with the upstream AGPL-3.0 license. All modifications are published under the same license.
