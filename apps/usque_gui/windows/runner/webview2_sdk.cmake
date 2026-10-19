# Headers and static loader only. The installed Evergreen runtime is discovered
# at runtime; this build never downloads or installs a browser runtime.
set(USQUE_WEBVIEW2_VERSION "1.0.4258.31")
set(USQUE_WEBVIEW2_SHA256 "56f7f4b8bf9aee4b8efefbbdd4f67d5f74ebd1b100ed0806da71bf76af481aa9")
set(USQUE_WEBVIEW2_PACKAGE "${CMAKE_BINARY_DIR}/webview2-${USQUE_WEBVIEW2_VERSION}.nupkg")
set(USQUE_WEBVIEW2_ROOT "${CMAKE_BINARY_DIR}/webview2-${USQUE_WEBVIEW2_VERSION}")
if(DEFINED ENV{USQUE_WEBVIEW2_NUPKG})
  set(USQUE_WEBVIEW2_PACKAGE "$ENV{USQUE_WEBVIEW2_NUPKG}")
endif()
if(NOT EXISTS "${USQUE_WEBVIEW2_PACKAGE}")
  file(DOWNLOAD
    "https://api.nuget.org/v3-flatcontainer/microsoft.web.webview2/${USQUE_WEBVIEW2_VERSION}/microsoft.web.webview2.${USQUE_WEBVIEW2_VERSION}.nupkg"
    "${USQUE_WEBVIEW2_PACKAGE}"
    EXPECTED_HASH "SHA256=${USQUE_WEBVIEW2_SHA256}"
    TLS_VERIFY ON TIMEOUT 120 STATUS USQUE_WEBVIEW2_DOWNLOAD)
  list(GET USQUE_WEBVIEW2_DOWNLOAD 0 USQUE_WEBVIEW2_STATUS)
  if(NOT USQUE_WEBVIEW2_STATUS EQUAL 0)
    message(FATAL_ERROR "The pinned WebView2 SDK could not be fetched.")
  endif()
endif()
file(SHA256 "${USQUE_WEBVIEW2_PACKAGE}" USQUE_WEBVIEW2_ACTUAL_SHA256)
if(NOT USQUE_WEBVIEW2_ACTUAL_SHA256 STREQUAL USQUE_WEBVIEW2_SHA256)
  message(FATAL_ERROR "WebView2 SDK package digest mismatch.")
endif()
# Re-extract verified bytes, so a stale cache cannot substitute a header/loader.
file(MAKE_DIRECTORY "${USQUE_WEBVIEW2_ROOT}")
execute_process(COMMAND "${CMAKE_COMMAND}" -E tar xf "${USQUE_WEBVIEW2_PACKAGE}"
  WORKING_DIRECTORY "${USQUE_WEBVIEW2_ROOT}" RESULT_VARIABLE USQUE_WEBVIEW2_EXTRACT)
if(NOT USQUE_WEBVIEW2_EXTRACT EQUAL 0)
  message(FATAL_ERROR "The pinned WebView2 SDK could not be extracted.")
endif()
if(FLUTTER_TARGET_PLATFORM STREQUAL "windows-arm64")
  set(USQUE_WEBVIEW2_ARCH "arm64")
elseif(FLUTTER_TARGET_PLATFORM STREQUAL "windows-x64")
  set(USQUE_WEBVIEW2_ARCH "x64")
else()
  message(FATAL_ERROR "Unsupported WebView2 target architecture.")
endif()
target_include_directories(${BINARY_NAME} SYSTEM PRIVATE "${USQUE_WEBVIEW2_ROOT}/build/native/include")
target_link_libraries(${BINARY_NAME} PRIVATE
  "${USQUE_WEBVIEW2_ROOT}/build/native/${USQUE_WEBVIEW2_ARCH}/WebView2LoaderStatic.lib"
  "bcrypt.lib" "version.lib")
install(FILES "${CMAKE_CURRENT_SOURCE_DIR}/WebView2-LICENSE.txt"
  DESTINATION "data/licenses" COMPONENT Runtime)
