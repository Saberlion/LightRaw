# AGC 权限一致性整改 · 「只在隐私政策中的权限」驳回

> 驳回原文：检测到当前软件包 user_grant 类型的权限与隐私政策中声明的权限不一致，请修改后提交。
> 只在隐私政策中的权限：[WRITE_IMAGEVIDEO, READ_WRITE_DESKTOP_DIRECTORY, READ_WRITE_DOCUMENTS_DIRECTORY, READ_WRITE_DOWNLOAD_DIRECTORY]
> 日期：2026-11-22 · 对应包：1.7.0 / versionCode 10700 · bundleName `io.github.CyberTimon.RapidRAW`

## 1. 现状对照（不一致根源）

| 权限 |  grant 模式 | 包内 module.json5 | 托管隐私声明 | 处置 |
|---|---|---|---|---|
| ohos.permission.INTERNET | system_grant | ✅ | ✅（非 user_grant，不参与本校验） | 保留 |
| ohos.permission.KEEP_BACKGROUND_RUNNING | system_grant | ✅（**代码零消费**，长任务实际用 setWindowKeepScreenOn） | — | 可选清理 |
| ohos.permission.FILE_ACCESS_PERSIST | system_grant（ACL） | ✅ | ✅ | 保留（release profile allowed-acls 已覆盖） |
| ohos.permission.MEDIA_LOCATION | **user_grant**（弃用/ACL 类） | ✅（**Rust/前端零消费**） | ✅ | A 保留 / B 删除 |
| ohos.permission.WRITE_IMAGEVIDEO | user_grant（ACL） | ❌（v3 后收敛移除） | ✅ 多余 | **声明中删除** |
| ohos.permission.READ_WRITE_DESKTOP_DIRECTORY | user_grant（ACL） | ❌ | ✅ 多余 | **声明中删除** |
| ohos.permission.READ_WRITE_DOCUMENTS_DIRECTORY | user_grant（ACL） | ❌ | ✅ 多余 | **声明中删除** |
| ohos.permission.READ_WRITE_DOWNLOAD_DIRECTORY | user_grant（ACL） | ❌ | ✅ 多余 | **声明中删除** |

根源：包侧权限收敛（§6.17 v3 起）后，AGC **标准化隐私声明托管服务**里的声明未同步更新。
应用文件访问全部走系统选择器（SAF / FileKit / PhotoViewPicker）用户授权 URI，**不需要**媒体库读写与公共目录读写权限；EXIF（含 GPS）经授权文件字节读取，亦不需要 MEDIA_LOCATION。

## 2. 方案 A：只改隐私声明（不改包、不重建，最快解锁）

1. AGC 控制台 → 我的应用 → LightRaw → **隐私管理服务 / 标准化隐私声明托管**（agreementId=2068880948149116608）→ 编辑声明。
2. 在「权限声明」部分**删除**上表 4 项多余 user_grant 权限；**保留 MEDIA_LOCATION**，用途文案与包内 reason 一致：
   > 读取您照片中嵌入的位置信息，用于在元数据面板中展示拍摄地点。仅在您查看元数据时使用，不会用于后台收集或上传。
3. 补充/保留总体说明（建议原文）：
   > 本应用的文件读取与保存均通过系统文件选择器/照片选择器由您主动授权完成，应用仅访问您选定的文件或文件夹；不声明媒体库读写、公共目录读写类权限。联网权限用于按需下载 AI 模型与社区预设；文件夹授权持久化权限用于在您重启应用后延续您已授予的文件夹访问；以上均不会扩大到您未选定的内容。
4. 提交声明审核 → 通过后在「版本管理」**原包重新提交**（无需重建）。
5. 复核：声明 user_grant 集合 = {MEDIA_LOCATION} = 包内 user_grant 集合，逐项一致。

风险预告：MEDIA_LOCATION 属弃用/ACL 类且代码零消费，下一轮审核可能以"非必要权限"点名（docs §6.17 已预告），届时仍需走方案 B。

## 3. 方案 B（推荐）：包与声明同时收敛到 user_grant 空集

一次性消除本轮不一致 + 预告中的下一轮风险。

**包侧（我可执行）：**
1. `src-tauri/gen/ohos/entry/src/main/module.json5` 删除 MEDIA_LOCATION 块（gen/ 为本地修改，`cargo tauri ohos init` 重跑后需重加）；可选同时删除零消费的 KEEP_BACKGROUND_RUNNING。
2. `scripts/build-ohos.ps1 -Release` 全量出包（ORT/CANN 注入 + strip + 删 sourceMaps）。
3. `.csr/sign-injected-hap.ps1 -Config release` → `.csr/pack-app.ps1` 产出 `entry-default.app`。
4. 二进制复核：HAP 内 module.json 的 requestPermissions = INTERNET + FILE_ACCESS_PERSIST（+可选保留项），**零 user_grant**；42 库零 DWARF/symtab、无 ets/sourceMaps.map。
5. 字符串资源 `perm_media_location` 闲置保留（无引用，无害，同 perm_read_imagevideo 先例）。

**声明侧（控制台，您操作）：**
1. 编辑托管声明：权限声明部分**删除全部 5 项 user_grant**（4 项多余 + MEDIA_LOCATION），改为：
   > 本应用不声明任何 user_grant（用户授权）类权限。您的照片与文件夹访问均通过系统选择器由您主动选定并授权，应用仅访问您选定的内容；保存/导出 likewise 经系统保存选择器完成。
2. 保留 system_grant 说明：INTERNET（按需下载 AI 模型/社区预设）、FILE_ACCESS_PERSIST（延续您已授予的文件夹授权，跨重启续编）。
3. 声明过审后上传新 `.app` 提审。

**功能影响核对（已验证零消费）：** 图库导入/文件夹持久授权/相册选择/导出/EXIF 元数据均不依赖被删权限（SAF URI 授权模型，§6.14/§6.21 真机实证）；AI 模型下载走 INTERNET 不受影响。

## 4. 构建侧记录（2026-11-22 整改重建期间）

- **ArkTS 严格模式两连坑**（`entry/src/main/ets/entryability/EntryAbility.ets`，tethering USB 授权段）：
  1. `arkts-no-any-unknown`：`const x = mod.foo ?? mod.default?.foo` 推断为 any → 必须显式注解；
  2. `arkts-no-func-apply-call`：注解成 `Function | undefined` 后 `.call(...)` 又被禁。
  **正解 = 注解为 `ESObject`**（同文件 `pushColorMode` 的 `const notify: ESObject = …` 为先例：显式 ESObject 过 no-any-unknown，且 ESObject 上的 `.call` 不受 func-apply-call 限制）。for-of 遍历 `Array<ESObject>` 也会触发 no-any-unknown，改索引循环 + `const device: ESObject = cameras[i]`。
- **模板与 gen 双份**：`src-tauri/ohos/EntryAbility.template.ets` 与 gen 下同名文件需保持一致；hvigor 只编译 gen。整改期间存在并行编辑互相覆盖（11:37 / 12:23 两次保存），最终统一为 ESObject 方案。**后续改 tethering ets 请两份同改或交由单一会话改。**
- 第二轮 cargo 曾出现 rustc 无诊断 exit 1（链接阶段瞬时问题）；同环境独立 `cargo build --release --target aarch64-unknown-linux-ohos` 复跑成功（26m26s），release lib 已缓存，非源码问题。
- 权限收敛改动本身（module.json5 删 MEDIA_LOCATION / KEEP_BACKGROUND_RUNNING）与上述编译问题无关，ArkTS 通过后即进入注入/打包/签名链。

## 5. 执行结果（2026-11-22 13:2x，方案 B 包侧完成）

- 包：`requestPermissions = [INTERNET, FILE_ACCESS_PERSIST]`，**user_grant 零集**；deviceTypes = 2in1/tablet/phone；版本 1.7.0/10700。
- 产物：`src-tauri/gen/ohos/entry/build/default/outputs/default/entry-default.app`
  - 大小 519,517,603 B（495.45 MB）
  - SHA256 `7B2CD07AAE7A548087C0A5BC413AED8712A7ADB65AF04B9B61610A4047334F5F`
  - 内层 HAP SHA256 `333FDA6C16C1BFF6CB2925EE7ADBBBCFE3706E06F0EDC53BCDB5FF833F892FA3`（release 签名，字节保真）+ .app 级签名块，双层验签通过
- 复核（`tools/verify_package.py`）：48 个 .so 零调试段/零 symtab、无 ets/sourceMaps.map、权限集合断言通过。
- 构建侧坑与修复见 §4；移植文档留档 `docs/HARMONYOS_PORTING.md` §6.31。
- **待您操作**：托管隐私声明清空 5 项 user_grant（§3 文案）→ 声明过审 → 上传本 .app → 重提。
- 残留提示：NPU 降噪模型 `nind_denoise_utnet_684.ms` 未内置（运行时下载/侧载），不阻塞提审。

## 7. 第二轮驳回（2026-11-22 17:xx）· 诊断与唯一待办

> 驳回原文：检测到当前软件包 user_grant 类型的权限与隐私政策中声明的权限不一致，请修改后提交。
> 只在隐私政策中的权限：[WRITE_IMAGEVIDEO, READ_WRITE_DESKTOP_DIRECTORY, READ_WRITE_DOCUMENTS_DIRECTORY, READ_WRITE_DOWNLOAD_DIRECTORY, MEDIA_LOCATION]

**诊断（包侧实证，2026-11-22 复核）**：清单从第一轮的 4 项增至 **5 项（+MEDIA_LOCATION）** —— 恰是「user_grant 空集的包」对照「未同步的声明」的预期形状：
提审包已是方案 B 产物（HAP 内 module.json `requestPermissions = [INTERNET, FILE_ACCESS_PERSIST]`，5 项 user_grant 全部不在包内，MEDIA_LOCATION 也被列出即为明证），
而托管隐私声明（agreementId=2068880948149116608）仍保留 5 项 user_grant → 全部落入「只在隐私政策中」。
**包侧无需再动**；本轮整改全部在声明侧，即 §3 的「声明侧（控制台，您操作）」：删除全部 5 项 user_grant、替换为 §3 第 1 条文案、保留 INTERNET / FILE_ACCESS_PERSIST 说明 → 声明过审 → 上传 .app 重提。

**当前磁盘产物（与 §5 哈希不同，系 16:52–17:13 构建链重建/重签产物，语义一致且全量复核通过）**：

- `src-tauri/gen/ohos/entry/build/default/outputs/default/entry-default.app`
  - 大小 519,532,245 B；SHA256 `2FD385BF23A87B2E017531E7C01123611818A73F3CF6ECBB557CD6F4BA031F56`
  - 内层 HAP SHA256 `906FFC014124D0DC13709DFA1669630D2B2FB8BECA739F17A8BC0A432BE9C200`
- `store-listing/tools/verify_package.py` 复核（2026-11-22）：entries = {hap, pack.info}、权限 = 2 项 system_grant、无 `ets/sourceMaps.map`、48 个 .so 零 `.debug_*` 零 symtab、deviceTypes = 2in1/tablet/phone —— `VERIFY OK`。
- 提审以**本节哈希**为准；§5 的 `7B2CD07A…` 为同日更早构建，若 AGC 版本管理里已上传过旧哈希，重提时直接覆盖上传新包即可。

**教训**：方案 B 的两条腿（包侧收敛 + 声明侧清空）必须**同步落地后再提审**——只完成包侧就提审，必然触发本条 5 项全列的驳回。

## 8. 两方案共同核对清单

- 声明与包 user_grant **逐项双向一致**（多任一方向即拒）。
- 支持设备勾选 = 包内 deviceTypes（2in1 + tablet + phone），否则 998。
- 名称/图标 = LightRaw + LR 光圈标（与包内资源一致）。
- 版本 1.7.0 / 10700 已同步（AppScope/app.json5）。
- AI 功能声明（降噪/蒙版/超分/生成式修复）保留；隐私弹窗仅平台托管一套。
- 提审包必须 `-Release`（debug 信息驳回先例 §6.17）。

## 9. 第三轮决策反转：方案 C —— 包侧回加 5 项 user_grant（2026-11-23）

**背景**：§7 唯一待办（声明侧清空 5 项 user_grant）未执行，驳回原文同 §7。**决策反转**：声明不动，包侧把 5 项原样加回，使 包 user_grant = 声明 user_grant = 5 项，双向一致。

**已执行（2026-11-23，包侧源文件级）**：
1. `src-tauri/gen/ohos/entry/src/main/module.json5` requestPermissions 2 → **7 项**（5 个权限块取自 `module.json5.agc-backup`，reason 复用 string.json 闲置的 `perm_*` 资源，零新增）。
2. **签名链前置实证**：`openssl cms -verify` 解码 `RapidRAWRelease.p7b` → release profile `allowed-acls` **5 条**（4 项新增 ACL 权限 + `FILE_ACCESS_PERSIST`），全覆盖，profile 无需换发；`MEDIA_LOCATION` 非 ACL（2026-11-04 装机实证）。⚠️ §8 佐证记录「四条 ACL 齐全」不准，实为 5 条。
3. `store-listing/tools/verify_package.py` 断言更新：EXPECTED = 7 项，FORBIDDEN = {`READ_IMAGEVIDEO`}。

**执行结果（2026-11-23 23:5x，全链完成）**：`build-ohos.ps1 -Release -Target aarch64 -SkipOrtBuild -ResetHvigor`（杀 3 个陈旧 hvigor 守护进程，防 §6.17 权限静默丢失先例）→ `sign-injected-hap.ps1 -Config release`（预检链路，verify-app 全绿）→ `pack-app.ps1`（字节保真组包 + .app 级签名 + 双层验签全过）。产物 `entry-default.app` **519,554,853 B（495.49 MB）**，SHA256 `825312CB9127D069FFE2DCA144C563776FADD29A6820046A33CE322C2E59BBB8`（内层 HAP `B67C3336202B54A2FE3B160B1D6540ACE45A4FD9A7C3E2E6499ABA1E9E0B7C7C`，release 签名，9 个高频模型 + `nind_v2.om` 全量内置；`nind_denoise_utnet_684.ms` 仍缺，运行时下载/侧载，不阻塞提审）。`verify_package.py` 产物级复核 **VERIFY OK**：module.json 实际进包 = **7 权限**（5 user_grant 与声明逐项一致）、无 `ets/sourceMaps.map`、48 个 .so 零 `.debug_*`/零 symtab、deviceTypes = 2in1/tablet/phone。**声明侧零操作；直接上传本 .app 重提。**

**风险**：5 项零运行时消费 + `WRITE_IMAGEVIDEO`/`MEDIA_LOCATION` 弃用/ACL 类 → 下一轮可能以「非必要权限」点名（§2 风险预告、§6.17 先例）；回退路径 = 方案 B（§3：声明清空 + 包删 5 块），改动可逆。