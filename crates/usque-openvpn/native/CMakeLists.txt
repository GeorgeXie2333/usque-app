cmake_minimum_required(VERSION 3.22)
project(usque_openvpn LANGUAGES C CXX)
if(MSVC)
    # build.rs pins VSLANG; override a prefix cached by an older localized
    # configure so Ninja cannot silently miss header changes.
    set(CMAKE_CL_SHOWINCLUDES_PREFIX "Note: including file: ")
    set(CMAKE_C_CL_SHOWINCLUDES_PREFIX "Note: including file: ")
    set(CMAKE_CXX_CL_SHOWINCLUDES_PREFIX "Note: including file: ")
endif()
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
set(CMAKE_C_VISIBILITY_PRESET hidden)
set(CMAKE_CXX_VISIBILITY_PRESET hidden)
set(CMAKE_VISIBILITY_INLINES_HIDDEN ON)
get_filename_component(VENDOR "${CMAKE_CURRENT_SOURCE_DIR}/../../../third_party" ABSOLUTE)
set(CORE "${VENDOR}/openvpn3-3.11.7")

set(ENABLE_PROGRAMS OFF CACHE BOOL "" FORCE)
set(ENABLE_TESTING OFF CACHE BOOL "" FORCE)
set(GEN_FILES OFF CACHE BOOL "" FORCE)
set(USE_STATIC_MBEDTLS_LIBRARY ON CACHE BOOL "" FORCE)
set(USE_SHARED_MBEDTLS_LIBRARY OFF CACHE BOOL "" FORCE)
set(DISABLE_PACKAGE_CONFIG_AND_INSTALL ON CACHE BOOL "" FORCE)
add_subdirectory("${VENDOR}/mbedtls-3.6.7" mbedtls)

# Only the protocol client is compiled. All OS TUN and socket implementations
# are replaced by the external factories in bridge.cpp; no DCO or driver code.
add_library(usque_openvpn STATIC
    bridge.cpp
    "${CORE}/client/ovpncli.cpp"
    "${CORE}/openvpn/crypto/data_epoch.cpp"
    "${VENDOR}/lz4-1.10.0/lib/lz4.c"
    "${VENDOR}/xxhash-0.8.3/xxhash.c")
target_include_directories(usque_openvpn PRIVATE
    "${CMAKE_CURRENT_SOURCE_DIR}/compat" "${CORE}" "${VENDOR}/asio-1.30.2/asio/include"
    "${VENDOR}/lz4-1.10.0/lib" "${VENDOR}/xxhash-0.8.3")
target_compile_definitions(usque_openvpn PRIVATE
    ASIO_STANDALONE USE_ASIO USE_MBEDTLS HAVE_LZ4
    OPENVPN_EXTERNAL_TUN_FACTORY OPENVPN_EXTERNAL_TRANSPORT_FACTORY
    OPENVPN_CORE_API_VISIBILITY_HIDDEN)
target_link_libraries(usque_openvpn PRIVATE mbedtls mbedx509 mbedcrypto)
# Forced includes are not reliably reported by every MSVC showIncludes build.
file(GLOB_RECURSE CORE_HEADERS CONFIGURE_DEPENDS
    "${CORE}/*.hpp" "${CORE}/*.h" "${CMAKE_CURRENT_SOURCE_DIR}/compat/*.hpp")
set_source_files_properties(bridge.cpp "${CORE}/client/ovpncli.cpp"
    "${CORE}/openvpn/crypto/data_epoch.cpp" test_peer.cpp
    PROPERTIES OBJECT_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/privacy_log.h;${CORE_HEADERS}")
option(USQUE_INTEROP_TEST "Build the memory-only protocol test peer" OFF)
if(USQUE_INTEROP_TEST)
    target_sources(usque_openvpn PRIVATE test_peer.cpp)
    target_compile_definitions(usque_openvpn PRIVATE USQUE_INTEROP_TEST)
endif()
if(WIN32)
    target_compile_definitions(usque_openvpn PRIVATE _WIN32_WINNT=0x0A00
        _CRT_SECURE_NO_WARNINGS ASIO_DISABLE_LOCAL_SOCKETS)
    target_link_libraries(usque_openvpn PRIVATE ws2_32 bcrypt crypt32 advapi32 iphlpapi)
endif()
if(MSVC)
    target_compile_options(usque_openvpn PRIVATE /bigobj /EHsc /Zc:preprocessor)
    target_compile_options(usque_openvpn PRIVATE "$<$<COMPILE_LANGUAGE:CXX>:/FI${CMAKE_CURRENT_SOURCE_DIR}/privacy_log.h>")
else()
    target_compile_options(usque_openvpn PRIVATE "$<$<COMPILE_LANGUAGE:CXX>:-include${CMAKE_CURRENT_SOURCE_DIR}/privacy_log.h>")
endif()
install(TARGETS usque_openvpn ARCHIVE DESTINATION lib)
