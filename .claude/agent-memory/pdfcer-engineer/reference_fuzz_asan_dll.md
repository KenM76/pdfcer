---
name: fuzz-asan-dll
description: Windows cargo-fuzz needs the MSVC clang_rt ASan DLL on PATH or the fuzz binary dies with STATUS_DLL_NOT_FOUND (0xc0000135)
metadata:
  type: reference
---

Running `cargo +nightly fuzz run <target>` on this machine (Windows-MSVC)
fails at launch with exit code `0xc0000135` (STATUS_DLL_NOT_FOUND) unless
the MSVC clang_rt AddressSanitizer runtime DLL is on `PATH`. The build
succeeds; only the *run* fails.

Fix (prepend to PATH for the fuzz invocation):
`C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64`
(contains `clang_rt.asan_dynamic-x86_64.dll`). The MSVC toolchain version
folder (`14.44.35207`) may bump — locate with
`find "/c/Program Files (x86)/Microsoft Visual Studio" -iname "clang_rt.asan_dynamic-x86_64.dll"`.

Confirmed 2026-07-31 running Pass 6.1's `annot_author` fuzz target:
696,098 runs / 61 s / 0 crashes once the DLL was on PATH. The rustup
sysroot (stable *and* nightly) does **not** ship this DLL — it comes from
the Visual Studio Build Tools install only.

**How to apply:** whenever running any `cargo fuzz run` on this machine,
export that dir onto PATH first. Belongs in `D:/dev/rag/rust` as an
ecosystem finding on the next librarian pass (this session was directed
not to dispatch librarians).
