# Native Windows/MSVC Whisper build tuning.
# whisper-rs-sys includes this file before ggml configures its CPU backend.
# Keep this separate from whisper-windows-simd.cmake, which is a MinGW
# cross-compilation toolchain file and must not be used with Visual Studio.
set(GGML_AVX  ON CACHE BOOL "" FORCE)
set(GGML_AVX2 ON CACHE BOOL "" FORCE)
set(GGML_FMA  ON CACHE BOOL "" FORCE)
set(GGML_F16C ON CACHE BOOL "" FORCE)
