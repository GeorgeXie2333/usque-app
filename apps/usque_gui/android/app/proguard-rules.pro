# JNI entry points are resolved by exact Java class and method names.
-keep class io.github.georgexie2333.usque.NativeEngine {
    *;
}
-keepclasseswithmembernames class * {
    native <methods>;
}

# Android instantiates these components from the manifest. Keep the service's
# @Keep callbacks as their names are also resolved by Rust through JNI.
-keep class io.github.georgexie2333.usque.MainActivity { *; }
-keep class io.github.georgexie2333.usque.UsqueVpnService { *; }
-keep @androidx.annotation.Keep class * { *; }
-keepclassmembers class * {
    @androidx.annotation.Keep <methods>;
}
