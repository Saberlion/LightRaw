# 空闲 CPU 占用高的分析与修复(2026-11-02)

> 现象:应用在设备上空闲(不操作)时 CPU 持续偏高、风扇转。
> 测量环境:2in1(HAD-W32,20 核),应用停在图库/编辑器,无输入。
> 工具:`/proc/<pid>/task` tick 采样(10s 窗口)、CDP Profiler、DevTools timeline trace、`document.getAnimations()`。
> **状态:根因已修复(见 §6),修复后实测 renderer 空闲 CPU 53% → ≈0。**

## 结论(TL;DR)

**根因是 6 个永不停止的 `animate-spin` 加载图标,把页面钉在 60fps 的完整 style→layout→paint→composite 生命周期上**。这些 spinner 对应两个"卡死"的 loading 状态(AI 待处理徽标、缩略图忙碌指示),其源头又与 D-07(解码 panic 后任务永不完成)同源。

| 层 | 空闲 CPU(占单核) | 停掉动画后 |
|---|---|---|
| ArkWeb renderer 主线程(页面进程) | **53%** | **1.1%** |
| ArkWeb Compositor | 11~26% | ≈0 |
| 应用 Rust 主线程 | ~10% | ~10%(次要,见 §4) |
| 应用进程合计 | ≈1 核当量持续消耗 | ≈0.15 核 |

## 证据链

1. **tick 采样**(idle,10s):renderer 主线程 529/1000 ticks;app 主线程 93/1000。JS invoke 5 秒内 **0 次**(排除前端轮询)。
2. **CDP Profiler**(5s,3971 samples):JS 函数占比 ≈ 0%,`50.8% (idle) + 49.2% (program)` → 时间花在**原生渲染**而非 JS。
3. **timeline trace**(6s,73334 events):`ProxyMain::BeginMainFrame` ×179(~30fps),配套每帧全套 `UpdateStyleAndLayout / Layerize / PaintArtifactCompositor::Update / pushPaintArtifactToCompositor`——空闲时每帧全量走样式+布局+绘制+合成。
4. **`document.getAnimations()`**:6 个 `playState:"running"` 的 `spin` 动画:5 个在缩略图角标(`data-tooltip="等待 AI 处理"`,即 `library.aiJobs.thumbnailPending`)+ 1 个在图库头部(缩略图忙碌指示,`isBusyLoaderMounted`/`hasThumbnailWork`)。
5. **因果验证**:CDP 注入把 6 个动画 `style.animation='none'` → renderer 主线程 53% → **1.1%**(~50×)。

## 卡死状态的来源

| Spinner | 代码 | 驱动状态 | 为什么卡 |
|---|---|---|---|
| 每缩略图「等待 AI 处理」 | `LibraryItems.tsx:61,367-372`(`aiPendingPaths[path]`) | `ai-jobs-pending` 事件的 `added`/`removed` **增量对账** | 队列实际已空(`ai_jobs.json` = `{"pending":[]}`、`get_ai_jobs_status` queued=0),但前端 `aiPendingPaths` 仍留着 5 条——增量事件丢失/跨重启即永久泄漏;另外 `ai_jobs::prioritize()` 对 sidecar 里带 pending AI job(昨天测试留下的"蒙版 3 · 前景")的图每次扫描都会重新 enqueue |
| 图库头部忙碌指示 | `MainLibrary.tsx:549`(`hasThumbnailWork`) | 缩略图队列 | 有请求永不完成 → busy 常真。与 **D-07** 同源:解码 panic(`rawler pixarray.rs:119`)后任务挂起,上游一切"进行中"状态不落定 |

**结构性问题**:任何**常驻 CSS 动画**(哪怕 12px 的 spinner)都会让 Blink 每个垂直同步跑一帧完整生命周期;这些 spinner 还被渲染在**每个缩略图**上(列表里有 N 张待处理图就有 N 个),并被大量 canvas/DOM 放大成本。

## 交互期(操作时)CPU——设计内/已知

- **compute+回读管线**(§9.4 未立项直渲):每次滑块改动 = GPU compute → CPU `downscale_f32_image` → **q94 JPEG encode(15-25ms)** → IPC → **renderer 再 JPEG decode** → canvas 绘制。交互 8~16 FPS,合计约 1 核当量。
- **爆发任务**:`estimate_export_size` 全分辨率 1.5~2.2s/次且面板内反复触发(实测 4 次/分钟);`generate_uncropped_preview` 2s/次;缩略图同路径重复生成(实测秒级 5 次);AI CPU 降噪 100MP ≈12 分钟占满所有核。

## 次要:Rust 主线程 ~10%(10 ticks/s)

空闲时 JS 0 invoke,排除前端轮询 → **原生侧存在 ~10Hz 周期任务**(候选:缩略图队列轮询、wgpu/事件循环周期 tick、桥接层定时器)。量级小,留待 perf pass。

## 修复记录(2026-11-02,已装机验证)

| 改动 | 内容 |
|---|---|
| `ai_jobs.rs` | `AiJobsStatus` 新增 **`pending: Vec<String>`**(队列 + 当前处理中的权威全集),`refresh_status()` 统一填充,每次 `ai-jobs-status` 事件与 `get_ai_jobs_status` 都带全量 |
| `useTauriListeners.ts` | `ai-jobs-status` 监听器改为**全量对账** `aiPendingPaths`(不再只信增量事件);启动时 `get_ai_jobs_status` 拉一次做种子 |
| `LibraryItems.tsx` / `Filmstrip.tsx` | 缩略图 AI 徽标去掉 `animate-spin`(**静态图标**——每个缩略图一个动画是放大器) |
| `MainLibrary.tsx` | ① `hasThumbnailWork` 补上 `current < total` 条件(total 残留不再算"有工作");② 忙碌 spinner **30 秒无进度自动停止动画**(真实工作随进度 tick 重置计时) |

**装机复测(2in1,图库空闲)**:`get_ai_jobs_status` 返回 `pending: []`;`document.getAnimations()` = **0**;`:render`/`:gpu` 进程 7 分钟累计 CPU 0~1 秒(≈0%,修复前 renderer 主线程 53% + compositor 26%)。Rust 主线程 ~10% 的原生侧周期 tick 仍存在(§4 次要项,未处理)。

## 修复建议(剩余,按性价比)

| 优先级 | 修复 |
|---|---|
| P0 | **loading 状态对账**:`ai-jobs-status` 带上 pending 路径全集,前端定期/收到 status 时 reconcile(而不是只靠增量事件);启动时前端 `aiPendingPaths` 直接向 `get_pending_ai_paths` 拉一次全量 |
| P0 | **缩略图忙碌指示加兜底**:队列非空但 X 秒无完成事件时视为停滞,降级为静态图标(不转);或干脆把库内徽标改为非动画图标 |
| P0 | **D-07 修复**:`load_image`/解码在 panic 边界 resolve(Err),不让任务挂起——否则任何上游"进行中"状态永久卡死 |
| P1 | 任何>几秒的持续 loading 徽标一律不用 `animate-spin`(静态图标 + 文本"处理中");全列表动画是放大器 |
| P1 | 缩略图请求去重(同 path 在队列里只保留一个);`estimate_export_size` 防抖 + 用低分辨率估算 |
