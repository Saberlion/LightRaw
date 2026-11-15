# LightRaw 测试矩阵

> 本文件是 `.opencode/skills/lightraw-qa/SKILL.md` 的附件:逐资产的覆盖点与预期结果。
> 语料权威来源:`test-assets/MANIFEST.md`(哈希、大小、来源)。冲突时以 MANIFEST 为准并回改本文件。

## 通用步骤(每个资产)

1. 在应用中「打开文件夹」选中包含 `test-assets/raw` + `test-assets/jpeg` 内容的文件夹(或单文件打开)。
2. 等待缩略图/预览生成 → 双击进入编辑器视图。
3. 验证下方矩阵中该资产的覆盖点。
4. 通用预期(所有资产):
   - `load_image` 的 promise **必须 resolve**(无论成败);加载失败要给出 UI 提示,不允许无限挂起。
   - 无 Rust panic(观察日志/CDP);`vendor\rawler\src\pixarray.rs` 的 debug 断言不触发。
   - 编辑器视图可正常进入/退出,切换到下一个资产后状态干净(无跨加载泄漏)。

## RAW 资产(8 个)

| # | 文件(相对 `test-assets/`) | 相机/格式 | 覆盖点 | 预期 |
|---|---|---|---|---|
| R1 | `raw/sony/ILCE-7M4_DSC06674_FullFrame-LossLess-Compressed-Large.ARW` | Sony A7 IV · ARW(TIFF) | 全画幅无损压缩 L 解码 | 正常解码,全分辨率可编辑 |
| R2 | `raw/canon/Canon_EOS_R6M3_IMG_4241.CR3` | Canon EOS R6M3 · CR3(ISO BMFF `ftypcrx`) | 现代 CR3 容器 | 正常解码 |
| R3 | `raw/canon/Canon_EOS_5DM4_B13A0732.CR2` | Canon 5D Mark IV · CR2(TIFF) | 传统 CR2;**D-TA-01 触发源** | 正常解码;其后紧跟加载 R2/R5 不得 panic/挂起 |
| R4 | `raw/nikon/Nikon_Z8_raw_14bit_lossless.NEF` | Nikon Z 8 · NEF(TIFF) | 14bit 无损压缩 | 正常解码 |
| R5 | `raw/nikon/Nikon_Z8_high_efficiency.NEF` | Nikon Z 8 · NEF(TIFF) | High Efficiency 压缩(解码覆盖点) | **优雅失败**:若解码器不支持,走内嵌预览兜底,不许崩溃 |
| R6 | `raw/fujifilm/Fujifilm_X-T5_DSCF0022.RAF` | Fujifilm X-T5 · RAF(`FUJIFILMCCD-RAW`) | X-Trans V 传感器 demosaic | 正常解码 |
| R7 | `raw/hasselblad/Hasselblad_X2D_100C_Job_0079.fff` | Hasselblad X2D 100C · FFF(大端 TIFF) | 100MP 中画幅;≥8192px 纹理上限/内存路径主力(153.4 MB) | 正常解码,或按设计走超大图降级路径;不 OOM、不崩溃 |
| R8 | `raw/panasonic/Panasonic_DC-S5M2_P1126458_mechanical.RW2` | Panasonic S5 II · RW2(`IIU`) | 机械快门变体 | 正常解码 |

## JPEG 资产(6 个)

| # | 文件(相对 `test-assets/`) | 覆盖点 | 预期 |
|---|---|---|---|
| J1 | `jpeg/landscape_4096x2732_noexif.jpg` | 无 EXIF 的非 RAW 全管线;色彩斑块(白平衡/颜色混合器) | 无 EXIF 不报错;调白平衡/颜色混合器取到正确色块 |
| J2 | `jpeg/landscape_4928x3264_exif.jpg` | EXIF Make/Model/DateTime 读取 | 元数据显示 `LightRaw` / `Test Camera Synthetic` / `2026:11:02 10:00:00` |
| J3 | `jpeg/portrait_3264x4928_orientation6.jpg` | **Orientation=6** 方向标签 | 显示为旋转 90°(横向)后的正确方向 |
| J4 | `jpeg/grayscale_gradient_2048x2048.jpg` | 8-bit 灰度渐变 | 识别为灰度;渐变平滑无色偏 |
| J5 | `jpeg/gray18_1024x1024.jpg` | 18% 灰平场 | 直方图集中于中部灰阶;白平衡取点(灰吸管)应得到接近中性结果 |
| J6 | `jpeg/tiny_64x64.jpg` | 极小图边界 | 正常加载;缩略/ROI/缩放不越界、不死循环 |

## 回归序列(每次必测)

| # | 序列 | 来源 | 预期 |
|---|---|---|---|
| T1 | 加载 R3 → **立即**加载 R3(再次 CR2) | D-TA-01 | 无 panic;两次 promise 均 resolve |
| T2 | 加载 R3 → **立即**加载 R6(RAF) | D-TA-01 | 无 panic;两次 promise 均 resolve |
| T3 | 单独加载 R5、R6 | D-TA-01 对照 | 正常(HE NEF 走预览兜底) |
| T4 | 连续快速切换 R1–R8 + J1–J6(顺序遍历,不等待停顿) | 跨加载状态泄漏 | 全部 resolve;无累积内存增长导致的失败 |

## 鸿蒙设备端追加项

- 推送命令见 SKILL.md §4;目标路径 `/data/app/el2/100/base/io.github.CyberTimon.RapidRAW/files/appdata/library/`。
- 追加验证:大文件(R7, 153 MB)经 FUSE 沙箱路径读取不失败(EACCES/EPERM 类问题重点观察)。
- 真机上用户实拍 `2022.6.14_Swiss_Linksphotograph.fff`(X2D 100C, 101.9MP)可作第二基准。

## 结果记录模板

```
日期:YYYY-MM-DD · 构建:debug/release · commit:<hash> · 执行模型:alibaba-cn/deepseek-v4.1-flash-1017
R1 通过 / R2 通过 / ... (失败附现象+复现次数)
T1–T4:通过/失败
新缺陷编号:D-TA-XX(现象、构建 profile、复现次数)
```
