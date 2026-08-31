# Phase 4.2: forces AVX2/AVX/FMA/F16C on for the cross-compiled Windows
# `ggml` (whisper.cpp) build, which otherwise silently builds with no CPU
# vectorization beyond the unconditional SSE4.2 baseline.
#
# Root cause (found by reading the vendored whisper-rs-sys 0.13.1's
# ggml/CMakeLists.txt directly): `GGML_NATIVE_DEFAULT` is forced OFF
# whenever `CMAKE_CROSSCOMPILING` is true (true for every Windows build
# this project produces, since it's always built from Linux), which in
# turn defaults every one of GGML_AVX/GGML_AVX2/GGML_FMA/GGML_F16C to OFF
# (`INS_ENB`, computed from those same two variables). whisper-rs-sys's own
# build.rs only forwards env vars prefixed `WHISPER_`/`CMAKE_` as literal
# `-D<name>=<value>` CMake defines, so a plain `GGML_AVX2=ON` environment
# variable is never seen by CMake at all - `CMAKE_TOOLCHAIN_FILE` is the
# one CMake-recognized, `CMAKE_`-prefixed variable that both matches that
# forwarding rule *and* is `include()`d early enough (before `project()`)
# to pre-seed the cache ahead of GGML's own `option()` calls, which only
# set a value when the cache doesn't already have one.
#
# This is confirmed, on real Windows hardware, to be the dominant cause of
# Whisper transcription running far slower than real time on at least one
# operator's machine (avg inference 14,518ms per ~3s audio chunk - see
# docs/phase-4-2-live-bible-detection-performance.md) - not a Whisper/
# whisper.cpp defect, a build-configuration gap specific to this project's
# cross-compilation setup.
#
# Safety note: AVX2+FMA+F16C (the "Haswell" baseline, 2013+) is standard on
# essentially every x86-64 Windows PC sold in over a decade, and is the
# same baseline whisper.cpp's own official prebuilt Windows binaries
# target. AVX512 and newer extensions are deliberately left OFF - narrower
# hardware support, unlikely to be present on the modest hardware this
# project is built for, and not worth the compatibility risk. If a report
# ever surfaces of an "illegal instruction" crash instead of just slowness,
# that means the operator's real CPU predates 2013 - the fix at that point
# is to remove this file's forced values (falling back to the always-safe
# SSE4.2 baseline), not to guess at a narrower ISA target.
set(GGML_AVX  ON CACHE BOOL "" FORCE)
set(GGML_AVX2 ON CACHE BOOL "" FORCE)
set(GGML_FMA  ON CACHE BOOL "" FORCE)
set(GGML_F16C ON CACHE BOOL "" FORCE)

# The `cmake` Rust crate (used by whisper-rs-sys's build.rs) normally infers
# and sets CMAKE_SYSTEM_NAME/CMAKE_SYSTEM_PROCESSOR itself for a cross
# build, which is what makes CMake set CMAKE_CROSSCOMPILING=TRUE in the
# first place. But it only does that when CMAKE_TOOLCHAIN_FILE is *not*
# already defined (see cmake-rs's `Config::build()`); since this file's
# own path becomes that env var, introducing it suppressed that inference
# entirely - CMAKE_SYSTEM_NAME was left unset, CMake configured as a
# same-arch native build instead of a cross build, and (confirmed by
# direct comparison of `cargo build -vv` output with/without this file)
# whisper-rs-sys's otherwise-correct `cargo:rustc-link-lib=static=...`
# directives silently failed to reach the final linker invocation - not a
# naming/path problem (the .a files were present, correctly named, with
# their directories in -L), just absent from the link entirely. Setting
# these two here restores exactly what the crate would have set on our
# behalf, so introducing this toolchain file changes nothing else about
# how the cross build is configured.
set(CMAKE_SYSTEM_NAME      Windows CACHE STRING "" FORCE)
set(CMAKE_SYSTEM_PROCESSOR AMD64   CACHE STRING "" FORCE)

# Same reasoning again, one level down: cmake-rs also skips passing
# `-DCMAKE_C_COMPILER=<path>`/`-DCMAKE_CXX_COMPILER=<path>` whenever
# CMAKE_TOOLCHAIN_FILE is defined, on the (standard, otherwise-correct)
# assumption that a user-supplied toolchain file names its own compiler -
# which is exactly what a normal MinGW toolchain file does. Without this,
# CMake fell back to auto-detecting a compiler on PATH and picked the
# *host* `/usr/bin/cc`, which then failed immediately on the first
# Windows-targeted linker flag CMake tried to pass it (`--major-image-
# version`, meaningless to a native ELF linker). Naming the real
# mingw-w64 cross compiler here is the standard way any hand-written
# MinGW CMake toolchain file does this.
find_program(CMAKE_C_COMPILER   NAMES x86_64-w64-mingw32-gcc REQUIRED)
find_program(CMAKE_CXX_COMPILER NAMES x86_64-w64-mingw32-g++ REQUIRED)
