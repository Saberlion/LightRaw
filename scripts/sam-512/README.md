# SAM 512² 导出 + int8 静态量化

把随 App 下发的 SAM ViT-B（1024²，动态量化）改成 **512² 输入 + int8 静态量化**，encoder 从 3.6 s 降到 0.6 s，
同时让静态量化的内存需求从 >24 GB 降到 ~12.7 GB。

两条路线：

| 路线 | 做法 | 结论 |
|---|---|---|
| A（初版） | 512 encoder + **32×32 decoder** | ❌ 空掩码 22.7%、IoU 中位数≈0，明显回归 |
| **B（推荐）** | **512 encoder + 2× 上采样到 64×64 + 原 decoder** | ✅ 空掩码 0%、IoU 中位数 0.769、端到端 654 ms |

## 用法

```bash
# 512 encoder（输出 32×32）
python make_encoder_512.py sam_enc_fp32.onnx sam_enc512_fp32.onnx

# 路线 B：在 512 encoder 末尾追加 2× nearest Resize，输出 64×64
python make_encoder_512_up.py sam_enc512_fp32.onnx sam_enc512_up_fp32.onnx

# 量化（MatMul+Conv 最优）
python make_input.py photo.jpg calib_512.bin 512
python quantize_512.py static sam_enc512_up_fp32.onnx sam_enc512_up_mm_conv.onnx calib_512.bin MatMul,Conv
```

路线 A 才需要 `make_decoder_512.py`（把 decoder 降到 32×32，见下文缺陷说明）。

## 改了什么

**encoder**：HF 导出里窗口切分/padding/reshape 全部由 `Shape()` 运行时推导，唯一写死的是
`pos_embed (1,64,64,768)`，按 PyTorch `bicubic / align_corners=False` 插到 32×32。
**必须清空 `value_info`**，否则 1024 的旧维度会被合并回来，加载直接失败。

**decoder（仅路线 A）**：五处，后两处只有带提示点才暴露：

| 改动 | 说明 |
|---|---|
| `image_embeddings` 32×32、`mask_input` 128×128 | 形状 |
| 4 个成形常量 64→32 | `[-1,256,64,64]` 等 |
| 稠密位置编码 | 64 网格 **2×2 块中点均值**（32 网格点落在 64 网格两点之间） |
| `ortshared_13_1_0`（`input_image_size`） | **1024.0 → 512.0**；不改则点提示全映射到左上四分之一 |
| `point_coords` / `point_labels` 维度 | 保留符号维 `num_points`；写成字面量 0 会让 1 点/2 框角直接报错 |

## 实测（MOR-M1 / Kirin XE90，`ort_probe`，3 次取最小）

| 流水线 | 编码器 | 解码器 | 合计 | vs 1024 现状 |
|---|---|---|---|---|
| 1024 现状 | 3599.4 ms | 31.0 ms | 3630.4 ms | 1.00× |
| 512→32 | 608.1 ms | 14.6 ms | 622.7 ms | 5.83× |
| **512→up64（推荐）** | 622.5 ms | 31.0 ms | **653.5 ms** | **5.55×** |

up64 编码器在另外两张图 599.0 / 602.2 ms；4 线程 758.3 ms。上采样焊成模型里的 `Resize` 节点。

## 精度（3 张照片 × 25 个点提示）

| 指标 | 512→32 | 512→up64 fp32 | 512→up64 int8 | 1024 参考 |
|---|---|---|---|---|
| 掩码 IoU 中位数 | ≈0 | 0.764 | **0.769** | 1.000 |
| 空掩码率 | 22.7% | 0% | **0%** | 0% |
| 面积中位数 | 0.071 | 0.331 | 0.317 | 0.414 |
| 提示点落在掩码内 | 40.0% | 36.0% | 37.3% | 40.0% |

**量化层**（int8 vs 同分辨率 fp32）：up64 配置掩码 IoU 中位数 **0.954**，像素一致率 96%+ → 量化安全。
**分辨率层**：up64 相对 1024 的 IoU 中位数 0.769、空掩码 0 → 可用；512→32 的 ≈0 / 22.7% 不可用。

上采样方式实测等价：nearest 0.764 / bilinear 0.761 / bicubic 0.755。

## 部署注意

- **推荐**：编码器换 `sam_enc512_up_mm_conv.onnx`，`SAM_INPUT_SIZE=512`，**decoder 保持原文件**；
- **坑**：点提示仍按 **1024** 缩放（decoder 的 `input_image_size` 没变）。`ai_processing.rs` 里
  `scale = SAM_INPUT_SIZE / long_side` 目前一个常量管两处，接入时要拆成"编码器 512 / 解码器 1024"；
- 设备 ORT 不支持 `ConvInteger`，动态量化只能量化 `MatMul`；
- 静态量化在这台 32 GB 机器上峰值 12.7 GB（1024² 同流程会崩 VM）。
