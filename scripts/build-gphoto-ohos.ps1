# build-gphoto-ohos.ps1 - cross-compile the libgphoto2 tethering stack for OpenHarmony
#
# Builds, from the vendored pins (docs/OHOS_USB_TETHERING_PLAN.md §4.1):
#   libusb 1.0.29          (static -> linked into libgphoto_port_usb1.so)
#   libltdl (GNU libtool)  (static -> embedded in libgphoto2/libgphoto2_port)
#   libgphoto2 2.5.34      (core + port + usb1 io driver + ptp2/canon/directory camlibs)
#
# and installs, at src-tauri/libs/ohos/<abi>/:
#   libgphoto2.so  libgphoto2_port.so  libgphoto_port_usb1.so
#   libgphoto_cam_ptp2.so  libgphoto_cam_canon.so  libgphoto_cam_directory.so
#
# plus a link prefix at src-tauri/target/gphoto-ohos/<target>/prefix/ for the
# Rust side (GPHOTO2_LIB_DIR / GPHOTO2_INCLUDE_DIR, set by build-ohos.ps1).
#
# libgphoto2 is an autotools project, so unlike build-ort-ohos.ps1 (pure CMake)
# this needs a POSIX shell toolchain: MSYS2 supplies sh/make/autoconf/automake/
# libtool, while the compiler stays the Windows OHOS NDK clang (the NDK itself
# ships sh wrapper scripts, so this is the NDK's intended consumption pattern).
# MSYS2 is expected at $Msys2Root with base-devel+libtool+gettext-devel
# installed (see scripts README / docs; one-time setup, user-space, no admin).
#
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\build-gphoto-ohos.ps1
#   ... -Target x86_64            # emulator
#   ... -Clean                    # wipe the build dir first
#   ... -Jobs 8                   # parallel jobs (default: nproc)
param(
  [string]$Target = 'aarch64',   # aarch64 | armv7 | x86_64
  [switch]$Clean,
  [int]$Jobs = 0,
  [switch]$NoStrip,
  [string]$Msys2Root = "$env:USERPROFILE\msys64\msys64"
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Fail([string]$msg) { Write-Host "!! $msg" -ForegroundColor Red; exit 1 }

# ---------- resolve target ----------
$abiByTarget = @{ 'aarch64' = 'arm64-v8a'; 'armv7' = 'armeabi-v7a'; 'x86_64' = 'x86_64' }
$clangTargetByTarget = @{ 'aarch64' = 'aarch64-linux-ohos'; 'armv7' = 'arm-linux-ohos'; 'x86_64' = 'x86_64-linux-ohos' }
$hostTripleByTarget = @{ 'aarch64' = 'aarch64-linux'; 'armv7' = 'arm-linux'; 'x86_64' = 'x86_64-linux' }
$abi = $abiByTarget[$Target]
$clangTarget = $clangTargetByTarget[$Target]
$hostTriple = $hostTripleByTarget[$Target]
if (-not $abi) { Fail "unknown -Target '$Target' (expected aarch64|armv7|x86_64)" }

# ---------- resolve OHOS SDK (must be space-free: see HARMONYOS_PORTING.md §6.3) ----------
$dev = 'C:\Program Files\Huawei\DevEco Studio'
$sdkCandidates = @(
  "$env:USERPROFILE\ohos-devstudio\sdk\default\openharmony",
  "$dev\sdk\default\openharmony"
)
$sdk = $sdkCandidates | Where-Object { Test-Path "$_\native\llvm\bin\clang.exe" } | Select-Object -First 1
if (-not $sdk) { Fail "OHOS SDK not found (checked: $($sdkCandidates -join ', '))" }
if ($sdk -match ' ') {
  Fail "SDK path '$sdk' contains spaces - create the junction alias per HARMONYOS_PORTING.md §6.3 first"
}
$ndk = "$sdk\native"
Write-Host "using NDK: $ndk"

# ---------- resolve MSYS2 ----------
$msysBash = "$Msys2Root\usr\bin\bash.exe"
if (-not (Test-Path $msysBash)) { Fail "MSYS2 bash not found at $msysBash (pass -Msys2Root)" }
foreach ($t in @('make','autoconf','automake','libtoolize','autopoint','pkg-config')) {
  $found = & $msysBash -lc "command -v $t" 2>$null
  if (-not $found) { Fail "MSYS2 is missing '$t' - run: pacman -S --noconfirm base-devel libtool gettext gettext-devel" }
}
Write-Host "using MSYS2: $Msys2Root"

# ---------- vendored sources ----------
$vendorGphoto = "$repo\src-tauri\vendor\libgphoto2"
$vendorLibusb = "$repo\src-tauri\vendor\libusb"
foreach ($d in @($vendorGphoto, $vendorLibusb)) {
  if (-not (Test-Path $d)) { Fail "vendored sources missing at $d - run: git submodule update --init" }
}
$gphotoVer = git -C $vendorGphoto describe --tags 2>$null
$libusbVer = git -C $vendorLibusb describe --tags 2>$null
Write-Host "pins: libgphoto2 $gphotoVer / libusb $libusbVer"

# ---------- libtool tarball (libltdl source) ----------
$libtoolVer = '2.5.4'
$root = "$repo\src-tauri\target\gphoto-ohos\$Target"
$srcDir = "$root\src"
$libtoolTgz = "$root\libtool-$libtoolVer.tar.gz"
New-Item -ItemType Directory -Force -Path $srcDir | Out-Null
if (-not (Test-Path "$srcDir\libtool-$libtoolVer\libltdl\configure.ac")) {
  if (-not (Test-Path $libtoolTgz)) {
    Write-Host "==> fetching GNU libtool $libtoolVer (tuna gnu mirror)"
    & curl.exe -sSL --retry 5 --retry-all-errors --max-time 600 -o $libtoolTgz "https://mirrors.tuna.tsinghua.edu.cn/gnu/libtool/libtool-$libtoolVer.tar.gz"
    if ($LASTEXITCODE -ne 0) { Fail "libtool download failed" }
  }
  & tar.exe -xzf $libtoolTgz -C $srcDir
  if ($LASTEXITCODE -ne 0 -or -not (Test-Path "$srcDir\libtool-$libtoolVer\libltdl\configure.ac")) { Fail "libtool extraction failed" }
}

# ---------- stage submodule sources (autoreconf must not dirty the pins) ----------
foreach ($pair in @(
  @{ src = $vendorGphoto; dst = "$srcDir\libgphoto2" },
  @{ src = $vendorLibusb; dst = "$srcDir\libusb" }
)) {
  if ($Clean -and (Test-Path $pair.dst)) { Remove-Item -Recurse -Force $pair.dst }
  # Always re-sync rather than only staging when configure.ac is missing: edits to
  # the vendored pins (e.g. scripts/patches/*.patch applied to libusb) otherwise
  # never reach the compiler, and the build silently reuses the old objects.
  # robocopy without /MIR only copies new/changed files, so generated files
  # (configure, Makefile) in the staged tree survive.
  Write-Host "==> staging $($pair.src) -> $($pair.dst)"
  & robocopy $pair.src $pair.dst /E /XD .git /NFL /NDL /NJH /NJS | Out-Null
  if ($LASTEXITCODE -ge 8) { Fail "robocopy failed for $($pair.src)" }
}
# robocopy sets nonzero exit codes for success; reset so $ErrorActionPreference stays quiet
$global:LASTEXITCODE = 0

if ($Clean -and (Test-Path "$root\build")) { Remove-Item -Recurse -Force "$root\build" }
# -Clean must also drop the installed prefix: gphoto-build-inner.sh caches libltdl
# and libusb by testing for prefix/lib/*.a, so keeping prefix means a -Clean run
# still links the previously installed libusb.
if ($Clean -and (Test-Path "$root\prefix")) { Remove-Item -Recurse -Force "$root\prefix" }
New-Item -ItemType Directory -Force -Path "$root\build", "$root\prefix" | Out-Null

# ---------- MSYS path conversion ----------
function ToMsys([string]$p) { return ('/' + $p.Substring(0, 1).ToLower() + $p.Substring(2).Replace('\', '/')) }

$inner = "$repo\scripts\gphoto-build-inner.sh"
if (-not (Test-Path $inner)) { Fail "missing $inner" }

$ndkMsys = ToMsys $ndk
$jobs = if ($Jobs -gt 0) { "$Jobs" } else { '0' }   # 0 -> inner script uses nproc

# ---------- ld shim (docs/HARMONYOS_PORTING.md §6.30) ----------
# libtool asks the compiler for its linker via `clang -print-prog-name=ld`,
# which is a PATH lookup for "ld" -> it would find MSYS2's PE ld.exe. On
# x86_64 that ld answers the `-m elf_x86_64` probe well enough to be treated
# as a real GNU ld, then fails every shared-library probe -> configure
# silently disables shared libs. Shadow it with the NDK's own lld (renamed
# ld.exe so the PATH lookup finds it) plus its sibling DLLs.
$shimDir = "$root\toolshim"
New-Item -ItemType Directory -Force -Path $shimDir | Out-Null
Copy-Item "$ndk\llvm\bin\ld.lld.exe" "$shimDir\ld.exe" -Force
foreach ($dll in @('libxml2.dll', 'liblzma.dll', 'libz.dll', 'zlib.dll', 'libzstd.dll')) {
  if (Test-Path "$ndk\llvm\bin\$dll") { Copy-Item "$ndk\llvm\bin\$dll" $shimDir -Force }
}
# NB: MSYS2 login shells re-prepend /usr/bin via /etc/profile, so the shim
# must be re-prepended INSIDE the inner script (LD_SHIM env var below).
$env:LD_SHIM = ToMsys $shimDir

$env:HOST = $hostTriple
$env:CC = "$ndkMsys/llvm/bin/clang.exe -target $clangTarget --sysroot=$ndkMsys/sysroot -D__MUSL__"
$env:AR = "$ndkMsys/llvm/bin/llvm-ar.exe"
$env:RANLIB = "$ndkMsys/llvm/bin/llvm-ranlib.exe"
$env:STRIP = "$ndkMsys/llvm/bin/llvm-strip.exe"
$env:CFLAGS = '-O2 -fPIC'
$env:PKG_CONFIG_PATH = "$(ToMsys "$root\prefix")/lib/pkgconfig"
$env:PKG_CONFIG_LIBDIR = $env:PKG_CONFIG_PATH
$env:SRC_LIBTOOL = ToMsys "$srcDir\libtool-$libtoolVer"
$env:SRC_LIBUSB = ToMsys "$srcDir\libusb"
$env:SRC_GPHOTO = ToMsys "$srcDir\libgphoto2"
$env:BLD = ToMsys "$root\build"
$env:PREFIX = ToMsys "$root\prefix"
$env:JOBS = "$jobs"
$env:CAMLIB_SET = 'directory.la ptp2.la canon.la'

Write-Host "==> running inner build (MSYS2 autotools x NDK clang, target $clangTarget)"
& $msysBash -lc ('set -o pipefail; sh ' + (ToMsys $inner) + ' 2>&1 | tee ' + (ToMsys "$root\inner.log"))
if ($LASTEXITCODE -ne 0) { Fail "inner build failed - see $root\inner.log" }

# ---------- harvest + install into the HAP libs dir ----------
# Upstream install names are bare (usb1.so / ptp2.so / canon.so / directory.so)
# under versioned dirs; rename to the libgphoto_port_* / libgphoto_cam_* forms
# (the same names CamRAW ships in its HAP libs dir) for clarity in the flat
# HAP libs layout - dlopen scanning does not care about the file names.
$prefixLib = "$root\prefix\lib"
$harvest = @(
  @{ find = 'libgphoto2.so';        into = 'libgphoto2.so' },
  @{ find = 'libgphoto2_port.so';   into = 'libgphoto2_port.so' }
)
$usb1 = Get-ChildItem -Path $prefixLib -Recurse -Filter 'usb1.so' -File -ErrorAction SilentlyContinue | Select-Object -First 1
if ($usb1) { $harvest += @{ find = $usb1.FullName; into = 'libgphoto_port_usb1.so' } }
foreach ($cam in @('ptp2', 'canon', 'directory')) {
  $p = Get-ChildItem -Path $prefixLib -Recurse -Filter "$cam.so" -File -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($p) { $harvest += @{ find = $p.FullName; into = "libgphoto_cam_$cam.so" } }
}

$shipped = @()
foreach ($h in $harvest) {
  $src = if ($h.find -like '*:*') { Get-Item $h.find } else {
    Get-ChildItem -Path $prefixLib -Filter $h.find -File -ErrorAction SilentlyContinue | Select-Object -First 1
  }
  if (-not $src) { Fail "$($h.find) not produced under $prefixLib (see $root\inner.log)" }
  $shipped += @{ src = $src.FullName; dst = $h.into }
}

$libsDir = "$repo\src-tauri\libs\ohos\$abi"
New-Item -ItemType Directory -Force -Path $libsDir | Out-Null
$strip = "$ndk\llvm\bin\llvm-strip.exe"
foreach ($so in $shipped) {
  $dest = Join-Path $libsDir $so.dst
  Copy-Item $so.src $dest -Force
  if (-not $NoStrip) {
    Copy-Item $dest "$dest.unstripped" -Force
    & $strip --strip-unneeded $dest
    if ($LASTEXITCODE -ne 0) { Fail "llvm-strip failed on $dest" }
  }
  Write-Host ("installed: {0} ({1:N1} KB)" -f $dest, ((Get-Item $dest).Length / 1KB))
}

$stamp = "libgphoto2 $gphotoVer + libusb $libusbVer + libltdl $libtoolVer (camlibs: ptp2 canon directory)"
Set-Content -Path "$libsDir\gphoto-stack-version.txt" -Value $stamp -Encoding ASCII
Write-Host "stamp: $stamp"
Write-Host "OK: gphoto tethering stack ready at $libsDir"
Write-Host "Rust link prefix: $root\prefix (set GPHOTO2_LIB_DIR=$prefixLib, GPHOTO2_INCLUDE_DIR=$root\prefix\include when building the app)"
