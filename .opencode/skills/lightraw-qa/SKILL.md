---
name: LightRaw QA
description: LightRaw 全量质量保障技能:鸿蒙移植 TC-* 用例集(192 条/18 模块,docs/TEST_CASES.md)+ test-assets 语料回归 + 静态门禁 + 测试报告流转。触发场景:测试用例、测试执行、回归测试、测试报告、TC-BUILD/TC-PLAT/TC-PATH/TC-FILE/TC-EXPORT/TC-AI 等模块测试、静态门禁、test-assets 语料、D-TA-01 复测、鸿蒙真机/模拟器测试。测试执行子代理必须使用 DeepSeek-V4.1-Flash-1017 模型(alibaba-cn/deepseek-v4.1-flash-1017)。
---

# LightRaw QA(TC 用例集 + test-assets 语料回归)

本仓**无自动化测试套件**(见根 `AGENTS.md`)。测试体系分两层:

1. **`docs/TEST_CASES.md` 用例集**(主):鸿蒙移植专项 + 核心功能回归,192 条用例、18 个模块(TC-* 编号),含优先级、环境矩阵、工具方法;
2. **`test-assets/` 语料回归**(辅):14 个 RAW/JPEG 资产的加载/解码/编辑管线回归 + 已知缺陷 D-TA-01 复测。

本文件中的路径均相对仓库根目录。

## 测试模型(强制)

所有测试必须使用 **DeepSeek-V4.1-Flash-1017**,精确模型 ID:

```
alibaba-cn/deepseek-v4.1-flash-1017
```

- 通过 subagent 工具派发测试执行、结果分析、回归复测、报告整理等子任务时,必须传 `model: "alibaba-cn/deepseek-v4.1-flash-1017"`;排障等深度推理场景可用变体 `alibaba-cn/deepseek-v4.1-flash-1017#high`。
- 不要改用其他模型,除非用户明确要求。

## 文档体系

| 文档 | 角色 |
|---|---|
| `docs/TEST_CASES.md` | **用例集权威**:192 条、18 模块明细(§3.1–3.18)、优先级 P0–P2、环境矩阵(§1.4)、工具与取证方法(§1.5)、静态门禁(§1.6)、维护约定(§5)。用例的"依据"列溯源到 `docs/HARMONYOS_PORTING.md` §6.x |
| `docs/TEST_REPORT.md` | 汇总状态报告(✅/🟡/⚫/⬜ 统计 + 逐条明细) |
| `docs/TEST_REPORT_TEMPLATE.md` | 每轮执行的空白模板(基本信息/环境前置检查/门禁快照/执行记录/缺陷/性能/结论) |
| `docs/reports/TEST_REPORT_<yyyy-mm-dd>.md` | 历轮归档(最新:2026-11-07 双机全量,192 条:✅142 / 🟡22 / 🔴3 / ➖1 / ⚪24) |
| `test-assets/MANIFEST.md` | 语料清单、SHA-256、已知缺陷 D-TA-01 |
| `references/test-suite-map.md` | 本 skill 附带:TC 用例集速查卡(模块地图/环境矩阵/工具表,内容摘自 TEST_CASES.md,冲突以 TEST_CASES.md 为准) |
| `references/test-matrix.md` | 本 skill 附带:test-assets 逐资产测试矩阵 |

> 注意:`docs/` 是**独立仓库**(`LightRaw-docs`,被根 `.gitignore` 忽略)。测试文档的修订在那个仓库提交;在本仓工作区内读取、执行、写报告均正常。

## 工作流

### 1. 确定执行范围

问清(或从用户请求推断):**全量 / 单模块 / 缺陷回归**。然后读 `docs/TEST_CASES.md` §2 用例索引,定位对应模块明细(§3.x)逐条执行。模块速览见 `references/test-suite-map.md`。

### 2. 静态门禁先跑([HOST],CI 硬门禁,必须全绿)

```powershell
# 前端(仓库根)
npm run typecheck; npm run lint; npm run format:check; npm run i18n:check
# Rust(src-tauri/,硬门禁)
cargo fmt -p RapidRAW -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check                                   # 宿主
cargo check --target aarch64-unknown-linux-ohos   # OHOS(需 HARMONYOS_PORTING.md §6.1 环境变量)
```

### 3. 设备端执行与取证

- 环境矩阵(HOST / SIM / DEV-A 2in1 主验证机 / DEV-B 大纹理 / DEV-C 平板 / DESK/AND)与 hdc/CDP/日志工具命令,速查 `references/test-suite-map.md`;完整版以 `docs/TEST_CASES.md` §1.4–1.5 为准。
- 出包一律 `scripts\build-ohos.ps1 [-Target x86_64] [-Release] [-SkipBuild]`(仓库根执行);机械前置检查清单见 `docs/TEST_REPORT_TEMPLATE.md` §2(逐项勾选后再动设备)。

### 4. 语料回归(test-assets,与 TC 用例并行或独立执行)

```powershell
# 仅校验存在性 + SHA256(不下载)
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\fetch-test-assets.ps1 -CheckOnly
# 缺失时下载 RAW / 生成合成 JPEG
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\fetch-test-assets.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\make-jpeg-test-assets.ps1
```

按 `references/test-matrix.md` 逐资产验证(R1–R8 RAW、J1–J6 JPEG、回归序列 T1–T4);**D-TA-01(CR2 加载后紧跟下一 RAW 的 panic + promise 挂起)每次必测**。

### 5. 结果记录与报告流转

- 每轮执行:复制 `docs/TEST_REPORT_TEMPLATE.md` → `docs/reports/TEST_REPORT_<yyyy-mm-dd>.md`,逐条填 ✅/❌/🟡/⚫/⬜ + 证据。
- **证据格式**(TEST_CASES.md §5 约定):命令原文 + 输出片段 + 截图字节数(判界面状态)+ app.log 摘录 + 导出字节数/SHA256 + PSS。
- 执行完一轮后,把结论同步回 `docs/TEST_REPORT.md` §1 摘要(或保留为独立轮次归档)。
- 新缺陷按模板 §5 记 `BUG-<date>-<seq>`;语料类缺陷沿用 `test-assets/MANIFEST.md` 的 `D-TA-XX` 格式。

### 6. 用例维护约定(TEST_CASES.md §5,改动代码后必读)

- 新增 OHOS 集成代码须**同步补用例**。
- 涉及 `gen/ohos/`(gitignored)手工补丁的用例(EntryAbility / Index.ets / module.json5 / 签名配置 / 隐私 metadata),须在前置条件中注明**重打步骤**,并注明 `cargo tauri ohos init` 重跑后失效。
- 涉及 Cargo.lock 冻结的改动,用例须附**双目标复核**(宿主 + `aarch64-unknown-linux-ohos`)。

## 硬性坑位(执行时必记)

- **平台判定用 `is_ohos_build`,不得用 `platform()`**(tauri-plugin-os 在 OHOS 返回 "linux")。
- `uinput` 触摸注入会**会话级死亡**;`reboot` 恢复。
- hilog 环形缓冲轮转快,**事件后 2~4s 内抓取**;导出类问题的根因现场在系统侧(CommonSaveAbility)日志。
- 系统 picker 对话框不在 webview 内,用 `uitest dumpLayout -i` 取 bounds;webview 内用 CDP(`fport tcp:9222`)零坐标误差直测。
- FUSE 慢:模拟器会话恢复 90~120s,不要误判为挂起。
- **离开本机的包必须 `-Release`**(AGC 拒收含调试信息的包);HAP 内不得有 `ets/sourceMaps.map`、DWARF/symtab。
- 性能/UI 平滑度对比**仅同机 scripted-vs-scripted** 可比(`bench/replay.js` + `bench/analyze.mjs`)。
