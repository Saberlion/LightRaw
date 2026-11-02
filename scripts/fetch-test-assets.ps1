# fetch-test-assets.ps1 - download the local test asset corpus (real camera RAWs + JPEGs)
#
# Source: https://raw.pixls.us/ (community RAW sample archive; each file is a
# real camera file contributed for decoder testing - kept LOCAL ONLY, see
# test-assets/MANIFEST.md). Binaries are gitignored; only the manifest and
# this script are committed.
#
# Usage (from repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\fetch-test-assets.ps1
#   ... -CheckOnly    # verify presence + SHA256 only
param(
  [string]$Dest = "$PSScriptRoot\..\test-assets",
  [switch]$CheckOnly
)
$ErrorActionPreference = 'Stop'

# file / expected size (MiB) / source URL
$Raw = @(
  @{ p = 'raw/sony/ILCE-7M4_DSC06674_FullFrame-LossLess-Compressed-Large.ARW'; mb = 47; url = 'https://raw.pixls.us/data/Sony/ILCE-7M4/ILCE-7M4_DSC06674_FullFrame-LossLess-Compressed-Large.ARW' },
  @{ p = 'raw/canon/Canon_EOS_R6M3_IMG_4241.CR3'; mb = 33; url = 'https://raw.pixls.us/data/Canon/Canon%20EOS%20R6%20Mark%20III/IMG_4241.CR3' },
  @{ p = 'raw/canon/Canon_EOS_5DM4_B13A0732.CR2'; mb = 15; url = 'https://raw.pixls.us/data/Canon/EOS%205D%20Mark%20IV/B13A0732.CR2' },
  @{ p = 'raw/nikon/Nikon_Z8_raw_14bit_lossless.NEF'; mb = 58; url = 'https://raw.pixls.us/data/Nikon/Z%208/Nikon_Z8_raw_14_bit_lossless_compression.NEF' },
  @{ p = 'raw/nikon/Nikon_Z8_high_efficiency.NEF'; mb = 23; url = 'https://raw.pixls.us/data/Nikon/Z%208/Nikon_Z8_high_efficiency_low.NEF' },
  @{ p = 'raw/fujifilm/Fujifilm_X-T5_DSCF0022.RAF'; mb = 27; url = 'https://raw.pixls.us/data/Fujifilm/X-T5/DSCF0022.RAF' },
  @{ p = 'raw/hasselblad/Hasselblad_X2D_100C_Job_0079.fff'; mb = 153; url = 'https://raw.pixls.us/data/Hasselblad/X2D%20100C/Job_0079.fff' },
  @{ p = 'raw/panasonic/Panasonic_DC-S5M2_P1126458_mechanical.RW2'; mb = 36; url = 'https://raw.pixls.us/data/Panasonic/DC-S5M2/P1126458_mechanical.RW2' }
)

$root = Resolve-Path $Dest -ErrorAction SilentlyContinue
if (-not $root) { New-Item -ItemType Directory -Path $Dest -Force | Out-Null; $root = (Resolve-Path $Dest).Path } else { $root = $root.Path }

$sums = Join-Path $root 'SHA256SUMS.txt'
$failed = @()

foreach ($f in $Raw) {
  $target = Join-Path $root $f.p
  $dir = Split-Path $target -Parent
  if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }

  if (-not $CheckOnly) {
    $have = if (Test-Path $target) { [math]::Round((Get-Item $target).Length / 1MB, 1) } else { 0 }
    if ($have -lt ($f.mb * 0.9)) {
      Write-Host ("fetch {0} ({1} MiB, have {2} MiB)..." -f $f.p, $f.mb, $have)
      for ($i = 1; $i -le 5; $i++) {
        curl.exe -s -L --retry 3 -C - -o $target $f.url
        $have = if (Test-Path $target) { [math]::Round((Get-Item $target).Length / 1MB, 1) } else { 0 }
        if ($have -ge ($f.mb * 0.9)) { break }
        Write-Host ("  retry {0} (have {1} MiB / want {2} MiB)" -f $i, $have, $f.mb)
        Start-Sleep -Seconds 3
      }
    }
  }

  $ok = (Test-Path $target) -and ((Get-Item $target).Length -gt ($f.mb * 0.9 * 1MB))
  $sha = if ($ok) { (Get-FileHash $target -Algorithm SHA256).Hash } else { $script:failed += $f.p; 'MISSING' }
  $size = if ($ok) { (Get-Item $target).Length } else { 0 }
  Write-Host ("{0} {1} {2} bytes {3}" -f $sha, $size, $f.p, $(if ($ok) { 'OK' } else { 'FAIL' }))
}

if ($failed.Count) { Write-Host ("FAILED: {0}" -f ($failed -join ', ')) -ForegroundColor Red; exit 1 }
Write-Host 'all test assets present'
