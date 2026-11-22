#Requires -Version 5.1
<#
.SYNOPSIS
  Fast iteration loop for the OHOS tethering stack (libgphoto2 + libusb + the
  Rust glue) against a connected device.

.DESCRIPTION
  A full `scripts/build-ohos.ps1` run costs ~10 min because it recompiles Rust
  and repackages the whole HAP. When only the C stack changed (libgphoto2,
  libusb, camlibs) none of that is needed: the .so files are injected into the
  already-built HAP post-package, so the whole loop is ~70 s.

  Stages (measured on MOR-M1 / i7-12700H, 2026-11-22):
    gphoto incremental rebuild   43-45 s   (`-Clean` full stack is ~8-10 min)
    HAP lib re-injection          2 s      (`build-ohos.ps1 -SkipBuild`)
    signing                      12 s
    install + cold start         10 s

  Prereq: a HAP must already exist (run `scripts/build-ohos.ps1` once, and again
  whenever Rust/ArkTS/frontend sources change).

.PARAMETER Target
  OHOS ABI to build. Default aarch64.

.PARAMETER Device
  hdc target serial. Default: the single connected device.

.PARAMETER NoInstall
  Stop after signing; do not touch the device.

.PARAMETER SkipGphoto
  Re-inject the current libs without rebuilding the C stack.

.PARAMETER Clean
  Wipe the gphoto build tree first (slow; only after changing the libusb pin).

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File scripts\dev-ohos-tether.ps1
#>
param(
  [string]$Target = 'aarch64',
  [string]$Device = '',
  [switch]$NoInstall,
  [switch]$SkipGphoto,
  [switch]$Clean
)

# 'Continue' (not 'Stop'): PowerShell 5.1 turns a native command's stderr into a
# terminating ErrorRecord under 'Stop', and every tool here writes progress to
# stderr. Each stage validates its own success explicitly instead.
$ErrorActionPreference = 'Continue'
$repo = Split-Path -Parent $PSScriptRoot
$bundle = 'io.github.CyberTimon.RapidRAW'
$total = [System.Diagnostics.Stopwatch]::StartNew()

function Stage([string]$Name, [scriptblock]$Body) {
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  Write-Host "==> $Name" -ForegroundColor Cyan
  & $Body
  Write-Host "    $Name done in $([math]::Round($sw.Elapsed.TotalSeconds))s" -ForegroundColor DarkGray
}

# ---------- 1. C stack (incremental) ----------
if (-not $SkipGphoto) {
  Stage 'gphoto stack (incremental)' {
    $gargs = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "$PSScriptRoot\build-gphoto-ohos.ps1", '-Target', $Target)
    if ($Clean) { $gargs += '-Clean' }
    $out = & powershell.exe @gargs 2>&1
    $out | Select-String -Pattern 'installed:|error' | Select-Object -Last 2 | ForEach-Object { Write-Host "    $($_.Line.Trim())" }
    if (-not ($out | Select-String -Pattern 'OK: gphoto tethering stack ready' -Quiet)) {
      throw 'gphoto stack build failed (see output above)'
    }
  }
}

# ---------- 2. re-inject libs into the existing HAP ----------
Stage 'HAP lib re-injection' {
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$PSScriptRoot\build-ohos.ps1" -SkipBuild -Target $Target -TimeoutSec 900 2>&1 |
    Select-String -Pattern 'HAP:' | Select-Object -Last 1 | ForEach-Object { Write-Host "    $($_.Line.Trim())" }
}

# ---------- 3. sign (injection strips the signature) ----------
$signed = ''
Stage 'sign' {
  $signScript = Join-Path $repo '.csr\sign-injected-hap.ps1'
  if (Test-Path $signScript) {
    # This helper exits 1 even on success (it reports via stdout), so judge it by
    # the OUT=/SHA256= lines rather than the exit code.
    $out = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $signScript 2>&1
    $out | Select-String -Pattern 'OUT=|SHA256=' | ForEach-Object { Write-Host "    $($_.Line.Trim())" }
    $outLine = $out | Select-String -Pattern '^OUT=' | Select-Object -Last 1
    if (-not $outLine) { throw 'HAP signing failed' }
    # Use exactly what the signer produced; picking a *.hap by timestamp is racy
    # (entry-default-signed.hap and the debug copy get written seconds apart).
    $script:signed = ($outLine.Line -replace '^OUT=', '').Trim()
  } else {
    Write-Warning ".csr\sign-injected-hap.ps1 not present (signing material is untracked) - the HAP stays unsigned and cannot be installed"
  }
}

if ($NoInstall) {
  Write-Host "==> finished in $([math]::Round($total.Elapsed.TotalSeconds))s (-NoInstall)" -ForegroundColor Green
  exit 0
}

# ---------- 4. install + cold start ----------
$hdc = $null
foreach ($candidate in @(
    'C:\Program Files\Huawei\DevEco Studio\sdk\default\openharmony\toolchains\hdc.exe',
    "$env:LOCALAPPDATA\Huawei\Sdk\openharmony\toolchains\hdc.exe")) {
  if (Test-Path $candidate) { $hdc = $candidate; break }
}
if (-not $hdc) { $hdc = (Get-Command hdc -ErrorAction SilentlyContinue).Source }
if (-not $hdc) { throw 'hdc not found - install DevEco Studio or put hdc on PATH' }

# This hdc build refuses every command without an explicit -t (it answers
# "[Fail]ExecuteCommand need connect-key?"), so resolve the serial up front.
# With several devices attached, picking one silently is worse than stopping:
# installing onto the wrong device looks like a broken build.
if (-not $Device) { $Device = $env:OHOS_DEVICE }
if (-not $Device) {
  $targets = @(& $hdc list targets 2>&1 | Where-Object { $_ -and $_ -notmatch '^\[' -and $_.Trim() } | ForEach-Object { $_.Trim() })
  if ($targets.Count -eq 0) { throw 'no OHOS device connected (hdc list targets is empty)' }
  if ($targets.Count -gt 1) {
    Write-Host "    multiple devices attached:" -ForegroundColor Yellow
    $targets | ForEach-Object { Write-Host "      $_" -ForegroundColor Yellow }
    throw 'refusing to guess - re-run with -Device <serial> or set $env:OHOS_DEVICE'
  }
  $Device = $targets[0]
}
$hdcArgs = @('-t', $Device)
Write-Host "==> device $Device" -ForegroundColor Cyan

Stage 'install + start' {
  $hap = if ($script:signed) { $script:signed } else {
    (Get-ChildItem "$repo\src-tauri\gen\ohos\entry\build\default\outputs\default\*.hap" |
      Where-Object { $_.Name -like '*-signed.hap' } |
      Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
  }
  if (-not $hap -or -not (Test-Path $hap)) { throw 'no signed HAP found - run scripts\build-ohos.ps1 first' }
  Write-Host "    installing $([System.IO.Path]::GetFileName($hap))"

  & $hdc @hdcArgs install -r $hap 2>&1 | ForEach-Object { Write-Host "    $_" }
  if ($LASTEXITCODE -ne 0) { throw "hdc install failed (exit $LASTEXITCODE)" }

  & $hdc @hdcArgs shell "aa force-stop $bundle" 2>&1 | ForEach-Object { Write-Host "    $_" }
  Start-Sleep -Seconds 2
  & $hdc @hdcArgs shell "aa start -a EntryAbility -b $bundle" 2>&1 | ForEach-Object { Write-Host "    $_" }
  Start-Sleep -Seconds 3

  $pidOut = (& $hdc @hdcArgs shell "pidof $bundle" 2>&1) -join ''
  if (-not $pidOut.Trim()) { throw 'app is not running after start - check the device screen' }
  Write-Host "    running as pid $($pidOut.Trim())"
}

# The tether probe runs ~4 s after the window controller registers, which is
# ~15-20 s after `aa start`, so poll instead of sleeping a fixed amount.
Write-Host '==> waiting for the tethering probe' -ForegroundColor Cyan
$deadline = (Get-Date).AddSeconds(60)
$logLines = @()
while ((Get-Date) -lt $deadline) {
  Start-Sleep -Seconds 5
  $logLines = & $hdc @hdcArgs shell "grep -E 'tether-ohos' /data/app/el2/100/base/$bundle/files/logs/app.log | tail -8" 2>&1
  if ($logLines -match 'autodetect|usbManager device|CAMLIBS') { break }
}
Write-Host '==> tethering log' -ForegroundColor Cyan
$logLines

Write-Host "==> full loop $([math]::Round($total.Elapsed.TotalSeconds))s" -ForegroundColor Green
