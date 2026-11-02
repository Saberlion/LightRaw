# 鸿蒙手机（phone）形态适配方案

> 起草：2026-11-01 · 分支 `feature/harmonyos-port` · 代码基线 `2d972d4d`
> 关联：`docs/HARMONYOS_PORTING.md` §6.21（平板）· §6.23（触屏/保活/open-with）· `docs/TEST_CASES.md` §3.17（TC-PAD）
> 状态：**方案 + 首批代码已落地**（§5），实机验证待手机设备

---

## 1. 结论摘要

鸿蒙手机**不是从零适配**：手机与 Android 手机在交互模型上几乎同构，现有 `compact / wide / full` 三档布局、`isTouchUi`、底部抽屉、相册导入、gallery 导出、返回手势等设施都已存在。真正缺的是**三处形态路由**和**两处平台能力差异**：

| 类别 | 缺口 | 影响 |
|---|---|---|
| 形态路由 | `isTouchUi` 只认 `android \| ohos-tablet` | 手机上仍按桌面渲染：hover 工具条、右栏常驻、桌面标题栏 |
| 形态路由 | `layoutMode` 把 `phone` 落到桌面分支 | 横屏/平铺时出现三栏桌面布局 |
| 形态路由 | 返回手势只在 `tablet` 上关闭抽屉 | 手机横屏抽屉无法用返回关闭 |
| 平台能力 | `FILE_ACCESS_PERSIST` **仅 2in1 支持**（`EntryAbility` 注释已注明） | 手机每次冷启都要重新授权文件夹 |
| 平台能力 | 导出默认"源目录"，而手机库根可能是应用沙箱 | 导出的图用户看不到（沙箱内） |

以上五项中，前四项已在本次提交里改掉（§5），第五项给出实现建议（§4.3）。

---

## 2. 现状盘点（代码锚点）

| 能力 | 位置 | 对手机是否可用 |
|---|---|---|
| 形态桥（`phone`/`tablet`/`2in1`） | `ohos_integration.rs` `system_bridge::notify_ohos_device_type`、`get_ohos_device_type` | ✅ 原生值已含 `phone`，无需改 |
| 布局三档 | `src/App.tsx:294-314` (`layoutMode`) | ⚠️ 已改：phone 复用 Android 规则 |
| 触控 UI 判定 | `src/App.tsx:287` (`isTouchUi`) | ⚠️ 已改：加入 `isOhosPhone` |
| 抽屉/浮层返回关闭 | `src/hooks/useAndroidBackHandler.ts:96-105` | ⚠️ 已改：加入 `phone` |
| 触屏 hover 控件展开 | `styles.css` `@media (hover: none), (pointer: coarse)` | ✅ 通用，手机自动命中 |
| 拖拽排序 | `App.tsx`/各面板 dnd-kit `TouchSensor` | ✅ 通用 |
| 保活（锁屏不杀任务） | `useOhosKeepScreenOn.ts` + `set_ohos_keep_screen_on` | ✅ 通用，手机更需要 |
| 深浅色跟随系统 | `get_ohos_color_mode` + `onConfigurationUpdate` | ✅ 通用 |
| 文件夹授权持久化 | `EntryAbility.persistFolderGrant()` | ❌ 平台仅 2in1；手机走每次重授权 |
| 相册导入（MediaLibraryKit） | `pick_gallery_images` + `import_staging` | ✅ 手机最自然的入口 |
| 导出直写目录 / 图库桥 | `export_processing.rs:1197-1217` | ⚠️ 见 §4.3（源目录可能是沙箱） |
| 应用内库根（无授权兜底） | `get_or_create_internal_library_root` | ✅ 已存在（当前仅 Android 用，手机应复用） |
| open-with / 分享 | `EntryAbility.captureOpenWant` + `open-with-file` 事件 | ✅ 通用，手机上更常用 |
| 窗口装饰/最小化 | `TitleBar.tsx`、`ohos_window_control` | ✅ OHOS 已隐藏自绘三按钮；手机无窗口概念 |

---

## 3. 手机与平板的关键差异

1. **屏幕**：竖屏 360~430 CSS 宽（平板 1298）。三栏布局必然不可用，只能"画布 + 底部抽屉"。
2. **安全区**：状态栏、圆角、挖孔、手势条。平板验证过的避免区日志（`getWindowAvoidArea`）目前只打印，未参与布局。
3. **授权模型**：`FILE_ACCESS_PERSIST` 不在手机形态提供 → 每次冷启要重授权；**手机的"图库"应是应用内库根 + 相册导入**，而不是让用户去文件管理器选目录。
4. **导出模型**：手机没有用户可直达的"公共工作目录"，正确落点是**系统相册**（`save_image_bytes_to_ohos_gallery`），而不是直写目录。
5. **资源**：手机 RAM 8~12 GB、SoC 纹理上限更低；101.9 MP 已在 2in1 触发 >8192 px 旁路（D-06），手机更容易撞内存墙。
6. **功耗/热**：AI 推理（NPU 2 分钟级）+ 长时间亮屏，需要保活（已有）与温度/电量提示（待做）。

---

## 4. 适配清单

### 4.1 P0 —— 让手机能跑起来（本次已完成 ①~④）

| # | 项 | 做法 | 状态 |
|---|---|---|---|
| ① | 打包形态 | `gen/ohos/entry/src/main/module.json5` 的 `deviceTypes` 增 `"phone"`（`gen/` 为本地生成，重跑 `cargo tauri ohos init` 后需重打；建议后续把该字段纳入构建脚本的幂等补丁） | ✅ 已改 |
| ② | 触控形态 | `isTouchUi = isAndroid \|\| isOhosTablet \|\| isOhosPhone` | ✅ 已改 |
| ③ | 布局路由 | `usePhoneLayout = isAndroid \|\| isOhosPhone`，复用 Android 规则：`≥1000` → full，竖屏 `<600` → compact，其余 → wide | ✅ 已改 |
| ④ | 返回手势 | 抽屉浮层关闭分支从 `tablet` 扩到 `tablet \| phone` | ✅ 已改 |
| ⑤ | 首次进入不逼用户选目录 | `handleOpenFolder(intent)`:欢迎页主入口传 `'open'`(手机落应用内库根),「添加文件夹」与已有库时的按钮传 `'add'`(照常拉起 FileKit picker)——两个语义不再共用一条路径 | ✅ 已改(按钮文案建议后续改「导入照片」,需 15 locale 同步) |
| ⑥ | 导出落点 | `export_processing.rs`:**导出目标位于应用沙箱内**(`app_data_dir` / `app_cache_dir` 前缀,即手机内部库根或其上的"原始文件夹")→ 走图库桥;用户显式选择的公共目录照常直写。判定与形态无关,同时修掉 2in1/平板上同一隐患 | ✅ 已改 |
| ⑦ | 安全区 | 把 `getWindowAvoidArea` 的 top/bottom 通过新命令 `get_ohos_avoid_area` 传给前端，用 CSS 变量做 `padding-top/bottom`（ArkWeb 的 `env(safe-area-inset-*)` 不可依赖） | ⬜ 待做 |

### 4.2 P1 —— 手机上"好用"

| # | 项 | 做法 |
|---|---|---|
| ⑧ | 底部抽屉手势 | compact 模式下抽屉可下滑关闭，返回手势优先关抽屉（当前只覆盖宽屏右抽屉） |
| ⑨ | 触控热区 | 复用 §6.23 遗留项：`Resizer` 命中区 8→16/20 px、栏位图标与裁剪手柄 ≥44 px |
| ⑩ | 输入法遮挡 | 重命名/搜索输入框聚焦时把底部抽屉上移（`AvoidAreaType.TYPE_KEYBOARD`） |
| ⑪ | 竖屏编辑器 | 编辑画面 + 底部面板比例按 3:2 收敛；胶片栏在竖屏改为横向条 |
| ⑫ | 加载前资源预检 | 实现 §6.11-3 的 C+D 方案（内存/纹理上限预判 + 降级提示），手机上是硬需求 |
| ⑬ | AI 模型按需 | 默认只带 NIND（124 MB 或 `.om` 62 MB）；SAM/前景/深度按首次使用时下载并给出体积提示 |
| ⑭ | 电量/温度提示 | AI 长任务前检查 `batteryInfo` 与热区状态（`thermal`），低电提示 |

### 4.3 说明：为什么导出必须走图库

`export_processing.rs` 的 `destination_type == "originalFolder"` 取 `path.parent()`；而手机形态下库根很可能是 `get_or_create_internal_library_root()` 返回的**应用沙箱目录**（用于相册导入的落地）。直写沙箱 = 用户在系统相册/文件管理里看不到成品。
因此手机形态要么把 `originalFolder` 解释为"图库"，要么显式提供「保存到相册」目标；两者都复用现有 `save_image_bytes_to_ohos_gallery`（已在平板/模拟器验证）。

### 4.4 发布侧

| 项 | 说明 |
|---|---|
| AGC 支持设备 | 控制台需勾选手机（与 `deviceTypes` 一致，否则报 998） |
| 权限 | 手机同样**不声明** `READ_IMAGEVIDEO`（相册选择器由系统承载）；`FILE_ACCESS_PERSIST` 可保留但不得依赖 |
| 截图/文案 | 需要手机竖屏截图（应用市场按形态分别要求） |
| 调试 | `hdc install` + 调试 Profile（release 直装仍 9568322）；devtools socket 只在 debug 包存在 |

---

## 5. 本次已落地的代码改动

```text
src/App.tsx                          + isOhosPhone / isTouchUi 纳入 phone
                                     + usePhoneLayout 复用 Android 三档规则
src/hooks/useAndroidBackHandler.ts   + 抽屉返回关闭覆盖 phone
src/hooks/useAppNavigation.ts        + handleOpenFolder(intent)：'open'（手机落应用内库根）与 'add'（picker）分离
src/components/panel/MainLibrary.tsx + 欢迎页按钮按 hasLastPath 传 intent
src-tauri/src/export_processing.rs   + export_destination_is_sandboxed()：目标在应用沙箱内 → 图库桥（图片 + 蒙版 alpha）
src-tauri/gen/ohos/entry/src/main/module.json5  deviceTypes += "phone"（本地生成文件）
```

当前状态（2026-11-01）：

| 门禁 | 结果 |
|---|---|
| `cargo fmt -- --check` | ✅ EXIT=0 |
| `cargo clippy --all-targets -- -D warnings`（宿主） | ✅ EXIT=0 |
| `scripts/build-ohos.ps1`（aarch64 交叉，真正编译 `cfg(ohos)` 分支） | ✅ hvigor `OK`，HAP `deviceTypes=["2in1","tablet","phone"]`，ORT/CANN 注入完成 |
| `npm run typecheck` | 🟡 65（与基线一致，未新增） |
| `eslint`（3 个改动文件） | 🟡 仅既有 `no-explicit-any`，改动行零命中 |
| `prettier --check`（3 个改动文件） | ✅ 通过 |

双形态回归（改动后重装、冷启）：

| 设备 | 日志 | 结论 |
|---|---|---|
| PC（2in1） | `deviceType=2in1 mode=full touchUi=false` | ✅ 与改动前一致 |
| 平板（tablet） | `deviceType=tablet mode=wide touchUi=true` | ✅ 与改动前一致 |
| 平板导出 | `Batch Export … → process_image_for_export`，**无** `via the gallery bridge` 告警，成品直写库目录 | ✅ 直写路径未被电话分支影响 |

导出路由修复后的定向验证（2026-11-02，2in1，`export_destination_is_sandboxed()` 版本）：

| 场景 | 结果 |
|---|---|
| 目标 = 公共目录（`/storage/Users/currentUser/Download`） | ✅ `regression_directwrite.jpg` 直写落盘，日志零告警 |
| 目标 = 应用沙箱（`…/files/appdata/library`） | ✅ 日志 `OHOS export to '…' via the gallery bridge instead of a direct write (destination is inside the app sandbox)`，沙箱内**未**创建任何文件 |

> 平板(GEM-W40)在 2026-11-02 晨间断开,本轮回归在 2in1 上执行;两条路径(直写/图库桥)覆盖了同一代码分支,平板重连后按 TC-PAD-06/07 顺带复测即可。

---

## 6. 手机验证用例（新增 TC-PHONE）

| ID | 名称 | 步骤 | 预期 |
|---|---|---|---|
| TC-PHONE-01 | 可安装 | `hdc install -r`（`deviceTypes` 含 phone） | 安装成功；图标/名称正常 |
| TC-PHONE-02 | 形态路由 | 冷启 → 看 `app.log` | `deviceType=phone mode=compact touchUi=true hoverNone=true coarsePointer=true` |
| TC-PHONE-03 | 首屏 | 冷启（竖屏） | 欢迎页单列、按钮「导入照片 / 继续会话」；无桌面标题栏、无 hover 工具条 |
| TC-PHONE-04 | 相册导入 | 底部 FAB → 系统相册多选 → 完成 | 落地到库根；进度事件；`import_staging` 清理；缩略图与 EXIF 正常 |
| TC-PHONE-05 | 授权模型 | 冷启两次、force-stop 后重启 | 第二次仍可直接进库（应用内库根），**不**要求重新选目录 |
| TC-PHONE-06 | 编辑器 | 打开 JPEG / RAW，拖曝光 | 画布 + 底部抽屉；`apply_adjustments` 出图；抽屉可下滑/返回关闭 |
| TC-PHONE-07 | 导出 | 导出 → 保存到相册 | 「来自 LightRaw」出现在系统相册；文件不在沙箱 |
| TC-PHONE-08 | 返回手势 | 编辑器 → 抽屉 → 图库 → 退出 | 逐级返回，不误退出 |
| TC-PHONE-09 | 安全区 | 竖屏/横屏/挖孔机型 | 状态栏不遮挡、手势条不压按钮 |
| TC-PHONE-10 | 输入法 | 重命名文件 | 输入框不被 IME 遮挡，抽屉上移 |
| TC-PHONE-11 | 保活 | 100MP AI 降噪期间不操作 3 分钟 | 不锁屏、不被 `AMS Kill`；任务完成后 `release` |
| TC-PHONE-12 | 内存边界 | 打开 61MP / 101MP | 有资源预检提示或优雅降级，不闪退 |
| TC-PHONE-13 | 分享入口 | 系统相册「分享到 LightRaw」 | 冷/热启动均打开该图 |
| TC-PHONE-14 | 横竖屏 | 旋转设备 | 布局在 compact/wide 间切换，无重启、无错位 |
