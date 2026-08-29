#!/usr/bin/env bash
# Phase 3.8.6: builds the Windows x64 installer WITH the `whisper` Cargo
# feature (real local transcription) enabled. Plain `cargo build --features
# whisper --target x86_64-pc-windows-gnu` (or `tauri build` with the same
# flags) does NOT work out of the box when cross-compiling from Linux to
# `x86_64-pc-windows-gnu` - it hits two independent, real upstream/toolchain
# defects, both root-caused during this phase's audit (see
# docs/phase-3-8-6-audit.md and docs/phase-3-8-6-windows-whisper-build.md):
#
# 1. whisper.cpp's vendored `ggml` CMake build (pulled in transitively by
#    the `whisper-rs-sys` crate) unconditionally does
#    `set(CMAKE_STATIC_LIBRARY_PREFIX "")` for any Windows target, assuming
#    MSVC-style unprefixed `.lib` naming. The actual linker here is MinGW's
#    GNU `ld` (via `x86_64-w64-mingw32-gcc`), which still expects Unix-style
#    `libX.a` naming for a plain `-lX` flag - so `ggml.a`/`ggml-base.a`/
#    `ggml-cpu.a` (missing prefix) are produced but never found. The
#    sibling `whisper` library itself is unaffected (a different
#    CMakeLists.txt), which is why only these three archives need fixing.
#    Cargo does not offer a supported hook to fix a dependency's own build
#    script output before that dependency compiles (a downstream crate's
#    build.rs runs too late - confirmed by direct testing this phase), so
#    this script does it as a real two-pass build: attempt once, then copy
#    correctly-named files into the exact directories the failed compile's
#    own `-L` flags already pointed at, then retry.
#
# 2. The default `x86_64-w64-mingw32-gcc`/`g++` alternative in this
#    container is the "win32" (Win32 threading model) variant, but
#    whisper.cpp/ggml's build links against `-lpthread`/expects POSIX
#    threading, producing `undefined reference to '__mingwthr_key_dtor'`
#    at final link time. The "posix" threading variant is already
#    installed alongside it (Ubuntu ships both) - this script switches to
#    it via `update-alternatives` before building.
#
# Phase 3.8.6.1: the resulting `cip-desktop.exe` genuinely, dynamically
# depends on `libstdc++-6.dll` (confirmed via
# `x86_64-w64-mingw32-objdump -p` against the built binary - whisper.cpp's
# C++ code pulls it in), which itself dynamically depends on
# `libgcc_s_seh-1.dll` and `libwinpthread-1.dll` (confirmed the same way,
# against each DLL in turn). None of the three ship with a stock Windows
# installation, and Tauri's NSIS bundler does not know to include them
# automatically - installing on a real Windows machine without them fails
# immediately with "libstdc++-6.dll was not found" (a real Environment C
# failure, not a hypothetical one). This script stages exactly those three
# files, resolved from the *active* mingw-w64-x86-64 toolchain via
# `-print-file-name` (never a hardcoded GCC-version path, so this keeps
# working across a toolchain upgrade), strips their debug info to keep the
# installer size sane, and `tauri.windows.conf.json`'s `bundle.resources`
# (a Windows-only config override Tauri merges automatically) places them
# in `$INSTDIR` next to `cip-desktop.exe` - the directory Windows' default
# DLL search order checks first.
#
# Usage: scripts/build-windows-whisper.sh
# Must be run with permission to call `update-alternatives` (root, in this
# container) and from the repository root.

set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v 7z >/dev/null 2>&1; then
  echo "ERROR: 7z is required to verify the packaged installer's contents (e.g. 'apt-get install -y p7zip-full')" >&2
  exit 1
fi

echo "==> Switching x86_64-w64-mingw32-gcc/g++ to the posix-threads variant"
update-alternatives --set x86_64-w64-mingw32-gcc /usr/bin/x86_64-w64-mingw32-gcc-posix
update-alternatives --set x86_64-w64-mingw32-g++ /usr/bin/x86_64-w64-mingw32-g++-posix

echo "==> Staging Windows runtime DLLs (libstdc++-6.dll, libgcc_s_seh-1.dll, libwinpthread-1.dll) from the active mingw toolchain"
# Must happen BEFORE the first `cargo build`, not just before packaging:
# tauri-build's own build.rs (run as part of compiling cip-desktop, even a
# plain `cargo build`) validates that every `tauri.windows.conf.json`
# `bundle.resources` path actually exists on disk, and fails the whole
# compile with "resource path ... doesn't exist" otherwise - confirmed by
# directly running this script and observing the exact failure.
runtime_dir="apps/desktop/src-tauri/windows-runtime"
mkdir -p "$runtime_dir"
for dll in libstdc++-6.dll libgcc_s_seh-1.dll; do
  src="$(x86_64-w64-mingw32-g++ -print-file-name="$dll")"
  if [ ! -f "$src" ]; then
    echo "ERROR: could not locate $dll via 'x86_64-w64-mingw32-g++ -print-file-name' - got '$src'" >&2
    exit 1
  fi
  cp "$src" "$runtime_dir/$dll"
  echo "    staged: $dll (from $src)"
done
winpthread_src="$(x86_64-w64-mingw32-gcc -print-file-name=libwinpthread-1.dll)"
if [ ! -f "$winpthread_src" ]; then
  echo "ERROR: could not locate libwinpthread-1.dll via 'x86_64-w64-mingw32-gcc -print-file-name' - got '$winpthread_src'" >&2
  exit 1
fi
cp "$winpthread_src" "$runtime_dir/libwinpthread-1.dll"
echo "    staged: libwinpthread-1.dll (from $winpthread_src)"
x86_64-w64-mingw32-strip --strip-debug "$runtime_dir"/*.dll
echo "    stripped debug info from staged DLLs"
for dll in "$runtime_dir"/*.dll; do
  arch="$(file "$dll")"
  case "$arch" in
    *"x86-64"*) ;;
    *) echo "ERROR: $dll is not x86-64 - refusing to package a mismatched-architecture DLL: $arch" >&2; exit 1 ;;
  esac
done
echo "    architecture verified: all staged DLLs are x86-64"

echo "==> First build attempt (expected to fail on the ggml lib-name defect if whisper-rs-sys has not been built with this toolchain variant before)"
if cargo build --target x86_64-pc-windows-gnu -p cip-desktop --features whisper --release; then
  echo "==> First attempt succeeded (whisper-rs-sys build was already cached with correctly-named libs)"
else
  echo "==> First attempt failed as expected - applying the ggml lib-name fix"
  build_dir="$(find target/x86_64-pc-windows-gnu/release/build -maxdepth 1 -name 'whisper-rs-sys-*' | head -1)"
  if [ -z "$build_dir" ]; then
    echo "ERROR: could not find whisper-rs-sys's build directory - the CMake build step itself may have failed for an unrelated reason. Re-run with 'cargo build -v ...' to see the real error." >&2
    exit 1
  fi
  for lib in ggml ggml-base ggml-cpu; do
    find "$build_dir" -name "${lib}.a" | while read -r src; do
      cp "$src" "$(dirname "$src")/lib${lib}.a"
      echo "    fixed: $(dirname "$src")/lib${lib}.a"
    done
  done

  echo "==> Retrying build with correctly-named libs in place"
  cargo build --target x86_64-pc-windows-gnu -p cip-desktop --features whisper --release
fi

echo "==> Rust build succeeded. Packaging the NSIS installer."
(cd apps/desktop && npx tauri build --target x86_64-pc-windows-gnu --features whisper)

echo "==> Verifying the packaged installer actually contains the runtime DLLs"
installer="target/x86_64-pc-windows-gnu/release/bundle/nsis/Church Intelligence Platform_0.1.0_x64-setup.exe"
extract_dir="$(mktemp -d)"
(cd "$extract_dir" && 7z x "$OLDPWD/$installer" -y >/dev/null)
for dll in libstdc++-6.dll libgcc_s_seh-1.dll libwinpthread-1.dll; do
  if [ ! -f "$extract_dir/$dll" ]; then
    echo "ERROR: $dll is missing from the packaged installer - the resources config did not apply" >&2
    exit 1
  fi
done
rm -rf "$extract_dir"
echo "    confirmed: libstdc++-6.dll, libgcc_s_seh-1.dll, libwinpthread-1.dll are all present in the installer"

echo "==> Done. Installer at target/x86_64-pc-windows-gnu/release/bundle/nsis/"
