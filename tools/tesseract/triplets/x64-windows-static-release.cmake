# Static libraries, static CRT, release only: one self-contained tesseract.exe
# with no DLLs and no Visual C++ redistributable needed on the target machine.
set(VCPKG_TARGET_ARCHITECTURE x64)
set(VCPKG_CRT_LINKAGE static)
set(VCPKG_LIBRARY_LINKAGE static)
set(VCPKG_BUILD_TYPE release)
