#Requires -Version 5.1
<#
.SYNOPSIS
   Free a USB camera from HarmonyOS's automatic MTP mount so libgphoto2 can claim it.

.DESCRIPTION
   A camera attached in its MTP/PTP USB mode (e.g. Sony pid 0x0e78) is auto-mounted
   by the system gphotofs (FUSE) at /mnt/data/external/mtp-<vid>-<pid> (DECIMAL ids,
   e.g. mtp-1356-3704 = Sony 054c:0e78). gphotofs claims interface 0 through its own
   usbfs fd, so libusb_claim_interface inside libgphoto2 fails with EBUSY and every
   tether_connect attempt dies (docs/HARMONYOS_PORTING.md 6.30, commit ffc466fb).

   Root-unmounting the gphotofs mount frees the interface. This helper lists every
   mtp-* mount on the device and unmounts them (lazy when busy), then verifies.

   Requires an hdc shell with root (dev units: MOR-M1 is uid=0). On user builds the
   shell is not root - the only remedies there are switching the camera to its
   PC Remote USB mode (pid 0d68 on Sony, never MTP-mounted) or keeping the app
   connected (an app-held interface claim blocks gphotofs from mounting at all).

.PARAMETER Device
   hdc target serial. Default: the single connected device (refuses to guess when
   several are attached, like dev-ohos-tether.ps1).

.EXAMPLE
   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\umount-camera-mtp.ps1
   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\umount-camera-mtp.ps1 -Device 7QB0126509000017
#>
param(
  [string]$Device = ''
)
$ErrorActionPreference = 'Continue'

$hdc = $null
foreach ($candidate in @(
    'C:\Program Files\Huawei\DevEco Studio\sdk\default\openharmony\toolchains\hdc.exe',
    "$env:LOCALAPPDATA\Huawei\Sdk\openharmony\toolchains\hdc.exe")) {
  if (Test-Path $candidate) { $hdc = $candidate; break }
}
if (-not $hdc) { $hdc = (Get-Command hdc -ErrorAction SilentlyContinue).Source }
if (-not $hdc) { throw 'hdc not found - install DevEco Studio or put hdc on PATH' }

if (-not $Device) { $Device = $env:OHOS_DEVICE }
if (-not $Device) {
  $targets = @(& $hdc list targets 2>&1 | Where-Object { $_ -and $_ -notmatch '^\[' -and $_.Trim() } | ForEach-Object { $_.Trim() })
  if ($targets.Count -eq 0) { throw 'no OHOS device connected (hdc list targets is empty)' }
  if ($targets.Count -gt 1) {
    throw "several devices attached ($($targets -join ', ')) - pass -Device <serial> explicitly"
  }
  $Device = $targets[0]
}

# Root check first - a plain "permission denied" from umount is slower to diagnose.
$id = (& $hdc -t $Device shell id 2>&1) -join ''
if ($id -notmatch 'uid=0\(root\)') {
  Write-Warning "hdc shell on $Device is not root ($id) - umount will fail on user builds."
  Write-Host "  Alternatives: switch the camera to PC Remote USB mode, or reconnect from the app"
  Write-Host "  (an app-held interface claim blocks the gphotofs mount entirely)."
}

$mounts = @((& $hdc -t $Device shell "mount | grep -o '/mnt/data/external/mtp-[0-9]*-[0-9]*'" 2>&1) |
  Where-Object { $_ -and $_.Trim() -and $_ -notmatch '^\[' } | ForEach-Object { $_.Trim() })
if ($mounts.Count -eq 0) {
  Write-Host 'no mtp-* mounts present - nothing to do (camera absent, or app already holds the interface)' -ForegroundColor Green
  exit 0
}

foreach ($m in $mounts) {
  Write-Host "umount $m" -ForegroundColor Cyan
  $out = (& $hdc -t $Device shell "umount $m" 2>&1) -join ''
  if ($out -match 'busy|device or resource busy') {
    Write-Host "  busy - retrying lazy (umount -l)" -ForegroundColor Yellow
    & $hdc -t $Device shell "umount -l $m" 2>&1 | Out-Null
  } elseif ($out) {
    Write-Host "  $out" -ForegroundColor Yellow
  }
}

$left = (& $hdc -t $Device shell "mount | grep -o '/mnt/data/external/mtp-[0-9]*-[0-9]*'" 2>&1) -join ''
if (-not $left) {
  Write-Host 'OK: all mtp-* mounts released - tether_connect can claim the camera now' -ForegroundColor Green
} else {
  Write-Warning "mounts still present: $left"
  exit 1
}
