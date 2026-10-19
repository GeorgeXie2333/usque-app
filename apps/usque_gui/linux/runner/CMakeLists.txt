cmake_minimum_required(VERSION 3.13)
project(runner LANGUAGES CXX)

# Define the application target. To change its name, change BINARY_NAME in the
# top-level CMakeLists.txt, not the value here, or `flutter run` will no longer
# work.
#
# Any new source files that you add to the application should be added here.
add_executable(${BINARY_NAME}
  "main.cc"
  "my_application.cc"
  "engine_bridge.cc"
  "unix_engine_socket.cc"
  "desktop_shell.cc"
  "proton_verification.cc"
  "${FLUTTER_MANAGED_DIR}/generated_plugin_registrant.cc"
)

# Apply the standard set of build settings. This can be removed for applications
# that need different build settings.
apply_standard_settings(${BINARY_NAME})
target_compile_features(${BINARY_NAME} PRIVATE cxx_std_17)

# Add preprocessor definitions for the application ID.
add_definitions(-DAPPLICATION_ID="${APPLICATION_ID}")
string(REGEX REPLACE "[+].*$" "" USQUE_VERSION "${FLUTTER_VERSION}")
target_compile_definitions(${BINARY_NAME} PRIVATE USQUE_VERSION="${USQUE_VERSION}")
# Keep the runner buildable with Ubuntu 22.04's GLib, even on newer hosts.
target_compile_definitions(${BINARY_NAME} PRIVATE
  GLIB_VERSION_MIN_REQUIRED=GLIB_VERSION_2_72
  GLIB_VERSION_MAX_ALLOWED=GLIB_VERSION_2_72
)
if(USQUE_PREVIEW)
  target_compile_definitions(${BINARY_NAME} PRIVATE USQUE_PREVIEW=1)
endif()

# Add dependency libraries. Add any application-specific dependencies here.
target_link_libraries(${BINARY_NAME} PRIVATE flutter)
target_link_libraries(${BINARY_NAME} PRIVATE PkgConfig::GTK ${CMAKE_DL_LIBS})

target_include_directories(${BINARY_NAME} PRIVATE "${CMAKE_SOURCE_DIR}")

# Workstation-safe native IPC tests: sockets live only in a temporary directory.
add_executable(usque_linux_socket_test EXCLUDE_FROM_ALL
  "unix_engine_socket_test.cc" "unix_engine_socket.cc")
apply_standard_settings(usque_linux_socket_test)

add_executable(usque_proton_verification_test EXCLUDE_FROM_ALL
  "../../native/proton_verification_test.cc")
apply_standard_settings(usque_proton_verification_test)
target_compile_features(usque_proton_verification_test PRIVATE cxx_std_17)
