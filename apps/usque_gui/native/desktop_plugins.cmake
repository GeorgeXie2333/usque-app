# shared_preferences_android brings in package:jni for Android. Usque's desktop
# preferences do not use Java. Keep the optional desktop JNI bridge out even
# when the build host has a JDK, without changing generated Flutter plugin files.
include_guard(GLOBAL)

function(usque_add_desktop_plugins generated_plugins)
  set(CMAKE_DISABLE_FIND_PACKAGE_JNI TRUE)
  # Shadow a JNI_FOUND value left by a previous configure with a host JDK.
  set(JNI_FOUND FALSE)
  include("${generated_plugins}")
  if(TARGET jni)
    message(FATAL_ERROR "The desktop plugin graph must not build a JNI runtime")
  endif()
  # Generated plugin rules run in this scope; preserve every other plugin's
  # libraries for the platform's normal install rules and package validation.
  set(PLUGIN_BUNDLED_LIBRARIES "${PLUGIN_BUNDLED_LIBRARIES}" PARENT_SCOPE)
endfunction()
