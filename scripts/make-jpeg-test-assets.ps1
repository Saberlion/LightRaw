# make-jpeg-test-assets.ps1 - generate the local JPEG test corpus (license-clean,
# fully synthetic, with EXIF injected via GDI+).
#
# These exercise the non-RAW pipeline paths: large decode, grayscale, tiny
# images, solid color, EXIF make/model parsing, and the Orientation tag.
#
# Usage (from repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\make-jpeg-test-assets.ps1
param(
  [string]$Dest = "$PSScriptRoot\..\test-assets\jpeg"
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$dir = Resolve-Path $Dest -ErrorAction SilentlyContinue
if (-not $dir) { New-Item -ItemType Directory -Path $Dest -Force | Out-Null; $dir = (Resolve-Path $Dest).Path } else { $dir = $dir.Path }

$jpegCodec = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() | Where-Object { $_.MimeType -eq 'image/jpeg' }
function New-EncoderParams([int]$quality) {
  $p = New-Object System.Drawing.Imaging.EncoderParameters(1)
  $p.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter([System.Drawing.Imaging.Encoder]::Quality, [long]$quality)
  return $p
}

# PropertyItem has no public constructor; take an uninitialized instance.
function New-PropertyItem {
  return [System.Runtime.Serialization.FormatterServices]::GetUninitializedObject([System.Drawing.Imaging.PropertyItem])
}

function New-AsciiProperty([int]$id, [string]$text) {
  $item = New-PropertyItem
  $bytes = [System.Text.Encoding]::ASCII.GetBytes($text + "`0")
  $item.Id = $id
  $item.Type = 2           # ASCII
  $item.Len = $bytes.Length
  $item.Value = $bytes
  return $item
}

function New-ShortProperty([int]$id, [uint16]$v) {
  $item = New-PropertyItem
  $item.Id = $id
  $item.Type = 3           # SHORT
  $item.Len = 2
  $item.Value = [BitConverter]::GetBytes([uint16]$v)
  return $item
}

# --- shared painters ---------------------------------------------------------
function Draw-Patches([System.Drawing.Graphics]$g, [int]$w, [int]$h) {
  $rect = New-Object System.Drawing.Rectangle(0, 0, $w, $h)
  $sky = New-Object System.Drawing.Drawing2D.LinearGradientBrush($rect, [System.Drawing.Color]::FromArgb(30, 120, 200), [System.Drawing.Color]::FromArgb(235, 235, 230), 90)
  $g.FillRectangle($sky, $rect)
  # horizon + "ground" gradient
  $ground = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    (New-Object System.Drawing.Rectangle(0, [int]($h * 0.6), $w, [int]($h * 0.4))),
    [System.Drawing.Color]::FromArgb(60, 90, 40), [System.Drawing.Color]::FromArgb(20, 30, 15), 90)
  $g.FillRectangle($ground, 0, [int]($h * 0.6), $w, [int]($h * 0.4))
  # sun
  $g.FillEllipse([System.Drawing.Brushes]::Yellow, ($w * 0.72), ($h * 0.16), ($w * 0.09), ($w * 0.09))
  # color patches (for white-balance / color-mixer testing)
  $colors = @(
    [System.Drawing.Color]::Red,   [System.Drawing.Color]::Lime,  [System.Drawing.Color]::Blue,
    [System.Drawing.Color]::Cyan,  [System.Drawing.Color]::Magenta, [System.Drawing.Color]::Yellow,
    [System.Drawing.Color]::Black, [System.Drawing.Color]::White,  [System.Drawing.Color]::Gray
  )
  $pw = [int]($w * 0.07); $ph = [int]($h * 0.07)
  for ($i = 0; $i -lt $colors.Count; $i++) {
    $b = New-Object System.Drawing.SolidBrush($colors[$i])
    $g.FillRectangle($b, [int]($w * 0.06) + ($i % 3) * ($pw + 8), [int]($h * 0.06) + [Math]::Floor($i / 3) * ($ph + 8), $pw, $ph)
  }
  $font = New-Object System.Drawing.Font('Consolas', [Math]::Max(10, [int]($h / 90)))
  $g.DrawString('LightRaw JPEG test asset', $font, [System.Drawing.Brushes]::White, [int]($w * 0.06), [int]($h * 0.8))
}

function Save-Jpeg([System.Drawing.Bitmap]$bmp, [string]$name, [int]$quality = 95, $props = @()) {
  foreach ($p in $props) { $bmp.SetPropertyItem($p) }
  $path = Join-Path $dir $name
  $bmp.Save($path, $jpegCodec, (New-EncoderParams $quality))
  $bmp.Dispose()
  Write-Host ("{0}  {1} bytes  {2}" -f $name, (Get-Item $path).Length, $path)
}

# --- 1. large landscape scene, no EXIF ---------------------------------------
$w = 4096; $h = 2732
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
Draw-Patches $g $w $h
$g.Dispose()
Save-Jpeg $bmp 'landscape_4096x2732_noexif.jpg'

# --- 2. large landscape with EXIF (make/model/date) ---------------------------
$w = 4928; $h = 3264
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
Draw-Patches $g $w $h
$g.Dispose()
Save-Jpeg $bmp 'landscape_4928x3264_exif.jpg' 95 @(
  (New-AsciiProperty 0x010F 'LightRaw'),
  (New-AsciiProperty 0x0110 'Test Camera Synthetic'),
  (New-ShortProperty  0x0112 1),
  (New-AsciiProperty 0x9003 '2026:11:02 10:00:00'),
  (New-AsciiProperty 0x0132 '2026:11:02 10:00:00')
)

# --- 3. portrait content with EXIF Orientation=6 (rotate 90 CW) ---------------
$w = 3264; $h = 4928
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
Draw-Patches $g $w $h
$g.Dispose()
Save-Jpeg $bmp 'portrait_3264x4928_orientation6.jpg' 95 @(
  (New-AsciiProperty 0x010F 'LightRaw'),
  (New-AsciiProperty 0x0110 'Test Camera Synthetic'),
  (New-ShortProperty  0x0112 6),
  (New-AsciiProperty 0x9003 '2026:11:02 10:05:00')
)

# --- 4. grayscale gradient -----------------------------------------------------
$w = 2048; $h = 2048
$bmp = New-Object System.Drawing.Bitmap($w, $h, [System.Drawing.Imaging.PixelFormat]::Format8bppIndexed)
$pal = $bmp.Palette
for ($i = 0; $i -lt 256; $i++) { $pal.Entries[$i] = [System.Drawing.Color]::FromArgb($i, $i, $i) }
$bmp.Palette = $pal
$data = $bmp.LockBits((New-Object System.Drawing.Rectangle(0, 0, $w, $h)), [System.Drawing.Imaging.ImageLockMode]::WriteOnly, [System.Drawing.Imaging.PixelFormat]::Format8bppIndexed)
$row = New-Object byte[] ($data.Stride)
for ($y = 0; $y -lt $h; $y++) {
  for ($x = 0; $x -lt $w; $x++) { $row[$x] = [byte][int](255 * $x / ($w - 1)) }
  [System.Runtime.InteropServices.Marshal]::Copy($row, 0, [IntPtr]::Add($data.Scan0, $y * $data.Stride), $w)
}
$bmp.UnlockBits($data)
Save-Jpeg $bmp 'grayscale_gradient_2048x2048.jpg'

# --- 5. 18% gray flat field -----------------------------------------------------
$w = 1024; $h = 1024
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.Clear([System.Drawing.Color]::FromArgb(46, 46, 46))
$g.Dispose()
Save-Jpeg $bmp 'gray18_1024x1024.jpg'

# --- 6. tiny image --------------------------------------------------------------
$bmp = New-Object System.Drawing.Bitmap(64, 64)
$g = [System.Drawing.Graphics]::FromImage($bmp)
Draw-Patches $g 64 64
$g.Dispose()
Save-Jpeg $bmp 'tiny_64x64.jpg'

Write-Host 'jpeg test assets done'
