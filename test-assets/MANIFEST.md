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

**D-TA-01 复测更新(2026-11-15,GEM-W40 / 7QD0126126000173,debug,HEAD `fb632f0a`,复现 4/4)**:

- **症状变化**:CR2 现在**首次单独加载即失败**,不再需要“后接下一个 RAW”。4 次尝试(R3 单载、T1 第二次、T4b 顺序遍历、UI 双击)全部返回 `task N panicked with message "assertion failed: self.initialized"`(2.1–2.3 s)。**promise 会 resolve**(旧缺陷的“永不 resolve/240 s 挂起”已不复现);进程存活,随后加载 RAF/ARW 正常;库网格里 CR2 的元数据(1/3200 s f/2.8 ISO100 40 mm)正常显示,双击时前端 `toast.error("Failed to load image: task N panicked …")` 并把 `selectedImage` 置空后退回图库(错误文案把 Rust panic 原文暴露给了用户)。
- **根因(静态定位,已可闭合)**:`decode/as_shot.rs` 用 rawler 的 dummy(仅元数据)模式 `decoder.raw_image(&source, …, true)`;`vendor/rawler/src/decoders/cr2.rs:167` 在调用 `convert_to_rgb(…, ljpegout.pixels_mut(), dummy)` 时**先求值 `pixels_mut()`**,而 dummy 的 PixArray 来自 `PixU16::new_uninit`(`Vec::with_capacity`,len=0,`initialized=false`)→ debug 下触发 `pixarray.rs:119 debug_assert!(self.initialized)`;`convert_to_rgb` 体内的 `if dummy { return Ok(()) }` 只挡写入、挡不住实参求值时的断言。5D4 CR2 满足 `cpp==3 && Cr2StripeWidths==0` 分支;ARW/CR3/NEF/RAF/RW2 不走该分支,故只有 CR2 命中。
- **结论修正**:不是“跨加载的全局状态泄漏”,而是**单次加载内 dummy 读路径的 debug 断言误报**;`release` 不编译 `debug_assert`,预计 CR2 正常(仍待 release 包复测确认)。旧记录里“CR2 装载成功、下一个 RAW 才 panic”只是同一 dummy 读在不同调用时机被触发,CR2 自身其实一直在带病返回。
- **处置建议**:① `load_image` 里 `as_shot` / exif 读取同样加 `catch_unwind`,debug 包不再整次加载失败;② rawler 侧把 `pixels_mut()` 移入 `if !dummy` 分支;③ 前端把该错误降级为“无法解码该 RAW”,不要透出 `task N panicked …`。

**D-TA-01 已修复(2026-11-15,同机复测通过)**:

- **精确根因**:宿主探针(`rawler` 直接 `raw_image(…, dummy=true)` + `RUST_BACKTRACE=1`)栈顶为 `cr2.rs:245`(另一处与 167 行同构)——两处 `convert_to_rgb(…, pixels_mut(), dummy)` 都在 `dummy` 分支上先求值 `pixels_mut()`。修复:`cr2.rs` 两处调用改为 `if cpp == 3 && !dummy`(`convert_to_rgb` 本身对 dummy 已经是 no-op,语义不变)。
- **纵深防御**:`decode/as_shot.rs::cached_or_read` 与 `decode/image_loader.rs::decode_pristine` 的 exif 读取都加了 `catch_unwind`,即使将来别的格式再踩同类 dummy 断言,也只是 as-shot/exif 降级,不会整次加载失败。
- **复测(debug,同一 GEM-W40,`hdc install -r` 升级包)**:CR2 首次单独加载 **OK 3360×2240、2.0 s**;T1(CR2→CR2)第二次 OK;T2(CR2→RAF)OK;全矩阵 14 资产 + T4a/T4b 遍历 **0 次 panic**(`grep -cE 'PANIC!|Panic while'` = 0)。白平衡从修复前的 reference 6503 K 恢复为相机值 **4992 K / tint 15.4**,证明 as-shot 元数据读已真正走通。
- **附注**:该样张是 **Canon sRAW**(3360×2240、cpp=3),所以此前“半分辨率”不是缺陷;也正因为 cpp==3 才命中这条 CR2 分支。

**D-TA-02 已修复(2026-11-15,同机复测通过)**:

- **精确根因**:`decode/develop.rs::finish_rgb` 把整帧 f32 RGB(12 B/px)上传成单个 buffer。真机 `max_buffer_size` = **1 GiB**,100 MP 需要 1 142 761 024 B(`Develop Input`)→ `MaxBufferSize` validation 错误 → 整个 develop 失败 → 内嵌预览。与纹理尺寸无关(设备 `max_texture_dimension_2d` = 16384,11904×8842 并不越界)。
- **修复**:① `decode/raw_processing.rs` 的 GPU develop 前增加预算门控(`max_texture` + `min(max_buffer_size, max_storage_buffer_binding_size)`,camera plane 12 B/px、raw plane 2 B/px),超限直接打 warn 走 CPU 去马赛克;**fast(缩略图)路径按 `bin()` 折叠后尺寸门控**(Bayer /4、X-Trans /3,保守取 /3),否则 100 MP 缩略图会被误踢去全分辨率 CPU develop;② GPU finish 同样加预算,超限时在 CPU 上做 camera→sRGB 矩阵 + 色域压缩 + clamp(跳过 decode 期 denoise/sharpen 并 warn);③ `develop.rs` 的 validation 详情走 `log::error!("{error:?}")`,返回给前端的错误保持短文案(避免 Debug 串灌进 toast)。
- **数据契约(易踩,写在这里防回归)**:GPU finish shader(`develop.wgsl::to_output`)先 `p * neutral`(= 1/wb,抵消 wb 预乘)再乘 camera→sRGB 矩阵,所以 GPU 分支喂 **wb 预乘后**的数据;CPU 的 `CameraToSrgb::pixel()` 是矩阵直乘(as-shot 白点已折叠在矩阵里),必须喂**相机原生值**。CPU 分支若照抄 GPU 分支的 `* wb[c]` 预乘,白平衡会被算两次 → 灰卡偏色(初版修复踩过,已改)。
- **复测(含颜色校验)**:R7 `.fff` **11656×8742(101.9 MP)全分辨率加载成功、7.0 s**,`is_embedded_preview=false`;渲染 2048 长边 1.33 MB / 9.7 s。颜色以 `.fff` 内嵌预览为基准(通道均值比 R/G=1.004、B/G=0.984,近中性场景):CPU finish 渲染 640px 得 **R/G=0.991、B/G=1.025**,在应用自身色调曲线的正常偏差内(双重 WB 会偏 1.5× 以上,一眼可辨)。R7 缩略图走 GPU fast 路径(日志无 `fast=true` skip);首次 `generate_preview_for_path` 对 100 MP 全管线约 49 s(decode 7 s + 平铺 adjustments 渲染),之后有缓存——大图预览首代偏慢属管线既有行为,非本修复引入。全矩阵再跑一遍无回归;设备峰值内存 **VmHWM 5.0 GB**(100 MP CPU 路径,12 GB 设备可承受),跑完 RSS 回落(无泄漏)。
- **CPU finish 与 GPU 的差异(超限才走,已 warn)**:无高光重建(hl_* 阶段)、无 decode 期 denoise/sharpen;输出经 `apply_orientation` 在 CPU 侧旋转(与 shader 的 `source()` 映射等价)。
- **后续可优化**:CPU finish 目前同时持有中间 f32 RGB 与输出,峰值偏高;若要压到 3 GB 以内,可把 CPU finish 按行分块写进输出缓冲(无 GPU 依赖,改动局限在该函数)。8 GB 平板 + 100 MP 组合可能触发 LMK,需实机验证。

**D-TA-02(2026-11-15,GEM-W40,debug,HEAD `fb632f0a`,复现 2/2)**:`Hasselblad_X2D_100C_Job_0079.fff`(100 MP)GPU develop 失败——app.log `Error developing RAW file '…fff': GPU develop failed: Validation Error`,随后走内嵌预览兜底,编辑器拿到 **1295×971**。不崩溃、不 OOM(全程 VmHWM 1.46 GB / 12 GB)。矩阵允许"超大图降级路径",但 1295×971 远低于可用编辑质量,且根因是 wgpu Validation 错误 → 需查 100 MP develop 是否绕过了 `>8192` 的 CPU 旁路与纹理分块门控(设备 GPU:Maleoon 935A / Vulkan 1.3)。

**D-TA-03(2026-11-15 发现并当日修复,MOR-M1 / MateBook Pro S 复现+验证)**:合并 `fb632f0a`(上游模块重构)后,**图库/胶片栏缩略图全部不显示**——后台缩略图 worker 整段静默跳过,前端永远收不到 `thumbnail-generated`,库头忙碌指示永不落定(对应 CPU_IDLE_SPINNERS.md "队列非空但永不完成"现象)。

- **根因**:`library/thumbnails.rs` 的 `resolve_thumbnail_cache_dir`/`clear_thumbnail_cache` 用回了 tauri 桌面语义 `app_handle.path().app_cache_dir()`,绕过 `crate::app_paths`(违反 AGENTS.md OHOS 规则)。真机上该路径解析为 FUSE 用户存储 `$HOME/.cache/<bundle>`(`/storage/Users/currentUser/.cache`,实测不存在、EPERM),`create_dir_all` 失败 → `start_thumbnail_workers` 的 `if let Ok(cache_dir)` **无 else、无日志**吞掉错误。而 10 月 `964d4eab` 的 app_paths 修复留在了旧 `file_management.rs`——**该文件(连同 `ai_denoise.rs`/`ai_processing.rs`/`gpu_processing.rs`/`export_processing.rs`/根 `image_processing.rs` 共 6 个旧 monolith)合并后完全未参与编译**(无任何 `mod` 声明指向它们),修复被留在死代码里,git 考古/grep 还会持续误导。
- **修复**:① 两处改走 `crate::app_paths::app_cache_dir`;② worker 的 Err 分支改为 warn 日志 + `thumbnail-generation-error` 事件 + progress 计数,队列不再无声卡死;③ `platform/main_window.rs` 桌面门内的 `app_config_dir` 也统一走 app_paths(桌面端原样委托,行为不变),使门禁可零例外全仓扫描;④ **删除 6 个孤儿 monolith 文件**,根除"修到死代码里"的漂移源;⑤ 新增 `scripts/check-app-paths.sh` + lint.yml 硬门 `app-paths`:全仓禁止 app_paths.rs 之外出现 `.app_data_dir()/.app_cache_dir()/.app_config_dir()/.app_local_data_dir()` 直调(已用探针验证能拦截)。
- **验证(MOR,release 包)**:`update_thumbnail_queue` 推入 14 资产 → `cache/thumbnails` **78 个文件全新落盘**、app.log 0 条 "Thumbnail cache unavailable";CDP 点「继续会话」进图库,DOM 出现 `asset://localhost/…/data%2Fstorage%2Fel2%2Fbase%2Fcache%2Fthumbnails…` 的 `<img>` 且截图网格可见真实照片缩略图(asset 协议 scope 白名单 `/data/storage/el2/base/cache/thumbnails/*` 只有在写入路径正确后才会生效);同一包全矩阵 14/14、T1–T4 通过、0 panic。
- **测试配方注记**:release 包默认**没有 CDP**(tauri 的 devtools feature 仅 debug 默认开启;wry OHOS 后端 `#[cfg(any(debug_assertions, feature="devtools"))]`),uinput 合成触摸进不了 ArkWeb(系统弹窗可以)。真机跑 release 脚本化测试的配方:Cargo.toml 的 tauri features 临时加 `"devtools"` 出测试变体 → `sign-injected-hap.ps1 -Config debug` 重签(debug profile 实测 MateBook Pro S 接受安装);**AGC 提审构建前必须去掉该 feature**。另:普通 release 包的 UI 驱动测试只能走截图 + `uitest dumpLayout` 轮询。

## 用法

- **桌面/模拟器**:把 `test-assets/raw` 与 `test-assets/jpeg` 里的文件放进一个文件夹,在应用里「打开文件夹」选它。
- **鸿蒙设备**:`hdc file send <file> /data/app/el2/100/base/io.github.CyberTimon.RapidRAW/files/appdata/library/`(沙箱库根;或推到设备公共目录后用 FileKit picker 选目录)。
  - **路径命名空间坑**:推送路径 `/data/app/el2/100/base/<bundle>/files/…` 只在 hdc/shell 侧可见;应用内读同一文件要传 `/data/storage/el2/base/files/…`。用前者调 `load_image` 会立刻返回 `Fallback read failed … No such file or directory (os error 2)`(脚本化测试时最容易踩)。
  - 公共目录方式:推到 `/storage/Users/currentUser/Download/…` 后,应用已持久化的库根(默认 Download)重启即索引,可做纯 UI 流程验证。
- 校验:`powershell ... fetch-test-assets.ps1 -CheckOnly`。
