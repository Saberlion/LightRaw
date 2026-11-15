# TC 用例集速查卡

> 本文件是 `.opencode/skills/lightraw-qa/SKILL.md` 的附件,内容摘自 `docs/TEST_CASES.md`(2026-10-25 版,192 条用例)。
> 仅作速查:执行时以 `docs/TEST_CASES.md` 原文为准;两边冲突时以 TEST_CASES.md 为准并回改本文件。

## 用例编号与优先级

- 编号:`TC-<模块>-<序号>`;性质标签:`[功能]` `[回归]` `[性能]` `[静态]` `[探索]`。
- 优先级:**P0** 阻塞发布/核心链路 · **P1** 重要功能/主路径 · **P2** 次要/外观/运维。

## 模块地图(§2 索引 + §3 明细位置)

| 模块 | 前缀 | 用例数 | 关注点 | 明细 |
|---|---|---|---|---|
| 构建与打包 | TC-BUILD | 18 | 双目标编译、出包脚本、ORT/CANN 注入、strip | §3.1 |
| 平台判定与 cfg 门控 | TC-PLAT | 7 | `is_ohos_build`、dialog 三处排除、cfg 等价性 | §3.2 |
| 沙箱路径 | TC-PATH | 8 | `app_paths` 四映射、设置/日志/asset 协议 | §3.3 |
| FileKit 文件桥 | TC-FILE | 15 | 目录/文件/另存为桥、授权持久化、重授权自愈 | §3.4 |
| 导入链 | TC-IMPORT | 7 | FAB/选择器/LUT/预设消费点 | §3.5 |
| 导出链 | TC-EXPORT | 15 | 直写目录、相册桥、13900002/14000011、大尺寸分块 | §3.6 |
| 窗口控制 | TC-WIN | 7 | 系统装饰栏、拖动桥、exit_app | §3.7 |
| 系统 UI 适配 | TC-UI | 12 | 深浅模式、返回手势、协议门控、存储兜底 | §3.8 |
| 图库 | TC-LIB | 8 | 扫描、缩略图、会话恢复、元数据 | §3.9 |
| 编辑器 GPU 管线 | TC-EDIT | 12 | 后端选择(Vulkan)、回读路径、预热、直渲评估 | §3.10 |
| 调整项 | TC-ADJ | 12 | 基础/曲线/色彩/细节/效果/蒙版/镜头 | §3.11 |
| AI 蒙版与降噪 | TC-AI | 15 | 模型下载持久化、ORT/NNRT/CANN、回退 | §3.12 |
| 性能与内存 | TC-PERF | 7 | 高像素基线、交互 FPS、预热 | §3.13 |
| 签名与上架 | TC-SIGN | 10 | HAP/.app 双层签名、AGC 991/权限对齐 | §3.14 |
| 回归与静态门禁 | TC-REG | 9 | fmt/clippy/tsc/eslint/i18n、桌面回归 | §3.15 |
| AppGallery 隐私管理 | TC-PRIV | 13 | 隐私链接、签署状态、取权门控、未上架预置 | §3.16 |
| 平板形态 | TC-PAD | 12 | 抽屉交互、悬浮窗宽窄、重启重授权 | §3.17 |
| 相册导入 | TC-ALBUM | 5 | 相册 FAB、多选导入、暂存清理 | §3.18 |

> 注:TEST_CASES.md §2 索引合计 175 条(未含后来补充的 TC-PAD 12 条与 TC-ALBUM 5 条);最新一轮全量实跑 192 条(见 `docs/reports/TEST_REPORT_2026-11-07.md`)。

## 执行环境矩阵(§1.4)

| 代号 | 环境 | 关键特征 | 用途 |
|---|---|---|---|
| HOST | Windows 11 开发机 | Rust 1.98/1.99 MSVC · CMake+Ninja · DevEco SDK API 26 | 宿主构建、静态门禁、出包脚本 |
| SIM | DevEco x86_64 模拟器 | 4 GB RAM · Mali-G77 GLES 透传 · 无 Vulkan/NNRT/CANN | 快速功能验证(性能绝对值不可外推) |
| DEV-A | 真机 HAD-W32(2in1 平板) | Kirin + Maleoon 916 · `deviceType=2in1` · 23.3 GiB RAM · `max_texture_dimension_2d=8192` | 主验证机:全链路 + 持久化 + CANN |
| DEV-B | 真机 MateBook Pro S | Maleoon 935 · `max_texture_dimension_2d ≥ 11776` · ≥12 GiB | 大尺寸 GPU 导出色块/分块验证 |
| DEV-C | 真机 pad 形态(GEM-W40 等) | `deviceType=tablet` | 双形态持久化矩阵 + 平板布局 |
| DESK/AND | 桌面 / Android 构建 | Windows/macOS/Linux/Android | 回归等价性冒烟 |

## 工具与取证速查(§1.5)

| 能力 | 命令 / 手段 |
|---|---|
| 安装/启动 | `hdc install -r <hap>` / `hdc shell aa start -a EntryAbility -b io.github.CyberTimon.RapidRAW` |
| 截屏 | `hdc shell snapshot_display -f /data/local/tmp/x.jpeg` + `hdc file recv`(以**字节数**判界面状态) |
| 触摸/按键 | `hdc shell uinput -T -m x y x y 120`(带位移即 swipe);`hdc shell "uitest uiInput keyEvent Back"` |
| UI 元素定位 | `hdc shell uitest dumpLayout`(**加 `-i` 才能看到系统 picker 窗口**) |
| Webview 调试 | `hdc -t <key> fport tcp:9222 localabstract:webview_devtools_remote_<pid>` + CDP `Runtime.evaluate` |
| 应用日志 | `/data/app/el2/100/base/<bundle>/files/logs/app.log`(Rust fern;fast mode) |
| 系统日志 | `hdc shell "hilog -x \| grep -E 'LowMemoryKill\|onAbilityDied\|CppCrash'"`(事件后 2~4s 内抓) |
| 内存 | `hdc shell "hidumper --mem <pid>"`(PSS 分解) |
| 出包 | `scripts/build-ohos.ps1 [-Target x86_64] [-Release] [-SkipBuild]`(仓库根执行) |
| 平滑度 | `bench/replay.js` + `bench/analyze.mjs`(仅同机前后可对比) |

## 静态门禁(§1.6,CI 硬门禁)

```powershell
# 前端(仓库根)
npm run typecheck; npm run lint; npm run format:check; npm run i18n:check
# Rust(src-tauri/)
cargo fmt -p RapidRAW -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check
cargo check --target aarch64-unknown-linux-ohos
```

## 当前状态基线(2026-11-07 双机全量,192 条)

✅142 · 🟡22 · 🔴3 · ➖1(N/A)· ⚪24。主要待办(源自 `docs/TEST_REPORT.md` §1):

- 真机 ≥60MP 基线与 AI 分级测试(TC-PERF-04 / TC-AI-05)
- AGC 权限/隐私一致性审核走查(TC-SIGN-04/05)
- 隐私管理服务 release 降级路径后续观察(TC-PRIV)
- 直渲 PoC 未立项(TC-EDIT-11)
- 桌面/Android 实机回归冒烟(TC-REG-07/08)

## 范围外(§4,不要误当遗漏)

tethering(OHOS 不构建)、云服务/社区页后端、桌面三平台全功能验收、AGC 正式上架人工流程、自动化回归(CI headless,已明确不做)。
