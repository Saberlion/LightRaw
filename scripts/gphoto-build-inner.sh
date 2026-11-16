#!/bin/sh
# gphoto-build-inner.sh - the autotools half of scripts/build-gphoto-ohos.ps1
#
# Runs INSIDE MSYS2 bash. All configuration arrives as environment variables
# exported by the PowerShell wrapper:
#   SRC_LIBTOOL SRC_LIBUSB SRC_GPHOTO  - staged source dirs (MSYS paths)
#   BLD PREFIX JOBS                    - build dir, install prefix, job count
#   HOST                               - config.host triple (e.g. aarch64-linux)
#   CC AR RANLIB STRIP CFLAGS          - cross toolchain (NDK clang via wrapper vars)
#   CAMLIB_SET                         - camlibs to build (see configure.ac GP_CAMLIB_SET)
#
# Produces in $PREFIX: lib/libgphoto2.so, lib/libgphoto2_port.so (both
# flattened via -avoid-version for HAP packaging), lib/libgphoto2_port/
# libgphoto_port_usb1.so, the CAMLIB_SET camlibs, plus headers and the static
# libusb-1.0.a / libltdl.a that were linked into them.

set -e

if [ -z "$JOBS" ] || [ "$JOBS" -le 0 ] 2>/dev/null; then JOBS=$(nproc); fi

# libtool linker shadowing (docs/HARMONYOS_PORTING.md §6.30): re-prepend the
# ld shim AFTER the login shell profile has prepended /usr/bin, so
# `clang -print-prog-name=ld` resolves to the NDK lld instead of MSYS2's
# PE ld.exe. Also force lt_cv_prog_gnu_ld=yes: lld's -v banner says
# "compatible with GNU linkers", which libtool's "GNU ld" grep misses, and
# without with_gnu_ld=yes the linux tag config emits empty archive_cmds.
if [ -n "$LD_SHIM" ]; then export PATH="$LD_SHIM:$PATH"; fi
GNU_LD_CACHE_ARG="lt_cv_prog_gnu_ld=yes"

echo "== inner env =="
echo "HOST=$HOST CC=$CC"
echo "SRC_LIBTOOL=$SRC_LIBTOOL"
echo "SRC_LIBUSB=$SRC_LIBUSB"
echo "SRC_GPHOTO=$SRC_GPHOTO"
echo "PREFIX=$PREFIX BLD=$BLD JOBS=$JOBS"
echo "CAMLIB_SET=$CAMLIB_SET"

# ---------- 1. libltdl (from GNU libtool) ----------
# libgphoto2 requires an external libltdl; OHOS has none, so we build it
# static-only with -fPIC and let libgphoto2/libgphoto2_port embed their copy
# (same pattern as libusb -> usb1 driver).
if [ ! -f "$PREFIX/lib/libltdl.a" ]; then
  echo "== [1/3] libltdl =="
  cd "$SRC_LIBTOOL/libltdl"
  if [ ! -f ./configure ]; then autoreconf -fi; fi
  mkdir -p "$BLD/libltdl" && cd "$BLD/libltdl"
  # NB: --enable-ltdl-install is required; libtool's default is convenience-only
  # (builds libltdlc.la but installs nothing into $PREFIX).
  "$SRC_LIBTOOL/libltdl/configure" \
    --host="$HOST" --prefix="$PREFIX" \
    $GNU_LD_CACHE_ARG \
    --disable-shared --enable-static \
    --enable-ltdl-install \
    lt_cv_sys_lib_dlsearch_path_spec="$PREFIX/lib"
  make -j"$JOBS"
  make install
else
  echo "== [1/3] libltdl cached =="
fi

# ---------- 2. libusb (static, linked into the usb1 port driver) ----------
if [ ! -f "$PREFIX/lib/libusb-1.0.a" ]; then
  echo "== [2/3] libusb =="
  cd "$SRC_LIBUSB"
  if [ ! -f ./configure ]; then autoreconf -fi; fi
  mkdir -p "$BLD/libusb" && cd "$BLD/libusb"
  "$SRC_LIBUSB/configure" \
    --host="$HOST" --prefix="$PREFIX" \
    $GNU_LD_CACHE_ARG \
    --enable-static --disable-shared --disable-udev \
    --disable-tests
  make -j"$JOBS"
  make install
else
  echo "== [2/3] libusb cached =="
fi

# ---------- 3. libgphoto2 core + port + usb1 + camlibs ----------
echo "== [3/3] libgphoto2 =="
cd "$SRC_GPHOTO"
if [ ! -f ./configure ]; then autoreconf -fi; fi
mkdir -p "$BLD/gphoto" && cd "$BLD/gphoto"
if [ ! -f Makefile ]; then
  LTDLINCL="-I$PREFIX/include" \
  LIBLTDL="-L$PREFIX/lib -lltdl" \
  "$SRC_GPHOTO/configure" \
    --host="$HOST" --prefix="$PREFIX" \
    $GNU_LD_CACHE_ARG \
    --disable-nls \
    --without-libxml2 --without-libcurl --without-gdlib \
    --without-libtiff --without-libexif --without-libjpeg
fi
# -avoid-version flattens SONAMEs (libgphoto2.so instead of libgphoto2.so.6):
# HAP packaging needs DT_NEEDED to equal the packaged file name.
make -j"$JOBS" GP_CAMLIB_SET="$CAMLIB_SET" LDFLAGS="-avoid-version"
make install GP_CAMLIB_SET="$CAMLIB_SET" LDFLAGS="-avoid-version"

echo "== products =="
ls -la "$PREFIX/lib/" | grep -E "gphoto|usb|ltdl" || true
find "$PREFIX/lib" -name "libgphoto_*.so" -o -name "libgphoto2*.so" | head -20

echo INNER_DONE
