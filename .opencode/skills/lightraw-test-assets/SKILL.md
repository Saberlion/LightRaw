---
name: LightRaw 测试用例
description: 使用 test-assets 测试语料(8 个真实相机 RAW + 6 个合成 JPEG)对 LightRaw 做加载/解码/编辑管线回归测试。触发场景:测试用例、测试语料、test-assets、RAW/JPEG 加载测试、回归测试、D-TA-01 复测、鸿蒙设备端测试。测试执行子代理必须使用 DeepSeek-V4.1-Flash-1017 模型(alibaba-cn/deepseek-v4.1-flash-1017)。
---

# LightRaw 测试用例(test-assets 回归测试)

本仓库**没有自动化测试套件**(见根 `AGENTS.md`)。"测试用例" = `test-assets/` 语料 + 本 skill 规定的手工/脚本化验证流程。语料清单、哈希与已知缺陷的**权威来源**是 `test-assets/MANIFEST.md`(入库);二进制不入库,按需重建。本文件中的路径均相对仓库根目录。

## 测试模型(强制)

所有测试必须使用 **DeepSeek-V4.1-Flash-1017**,精确模型 ID:

```
alibaba-cn/deepseek-v4.1-flash-1017
```

- 通过 subagent 工具派发测试执行、结果分析、回归复测等子任务时,必须传 `model: "alibaba-cn/deepseek-v4.1-flash-1017"`;排障等深度推理场景可用变体 `alibaba-cn/deepseek-v4.1-flash-1017#high`。
- 不要改用其他模型,除非用户明确要求。
- 派发示例:subagent 工具(agent: general)+ 上述 model + prompt 中附带本 skill 的 `references/test-matrix.md` 对应矩阵段落。

## 流程

### 1. 准备/校验语料(仓库根目录执行)

```powershell
# 仅校验存在性 + SHA256(不下载)
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\fetch-test-assets.ps1 -CheckOnly

# 缺失时下载 RAW(断点续传 + 重试 + 体积校验,约 390 MB)
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\fetch-test-assets.ps1

# 生成合成 JPEG(GDI+,零外部依赖)
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\make-jpeg-test-assets.ps1
```

哈希以 `test-assets/MANIFEST.md` 中的 SHA-256 为准(`test-assets/SHA256SUMS.txt` 同源)。校验失败先重建再测试,不要带着坏语料出结论。

### 2. 逐资产执行测试矩阵

读取 `references/test-matrix.md`(相对本 skill 目录),按矩阵逐项验证:每个资产有明确的覆盖点与预期结果(如 Nikon HE NEF 应**优雅失败**走内嵌预览兜底,Hasselblad 100MP 是 ≥8192px 纹理上限/内存路径主力)。桌面端把 `test-assets/raw` 与 `test-assets/jpeg` 放进同一文件夹,在应用里「打开文件夹」选它。

### 3. 回归检查 D-TA-01(必测)

**D-TA-01**:debug 包上加载 `Canon_EOS_5DM4_B13A0732.CR2` 后**紧接着**加载下一个 RAW(再次 CR2 或 X-T5 RAF)→ `PANIC! at vendor\rawler\src\pixarray.rs:119:5 - assertion failed: self.initialized`,且该次 `load_image` 的 promise 永不 resolve。详见 MANIFEST「已知问题」。每次跑测试矩阵都要复测此序列:确认无 panic、无挂起。

### 4. 鸿蒙设备端(如适用)

```powershell
hdc file send <file> /data/app/el2/100/base/io.github.CyberTimon.RapidRAW/files/appdata/library/
```

(沙箱库根;或推到设备公共目录后用 FileKit picker 选目录。)

### 5. UI 流畅性(仅在用户要求时)

用 `bench/` 的脚本化回放(见 `bench/README.md`);只有**同机 scripted-vs-scripted** 的结果才可比。

## 结果记录

- 每个资产记录:通过 / 失败(附现象)/ 不适用。
- 失败项:先确认语料哈希无误,再按 MANIFEST「已知问题」格式(编号 D-TA-XX、日期、复现次数、构建 profile)记录新缺陷。
- 不要修改 `test-assets/MANIFEST.md` 之外的语料文件;新增测试用例时同步更新 MANIFEST 与 `references/test-matrix.md`。
