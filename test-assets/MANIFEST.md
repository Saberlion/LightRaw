# LightRaw 测试资源清单(test-assets/)

> 建立:2026-11-02 · 本目录为**本机测试资源**,**二进制不入库**(见 `.gitignore`);本清单、`scripts/fetch-test-assets.ps1` 与 `scripts/make-jpeg-test-assets.ps1` 入库,任何机器可重建整套资源。
> 校验:`test-assets/SHA256SUMS.txt`(SHA-256,小写)。重建后以本清单中的哈希为准。

## RAW(8 个,真实相机文件,约 390 MB)

来源:[raw.pixls.us](https://raw.pixls.us/) 社区 RAW 样张库(真实相机文件,供解码器测试;**仅限本地测试用途,不得再分发**)。下载:`powershell -NoProfile -ExecutionPolicy Bypass -File scripts\fetch-test-assets.ps1`(带断点续传 + 重试 + 体积校验)。

| 文件 | 相机 | 格式 | 说明 | 大小 |
|---|---|---|---|---|
| `raw/sony/ILCE-7M4_DSC06674_FullFrame-LossLess-Compressed-Large.ARW` | Sony A7 IV (ILCE-7M4) | ARW(TIFF) | 全画幅无损压缩 L | 46.9 MB |
| `raw/canon/Canon_EOS_R6M3_IMG_4241.CR3` | Canon EOS R6 Mark III | CR3(ISO BMFF `ftypcrx`) | 现代 CR3 | 33.0 MB |
| `raw/canon/Canon_EOS_5DM4_B13A0732.CR2` | Canon EOS 5D Mark IV | CR2(TIFF) | 传统 CR2 | 14.5 MB |
| `raw/nikon/Nikon_Z8_raw_14bit_lossless.NEF` | Nikon Z 8 | NEF(TIFF) | 14bit 无损压缩 | 58.0 MB |
| `raw/nikon/Nikon_Z8_high_efficiency.NEF` | Nikon Z 8 | NEF(TIFF) | High Efficiency 压缩(解码覆盖测试点;若当前解码器不支持应优雅失败) | 23.3 MB |
| `raw/fujifilm/Fujifilm_X-T5_DSCF0022.RAF` | Fujifilm X-T5 | RAF(`FUJIFILMCCD-RAW`) | X-Trans V | 26.7 MB |
| `raw/hasselblad/Hasselblad_X2D_100C_Job_0079.fff` | Hasselblad X2D 100C | FFF(大端 TIFF) | 100MP 中画幅;≥8192px 纹理上限/内存路径测试主力 | 153.4 MB |
| `raw/panasonic/Panasonic_DC-S5M2_P1126458_mechanical.RW2` | Panasonic Lumix S5 II | RW2(`IIU`) | 机械快门变体 | 35.6 MB |

另:真机上已有的 `2022.6.14_Swiss_Linksphotograph.fff`(同为 X2D 100C,用户实拍)可继续作为 101.9MP 基准。

## JPEG(6 个,合成,许可证干净)

生成:`powershell -NoProfile -ExecutionPolicy Bypass -File scripts\make-jpeg-test-assets.ps1`(GDI+ 合成 + EXIF 注入,零外部依赖)。

| 文件 | 尺寸 | 覆盖点 | 大小 |
|---|---|---|---|
| `jpeg/landscape_4096x2732_noexif.jpg` | 4096×2732 | 无 EXIF 的非 RAW 全管线;色彩斑块(白平衡/颜色混合器测试) | 0.4 MB |
| `jpeg/landscape_4928x3264_exif.jpg` | 4928×3264 | EXIF Make/Model/DateTime 读取 | 0.5 MB |
| `jpeg/portrait_3264x4928_orientation6.jpg` | 3264×4928 | **Orientation=6** 的方向标签处理 | 0.4 MB |
| `jpeg/grayscale_gradient_2048x2048.jpg` | 2048×2048 | 8-bit 灰度渐变 | 0.2 MB |
| `jpeg/gray18_1024x1024.jpg` | 1024×1024 | 18% 灰平场(直方图/白平衡取点) | 17 KB |
| `jpeg/tiny_64x64.jpg` | 64×64 | 极小图(缩略/ROI 边界) | 2.7 KB |

## 哈希(SHA-256)

```
a8012571f6be280e26b9d7219689e9c61560036ccaa0d14838ff8c1fa641efcf  jpeg/gray18_1024x1024.jpg
64d1c204ac9cf3e1704809aa1553aa101978c3150346215d7f3d12fe7e980785  jpeg/grayscale_gradient_2048x2048.jpg
ea3c33028ebbe5a4be9e1440b48ee6a7b78904584689c1032036480880269521  jpeg/landscape_4096x2732_noexif.jpg
2d5c8325225c67505513dd1fe97acc4462995f1c6716ebf157c9feb6f60313e8  jpeg/landscape_4928x3264_exif.jpg
743d71b4b4ecd3d5b1438e422200ea12c52bc6a24267821e93685e3c76eac9ee  jpeg/portrait_3264x4928_orientation6.jpg
9b5cd8bf1d818fadddcb4cc95b293bf74c7e84ba8d1256df865cb63b27f7042a  jpeg/tiny_64x64.jpg
25d928f9e9a65525ac796bdff13e93b7b36b627568d7a17338e12dc44424078a  raw/canon/Canon_EOS_5DM4_B13A0732.CR2
e491e4bb960961b5fa299361bf698310a80cbe7b15d30d8dad3bb21bc5457dab  raw/canon/Canon_EOS_R6M3_IMG_4241.CR3
cddd7ae0c43f9280876e5fdcbdf8878718972ebd3d034d8affc8cfc6752d33dc  raw/fujifilm/Fujifilm_X-T5_DSCF0022.RAF
44c38665694e07baf47f1fc34fb9ea0f1bf70eb71b7390183d3c0ef23d164e0f  raw/hasselblad/Hasselblad_X2D_100C_Job_0079.fff
82df041542b8d328738443bb5ff3373b396e406ecc8cbe7dc3b35252790c53e7  raw/nikon/Nikon_Z8_high_efficiency.NEF
c13a8675294456b9c979e6c1e556951654f35377b2870af2874bfb556f21ce40  raw/nikon/Nikon_Z8_raw_14bit_lossless.NEF
53a171521a7f9af6910b4ab61c5a4a42de0cceb24166e15b07eee1e00675028f  raw/panasonic/Panasonic_DC-S5M2_P1126458_mechanical.RW2
851b43c2116c4139104a5036f83ac3b6a148789b2142214dd7192c13972b25b6  raw/sony/ILCE-7M4_DSC06674_FullFrame-LossLess-Compressed-Large.ARW
```

## 已知问题(首个由本语料抓到的缺陷)

**D-TA-01(2026-11-02,2in1 实机复现 3 次)**:在 debug 包上,**加载 `Canon_EOS_5DM4_B13A0732.CR2` 后紧接着加载下一个 RAW(再次 CR2 或 X-T5 RAF)→ `PANIC! at vendor\rawler\src\pixarray.rs:119:5 - assertion failed: self.initialized`**,且该次 `load_image` 的 promise **永远不 resolve**(CDP 240s 超时,UI 无提示、编辑器卡在加载)。单独加载同一 RAF/NEF 正常(NEF 高效压缩走内嵌预览兜底,符合设计);panic 后进程存活、后续加载恢复。`pixarray.rs:119` 是 `pixels()` 的 `debug_assert!(self.initialized)` —— release 构建不会触发该断言,但**状态泄漏(PixArray 未初始化即被读取)与 invoke 挂起与构建 profile 无关**。待办:① release 包复测;② 定位跨加载的状态泄漏(疑似 rawler 全局/静态解码状态);③ `load_image` 应在 panic 边界 resolve(而非挂起)。

## 用法

- **桌面/模拟器**:把 `test-assets/raw` 与 `test-assets/jpeg` 里的文件放进一个文件夹,在应用里「打开文件夹」选它。
- **鸿蒙设备**:`hdc file send <file> /data/app/el2/100/base/io.github.CyberTimon.RapidRAW/files/appdata/library/`(沙箱库根;或推到设备公共目录后用 FileKit picker 选目录)。
- 校验:`powershell ... fetch-test-assets.ps1 -CheckOnly`。
