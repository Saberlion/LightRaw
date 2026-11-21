"""LightRaw AppGallery marketing asset generator.

Inputs : real device screenshots (temp opencode dir + tools/cap), in-package icon layers,
         bundled splash photo (public/splash-dark.jpg).
Outputs: store-listing/assets/{icon,plain,phone,tablet,2in1,promo}
Run    : python store-listing/tools/make_assets.py
"""
import os
import numpy as np
from PIL import Image, ImageDraw, ImageFont, ImageFilter

ROOT = r"C:\workspace\LightRaw"
TMP = r"C:\Users\fxre\AppData\Local\Temp\opencode"
REF = r"C:\workspace\camraw-ui-reference"
CAP = os.path.join(ROOT, "store-listing", "tools", "cap")
OUT = os.path.join(ROOT, "store-listing", "assets")
GEN = os.path.join(ROOT, "src-tauri", "gen", "ohos", "AppScope", "resources", "base", "media")
SPLASH = os.path.join(ROOT, "public", "splash-dark.jpg")

NAVY = [(4, 7, 15), (9, 19, 36), (16, 32, 58)]
ICE = (191, 217, 255)
WHITE = (245, 249, 255)
TAN = (192, 133, 82)
DIM = (140, 165, 205)

_fc = {}
def F(kind, size):
    key = (kind, size)
    if key not in _fc:
        path = {"zh": "C:/Windows/Fonts/msyhbd.ttc", "zhr": "C:/Windows/Fonts/msyh.ttc",
                "la": "C:/Windows/Fonts/segoeuib.ttf"}[kind]
        _fc[key] = ImageFont.truetype(path, size)
    return _fc[key]

def grad(w, h, stops=NAVY):
    t = np.linspace(0, 1, h)[:, None]
    seg = t * (len(stops) - 1)
    i = np.clip(seg.astype(int), 0, len(stops) - 2)
    f = (seg - i)[..., None]
    a = np.array(stops)[i]
    b = np.array(stops)[i + 1]
    arr = (a * (1 - f) + b * f).astype(np.uint8)
    return Image.fromarray(np.repeat(arr, w, axis=1), "RGB")

def icon(size):
    im = Image.open(os.path.join(GEN, "foreground.png")).convert("RGB")
    return im.resize((size, size), Image.LANCZOS)

def decor(canvas, size, x, y, alpha=42):
    d = icon(size).filter(ImageFilter.GaussianBlur(size // 14))
    d.putalpha(alpha)
    canvas.paste(d, (x, y), d)

def rounded(im, r):
    m = Image.new("L", im.size, 0)
    ImageDraw.Draw(m).rounded_rectangle([0, 0, im.size[0] - 1, im.size[1] - 1], r, fill=255)
    im = im.copy()
    im.putalpha(m)
    return im

def shadow(canvas, w, h, x, y, r, blur=28, alpha=150, dy=14):
    m = Image.new("L", (w, h), 0)
    ImageDraw.Draw(m).rounded_rectangle([0, 0, w - 1, h - 1], r, fill=255)
    m = m.filter(ImageFilter.GaussianBlur(blur)).point(lambda v: v * alpha // 255)
    canvas.paste(Image.new("RGB", (w, h), (0, 0, 0)), (x, y + dy), m)

def shot(canvas, path, x, y, w, r=28, border=True):
    im = Image.open(path).convert("RGB")
    h = round(w * im.height / im.width)
    im = im.resize((w, h), Image.LANCZOS)
    shadow(canvas, w, h, x, y, r)
    canvas.paste(rounded(im, r), (x, y), rounded(im, r))
    if border:
        ImageDraw.Draw(canvas).rounded_rectangle([x, y, x + w - 1, y + h - 1], r,
                                                 outline=(110, 150, 210), width=3)
    return h

def txt(d, x, y, s, font, fill=WHITE, anchor="lt"):
    d.text((x, y), s, font=font, fill=fill, anchor=anchor)

def wrap(d, s, font, maxw):
    tokens = s.split(" ") if " " in s else list(s)
    sep = " " if " " in s else ""
    lines, cur = [], ""
    for tk in tokens:
        cand = cur + sep + tk if cur else tk
        if d.textlength(cand, font=font) > maxw and cur:
            lines.append(cur)
            cur = tk
        else:
            cur = cand
        while d.textlength(cur, font=font) > maxw and len(cur) > 1:
            cut = len(cur) - 1
            while cut > 1 and d.textlength(cur[:cut], font=font) > maxw:
                cut -= 1
            lines.append(cur[:cut])
            cur = cur[cut:]
    if cur:
        lines.append(cur)
    return lines

def pill(d, cx, y, s, size, h=None):
    f = F("zh", size)
    w = int(d.textlength(s, font=f)) + size * 2
    h = h or int(size * 2.1)
    d.rounded_rectangle([cx - w // 2, y, cx + w // 2, y + h], h // 2, outline=TAN, width=3)
    txt(d, cx, y + h // 2, s, f, ICE, anchor="mm")
    return w, h

def wordmark(d, x, y, size, fill=WHITE, anchor="lt"):
    txt(d, x, y, "LightRaw", F("la", size), fill, anchor)

def mkdir(p):
    os.makedirs(p, exist_ok=True)
    return p

def resolve(p, *fallbacks):
    if os.path.exists(p):
        return p
    for f in fallbacks:
        if os.path.exists(f):
            return f
    raise FileNotFoundError(p)

# ---------------------------------------------------------------- phone frames
PHONE = [
    ("lr-ai-4.jpeg",  "RAW 图库，井井有条", "Every camera. One library."),
    ("lr-ai-2.jpeg",  "专业调色，指尖流转", "Pro tone, live on GPU."),
    ("lr-npu-2.jpeg", "曲线直方图，逐档精控", "Curves & histogram control."),
    ("ph-mask.jpeg",  "主体蒙版，一键生成", "One-tap AI subject mask."),
    ("ph-lut.jpeg",   "96 胶片模拟，一键风格", "96 film simulations, one tap."),
    ("lr-ai-12.jpeg", "AI 降噪，细节重生", "AI denoise + RapidRes 2x super-res."),
    ("ph-color.jpeg", "白平衡色温，感知调色", "Kelvin WB & perceptual color."),
    ("ph-crop.jpeg",  "裁剪构图，比例随手", "Crop, ratios & straighten."),
    ("ph-lib2.jpeg",  "深色浅色，跟随系统", "Dark / light, live follow."),
    ("lr-ai-8.jpeg",  "AI 修复，杂物一笔消失", "Generative fill & heal."),
]

def phone_promo(idx, src, head, sub):
    W, H = 1080, 1920  # AGC phone slot: 9:16
    c = grad(W, H)
    decor(c, 900, 520, -280, 36)
    d = ImageDraw.Draw(c)
    ic = rounded(icon(88), 20)
    c.paste(ic, (44, 64), ic)
    wordmark(d, 44, 176, 40)
    f = F("zh", 62)
    y = 268
    units = []
    for ch in head:
        if ch == " ":
            continue
        if ch.isascii() and ch.isalnum() and units and units[-1][0].isascii():
            units[-1] += ch
        else:
            units.append(ch)
    for u in units:
        if len(u) == 1 and not u.isascii():
            txt(d, 136, y, u, f, anchor="mt")
            y += 76
        else:
            tw = int(d.textlength(u, font=f)) + 4
            tmp = Image.new("RGBA", (tw, 70), (0, 0, 0, 0))
            ImageDraw.Draw(tmp).text((2, 0), u, font=f, fill=WHITE)
            rot = tmp.rotate(-90, expand=True)
            c.paste(rot, (136 - rot.width // 2, y), rot)
            y += rot.height + 12
    d.line([222, 268, 222, y - 24], fill=TAN, width=3)
    yy = y + 24
    for ln in wrap(d, sub, F("la", 26), 180):
        txt(d, 44, yy, ln, F("la", 26), DIM)
        yy += 34
    p = resolve(os.path.join(TMP, src), os.path.join(CAP, src),
                os.path.join(OUT, "plain", "phone", f"phone-{idx}.jpg"))
    im = Image.open(p)
    sh_h = 1520
    sw = round(sh_h * im.width / im.height)
    shot(c, p, W - sw - 60, 150, sw, r=36)
    txt(d, W // 2, H - 60, "GPU 加速 · 非破坏性 · 本地 AI", F("zhr", 26), DIM, anchor="mm")
    p = os.path.join(mkdir(os.path.join(OUT, "phone")), f"promo-{idx}.jpg")
    c.save(p, quality=92)
    return p

def phone_plain916(idx, src):
    """Pure screenshot letterboxed into 9:16 1080x1920 (no marketing text)."""
    W, H = 1080, 1920
    c = grad(W, H)
    p = resolve(os.path.join(TMP, src), os.path.join(CAP, src),
                os.path.join(OUT, "plain", "phone", f"phone-{idx}.jpg"))
    im = Image.open(p)
    sh_h = 1580
    sw = round(sh_h * im.width / im.height)
    shot(c, p, (W - sw) // 2, 170, sw, r=36)
    out = os.path.join(mkdir(os.path.join(OUT, "plain", "phone916")), f"phone-{idx}.jpg")
    c.save(out, quality=95)
    return out

def tablet_plain169(idx, src):
    """Tablet screenshot letterboxed into 16:9 1920x1080 (AGC tablet slot).

    The native capture is 3000x2000 (3:2); scaling to the 1080-height of a
    16:9 canvas leaves 150px brand navy bars left/right - UI stays uncropped
    (same no-crop policy as phone_plain916).
    """
    W, H = 1920, 1080
    c = grad(W, H)
    p = resolve(os.path.join(CAP, src),
                os.path.join(OUT, "plain", "tablet", f"tablet-{idx}.jpg"))
    im = Image.open(p)
    sh_h = 880
    sw = round(sh_h * im.width / im.height)
    shot(c, p, (W - sw) // 2, (H - sh_h) // 2, sw, r=24)
    out = os.path.join(mkdir(os.path.join(OUT, "plain", "tablet169")), f"tablet-{idx}.jpg")
    c.save(out, quality=95)
    return out

# ------------------------------------------------------------ landscape frames
LAND_2IN1 = [
    ("mor-12.jpeg", "图库 · 元数据|批量工作流", "Library, metadata & batch workflow.",
     ["评分 / 色标 / 关键词，XMP 随导出嵌入", "Lensfun 镜头校正与自动拉直", "批量重命名、批量导出、虚拟副本"]),
    ("mor-ai.png", "专业显影|GPU 实时驱动", "Pro develop engine, live on GPU.",
     ["亿级像素流畅缩放与调整", "高光重建保留色彩层次", "OKLab 色轮 · HSL · 曲线精调"]),
    (os.path.join(REF, "lightraw-lut-brand-nav-all.jpeg"), "96 胶片模拟|一键上身", "96 film simulations, one tap.",
     ["Sony / Fuji / Canon 风格 + CamRAW 胶片包", "LUT 强度无级可调", "自定义 .cube LUT 导入"]),
    ("mor-11.jpeg", "每一步|都可回头", "Non-destructive, always reversible.",
     ["侧车记录调整，原图始终如初", "前后分屏 / 并排对比", "编辑历史与虚拟副本"]),
]
LAND_TAB = [
    ("tab-lib.jpeg", "大屏图库|触控亦从容", "Big-screen library, touch-ready.",
     ["文件夹 / 相册导入，授权持久化", "评分与色标，批量管理", "重启续编，不重选目录"]),
    ("tab-tone.jpeg", "专业调色|指尖流转", "Pro tone at your fingertips.",
     ["GPU 实时显影，所见即所得", "色调映射 / 曝光 / 曲线精调", "触控优化的滑杆与热区"]),
    ("tab-lut.jpeg", "96 胶片模拟|一键风格", "96 film simulations, one tap.",
     ["Sony / Fuji / Canon 风格 chips", "LUT 强度无级可调", "实时预览，一键套用"]),
    ("tab-editor.jpeg", "亿级像素画布|流畅缩放", "100MP canvas, smooth zoom.",
     ["非破坏性侧车，原图始终如初", "胶片条快速切换", "深色 / 浅色跟随系统"]),
]

def land_promo(folder, idx, src, head, sub, bullets, W, H):
    s = W / 2560
    c = grad(W, H)
    decor(c, int(1500 * s), int(-350 * s), int(-500 * s), 34)
    d = ImageDraw.Draw(c)
    isz = int(120 * s)
    ic = rounded(icon(isz), int(26 * s))
    c.paste(ic, (int(110 * s), int(104 * s)), ic)
    wordmark(d, int(110 * s) + isz + int(28 * s), int(104 * s) + int(18 * s), int(62 * s))
    f = F("zh", int(88 * s))
    y = int(330 * s)
    for part in head.split("|"):
        for ln in wrap(d, part, f, int(880 * s)):
            txt(d, int(110 * s), y, ln, f)
            y += int(112 * s)
    txt(d, int(110 * s), y + int(8 * s), sub, F("la", int(34 * s)), DIM)
    y += int(86 * s)
    for b in bullets:
        d.ellipse([int(112 * s), y + int(16 * s), int(128 * s), y + int(32 * s)], fill=TAN)
        txt(d, int(152 * s), y, b, F("zhr", int(38 * s)), ICE)
        y += int(64 * s)
    txt(d, int(110 * s), H - int(220 * s), "手机 · 平板 · 2in1，同一间暗房", F("zh", int(40 * s)), ICE)
    txt(d, int(110 * s), H - int(96 * s), "LightRaw · HarmonyOS", F("la", int(30 * s)), DIM)
    p = src if os.path.isabs(src) else os.path.join(TMP, src)
    p = resolve(p, os.path.join(CAP, src), os.path.join(OUT, "plain", folder, f"{folder}-{idx}.jpg"))
    sw = int(1370 * s)
    im = Image.open(p)
    sh_h = round(sw * im.height / im.width)
    sx, sy = int(1080 * s), (H - sh_h) // 2
    shot(c, p, sx, sy, sw, r=int(26 * s))
    out = os.path.join(mkdir(os.path.join(OUT, folder)), f"promo-{idx}.jpg")
    c.save(out, quality=92)
    return out

# ------------------------------------------------------------------ hero/poster
def hero():
    W, H = 1920, 1080
    c = grad(W, H)
    sp = Image.open(SPLASH).convert("RGB")
    # cover-crop splash into right 62%
    tw, th = 1250, 1080
    r = max(tw / sp.width, th / sp.height)
    sp = sp.resize((round(sp.width * r), round(sp.height * r)), Image.LANCZOS)
    sp = sp.crop(((sp.width - tw) // 2, (sp.height - th) // 2, (sp.width - tw) // 2 + tw, (sp.height - th) // 2 + th))
    base = np.asarray(c).astype(np.float32)
    spa = np.asarray(sp).astype(np.float32)
    a = np.clip(np.arange(tw) / 700.0, 0, 1)[None, :, None]
    base[:, W - tw:] = base[:, W - tw:] * (1 - a) + spa * a
    c = Image.fromarray(base.astype(np.uint8))
    d = ImageDraw.Draw(c)
    ic = rounded(icon(116), 26)
    c.paste(ic, (100, 84), ic)
    wordmark(d, 244, 102, 68)
    txt(d, 100, 268, "轻于形，强于芯。", F("zh", 104))
    txt(d, 100, 402, "你的全设备 RAW 暗房。", F("zh", 66), ICE)
    y = 510
    for ln in ["Light in form. Mighty in core.", "Your RAW darkroom on every device."]:
        txt(d, 100, y, ln, F("la", 32), DIM)
        y += 44
    f = F("zh", 34)
    for i, s in enumerate(["GPU 实时显影", "AI 降噪 · 蒙版", "96 胶片模拟", "非破坏性编辑"]):
        r, cc = divmod(i, 2)
        w = int(d.textlength(s, font=f)) + 68
        x = 100 + cc * 400
        yy = 640 + r * 96
        d.rounded_rectangle([x, yy, x + w, yy + 76], 38, outline=TAN, width=3)
        txt(d, x + w // 2, yy + 38, s, f, ICE, anchor="mm")
    txt(d, 100, 960, "HarmonyOS · 手机 / 平板 / 2in1 · 摄影", F("zhr", 32), DIM)
    # device collage on the right
    shot(c, resolve(os.path.join(TMP, "mor-ai.png"), os.path.join(OUT, "plain", "2in1", "2in1-2.jpg")), 1150, 120, 700, r=20)
    shot(c, resolve(os.path.join(CAP, "tab-lut.jpeg"), os.path.join(OUT, "plain", "tablet", "tablet-3.jpg")), 1080, 620, 620, r=18)
    shot(c, resolve(os.path.join(TMP, "lr-ai-2.jpeg"), os.path.join(OUT, "plain", "phone", "phone-2.jpg")), 1560, 300, 300, r=22)
    p = os.path.join(mkdir(os.path.join(OUT, "promo")), "hero-1920x1080.jpg")
    c.save(p, quality=92)
    return p

def poster():
    W, H = 1080, 1920
    c = grad(W, H)
    decor(c, 1200, -60, -160, 46)
    d = ImageDraw.Draw(c)
    ic = rounded(icon(380), 84)
    shadow(c, 380, 380, (W - 380) // 2, 150, 84, blur=40, alpha=170)
    c.paste(ic, ((W - 380) // 2, 150), ic)
    wordmark(d, W // 2, 610, 84, anchor="mt")
    txt(d, W // 2, 730, "把专业暗房，", F("zh", 78), anchor="mt")
    txt(d, W // 2, 830, "装进每一台设备。", F("zh", 78), anchor="mt")
    txt(d, W // 2, 960, "Every ray of light deserves a careful develop.", F("la", 32), DIM, anchor="mt")
    ys = 1080
    for row in [["GPU 实时显影", "AI 降噪 · 蒙版"], ["96 胶片模拟", "非破坏性编辑"]]:
        ws = [int(d.textlength(s, font=F("zh", 38))) + 76 for s in row]
        total = sum(ws) + 32
        x = (W - total) // 2
        for s, w in zip(row, ws):
            d.rounded_rectangle([x, ys, x + w, ys + 92], 46, outline=TAN, width=3)
            txt(d, x + w // 2, ys + 46, s, F("zh", 38), ICE, anchor="mm")
            x += w + 32
        ys += 124
    d.line([140, 1420, W - 140, 1420], fill=(60, 84, 128), width=2)
    txt(d, W // 2, 1480, "手机 · 平板 · 2in1，同一间暗房", F("zh", 44), anchor="mt")
    txt(d, W // 2, 1560, "深色 / 浅色跟随 · 15 种语言 · 本地 AI 推理", F("zhr", 32), DIM, anchor="mt")
    txt(d, W // 2, 1780, "LightRaw · HarmonyOS · 摄影", F("zhr", 32), DIM, anchor="mt")
    p = os.path.join(OUT, "promo", "poster-1080x1920.jpg")
    c.save(p, quality=92)
    return p

# ------------------------------------------------------------- LUT showcase
LUT_DIR = r"C:\workspace\LightRaw-luts\camera-styles"
LUT_SHOW = [
    ("PROVIA", "camera-fuji-provia-v3-da2033e1f70e.cube"),
    ("Velvia", "camera-fuji-velvia-v3-975e1c2adcfc.cube"),
    ("Classic Neg", "camera-fuji-classic-neg-v3-9ce5bd402270.cube"),
    ("ACROS", "camera-fuji-acros-v3-ef58a2ddf33a.cube"),
    ("ETERNA", "camera-fuji-eterna-v3-f56b432da1a1.cube"),
    ("REALA ACE", "camera-fuji-reala-ace-v3-4b2a9bd806e4.cube"),
]

def load_cube(path):
    n, vals = None, []
    with open(path, encoding="utf-8", errors="ignore") as f:
        for ln in f:
            ln = ln.strip()
            if ln.startswith("LUT_3D_SIZE"):
                n = int(ln.split()[1])
            elif ln and ln[0].isdigit() or (ln and ln[0] in ".-"):
                vals.append([float(x) for x in ln.split()])
    return np.array(vals, dtype=np.float32).reshape(n, n, n, 3)

def apply_lut(arr, cube):
    n = cube.shape[0]
    c = np.clip(arr, 0, 1) * (n - 1)
    i0 = np.floor(c).astype(int)
    i1 = np.minimum(i0 + 1, n - 1)
    f = c - i0
    x0, y0, z0 = i0[..., 0], i0[..., 1], i0[..., 2]
    x1, y1, z1 = i1[..., 0], i1[..., 1], i1[..., 2]
    fx, fy, fz = f[..., 0:1], f[..., 1:2], f[..., 2:3]
    def at(x, y, z):
        return cube[z, y, x]  # .cube table: red fastest -> [B,G,R]
    c00 = at(x0, y0, z0) * (1 - fx) + at(x1, y0, z0) * fx
    c10 = at(x0, y1, z0) * (1 - fx) + at(x1, y1, z0) * fx
    c01 = at(x0, y0, z1) * (1 - fx) + at(x1, y0, z1) * fx
    c11 = at(x0, y1, z1) * (1 - fx) + at(x1, y1, z1) * fx
    c0 = c00 * (1 - fy) + c10 * fy
    c1 = c01 * (1 - fy) + c11 * fy
    return np.clip(c0 * (1 - fz) + c1 * fz, 0, 1)

def lut_showcase():
    W, H = 1080, 1920
    c = grad(W, H)
    decor(c, 900, 520, -280, 36)
    d = ImageDraw.Draw(c)
    ic = rounded(icon(80), 18)
    c.paste(ic, (60, 56), ic)
    wordmark(d, 160, 74, 44)
    txt(d, 60, 180, "96 款胶片模拟", F("zh", 76))
    txt(d, 60, 280, "96 film simulations. One tap.", F("la", 30), DIM)
    base = Image.open(SPLASH.replace("splash-dark", "splash-light")).convert("RGB")
    s = min(base.size)
    base = base.crop(((base.width - s) // 2, (base.height - s) // 2,
                      (base.width - s) // 2 + s, (base.height - s) // 2 + s))
    arr = np.asarray(base.resize((470, 470), Image.LANCZOS)).astype(np.float32) / 255.0
    for i, (name, fn) in enumerate(LUT_SHOW):
        r, cc = divmod(i, 2)
        x = 60 + cc * 490
        y = 350 + r * 490
        cube = load_cube(os.path.join(LUT_DIR, fn))
        tile = Image.fromarray((apply_lut(arr, cube) * 255).astype(np.uint8))
        tile = rounded(tile, 24)
        shadow(c, 470, 470, x, y, 24, blur=20, alpha=120, dy=10)
        c.paste(tile, (x, y), tile)
        ov = Image.new("RGBA", (470, 56), (0, 0, 0, 150))
        c.paste(ov, (x, y + 470 - 56), ov)
        txt(d, x + 20, y + 470 - 28, name, F("la", 30), WHITE, anchor="lm")
    txt(d, W // 2, 1856, "Sony / Fuji / Canon + CamRAW 胶片包 · 强度无级可调 · 自定义 .cube",
        F("zhr", 26), DIM, anchor="mm")
    p = os.path.join(OUT, "promo", "lut-1080x1920.jpg")
    c.save(p, quality=92)
    return p

# ----------------------------------------------------------------------- plain
def plains():
    jobs = [("phone", [n for n, _, _ in PHONE]),
            ("tablet", [n for n, _, _, _ in LAND_TAB]),
            ("2in1", [n for n, _, _, _ in LAND_2IN1])]
    for folder, names in jobs:
        od = mkdir(os.path.join(OUT, "plain", folder))
        for i, n in enumerate(names, 1):
            p = n if os.path.isabs(n) else os.path.join(TMP, n)
            dst = os.path.join(od, f"{folder}-{i}.jpg")
            if not os.path.exists(p):
                p = os.path.join(CAP, n)
            if not os.path.exists(p):
                if os.path.exists(dst):
                    print("plain", folder, i, "kept existing")
                    continue
                raise FileNotFoundError(p)
            im = Image.open(p).convert("RGB")
            im.save(dst, quality=95)
            print("plain", folder, i, im.size, os.path.basename(p))

def icons():
    od = mkdir(os.path.join(OUT, "icon"))
    for s in (1024, 512, 216):
        icon(s).save(os.path.join(od, f"lightraw-icon-{s}.png"))
    print("icons done")

if __name__ == "__main__":
    icons()
    plains()
    for i, (n, h, s) in enumerate(PHONE, 1):
        print(phone_promo(i, n, h, s))
        print(phone_plain916(i, n))
    for i, (n, h, s, b) in enumerate(LAND_2IN1, 1):
        print(land_promo("2in1", i, n, h, s, b, 2560, 1600))
    for i, (n, h, s, b) in enumerate(LAND_TAB, 1):
        print(land_promo("tablet", i, n, h, s, b, 1920, 1080))
        print(tablet_plain169(i, n))
    print(hero())
    print(poster())
    print(lut_showcase())