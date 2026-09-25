# Overlay of vcpkg's tesseract port: curl and libarchive removed so the shipped
# binary links no LGPL library and has no network code. See tools/tesseract/README.md.
vcpkg_from_github(
    OUT_SOURCE_PATH SOURCE_PATH
    REPO tesseract-ocr/tesseract
    REF "${VERSION}"
    SHA512 e9103c68ba186821aedd38de4d9949cd6732da93a2d0764de18aaaac70eb9c305384a6eb1fe656a8a269bee833178a583a91dd72027ae26d27c8329ed722f4a9
    PATCHES
        fix_static_link_icu.patch
        fix-link-include-path.patch
        target-curl.diff
)

vcpkg_find_acquire_program(PKGCONFIG)

vcpkg_cmake_configure(
    SOURCE_PATH "${SOURCE_PATH}"
    OPTIONS
        -DBUILD_TRAINING_TOOLS=OFF
        -DUSE_SYSTEM_ICU=True
        -DDISABLE_CURL=ON
        -DDISABLE_ARCHIVE=ON
        -DGRAPHICS_DISABLED=ON
        -DCMAKE_DISABLE_FIND_PACKAGE_CURL=ON
        -DCMAKE_DISABLE_FIND_PACKAGE_LibArchive=ON
        -DCMAKE_REQUIRE_FIND_PACKAGE_Leptonica=ON
        -DCMAKE_DISABLE_FIND_PACKAGE_OpenCL=ON
        -DLeptonica_DIR=YES
        -DSW_BUILD=OFF
        -DLEPT_TIFF_RESULT=ON
        "-DPKG_CONFIG_EXECUTABLE=${PKGCONFIG}"
    MAYBE_UNUSED_VARIABLES
        CMAKE_DISABLE_FIND_PACKAGE_OpenCL
        CMAKE_DISABLE_FIND_PACKAGE_CURL
        CMAKE_DISABLE_FIND_PACKAGE_LibArchive
)

vcpkg_cmake_install()
vcpkg_copy_pdbs()

vcpkg_cmake_config_fixup(CONFIG_PATH lib/cmake/tesseract)

vcpkg_replace_string("${CURRENT_PACKAGES_DIR}/share/tesseract/TesseractConfig.cmake"
    "find_dependency(Leptonica)"
[[
find_dependency(Leptonica)
if(ANDROID)
    find_dependency(CpuFeaturesNdkCompat CONFIG)
endif()
]]
)

vcpkg_copy_tools(TOOL_NAMES tesseract AUTO_CLEAN)
vcpkg_fixup_pkgconfig()



file(REMOVE_RECURSE "${CURRENT_PACKAGES_DIR}/debug/include")
file(REMOVE_RECURSE "${CURRENT_PACKAGES_DIR}/debug/share")

# Install:
file(GLOB API_HEADER_FILES LIST_DIRECTORIES false "${SOURCE_PATH}/src/api/*.h")
file(INSTALL ${API_HEADER_FILES} DESTINATION "${CURRENT_PACKAGES_DIR}/include/tesseract/api")

file(GLOB ARCH_HEADER_FILES LIST_DIRECTORIES false "${SOURCE_PATH}/src/arch/*.h")
file(INSTALL ${ARCH_HEADER_FILES} DESTINATION "${CURRENT_PACKAGES_DIR}/include/tesseract/arch")

file(GLOB CCMAIN_HEADER_FILES LIST_DIRECTORIES false "${SOURCE_PATH}/src/ccmain/*.h")
file(INSTALL ${CCMAIN_HEADER_FILES} DESTINATION "${CURRENT_PACKAGES_DIR}/include/tesseract/ccmain")

file(GLOB CCSTRUCT_HEADER_FILES LIST_DIRECTORIES false "${SOURCE_PATH}/src/ccstruct/*.h")
file(INSTALL ${CCSTRUCT_HEADER_FILES} DESTINATION "${CURRENT_PACKAGES_DIR}/include/tesseract/ccstruct")

file(GLOB CCUTIL_HEADER_FILES LIST_DIRECTORIES false "${SOURCE_PATH}/src/ccutil/*.h")
file(INSTALL ${CCUTIL_HEADER_FILES} DESTINATION "${CURRENT_PACKAGES_DIR}/include/tesseract/ccutil")

file(GLOB CLASSIFY_HEADER_FILES LIST_DIRECTORIES false "${SOURCE_PATH}/src/classify/*.h")
file(INSTALL ${CLASSIFY_HEADER_FILES} DESTINATION "${CURRENT_PACKAGES_DIR}/include/tesseract/classify")

file(GLOB CUTIL_HEADER_FILES LIST_DIRECTORIES false "${SOURCE_PATH}/src/cutil/*.h")
file(INSTALL ${CUTIL_HEADER_FILES} DESTINATION "${CURRENT_PACKAGES_DIR}/include/tesseract/cutil")

file(GLOB DICT_HEADER_FILES LIST_DIRECTORIES false "${SOURCE_PATH}/src/dict/*.h")
file(INSTALL ${DICT_HEADER_FILES} DESTINATION "${CURRENT_PACKAGES_DIR}/include/tesseract/dict")

file(GLOB LSTM_HEADER_FILES LIST_DIRECTORIES false "${SOURCE_PATH}/src/lstm/*.h")
file(INSTALL ${LSTM_HEADER_FILES} DESTINATION "${CURRENT_PACKAGES_DIR}/include/tesseract/lstm")

file(GLOB TEXTORD_HEADER_FILES LIST_DIRECTORIES false "${SOURCE_PATH}/src/textord/*.h")
file(INSTALL ${TEXTORD_HEADER_FILES} DESTINATION "${CURRENT_PACKAGES_DIR}/include/tesseract/textord")

file(GLOB VIEWER_HEADER_FILES LIST_DIRECTORIES false "${SOURCE_PATH}/src/viewer/*.h")
file(INSTALL ${VIEWER_HEADER_FILES} DESTINATION "${CURRENT_PACKAGES_DIR}/include/tesseract/viewer")

file(GLOB WORDREC_HEADER_FILES LIST_DIRECTORIES false "${SOURCE_PATH}/src/wordrec/*.h")
file(INSTALL ${WORDREC_HEADER_FILES} DESTINATION "${CURRENT_PACKAGES_DIR}/include/tesseract/wordrec")

# Handle copyright
vcpkg_install_copyright(FILE_LIST "${SOURCE_PATH}/LICENSE")
