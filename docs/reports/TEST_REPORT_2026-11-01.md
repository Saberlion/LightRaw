# LightRaw 鸿蒙移植 —— 2026-11-01 平板 + PC 双形态功能测试报告

> 报告编号：TEST-20261101-01
> 执行日期：2026-11-01 22:08 ~ 23:00（北京时间）
> 被测版本：`v1.6.7` @ `2d972d4d`（工作区干净；运行代码等价于 `ab4624be`）
> 用例来源：`docs/TEST_CASES.md`（175 条，本次按"双形态主链路 + 今日改动面"取子集）
> 设备：
> - **PC / DEV-A**：X90 `3XB0124617000013` · HUAWEI MateBook Pro（HAD-W32，`deviceType=2in1`）· HarmonyOS 7.0.0.17 · Maleoon 916（`max_texture_dimension_2d=8192`）· 24.4 GB
> - **平板 / DEV-P**：`7QD0126126000173` · GEM-W40（MatePad Pro Max 悦享款，`deviceType=tablet`）· OpenHarmony 7.0.0.47 · Maleoon 935A · 1298×799 CSS · 3000×1950 px

---

## 1. 结论

| 项 | 结论 |
|---|---|
| **构建与产物** | ✅ `-Release` 全链路通过；单 ABI、ORT + 38 CANN 库注入、零调试信息、无 sourcemap |
| **静态门禁** | ✅ `cargo fmt` / `cargo clippy -D warnings` / `i18n:check` 全绿；前端 typecheck/lint/format 为既有基线（详见 §4） |
| **双机启动** | ✅ 均命中 Vulkan；零新增 faultlog/cppcrash |
| **PC 主链路** | ✅ 授权持久化 → 图库 → 编辑器 GPU → 14 组调整 → 蒙版 → AI（模型下载/前景蒙版）→ 导出直写 |
| **平板主链路** | ✅ 布局/触控形态 → 图库 → 编辑器 → AI 降噪（NPU 模型就绪）→ 导出直写 → 冷/热「用 LightRaw 打开」 |
| **今日改动面（6.23）** | ✅ 保活 `keep-screen`、触控媒体查询、open-with 冷热启动 三项均实测生效 |
| **发布阻塞项** | ⚠️ **隐私管理服务 metadata 缺失**（`gen/ohos` 本地补丁丢失，见 D-02），提审前必须重打 |

---

## 2. 构建与产物（TC-BUILD-05/08/09/10/11/12/13、TC-PAD-12）

| 检查 | 结果 | 证据 |
|---|---|---|
| `scripts/build-ohos.ps1 -Release` | ✅ EXIT=0（22:08→22:30，hvigor 阶段 `OK`） | 构建日志 |
| 产物 | `entry-default-signed.hap` **51.74 MB / 54 条目** | ZIP 条目枚举 |
| 单 ABI | ✅ 仅 `libs/arm64-v8a/`；构建时清除残留 `libs/x86_64/librapidraw_lib.so` | 脚本日志 + 条目 |
| 主库 | `librapidraw_lib.so` 42.95 MB（release，stored） | 条目 |
| ORT | ✅ `libonnxruntime.so` 13.77 MB（压缩 4.98 MB）；`libc++_shared.so` 1.2 MB 同 ABI | 条目 |
| CANN | ✅ 38 个库注入；HAP 内 `.so` 合计 42（主库 + entry + ORT + libc++_shared + 38 CANN） | 条目 |
| 调试信息 | ✅ 42 个 `.so` 零 `.debug_*` / `.symtab`，`dynsym` 完好 | `llvm-readelf -S` |
| sourcemap | ✅ `ets/sourceMaps.map` 不存在 | 条目 |
| `module.json` | ✅ `deviceTypes=["2in1","tablet"]`；`supportWindowMode=[fullscreen,split,floating]`；`minWindowWidth=480`、`minWindowHeight=420`；skills 含 `viewData`/`sendData` | HAP 内 `module.json` |
| 权限 | `INTERNET`、`FILE_ACCESS_PERSIST`(ACL)、`MEDIA_LOCATION` | 同上 |
| 装机 | ✅ 双机 `install -r` 成功（ACL 调试 Profile 重签后；含 ACL 权限不再报 9568289） | `hdc install` |

## 3. 静态门禁（TC-REG-01~06、TC-BUILD-17）

| 门禁 | 结果 | 说明 |
|---|---|---|
| `cargo fmt -p RapidRAW -- --check` | ✅ EXIT=0 | — |
| `cargo clippy --all-targets -- -D warnings` | ✅ EXIT=0 | 剩余 warning 均来自依赖 `rawler`，非本仓 |
| `npm run i18n:check` | ✅ | 15 locale、1628 复数、27222 占位符 |
| `npm run typecheck` | 🟡 65 条 | 与 10-30 报告一致（既有基线） |
| `npm run lint` | 🟡 **895 problems（846 error / 49 warning）** | 文档基线为 849/45，**已漂移**，需回写 `TEST_CASES.md` |
| `npm run format:check` | 🟡 6 文件 | 与 10-30 报告一致（既有） |

## 4. 双机启动与平台（TC-PLAT、TC-PATH、TC-EDIT-01/06/08）

| 检查 | PC（2in1） | 平板（tablet） |
|---|---|---|
| GPU 后端 | ✅ `Maleoon 916 (Vulkan, IntegratedGpu, B318)` | ✅ `Maleoon 935A (Vulkan, IntegratedGpu, B320)` |
| 管线预热 | 22.43 s（首轮） | 3.81 s（首轮）/ 3.87 s（复测） |
| 布局日志 | `deviceType=2in1 mode=full viewport=1101x697 touchUi=false` | `deviceType=tablet mode=wide viewport=1298x799 touchUi=true hoverNone=true coarsePointer=true` |
| 沙箱路径 | ✅ `files/{logs,config,appdata}` | ✅ 同左 |
| 设置持久化 | ✅ `settings.json` 会话中增长至 11834 B | ✅ 11817 B（22:56 落盘） |
| 装机 ORT | ✅ `2b4c3afd…19736`（= 源码构建产物） | ✅ 同左 |
| faultlog | 仅 10-30 18:29/18:30/18:33 三条**修复前**历史记录 | 空 |
| `is_ohos_build` / `device_type` | `true` / `2in1` | `true` / `tablet` |

## 5. PC（2in1）功能执行

| 用例 | 结果 | 证据 |
|---|---|---|
| TC-FILE-01/07、TC-LIB-01/02 | ✅ | 重装后首次「继续会话」提示重授权 → FileKit 选 `Download` → 图库 3 张（`.tiff`/`.fff`/`.jpg`）缩略图 + EXIF 渲染 |
| TC-FILE-08/15（2in1 持久授权） | ✅ | hilog `persistPermission ok … (deviceType=2in1)`；`haps/entry/files/ohos_grants.json` 记录 `{uri,path}`；**force-stop → 重启 → 继续会话直入图库，零 picker** |
| TC-FILE-10 | ✅ | `check_paths_readable([Download])` → `[true]` |
| TC-EDIT-02/04/05 | ✅ | `.fff` 8742×11656（101.9 MP）：`Raw enhancing … 1.35s`；`GPU Processor 3072x3840`；`apply_adjustments 2880x3840 … 2.00s` |
| TC-EDIT-03（交互 ROI） | ✅ | 适应视图下 576×768：73~129 ms/趟（**7.7~16.6 FPS**） |
| TC-ADJ-01/03/04/05 | ✅ | 批量改 14 个滑块（对比度/高光/阴影/白/黑/亮度/色温/色调/鲜艳度/饱和度/锐度/清晰度/去雾/结构）→ 16 次 GPU 重算，全部生效 |
| TC-ADJ-07（蒙版） | ✅ | 蒙版面板「添加新蒙版 → 径向」创建 `蒙版 2`（羽化/不透明度属性 + 重算） |
| TC-ADJ-08/09（镜头/裁剪 UI） | ✅（UI 可达） | 「裁剪与变换」面板含宽高比/旋转/翻转/几何/镜头校正；镜头数据未命中该镜头（`No lens definition … XCD 55`，属 lensfun 库覆盖范围） |
| TC-AI-01（模型下载+持久化） | ✅ | 前景模型 `birefnet_general_lite.onnx` **224 MB** 下载完成并落 `files/appdata/models/` |
| TC-AI（NPU 状态行） | ✅ | `get_ohos_ai_npu_status` → `{available:false, model:"nind_v2.om", sizeBytes:0}`（PC 未侧载，符合预期） |
| TC-AI-05（前景蒙版链路） | ✅（链路） | 蒙版面板加入 `蒙版 3 · 前景`，触发推理期间 `[ohos-keep-screen] hold → release`（22:48:33→22:48:49） |
| TC-EXPORT-02/03 | ✅ | 导出 JPEG 直写 `/storage/Users/currentUser/Download`，新增 `2022.6.14_Swiss_Linksphotograph_edited.jpg`；目录被记忆 |
| TC-EXPORT-15 | ✅ | `Batch Export: 20 cores, 8.4 GB free RAM -> up to 1 threads` |
| TC-EXPORT-10（>8192px） | ⚫ 已知边界 | `Image dimensions (8742x11656) exceed GPU limits (8192). Bypassing GPU processing…`（调整项丢弃，见 D-03） |
| TC-PRIV-06 | ✅ | `get_ohos_privacy_state` → `code 1006700003`（未接入托管）；取文件夹照常（fail-open） |
| TC-UI（keep-screen 新功能） | ✅ | 导出/推理期间 `hold`，任务结束 `release` |

## 6. 平板（tablet）功能执行

| 用例 | 结果 | 证据 |
|---|---|---|
| TC-PAD-03（布局路由） | ✅ | `deviceType=tablet mode=wide viewport=1298x799 touchUi=true hoverNone=true coarsePointer=true`；自绘标题栏不渲染 |
| TC-PAD-04（抽屉/面板） | ✅ | 右栏图标可切「蒙版 / 图像修复 / 导出 / 元数据 / 预设」；全套调整项在抽屉中可见 |
| TC-PAD-05（来源树） | 🟡 | 抽屉可打开，但显示「打开一个文件夹以查看其结构」而**未自动展开 `currentUser` 子目录**（见 D-05） |
| TC-PAD-06（编辑与 GPU） | ✅ | `.fff` 101.9 MP：`Raw enhancing 3.03s`；`GPU Processor 1024x1280`；`apply_adjustments 960x1280 603ms`；交互 384×512 → 131 ms（7.6 FPS） |
| TC-PAD-09 / TC-UI-05（返回手势） | ✅ | 编辑器内注入 `keyEvent Back` → 回图库（未退出应用） |
| TC-LIB-03（会话恢复） | ✅ | 冷启「继续会话」直入图库（2 张，含 `.fff`） |
| TC-AI-01（模型持久化） | ✅ | `birefnet 224MB`、`nind_denoise 124MB`、`nind_v2.om 62MB`、SAM encoder/decoder 全部在沙箱 models |
| **TC-AI NPU 状态** | ✅ | `get_ohos_ai_npu_status` → `{available:true, model:"nind_v2.om", sizeBytes:62093739}`；设置 `aiDenoiseBackend=npu` |
| TC-AI-04（降噪产出） | 🟡 | AI 降噪默认开启时生成全分辨率缓存 `cache/ai_denoise/*.rrdn` **611,380,602 B = 8742×11656×3×2**（f16，与文件尺寸完全吻合）；关闭再开启后 3 分钟内**未重新生成**（见 D-06） |
| TC-EXPORT-02/03（平板直写） | ✅ | 导出 `lightraw-test.jpg` → `/storage/Users/currentUser/lightraw-test_edited.jpg` 落盘；`Batch Export: 8 cores, 3.7 GB free RAM -> 1 threads` |
| **open-with（热）** | ✅ | `OHOS open-with request (warm): …/2022.6.14_Swiss_Linksphotograph.fff` → 编辑器切换 |
| **open-with（冷）** | ✅ | force-stop → `aa start -A ohos.want.action.viewData -U …/lightraw-test.jpg` → `OHOS open-with request (cold start, parked)` → `Frontend is ready, returning initial path: …` |
| TC-UI keep-screen（平板） | ✅ | 推理/导出期间 `[ohos-keep-screen] hold` / `release` 成对出现 |
| TC-PRIV-06（平板） | ✅ | 同 PC：`code 1006700003`，取权 fail-open |

## 7. 缺陷与观察

| 编号 | 现象 | 严重度 | 处置 |
|---|---|---|---|
| **D-02** | **`gen/ohos/.../module.json5` 的 `appgallery_privacy_*` metadata 缺失**（`metadata` 只剩 `ohos.extension.backup`；`.agc-backup` 里也没有）。后果：`get_ohos_privacy_state` 恒返回 `1006700003`，关于页「未启用」，TC-PRIV-01/03/05/07/10 在本包上不可能通过 | **P0（提审阻塞）** | 按 §6.18 第 4 节重打 metadata（属 D-15 已知流程风险的实例化）；重打后需复测隐私链路 |
| D-03 | 2in1（Maleoon 916）上 101.9 MP 文件放大到高分辨率预览时：`Image dimensions (6528x8704) exceed GPU limits (8192). Bypassing GPU processing…` 后紧跟 `[ERROR] Preview worker error: Expected Rgba8 image from GPU for encoding`，该路径预览不更新（调整项丢失） | P1 | 已知边界 D-06（纹理上限）的**用户可见化**：建议旁路时回退到 CPU 处理或降采样路径，并抑制该 ERROR |
| D-04 | 前端门禁基线漂移：`lint` 895（文档 849/45）、`typecheck` 65、`prettier` 6 文件 | P2 | 回写 `TEST_CASES.md` 基线；或清理 |
| D-05 | 平板「来源」树抽屉打开后显示「打开一个文件夹以查看其结构」，未自动展开当前根（§6.21 记录为可见 `currentUser` 子目录） | P2 | 人工复核：`get_folder_tree` 的 `expandedFolders` 初始值是否包含当前根 |
| D-06 | 平板 AI 降噪开关「关→开」后 3 分钟内未重新生成 `.rrdn` 缓存 | P2 | 确认是否只在导出/全分辨率请求时才触发 `ensure_generated`；若预览应即时生效则为 UX 缺陷 |
| D-01 | `build-ohos.ps1` 末尾打印 `size: 108.9 MB`，实际产物 51.74 MB（`ZipFile` Update 模式后读 `$Length` 的时序问题） | P2（工具） | 改为 `Get-Item $hap.FullName` 重新取长度 |
| 观察 | release 包不可调试（无 `webview_devtools_remote` socket），功能级取证需 debug 包；本轮按此执行（与 10-30 报告同法） | — | 可考虑给 release 加 `debug` 变体开关（仅内测） |

## 8. 未执行 / 边界

| 项 | 原因 |
|---|---|
| TC-SIGN-01/02/08（发布签名 + `.app` 打包） | 本轮只做 ACL 调试签名装机；提审包需另行跑 `.csr/pack-app.ps1` |
| TC-EXPORT-01（导出入系统相册）、TC-EXPORT-08（蒙版 alpha PNG）、TC-EXPORT-09（另存为） | 需逐项点选系统对话框；本轮覆盖了直写目录路径 |
| TC-ALBUM-02/03（相册多选导入） | 系统 `PhotoViewPicker` 不接收 `uitest` 注入点击（§6.22），只能人工走查 |
| TC-PRIV-01/03/05/07/10 | 被 D-02 阻塞（metadata 缺失） |
| TC-UI-02（系统深浅模式实时切换） | `param set persist.global.colorMode 1` 不触发窗口配置回调，无法脚本化；需控制中心人工切换 |
| TC-WIN-01/02（系统装饰栏/拖动） | 本轮未注入窗口按钮与拖动 |
| TC-PAD-10/11（悬浮窗宽窄切换）、竖屏 | 需人工拖拽窗口/旋转设备 |
| TC-ADJ-02（曲线绘制）、TC-ADJ-06（字节级等价）、TC-ADJ-10/12 | 曲线需指针绘制；其余需对照导出字节 |
| TC-IMPORT-03/04/05/06（LUT/预设/替换图） | 未逐一取证 |
| TC-REG-07/08（桌面 / Android 回归） | 本轮只测鸿蒙双机 |

## 9. 判定

- **PC 与平板的主链路（启动 → 授权 → 图库 → 编辑 → 调整/蒙版 → AI → 导出 → open-with）在双形态上均可用**，今日 6.23 的三项改动（保活、触控媒体查询、open-with）实测生效。
- **提审前必须处理 D-02**（隐私 metadata），否则 AppGallery 隐私管理服务在发布包里不可用；其余为 P1/P2 体验或工具问题。
- 建议下一步：重打隐私 metadata 复测 TC-PRIV；修复 D-03 的旁路回退；收敛 D-04 基线并回写文档。
