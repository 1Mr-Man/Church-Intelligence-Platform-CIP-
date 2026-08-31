# Phase 4.2 — Live Bible Detection Performance & Search Filter

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `a464016` (Phase 4.1, semantic paraphrase Bible detection)

## Why this phase exists

The operator installed the Phase 4.1 build on real Windows hardware and
reported that live detection produced nothing at all across every
domain except Sermon Intelligence (which found 3 structural
transitions from timing/pacing signals alone, not transcript content).
The System Diagnostics panel in the operator's own screenshots showed
the actual cause was starvation, not a detection-logic defect: average
Whisper inference 14,518ms per ~3s audio chunk (worse than 4x
real-time), max 19,023ms, 51 overload events, and 742,044ms of audio
discarded in the session. Cross-referencing these numbers against
`docs/phase-3-8-7-7-audit.md` (avg inference 14,991ms on an
apparently similar machine, whose Environment C confirmation gate was
explicitly left open) confirmed this as a known, previously-flagged,
still-unresolved hardware/build-configuration limitation - not a
Phase 4.1 regression.

Given a choice between routing the operator to Service Replay (offline
transcript path, unaffected by inference latency) or fixing live
performance directly, the operator was explicit: real-time Bible
detection must work "same as music no larging" (no lag), and Bible
search needs a book-name prefix filter (type "A" -> suggests Acts,
Amos, ...) with chapter/verse listing.

## Audit — root cause of the Whisper slowness

Read directly from the vendored `whisper-rs-sys` 0.13.1 crate's build
system (crates.io registry cache, not guessed):

- `whisper.cpp/ggml/CMakeLists.txt` sets `GGML_NATIVE_DEFAULT=OFF`
  whenever `CMAKE_CROSSCOMPILING` is `TRUE` - which is always true for
  this project's Windows builds (cross-compiled from Linux via
  `x86_64-pc-windows-gnu`). That cascades to `INS_ENB=OFF`, which
  defaults `GGML_AVX`/`GGML_AVX2`/`GGML_FMA`/`GGML_F16C` all `OFF`.
- `ggml-cpu/CMakeLists.txt`'s non-MSVC x86 branch confirms the
  consequence: `-msse4.2` is the only unconditional flag; AVX/AVX2/
  FMA/F16C are each gated behind their own `option()`, defaulted off
  above.

Every Windows build this project has ever shipped has therefore only
ever used the SSE4.2 baseline for Whisper inference - never AVX2, FMA,
or F16C, a well-known multi-times slowdown for this kind of
neural-net-inference workload. This is a build-configuration gap, not
a defect in whisper.cpp or the model itself.

## Fix, attempt 1: CMAKE_TOOLCHAIN_FILE (led to a second, real bug)

`whisper-rs-sys`'s `build.rs` only forwards environment variables
prefixed `WHISPER_` or `CMAKE_` as literal `-D<name>=<value>` CMake
defines (confirmed via direct source read). `GGML_AVX2=ON` as a plain
env var is invisible to it. `CMAKE_TOOLCHAIN_FILE` is the one
`CMAKE_`-prefixed variable that both matches that forwarding rule and
is `include()`d early enough (before `project()`) to pre-seed the
cache ahead of GGML's own `option()` calls, which only set a value
when the cache doesn't already have one - so a new file,
`scripts/whisper-windows-simd.cmake`, force-sets
`GGML_AVX`/`GGML_AVX2`/`GGML_FMA`/`GGML_F16C` to `ON` via
`set(... CACHE BOOL "" FORCE)`, and `scripts/build-windows-whisper.sh`
was extended to export `CMAKE_TOOLCHAIN_FILE` and `cargo clean -p
whisper-rs-sys --target x86_64-pc-windows-gnu --release` first (the
crate declares no `rerun-if-env-changed` for this variable, so a
cached build would otherwise never notice the new toolchain file).

The first real rebuild attempt with this file surfaced a second,
previously undiscovered defect: the link step failed with pages of
`undefined reference to whisper_free`/`ggml_cpu_has_avx2`/etc, a
different failure signature than the already-known "ggml lib-name"
defect (that one fails with "could not find native static library
`ggml`" - the `-l` flag present but unresolvable; this one had no
`-l` flag for whisper/ggml at all). Direct inspection (`cargo build
-vv`, and reading whisper-rs-sys's own cached build-script output)
confirmed the `.a` files were built, correctly named, in the right
`-L` search paths, and `build.rs` genuinely printed
`cargo:rustc-link-lib=static=whisper`/`ggml`/`ggml-base`/`ggml-cpu` -
yet none of those four reached the final linker invocation for
`cip_desktop_lib`. Bisected by A/B-testing a from-clean rebuild with
`CMAKE_TOOLCHAIN_FILE` unset, which reproduced the classic, expected
"could not find `ggml`" failure instead - proving the toolchain file's
mere presence was the new variable.

Root cause, read directly from the `cmake` crate (`cmake-0.1.58`)
source: `Config::build()` only auto-sets
`CMAKE_SYSTEM_NAME`/`CMAKE_SYSTEM_PROCESSOR` (what makes CMake set
`CMAKE_CROSSCOMPILING=TRUE` in the first place) when
`CMAKE_TOOLCHAIN_FILE` is *not already defined* - and a few hundred
lines later, it applies the identical guard to skip passing
`-DCMAKE_C_COMPILER=<path>`/`-DCMAKE_CXX_COMPILER=<path>` too, on the
(otherwise reasonable) assumption that a user-supplied toolchain file
names its own compiler. Since `build.rs` defines
`CMAKE_TOOLCHAIN_FILE` (from the forwarded env var) before calling
`config.build()`, and the new toolchain file set neither
`CMAKE_SYSTEM_NAME` nor a compiler, both of `cmake-rs`'s own
auto-detections were silently disabled: CMake configured as a
same-arch native build (not cross-compiling) using whatever compiler
it found on `PATH` (`/usr/bin/cc`, the host's own GCC) instead of the
mingw-w64 cross compiler - which is why the resulting objects/link
metadata didn't behave like a genuine Windows cross build.

## Fix, complete

`scripts/whisper-windows-simd.cmake` now also sets, restoring exactly
what `cmake-rs` would otherwise have set on this project's behalf:

```cmake
set(CMAKE_SYSTEM_NAME      Windows CACHE STRING "" FORCE)
set(CMAKE_SYSTEM_PROCESSOR AMD64   CACHE STRING "" FORCE)
find_program(CMAKE_C_COMPILER   NAMES x86_64-w64-mingw32-gcc REQUIRED)
find_program(CMAKE_CXX_COMPILER NAMES x86_64-w64-mingw32-g++ REQUIRED)
```

With this in place, a from-clean rebuild reproduces exactly the
already-understood, already-handled "ggml lib-name" defect (CMake's
Windows-target build strips the `lib` prefix from `ggml.a`/
`ggml-base.a`/`ggml-cpu.a`, which MinGW's linker needs) - the existing
two-pass retry logic in `scripts/build-windows-whisper.sh`
(attempt, copy correctly-named files, retry) handles it automatically,
unchanged. The retry succeeds cleanly.

`ai/speech/src/whisper.rs`'s `run_inference()` was also given an
explicit `set_n_threads()` call using
`std::thread::available_parallelism()` (capped at 8) - whisper.cpp's
own default, used previously, is `min(4, hardware_concurrency())`,
leaving real parallelism on the table on any machine with more than 4
logical cores. This worker thread (Phase 3.8.7.2) is the only thing
running inference, so using every available core is safe.

### Direct binary evidence (not inferred from source)

After a from-clean rebuild with the complete fix:

- `x86_64-w64-mingw32-nm -D` against the built
  `cip_desktop_lib.dll` finds `ggml_cpu_has_avx`, `ggml_cpu_has_avx2`,
  `ggml_cpu_has_fma`, `ggml_cpu_has_f16c` all present and resolved
  (previously these were "undefined reference" link failures; now
  they compile and link cleanly).
- `x86_64-w64-mingw32-objdump -d` against the same DLL finds genuine
  AVX/AVX2 YMM-register instructions in the compiled code (`vmovdqa
  %ymm0,(%rax)`, `vpbroadcastb %xmm0,%ymm0`, `vmovups %ymm0,...`,
  etc.) - direct proof the SIMD paths are compiled in, not just that
  the CMake option was set.

### Safety note on the SIMD baseline

AVX2+FMA+F16C (the "Haswell" baseline, 2013+) is standard on
essentially every x86-64 Windows PC sold in over a decade, and is the
same baseline whisper.cpp's own official prebuilt Windows binaries
target. AVX512 and newer extensions are deliberately left off -
narrower hardware support, unlikely on the modest hardware this
project is built for. If a future report shows an "illegal
instruction" crash instead of slowness, that means the operator's real
CPU predates 2013; the fix at that point is to remove
`scripts/whisper-windows-simd.cmake`'s forced `GGML_*` values (falling
back to the always-safe SSE4.2 baseline), not to guess at a narrower
target.

## Bible search: book-name prefix filter + chapter/verse listing

New pure helper `filterBooksByPrefix` in
`apps/desktop/src/lib/libraryHelpers.ts`: filters the already-loaded,
provider-validated `BibleBook[]` list (from the existing
`listBibleBooks` command, Phase 3.6) to books whose display name
starts with the operator's typed prefix, case-insensitively and
whitespace-trimmed - typing "a" narrows the Browse grid to "Acts",
"Amos", etc. Deliberately name-prefix matching only, not
abbreviation/alias matching: most natural abbreviations (e.g. "Rom")
are already name prefixes, and one simple, predictable rule is easier
for an operator to reason about while typing than a second, fuzzier
match mode.

`BibleLibrary.tsx`'s Browse tab gained a filter `<input>` above the
book grid, wired to this helper; selecting a book (or using the "back
to all books" control) clears the filter. Chapter and verse listing
were already fully built in Phase 3.6 (book -> chapter grid -> verse
list, and `search_bible` already parses `"ROM 8"` chapter queries and
`"ROM 8:28"` verse queries) - confirmed by reading `BibleLibrary.tsx`
and the `search_bible`/`list_bible_books` Tauri commands directly, so
no backend change was needed for that half of the request.

Deliberately client-side only: the book list is already loaded,
provider-validated, canonical (`core/bible::book_alias::BOOKS`) state;
filtering it in the browser avoids a second book-identity source and
an unnecessary IPC round-trip for what is, by nature, an
instant-as-you-type interaction.

## Full regression result

- `cargo fmt --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings` (default
  features): clean.
- `cargo clippy -p cip-ai-speech --features whisper --all-targets -- -D
  warnings`: clean (unchanged from prior phases; only
  `run_inference()`'s thread-count logic changed).
- `cargo test -p cip-ai-speech --features whisper`: 7/7 passed
  (unchanged).
- Frontend: `tsc --noEmit` clean, `oxlint` clean (4 pre-existing
  warnings, none new/related), `vitest run` 218/218 passed (6 new for
  `filterBooksByPrefix`), production `vite build` clean.

## Windows artifact

- `scripts/build-windows-whisper.sh` run end-to-end with the complete
  fix: first attempt correctly reproduces the expected ggml lib-name
  defect, the script's existing retry logic fixes it automatically,
  second attempt succeeds, `tauri build` packages the NSIS installer,
  installer DLL-presence verification passes.
- See `pilot-evidence/4.2/` for the installer checksum, size, and the
  direct `nm`/`objdump` SIMD-instruction evidence captured this phase.

## Architectural safety diff

```
FILES MODIFIED: ai/speech/src/whisper.rs, scripts/build-windows-whisper.sh,
  apps/desktop/src/lib/libraryHelpers.ts,
  apps/desktop/src/components/library/BibleLibrary.tsx,
  apps/desktop/src/components/library/library.css,
  release/windows/*
FILES CREATED: scripts/whisper-windows-simd.cmake,
  docs/phase-4-2-live-bible-detection-performance.md,
  pilot-evidence/4.2/*,
  apps/desktop/src/lib/libraryHelpers.test.ts (extended)
FILES DELETED: NONE
RUST SOURCE CHANGED: ai/speech/src/whisper.rs (explicit thread count) -
  detection logic, buffering, resampling, database schema, event
  contracts all UNCHANGED
BUILD TOOLING CHANGED: new CMake toolchain file forces AVX/AVX2/FMA/
  F16C and restores cmake-rs's own cross-compile auto-detection
  (CMAKE_SYSTEM_NAME/PROCESSOR/C_COMPILER/CXX_COMPILER) that its
  presence would otherwise have suppressed
FRONTEND CHANGED: Bible Library Browse tab only - new client-side
  filter input, no new Tauri commands, no IPC contract change
TAURI COMMANDS ADDED/REMOVED/RENAMED: NONE
EVENT CONTRACTS CHANGED: NONE
SPEECHENGINE / AUDIOENGINE TRAITS: UNCHANGED
DATABASE / MIGRATIONS: UNCHANGED
NETWORK CAPABILITIES: NONE ADDED
OFFLINE ARCHITECTURE: preserved
```

## Environment A / B / C

- **Environment A (automated)**: full pass, including direct
  compiled-binary instruction-level evidence for the SIMD fix.
- **Environment B (Xvfb)**: not re-run this phase (no UI-rendering
  code path changed beyond a plain `<input>`, already covered by the
  frontend test suite).
- **Environment C (real Windows hardware)**: **NOT YET VERIFIED for
  this exact artifact.** The decisive pending gate is the operator's
  own re-test: does live Whisper inference now complete meaningfully
  faster than the previous ~14.5s/chunk average (the System
  Diagnostics panel reports this directly), does live Bible detection
  now produce results without the prior starvation, and does the new
  book-name filter behave as expected while typing.

## Known limitations

- This fix reduces per-chunk inference latency; it does not guarantee
  real-time (sub-3-second) transcription on every machine - CPU
  generation, background load, and model size all still matter. If a
  future report shows continued severe lag with AVX2 confirmed present
  (diagnostics panel), the next lever is a smaller/faster model, not
  another SIMD change.
- The book-name filter matches display-name prefixes only, not
  abbreviations that aren't also prefixes (e.g. "Jn" for "John") - a
  possible future refinement, not requested this phase.
- The real-Windows re-test has not yet occurred.

## Deferred work

The operator's own real-Windows re-test (inference latency, live Bible
detection, and the new search filter). Real audio fingerprinting
(Phase 4.3, the next Phase 4 gap-audit item the operator selected) is
tracked separately and not started in this phase.

## Final gate

| Item | Status |
|---|---|
| Diagnosed "nothing detected" against real evidence, ruled out Phase 4.1 regression | DONE |
| Root-caused Whisper's missing SIMD flags from vendored build source | DONE |
| Found and fixed the CMAKE_TOOLCHAIN_FILE side effect (cross-compile detection suppression) via direct A/B evidence, not guesswork | DONE |
| SIMD instructions verified present in the compiled binary (not just source-level) | DONE |
| Explicit thread-count tuning applied | DONE |
| Bible search book-name prefix filter implemented, client-side, zero new backend surface | DONE |
| Full regression green (backend + frontend) | DONE |
| Windows artifact rebuilt end-to-end via the existing script, unchanged retry logic still works | DONE |
| Real Windows re-test (Environment C) | **NOT YET PERFORMED** - pending the operator |

**Phase 4.2: Environment A verification PASS, including direct
compiled-binary evidence for the SIMD fix. Real Windows re-test
(Environment C) is the pending, decisive gate on whether inference
latency genuinely improved enough for live detection to work as the
operator described ("same as music, no lagging").**
