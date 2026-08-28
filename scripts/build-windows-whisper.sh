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
# Usage: scripts/build-windows-whisper.sh
# Must be run with permission to call `update-alternatives` (root, in this
# container) and from the repository root.

set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> Switching x86_64-w64-mingw32-gcc/g++ to the posix-threads variant"
update-alternatives --set x86_64-w64-mingw32-gcc /usr/bin/x86_64-w64-mingw32-gcc-posix
update-alternatives --set x86_64-w64-mingw32-g++ /usr/bin/x86_64-w64-mingw32-g++-posix

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

echo "==> Done. Installer at target/x86_64-pc-windows-gnu/release/bundle/nsis/"
