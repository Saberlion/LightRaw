# LightRaw 上架图片素材 · 使用与规格说明

生成脚本：`../tools/make_assets.py`（Pillow + numpy， bundled Python 即可运行）。
文案见 `../AGC上架文案.md`。所有截图均为**真机真实界面**，未伪造 UI；宣传框图只在真实截图外加标题/边框，不遮挡界面内容。

## 目录与上传映射

| 目录 | 内容 | 建议用途 |
|---|---|---|
| `icon/lightraw-icon-216.png / -512.png / -1024.png` | 包内同款 LR 光圈图标（`gen/ohos` foreground 层原图缩放） | AGC 应用图标槽（控制台若要求其他尺寸，用 1024 源缩放；**必须与包内图标一致**） |
| `plain/phone/phone-1..10.jpg` | 手机真机截图原分辨率 1280×2832（存档/源） | 源图存档；上传请用 `plain/phone916/` |
| `plain/phone916/phone-1..10.jpg` | **9:16 · 1080×1920** 纯截图留边版（深蓝底、无文案、不裁 UI） | AGC「手机」截图槽（≥3 张，按编号优先级传满上限） |
| `plain/tablet/tablet-1..4.jpg` | 平板真机截图 3000×2000（MatePad Pro Max 13.2，GEM-W40，v1.7.0）· 源图存档 | 源图存档；上传请用 `plain/tablet169/` |
| `plain/tablet169/tablet-1..4.jpg` | **16:9 · 1920×1080** 纯截图留边版（深蓝底、无文案、不裁 UI，左右各 ~150px 品牌留边） | AGC「平板」截图槽（≥3 张，建议 4 张全传；审核风险最低） |
| `plain/2in1/2in1-1..4.jpg` | 2in1 真机截图 3120×2080（第 2 张为窗口裁切 2092×1325） | AGC「2in1/PC」截图槽 |
| `phone/promo-1..10.jpg` | **9:16 · 1080×1920** 宣传框图（竖排标题+完整真机截图+品牌条） | 截图槽的"带文案"替代方案 / 商店推荐位 / 社媒 |
| `tablet/promo-1..4.jpg` | **16:9 · 1920×1080** 横版宣传框图 | 同上（平板形态；带文案替代方案） |
| `2in1/promo-1..4.jpg` | 2560×1600 横版宣传框图 | 同上（2in1 形态） |
| `promo/hero-1920x1080.jpg` | 主视觉横幅（ slogan + 三设备拼贴含 LUT 面板 + 冰川 splash 背景） | 推荐图 / 官网 / 社媒封面 |
| `promo/poster-1080x1920.jpg` | 竖版海报（图标 + slogan + 卖点胶囊） | 社媒 / 预热海报 |
| `promo/lut-1080x1920.jpg` | **9:16 胶片模拟专题图**：同一照片经仓库真实 .cube 渲染 6 风格对比（PROVIA/Velvia/Classic Neg/ACROS/ETERNA/REALA ACE） | 推荐位 / 社媒 / 详情页头图（营销渲染图，非 UI 截图） |

> 手机槽位按控制台要求 **9:16（1080×1920）**：`plain/phone916/` 与 `phone/promo-` 均已为该规格；
> 真机原生比例为 9:19.9，故采用"完整截图 + 品牌底色留边"，**不裁剪 UI**。竖版海报 `promo/poster-1080x1920.jpg` 亦为 9:16。
> 截图槽优先传 `plain/phone916/`（纯真实界面，审核风险最低）；`phone/promo-` 框图用于允许营销包装的槽位与站外宣传。
> 平板槽位为 **16:9（1920×1080）**：`plain/tablet169/`（纯截图留边，同"不裁 UI"策略，真机 3:2 → 左右留边）与 `tablet/promo-`（框图）均已为该规格（2026-11-21 起，原 3:2 版由 `make_tablet_169.py` 重制）；
> 2in1 为 16:10（2560×1600）框图，如控制台后续要求 16:9 可复用同一脚本模式扩展。

## 截图内容对照

- 手机：1 图库(深色) · 2 光线调整(深色) · 3 曲线(深色) · 4 主体蒙版已生成(浅色·新抓) · 5 LUT 已套用 Velvia(浅色·新抓) · 6 AI 降噪/RapidRes(深色) · 7 色彩/白平衡(浅色·新抓) · 8 裁剪与变换(浅色·新抓) · 9 图库(浅色·新抓) · 10 生成式修复(深色)
- 平板：1 图库 · 2 调整面板 · 3 LUT（已套用 Velvia、强度 100） · 4 编辑器画布+胶片条
- 2in1：1 图库+元数据 · 2 编辑器窗口 · 3 LUT 96 款全列表 · 4 元数据+调整工作流

## 素材来源与版权

- 手机/2in1 截图：2026-11-15~11-18 真机测试留存（`%TEMP%\opencode\`，已归档副本在 `plain/`）。
- 手机新增 4 张（主体蒙版/色彩/裁剪/浅色图库）：2026-11-21 本会话经 hdc 在 VYG-AL00 手机（1280×2832，v1.7.0 浅色）现抓；
  演示图为内置 splash 照片（推入 `Docs/Download` 后经文件选择器导入）。已知限制：图库多选栏「导出」按钮对 uitest 注入点击无响应，导出面板未抓；预设页为空态，不宜上架，未收录。
- 平板截图：2026-11-21 本会话经 hdc 在 GEM-W40 现抓（v1.7.0 浅色）；图库演示图为应用内置 splash 照片
  （`public/splash-*.jpg`，摄影 Timon Käch，应用内已署名），推入设备 `Pictures` 目录后授权导入。
  若审核/运营希望更丰富的图库画面，可换真实照片库重抓（配方见下）。
- 2in1 第 3 张 LUT 全列表来自 `C:\workspace\camraw-ui-reference\lightraw-lut-brand-nav-all.jpeg`（11 月初实机）。
- 图标：`src-tauri/gen/ohos/AppScope/resources/base/media/foreground.png`（**勿用旧 RapidRAW "R" 图标**，无授权）。
- 主视觉背景：`public/splash-dark.jpg`（同上署名）。

## 平板重抓配方（hdc）

```powershell
$sn='<tablet serial>'
hdc -t $sn shell aa start -b io.github.CyberTimon.RapidRAW -a EntryAbility
hdc -t $sn shell snapshot_display -f /data/local/tmp/s.jpeg
hdc -t $sn file recv /data/local/tmp/s.jpeg .\s.jpeg
# 交互：uitest uiInput click X Y / doubleClick X Y（坐标=截图像素；欢迎页「打开图库」→ SAF 选文件夹 →「选择」；
# 图库双击缩略图进编辑器；右侧竖条：调整=第3图标、LUT=立方体图标；LUT 面板「选择」展开后点片名套用）
```

注意：shell 视图的媒体根为 `/storage/media/100/local/files/`，选择器「个人/图片」映射到其下 `Docs/Images`；
推演示图用 `hdc file send <jpg> /storage/media/100/local/files/Docs/Images/<name>.jpg`。

## 再生成

```powershell
& '<bundled python>' store-listing\tools\make_assets.py
```

源图优先读 `%TEMP%\opencode\` 与 `tools/cap/`；temp 被清理后自动回退到 `assets/plain/` 归档副本，可离线复跑。
改标题/卖点：编辑脚本顶部 `PHONE / LAND_2IN1 / LAND_TAB` 表（`|` 为强制换行）。