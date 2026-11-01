# LUT / Log 管线方案调研（Raw Alchemy · libraw-wasm · darktable · RawTherapee · FFmpeg · libplacebo）

> 分析日期:2026-11-01 · 服务对象:本仓胶片模拟 / LUT 子系统的能力补强(场景参考 LUT、Log 输入、导出一致性)
> 证据标注:`[源码]` 已逐行核对 · `[README]` 仅依据仓库文档 · `[推断]` 由代码结构推断,未实机验证
> 涉及仓库:Raw-Alchemy v0.4.2(AGPL-3.0)· JiangOnlineRaw-joraw(上游 `libraw-wasm`,ISC)· darktable master · RawTherapee dev · FFmpeg master · libplacebo / mpv master · OpenColorIO v2
> 说明:本文只记录方案与结论,不复制第三方实现;所有外部结论均标注核对方式,便于复核。

---

## 1. 结论速览

1. **问题不是"LUT 坏了",而是"信号不对"**:视频 LUT 要求输入是某个 **Log 曲线 + 对应色域**,而 RAW 解码后的线性图默认是 sRGB/ProPhoto 原色。直接套 LUT = 曲线错 + 色域错,表现为"发灰/发闷/肤色偏"。
2. **Raw Alchemy 是这条路线的完整参考实现**:`RAW → ProPhoto 线性 → 目标 Log 色域 → 目标 Log 曲线 → LUT → 16-bit TIFF`,并且给出了 14 种 Log 空间与色域的映射表。它的核心贡献是**把"色域"和"曲线"拆成两个正交维度**,而不是一个布尔开关。
3. **本仓已有场景参考槽位且实现更"高动态"**:`shader.wgsl` 的 `linear_to_vlog()` + LUT 分支位置(`2040-2048`)在架构上与 Raw Alchemy 等价,但**曲线硬编码为 V-Log**、**色域不做换算**、**LUT 输入域被强制 `[0,1]`**。
4. **本仓可直接用 V-Log LUT**(如 V-Log Alchemy 的富士/徕卡/ARRI 套),对 F-Log / F-Log2 / S-Log3 / LogC4 一类则必须补 `lutInput` 枚举 + 各 OETF 分支;这条结论与 Raw Alchemy 的 `LOG_TO_WORKING_SPACE` 表一一对应。
5. **两个最容易修、收益最直接的缺陷**(本仓):`.cube` 的 `DOMAIN_MIN/MAX` 被丢弃(`lut_processing.rs:204`)、`LUT_1D_SIZE` 的数据被静默跳过(`parse_cube` 只在见过 `LUT_3D_SIZE` 后收数据)。
6. **不要在导出/预览一致性上抄 Raw Alchemy**:它的 16-bit TIFF **不写 ICC、不保留 EXIF**,而交互预览用 `BT.709→sRGB` 假装解码 Log(`preview.py:397`),预览与成品必然不一致。

---

## 2. 问题定义:为什么"视频 LUT 套 RAW"会错色

一条 LUT 隐含两个契约,缺一不可:

| 契约 | 含义 | 错配表现 |
|---|---|---|
| **OETF(曲线)** | 输入信号用哪条 Log 编码(V-Log / F-Log2 / S-Log3 / LogC4 …) | 整体亮度/对比偏移,暗部抬升或压死;两条曲线的黑位与中灰位置不同,不能互相替代 |
| **Gamut(色域)** | 输入 RGB 用的是哪组原色(V-Gamut ≈ BT.2020、S-Gamut3.Cine、ARRI WG4 …) | 饱和度与色相系统性偏移(尤其绿/肤色),矩阵离对角项越大越明显 |

**本仓现状**(核对于本仓源码):

- 曲线:`linear_to_vlog()`(`src-tauri/src/shaders/shader.wgsl:297-303`)是唯一实现,场景参考 LUT 一律走它(`2043`);
- 色域:喂给 LUT 的是本仓内部线性 RGB(**sRGB/Rec.709 原色**,由 `raw_processing.rs:191/343` 的 `camera_to_srgb` 得到),**没有** 709→V-Gamut/BT.2020 的矩阵;
- 标记:只有一个布尔 `lutIsSceneReferred`(`src/hooks/useEditorActions.ts:262`),无法表达"F-Log2"或"LogC4";
- 域:`.cube` 的 `DOMAIN_MIN/MAX` 被 `continue` 跳过(`lut_processing.rs:204`),shader 端再 `clamp` 到 `[0,1]`(`shader.wgsl:1563`)。

> 结论:**本仓的"胶片模拟"能工作,是因为内置 spektrafilm 资产恰好是按 V-Log→sRGB 生成的**;一旦换成别的 Log LUT,当前管线必然偏色。这正是 Raw Alchemy 想解决的问题。

---

## 3. Raw Alchemy(`shenmintao/Raw-Alchemy`)

### 3.1 定位与架构 `[源码]`

- 形态:Python 3.11 + `rawpy`(作者 fork)+ `colour-science@develop` + `numba` + `lensfun`;CLI(`cli.py`)+ tkinter GUI(`gui.py`/`preview.py`)。
- 目标:**离线把 RAW 转成"带 Log 信号的 16-bit TIFF"**,让视频 LUT 在 Photoshop/DaVinci 里正常使用。
- 规模:`core.py` 仅 144 行,管线逻辑全部集中在此;`utils.py` 689 行是 Numba 算子;`lensfun_wrapper.py` 556 行是 ctypes 绑定。
- 依赖可复现性差:`rawpy @ git+…/shenmintao/rawpy.git`、`colour-science @ …@develop`(`pyproject.toml`)。

### 3.2 六步流程(`core.py:38-140`)

| 步 | 动作 | 关键参数 / 理由 |
|---|---|---|
| 1 | RAW 解码到 **ProPhoto RGB 线性 16-bit** | `gamma=(1,1)` 拿线性;`no_auto_bright=True` 禁自动增亮;`use_camera_wb=True` 用机内白平衡;`output_bps=16` 保暗部;`output_color=ProPhoto` 得相机无关的统一中间空间;`highlight_mode=2`(Blend)在 LibRaw 内做高光重建;`demosaic=AAHD` |
| 2 | 曝光 | 手动 `gain=2^EV`,或 5 种自动测光(见 3.3) |
| 3 | 镜头校正 | lensfun:畸变 + TCA + 暗角 + `auto_scale` 裁切补偿;支持自定义 LCP 转换出的 XML 库 |
| 3.5 | "Camera-Match Boost" | 固定 `saturation=1.25`、`contrast=1.10`,**支点 0.18,作用在 ProPhoto 线性域**(`utils.py:416-445`) |
| 4 | 色域 + Log 编码 | `colour.matrix_RGB_to_RGB(ProPhoto → 目标色域)`(含 D50→D65 的 CAT)→ `colour.cctf_encoding(…, 目标曲线)` |
| 5 | LUT | `colour.read_LUT`;3D 走向量四面体(4 顶点 + domain 缩放),1D 走库默认 |
| 6 | 保存 | TIFF 16-bit `zlib+predictor=2`;HEIF 10-bit;JPEG q95 4:4:4 |

**为什么"先线性化、再按目标曲线重编码"是关键**:RAW 解码得到的是场景线性光;LUT 要的是"像相机输出的 Log 视频信号"。两步之间必须做 `线性 → 目标色域 → 目标曲线`,任何一步跳过都会偏色。

### 3.3 三个值得抄的实现 `[源码]`

**(a) Log 空间表(`config.py:11-32`)**

```
F-Log / F-Log2 → F-Gamut        F-Log2C → F-Gamut C
V-Log          → V-Gamut        N-Log   → N-Gamut
Canon Log 2/3  → Cinema Gamut   S-Log3  → S-Gamut3
S-Log3.Cine    → S-Gamut3.Cine  Arri LogC3/4 → ARRI WG3/WG4
Log3G10        → REDWideGamutRGB   D-Log → DJI D-Gamut   L-Log → BT.2020
```

外加 `LOG_ENCODING_MAP` 处理"同曲线不同色域"的特例(`S-Log3.Cine → S-Log3`、`F-Log2C → F-Log2`)。**色域与曲线正交**是整张表的精髓。

**(b) LUT domain 与四面体(`utils.py:54-306`)**

```python
scale_r = size_minus_1 / (domain_max[0] - domain_min[0])
raw_idx_r = (in_r - min_r) * scale_r          # 支持 domain ≠ [0,1]
...
if dx >= dy: ...  # 6 分支四面体,只读 4 个顶点
```

`@njit(parallel=True, fastmath=True, cache=True)` 原地并行;作者注释说明了"内存访问减半、灰阶/肤色更准"。

**(c) 测光策略(`metering.py`)**

| 模式 | 算法 | 备注 |
|---|---|---|
| `average` | 几何平均亮度 → 0.18 | 用 `exp(mean(log))`,避免高光主导 |
| `center-weighted` | 高斯权重(`sigma=min(h,w)/2`) | 人像/主体居中 |
| `highlight-safe` | 99 分位 → 0.9(ETTR) | 保高光,欠曝阴影 |
| `hybrid` | 平均增益 + 峰值限 6.0 | 兼顾 |
| `matrix` | 7×7 分区:中心偏置 ×1.5、高光区 ×0.2、暗部 ×1.2,再限峰 | 默认模式 |

全部在 **1024px 切片视图**上统计(`utils.py:401-410`,零拷贝),这是"交互式调参也能实时"的关键工程手段。

### 3.4 优点

1. **协议清晰**:输入规则(LUT 期望什么)、中间空间(ProPhoto 线性)、输出(16-bit)都写死在代码里,不靠用户猜。
2. **正交建模**:色域与曲线分离,14 种 Log 空间用两张小表表达,扩展只需加一行。
3. **domain 正确**:这是本仓缺失的一环。
4. **性能务实**:Numba 原地算子 + 并行 + 预览半尺寸 + 缓存镜头校正。
5. **工程完整**:批处理进程池、GUI 进度队列、LCP→lensfun 转换脚本。

### 3.5 缺陷(逐条可复核)

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| 1 | **输出 TIFF 无 ICC、无 EXIF** | `file_io.py:56-68` 只传 `photometric/compression/predictor` | 下游不知道这是 F-Log2/F-Gamut,按 sRGB 解释即偏色;等于把"元数据契约"丢给用户 |
| 2 | **16-bit 容器把场景高光截在 1.0** | `core.py:45-56`:`rawpy` 16-bit 输出归一化后 `/65535` | Log 曲线上半段靠后续数字增益去填(`max_allowed_peak=6.0`),不是真实高光数据 |
| 3 | **Camera-Match Boost 是经验值** | `core.py:88-89`、`utils.py:416-445` | 固定 1.25/1.10 对所有相机/LUT 一致;线性域对比度会把暗部推狠,再被 Log 放大 |
| 4 | **预览用 BT.709→sRGB 假装解码 Log** | `preview.py:397`、`utils.py:356-390` | 预览与导出不一致(尤其暗部),直方图同样失真 |
| 5 | **内存放大** | `float32` 全幅 + `ProcessPoolExecutor(max_workers=4)`(`orchestrator.py:67`) | 60MP × 4 进程峰值可达 8–10GB |
| 6 | **无 gamut mapping** | 保存前 `np.clip(0,1)`(`file_io.py:34`) | LUT 后超色域信息直接丢失(硬裁) |
| 7 | **解码参数写死** | `use_camera_wb` / `AAHD` / `highlight_mode=2` 全固定 | 无法按题材选择,也无法对齐参考实现 |
| 8 | **可复现性** | 依赖 `rawpy` fork + `colour@develop` | 结果随上游漂移,与"数学精度"主张矛盾 |

> 注:第 2 条容易被"16-bit 专业输出"误导——本仓保留线性 HDR 到 shader,`linear_to_vlog()` 在 clamp 前能容纳约 46 倍白(V-Log 1.0 ≈ 线性 46),**Log 头空间反而比 Raw Alchemy 更足**。

### 3.6 对本仓可复用的三件事

1. **`LOG_TO_WORKING_SPACE` 表**(附录 A)——直接用于 `lutInput` 枚举与色域矩阵生成。
2. **LUT domain 处理**——`parse_cube` 保留 `DOMAIN_MIN/MAX`,`Lut` 结构加 `domain_min/max`,GPU 上传时按 `1/(max-min)` 归一化。
3. **测光策略**(尤其 `highlight-safe` 与 7×7 `matrix`)——本仓的"自动调整"可借鉴,在缩略图上统计即可。

---

## 4. JiangOnlineRaw / libraw-wasm(joraw)

### 4.1 定位 `[源码]`

浏览器端 RAW 解码封装:LibRaw + lcms2 用 Emscripten 编成 WASM,外面套 Web Worker + transferable。上游是 `ybouane/libraw-wasm`(`package.json` 里 name/author/ISC 未改),本仓只有 2 个 commit、README 为空,属实验性分支。

### 4.2 三条 API 路径(这是它最有价值的设计)

| 路径 | 入口 | 实现 | 用途 |
|---|---|---|---|
| 缩略图 | `thumbnailData()` | `unpack_thumb()` | 首屏/胶片栏,最便宜 |
| 原始 Bayer | `getRawImage()` + `getBlackLevels()` | 只 `unpack()`,**故意不调用 `dcraw_process()`**(`libraw_wrapper.cpp:60-61`) | 交给 JS/GPU 自建管线(代码里叫 "God Mode"/"Detective Mode") |
| 成品图 | `imageData()` | `unpack()` + `dcraw_process()` + `dcraw_make_mem_image()` | LibRaw 全量 CPU 管线 |

### 4.3 先进点与局限

**先进点**

1. 解码与渲染解耦:显式提供"含黑边、未减黑电平"的 16-bit Bayer,这是自研管线的正确接口形态。
2. **lcms2 编进 WASM**(`compileLibraw.sh` 的 `--enable-lcms` + `output_profile`/`camera_profile`):走 ICC 而非固定 3×3 矩阵,支持相机双光源 profile。
3. makernote 覆盖 10 个品牌,其中 Fuji 段(`:508-593`)暴露 `FilmMode`、`DynamicRange`、`DRangePriority*`、`ExpoMidPointShift`、`BlackLevel[9]`、`RAFDataVersion` 等。
4. dcraw 参数面完整(`user_cblack`、`aber`、`gamm`、`user_mul`、`greybox`、`no_interpolation`、`dcb`、`fbdd_noiserd`…)。
5. 零拷贝:JS→WASM 一次 `set()`,`worker.js` 回传时把 typed array 的 buffer 放进 transfer list。

**局限**

1. 图像数学仍是 dcraw 时代(AHD/DCB/PPG/VNG),无 GPU;`dcraw_process` 全图一次,无 tile/ROI,不适合交互编辑。
2. 输出是 **8/16-bit gamma 编码**,没有线性工作空间,后续调整会二次量化。
3. `metadata()` **没有导出 `rgb_cam`/`cam_xyz`**(`:244` 注释"can also be copied if needed")→ 自建管线拿不到相机矩阵。
4. 内存:文件 + raw + 输出同驻 WASM 堆,`ALLOW_MEMORY_GROWTH` 只增不减。
5. `ex2.html` 与 `getBlackLevels()` 返回结构不匹配(`Array.from(obj)` 得空数组),说明示例未随接口更新。

### 4.4 对本仓的启示

架构上本仓已是"解码 → GPU 管线",无需照搬;可借鉴的是 **ICC 输入色彩管理**(本仓只有 `camera_to_srgb` 矩阵,Rust 侧无 lcms,ICC 仅导出时写 sRGB)与 **三档 API 语义的显式化**。

---

## 5. darktable 生态

### 5.0 先澄清:darktable-ai 没有 LUT

`darktable-org/darktable-ai` 是 **AI 模型的转换/打包流水线**(ONNX + `config.json`,模型清单为 NIND/NAFNet/SAM 2.1/SegNext/BSRGAN/RealPLKSR/OpenCLIP),不含任何 LUT 代码。LUT 相关实现都在 darktable 主仓 `src/iop/lut3d.c` 与 lua-scripts。

### 5.1 LUT3D 模块:用"应用色彩空间"声明契约 `[源码]`

```c
typedef enum dt_iop_lut3d_colorspace_t {
  DT_IOP_SRGB = 0, DT_IOP_REC709, DT_IOP_LIN_REC709,
  DT_IOP_ARGB, DT_IOP_LIN_PROPHOTO, DT_IOP_LIN_REC2020, ...
} // 默认 DT_IOP_SRGB
```

渲染时做**双向 profile 转换**(`lut3d.c:1030-1090`,OpenCL 路径 1120-1142):

```
work profile → LUT profile → 查表 → LUT profile → work profile
```

- 注意:**6 个选项里没有 log 空间**——darktable 也不支持"输入 F-Log 的 LUT",它假定信号已经是这几个空间之一。这一点常被误解,值得记录。
- 插值:`DT_IOP_TETRAHEDRAL = 0` 为默认,另有 `trilinear` / `pyramid`。
- 格式:`.cube` `.3dl` `.gmz` + HALD PNG(`_calculate_clut_haldclut`)。

### 5.2 存储层"烘焙":GMZ → 48³ + 磁盘缓存 `[源码]`

- 参数里可只存压缩关键点:`nb_keypoints` + `c_clut[2048*2*3]`(`lut3d.c:55,79-80`);
- 运行时 `lut3d_decompress_clut()` 展开成 **48³** 浮点网格(`DT_IOP_LUT3D_CLUT_LEVEL 48`);
- 结果落盘 `~/.cache/gmic/<digest>.cimgz`(`:468-511`)。

这是"LUT 要进数据库/边车而不膨胀"的工程取舍,与本仓把路径写进 `.rrdata` 的路线不同,无需照搬。

### 5.3 观感层"烘焙":`exportLUT.lua` `[源码]`

darktable 主程序**没有**"导出 LUT"按钮,实现是一个 Lua 脚本:

```lua
local identity = dt.database.import(身份 HaldCLUT 图)
for _, style in ipairs(dt.styles) do
  identity:reset()
  dt.styles.apply(style, identity)
  dt.new_format("png"):write_image(identity, 输出/<style>.png)
end
```

要点:① 烘焙对象是 **style**,输出 **Hald PNG**(不是 `.cube`);② 走普通导出路径 ⇒ 产物是**显示参考**,不能当场景参考 Log LUT;③ 身份图需用户自备。

### 5.4 与本仓交叉

| 维度 | darktable | 本仓 | 备注 |
|---|---|---|---|
| 空间声明 | UI 下拉 6 项 + ICC 双向转换 | 布尔 `lutIsSceneReferred` | 本仓表达能力更弱,但**独有的 log 槽位更强** |
| Log 支持 | ❌ 无 log 选项 | ✅ 硬编码 V-Log | 两边都需要补"多 Log" |
| domain | 支持(查表前缩放) | ❌ 丢弃 | 本仓待修 |
| 1D LUT | 部分格式支持 | ❌ 静默丢弃 | 本仓待修 |
| 插值 | 四面体默认 | 四面体(手写) | 一致 |

---

## 6. RawTherapee(消费端 + 文件名即契约)

### 6.1 只消费,不生产 `[源码]`

`rtgui/tools/filmsimulation.cc` 只有 CLUT 选择、强度、缓存清理;**没有**"导出 LUT"代码。参数仅三项(`procparams.cc:2661-2672`):

```
FilmSimulationParams { enabled, clutFilename, strength(100) }
```

写入 PP3 边车。

### 6.2 加载期"烘焙":Hald 图 → 网格 `[源码]`

`rtengine/clutstore.cc`:

- 由像素数反推 level(`while (level³ < w*h) ++level`),校验 `level³ == 像素数`;
- `clut_level *= clut_level`(Hald level 8 → 64³ 网格);
- 存 `uint16` 缓冲,`flevel_minus_one=(level-1)/65535` ⇒ 支持 8/16-bit 图像;
- 采样是**三线性(8 顶点)**,与 darktable/本仓的四面体不同;
- `strength` 混合写在采样循环里(`vintpf(v_strength, v_out, v_in)`)。

### 6.3 空间契约靠**文件名后缀** `[源码]`

```cpp
HaldCLUT::splitClutFilename(file, name, ext, profile_name, checkProfile) {
  profile_name = "sRGB";                     // 默认
  for (wp : ICCStore::getWorkingProfiles())
    if (name 以 wp 结尾) { profile_name = wp; name 去掉后缀; }
}
```

渲染时若 `clutProfile != workingProfile`,用 `workingSpaceMatrix/InverseMatrix` 做 XYZ 往返(`improcfun.cc:2226-2239, 3154-3208`)。

### 6.4 其他

- 只认 `.png` / `.tif`(`filmsimulation.cc:401`);
- CLUT 目录 `options.clutsDir`,GUI 用广度优先建目录树;
- `CLUTStore` 缓存 + `clutCacheSize == 1` 时清空;
- **无内置烘焙**;社区做法是"身份 Hald 图过一遍 RT 管线 → 导出 PNG"。

> 启示:**"文件名即契约"是零 UI 成本的方案**,适合用户自建 LUT 库;本仓若做 manifest,也应兼容"从文件名嗅探"。

---

## 7. 通用 LUT 处理参考(FFmpeg / libplacebo / OCIO)

### 7.1 FFmpeg `vf_lut3d` `[源码]`

- 格式:`.cube` `.dat` `.3dl` `.m3d` `.cinespace`;
- 插值:`nearest / trilinear / tetrahedral(默认) / pyramid / prism`;
- **domain 处理正确**:`DOMAIN_MIN/MAX` → `lut3d->scale`,采样前 `clip(x*scale, 0, lut_max)`;
- `cinespace` 支持 **1D prelut**(shaper);`.cube` 的 `LUT_1D_SIZE` 段同样被跳过(与业界常见行为一致);
- 逐像素 8 次数组访问 + slice 多线程。

### 7.2 mpv / libplacebo `[源码]`

- `.cube` 解析同时支持 `LUT_3D_SIZE`(≤1024)与 `LUT_1D_SIZE`(≤65536),并校验 `DOMAIN_MIN/MAX`;
- `pl_shader_custom_lut` 按尺寸/后端能力自动选 **3D 纹理 / uniform / 内联 GLSL 常量数组**,纹理不可用时降级 2D/1D;
- 方法默认 `SH_LUT_TETRAHEDRAL`;
- **`--lut-type native | normalized | conversion`**:显式声明 LUT 期待"原始信号 / 归一化 [0,1] / 一次色彩空间转换";
- 三个槽位:`--image-lut`(解码后)、`--lut`(色调映射前)、`--target-lut`(色彩管理后)。

> 这是"色彩空间契约"表达力最强的实现:**用枚举描述 LUT 期望什么**,而不是用布尔猜测。

### 7.3 OpenColorIO `[源码]`

- 变换图:`FileTransform → Lut1D/Lut3D/Matrix/Range/Exponent`,CPU/GPU 两套渲染器;
- 插值:`INTERP_BEST/TETRAHEDRAL → 四面体`;`DEFAULT/LINEAR/NEAREST → 线性`(v2 把 NEAREST 当三线性);
- `ociobakelut` 把整条链**烘焙**成一个 LUT——这正是"把适配链折叠进 LUT"的工业实现。

---

## 8. 横向对照

| 维度 | Raw Alchemy | joraw | darktable | RawTherapee | FFmpeg | libplacebo | **本仓** |
|---|---|---|---|---|---|---|---|
| LUT 输入契约 | Log 空间名(14 种) | 无(只解码) | 6 个显示/线性空间 | profile 名(文件名) | 无(用户保证) | `lut-type` 枚举 | **布尔 scene-ref** |
| 色域处理 | 矩阵(ProPhoto→目标) | ICC | ICC 双向 | ICC 双向 | 无 | 引擎自动 | **无** |
| 曲线处理 | `cctf_encoding` 多曲线 | 无 | 无 | 无 | 无 | 无 | **硬编码 V-Log** |
| domain | ✅ | — | ✅ | —(Hald 天然) | ✅ | ✅ | **❌ 丢弃** |
| 1D shaper | 1D LUT 单独路径 | — | — | — | ✅(cinespace) | ✅ | **❌ 静默丢弃** |
| 插值 | 四面体 | — | 四面体(默认) | 三线性 | 四面体(默认) | 四面体(默认) | 四面体 |
| 烘焙能力 | 无(直接输出) | 无 | Lua `exportLUT` | 无 | 无 | 无 | **✅ `CUBE LUT` 导出** |
| 输出 ICC | ❌ | — | ✅(XMP/输出 profile) | ✅ | — | — | ✅ 仅 sRGB |
| 执行位置 | 离线 CLI/GUI | 浏览器 WASM | 用户摆放的 IOP | 管线中段 | 任意 | 三槽位 | **两个固定槽** |

---

## 9. 四种"把 LUT 接进管线"的范式

| 范式 | 代表 | 优点 | 代价 |
|---|---|---|---|
| **A. 烘焙**(把适配链折叠进 LUT) | OCIO `ociobakelut`;本仓 `export_adjustments_as_lut` | 运行时零成本;跨平台一致 | 每个 LUT 一份产物;无法处理用户任意 LUT;色域/曲线一变就要重烘 |
| **B. 声明 + 运行时换算** | RawTherapee(文件名 profile)、darktable(下拉)、mpv(`lut-type`) | 一份 LUT 通吃;可处理用户文件 | 需要色彩管理引擎(ICC/矩阵) |
| **C. 固定槽位 + 硬编码** | **本仓现状** | 实现最简单;GPU 上零开销 | 只对一种 Log 正确 |
| **D. 用户摆放** | darktable(模块位置) | 最灵活 | 最易用错 |

> 结论:本仓应保留 C 的**性能**、补 B 的**表达力**、对内置资产用 A。

---

## 10. 本仓现状与差距(逐项代码位置)

| 能力 | 现状 | 位置 | 差距 |
|---|---|---|---|
| 场景参考 LUT | ✅ V-Log 硬编码 | `shader.wgsl:2040-2048`、`:297-303` | 只支持 V-Log |
| 显示参考 LUT | ✅ 曲线之后 | `shader.wgsl:2062-2065` | 正确,无需改 |
| LUT 资产扫描 | ✅ 内置 + 用户目录 | `lut_processing.rs:78-112`、`:487-517` | 无清单/元数据 |
| 内置目录 | `resources/film_luts` | `lut_processing.rs:62-68`、`tauri.conf.json:41` | **OHOS 上解析失败**(见下) |
| domain | ❌ 丢弃 | `lut_processing.rs:204` | 待修 |
| 1D shaper | ❌ 静默丢弃 | `parse_cube` `:184-280` | 待修 |
| 色彩空间标记 | 布尔 | `useEditorActions.ts:262`、`image_processing.rs:2540-2559` | 需枚举 |
| LUT 纹理 | 3D `Rgba16Float` + nearest | `gpu_processing.rs:2142-2194` | 正确 |
| LUT 缓存 | `Arc::ptr_eq` | `gpu_processing.rs:2143-2148` | 够用 |
| 预览 | 逐 LUT 渲染整图 | `lut_processing.rs:653-713` | 首展开有成本;缓存键不含强度 |
| 导出 | 全路径一致 | `export_processing.rs:1100/1504/1663` | 正确 |
| 烘焙 | ✅ CUBE LUT 导出(33³) | `export_processing.rs:1628-1685`、`ExportImportProperties.tsx:10,20` | 主动关闭空间性效果,做法正确 |
| 导出 ICC | 仅 sRGB | `export_processing.rs:1300/1359/1379` | 无 Log ICC |

**平台差异(重要)**:`film_luts_dir()` 走 `BaseDirectory::Resource`;在 OHOS 上 Tauri 走 desktop 解析器,`resource_dir_from()` 对非 Windows/非桌面 Linux 返回 `Err(UnsupportedPlatform)`;且 `src-tauri/gen/ohos/.../rawfile/` 里没有 `film_luts`。结果是**内置胶片模拟在 OHOS 上整体不显示且无日志**。`docs/HARMONYOS_PORTING.md:940` 的"`src-tauri/resources/` 无需打包"结论在胶片资产入库后已过期。修法:仿 `lens_correction.rs:1-11` 用 `include_dir!` 嵌入,或首启解压到 `app_paths`。

---

## 11. 建议路线

### P0 — 正确性(小改动,消坑)

1. `parse_cube` 保留 `DOMAIN_MIN/MAX`;`Lut` 结构加 `domain_min/domain_max`;上传 3D 纹理时按域归一化(`gpu_processing.rs:2153-2158`)。
2. 遇到 `LUT_1D_SIZE` 显式报错(或实现 prelut),禁止静默丢弃。
3. OHOS/Android 的内置 LUT 打包问题(见 §10),否则移动端"胶片模拟"是空列表。

### P1 — 表达力(`lutInput` 枚举)

1. `GlobalAdjustments` 增加 `lut_input: u32`(0=sRGB 显示,1=V-Log,2=F-Log,3=F-Log2,4=S-Log3,5=LogC4…),替代/补充 `lut_is_scene_referred`;老 sidecar 无该字段时按布尔推导(1→V-Log,0→sRGB)。
2. shader 增加对应 OETF 分支(至少 V-Log / F-Log / F-Log2 / S-Log3),色域用预计算 3×3 常量(附录 A 的目标色域),避免引入 colour。
3. 贯通六处:JSON(`image_processing.rs:2540-2559`)、sidecar(`app_settings.rs:149-154`)、复制粘贴分组(`adjustments.ts:897-904`)、选择逻辑(`useEditorActions.ts:247-289`)、预览(`lut_processing.rs:683`)、UI 徽标(`LUTControl.tsx`)。

### P2 — 色彩管理

1. 导出 Log TIFF 时写 ICC(至少把所选 Log 的 profile 写进 `icc_profile` tag),避免"16-bit 但无标记"。
2. 输入侧评估 ICC/DCP:可先"导入时把相机 profile → 线性 sRGB 烘焙成 3D LUT",复用现有 LUT 基础设施,不动渲染管线。

### 验收基准

1. 同一张 RAW,用 Raw Alchemy 输出 `F-Log2 + LUT` 的 TIFF,与本仓同 LUT 渲染结果比 **ΔE00**(24 色卡 + 21 级灰阶),目标 < 1–2。
2. 交错验证:用 FFmpeg `lut3d`(四面体)与 darktable `lut3d` 分别套同一 LUT,确认本仓插值实现无偏差(三者应一致到量化误差)。
3. domain/1D 用例:构造 `DOMAIN_MIN 0.1 / MAX 0.9` 的 cube 与带 shaper 的 cube,验证"拒绝或正确"。

---

## 12. 风险与许可

| 项 | 结论 |
|---|---|
| Raw Alchemy 代码 | **AGPL-3.0**,与本仓一致 → 法律上可复用;但 Python→Rust 属重写,建议只借表格/公式 |
| joraw / libraw-wasm | ISC;LibRaw 为 LGPL-2.1/CDDL;lcms2 为 MIT |
| darktable / RawTherapee | GPL-3.0 → 代码不可并入,算法/格式约定可参考 |
| FFmpeg | LGPL-2.1+ → 代码不可并入(除非动态链接),行为可参考 |
| spektrafilm LUT | CC BY-SA 4.0 + preamble:须署名、保持同许可、不得把 "spektrafilm" 用作产品品牌 |
| 富士官方 F-Log LUT | 免费下载 ≠ 可再分发;不要随包,走"用户导入/应用内下载" |
| 第三方 LUT 包 | 逐仓库确认 LICENSE;大量"免费包"仅限个人使用 |

---

## 附录 A:Log 空间 → 色域表(取自 Raw Alchemy `config.py`)

| Log 曲线 | 线性色域 | 备注 |
|---|---|---|
| F-Log | F-Gamut | |
| F-Log2 | F-Gamut | |
| F-Log2C | F-Gamut C | 编码复用 F-Log2 |
| V-Log | V-Gamut | 与 BT.2020 原色接近 |
| N-Log | N-Gamut | |
| L-Log | ITU-R BT.2020 | |
| Canon Log 2 | Cinema Gamut | |
| Canon Log 3 | Cinema Gamut | |
| S-Log3 | S-Gamut3 | |
| S-Log3.Cine | S-Gamut3.Cine | 编码复用 S-Log3 |
| ARRI LogC3 | ARRI Wide Gamut 3 | |
| ARRI LogC4 | ARRI Wide Gamut 4 | |
| Log3G10 | REDWideGamutRGB | |
| D-Log | DJI D-Gamut | |

## 附录 B:关键代码位置索引

**Raw Alchemy**(`shenmintao/Raw-Alchemy`,v0.4.2)
`src/raw_alchemy/core.py:38-140`(六步)· `config.py:11-32`(表)· `utils.py:54-306`(四面体+domain)· `utils.py:356-390`(预览假解码)· `metering.py:40-261`(测光)· `file_io.py:56-68`(无 ICC)· `preview.py:191-230/397`(半尺寸 + 假解码)

**joraw**(`y-g-jiang/JiangOnlineRaw-joraw-`)
`libraw_wrapper.cpp:21-46`(黑电平)· `:48-89`(raw bypass)· `:1002-1045`(dcraw_process)· `:1047-1076`(缩略图)· `:1078-1247`(参数面)· `:508-593`(Fuji makernote)· `index.js:4-40`/`worker.js`(transfer)· `compileLibraw.sh`(lcms2/SIMD/pthreads)

**darktable**(master)
`src/iop/lut3d.c:57-65,77`(色彩空间枚举)· `:1030-1090`(双向转换)· `:54-55,468-511`(GMZ/48³/缓存)· `lua-scripts/contrib/exportLUT.lua:111-139`(烘焙)

**RawTherapee**(dev)
`rtengine/clutstore.cc:40-47,137-139`(level/uint16)· `:178-268`(三线性 + strength)· `:273-308`(文件名 profile)· `rtgui/tools/filmsimulation.cc:68-192,311-407`(UI/目录)· `rtengine/improcfun.cc:2226-2239,3154-3208`(ICC 往返)

**FFmpeg**
`libavfilter/vf_lut3d.c`:`interp` 默认 `INTERPOLATE_TETRAHEDRAL`;`parse_cube` 的 `DOMAIN_* → scale`;`parse_cinespace` 的 prelut

**libplacebo / mpv**
`src/shaders/lut.c:56-130`(尺寸/域解析)· `:323-460`(texture/uniform/literal 选择)· `:235`(默认四面体)· mpv `vo_gpu_next.c:207-250`(`--lut-type`/三槽)

**本仓**
`lut_processing.rs:62-68/78-112/184-280/475-517/585-589/653-713` · `gpu_processing.rs:2142-2194` · `shader.wgsl:297-303/1560-1627/2040-2065` · `image_processing.rs:1757/2402-2413/2540-2559` · `useEditorActions.ts:247-289` · `app_settings.rs:149-154` · `export_processing.rs:1300/1628-1685/1975-1983`

## 附录 C:术语

- **OETF**:光→电的传递函数,Log 相机曲线的编码侧(如 F-Log2、S-Log3)。
- **Gamut**:一组 RGB 原色及其白点;Log 曲线与 gamut 是两件事。
- **CAT**:色适应变换(如 Bradford),处理 D50(ProPhoto)↔ D65(多数显示空间)白点差异。
- **HaldCLUT**:把 3D LUT 编码成一张 `level³ × level³` 图像(level 8 → 64³ 网格)的做法。
- **shaper / prelut**:3D LUT 之前的 1D 分段线性映射,用来把非均匀分布的信号铺到均匀网格。
- **四面体插值**:把立方体切成 6 个四面体、只读 4 个顶点的 3D LUT 插值,精度优于三线性。
- **DCP / ICC**:相机色彩配置文件;ICC 可含 3D LUT,比 3×3 矩阵精确。
