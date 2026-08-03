plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
    id("org.jlleitschuh.gradle.ktlint")
}

android {
    namespace = "io.github.georgexie2333.usque"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = "29.0.14206865"

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "io.github.georgexie2333.usque"
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        minSdk = 26
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

    signingConfigs {
        create("release") {
            val keystorePath = System.getenv("USQUE_ANDROID_KEYSTORE")
            if (keystorePath != null) {
                storeFile = file(keystorePath)
                storePassword = System.getenv("USQUE_ANDROID_STORE_PASSWORD")
                keyAlias = System.getenv("USQUE_ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("USQUE_ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.getByName("release")
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
            ndk {
                debugSymbolLevel = "NONE"
            }
        }
    }

    lint {
        warningsAsErrors = true
        checkReleaseBuilds = true
        abortOnError = true
    }
}

gradle.taskGraph.whenReady {
    // Fail-closed for release *builds*. Ignore incidental task names that
    // contain "Release" (for example ktlint's *ReleaseSourceSet* checks).
    val releaseRequested =
        allTasks.any { task ->
            val name = task.name
            name.contains("Release") && !name.contains("ktlint", ignoreCase = true)
        }
    if (releaseRequested) {
        val requiredVariables =
            listOf(
                "USQUE_ANDROID_KEYSTORE",
                "USQUE_ANDROID_STORE_PASSWORD",
                "USQUE_ANDROID_KEY_ALIAS",
                "USQUE_ANDROID_KEY_PASSWORD",
            )
        val missing = requiredVariables.filter { System.getenv(it).isNullOrBlank() }
        if (missing.isNotEmpty()) {
            throw GradleException(
                "Release signing is fail-closed; missing: ${missing.joinToString()}",
            )
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
        allWarningsAsErrors.set(true)
    }
}

ktlint {
    version.set("1.8.0")
    android.set(true)
}

flutter {
    source = "../.."
}

dependencies {
    implementation("androidx.activity:activity:1.8.1")
    testImplementation("junit:junit:4.13.2")
    // Flutter's Android plugin resolves this Kotlin metadata artifact late
    // while merging assets. Declare it explicitly so strict dependency
    // locking covers the same graph during both reports and real builds.
    runtimeOnly("org.jetbrains.kotlin:kotlin-stdlib-common:2.3.20")
}

dependencyLocking {
    lockAllConfigurations()
    // A single-ABI validation build intentionally does not resolve Flutter's
    // other engine artifacts. Keep normal/all-ABI CI strict, while permitting
    // unused locked ABI entries for an explicitly filtered local build.
    val filteredAbi = System.getenv("USQUE_ANDROID_ABI")
    lockMode.set(
        if (filteredAbi.isNullOrBlank() || filteredAbi == "all") {
            org.gradle.api.artifacts.dsl.LockMode.STRICT
        } else {
            org.gradle.api.artifacts.dsl.LockMode.LENIENT
        },
    )
}

val rustBuildScript = rootProject.file("../../../tool/build_android_rust.ps1")
val rustAbiFilter = providers.environmentVariable("USQUE_ANDROID_ABI").orElse("all")
val powerShellExecutable =
    if (System.getProperty("os.name").startsWith("Windows", ignoreCase = true)) {
        "powershell.exe"
    } else {
        "pwsh"
    }

fun registerRustAndroidBuild(
    name: String,
    profile: String,
) = tasks.register<Exec>(name) {
    group = "build"
    description = "Builds the Rust JNI library for every supported Android ABI."
    workingDir(rootProject.file("../../.."))
    commandLine(
        powerShellExecutable,
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        rustBuildScript.absolutePath,
        "-Profile",
        profile,
        "-AbiFilter",
        rustAbiFilter.get(),
    )
    inputs.files(
        rustBuildScript,
        rootProject.file("../../../Cargo.toml"),
        rootProject.file("../../../Cargo.lock"),
        rootProject.file("../../../rust-toolchain.toml"),
        rootProject.fileTree("../../../crates") {
            include("**/*.rs", "**/Cargo.toml")
        },
        rootProject.fileTree("../../../third_party/boring-sys-4.22.0"),
        rootProject.fileTree("../../../third_party/ts_netstack_smoltcp_core"),
    )
    outputs.dir(file("src/main/jniLibs"))
    inputs.property("rustAbiFilter", rustAbiFilter)
}

val buildRustAndroidDebug = registerRustAndroidBuild("buildRustAndroidDebug", "debug")
val buildRustAndroidRelease = registerRustAndroidBuild("buildRustAndroidRelease", "release")

tasks.matching { it.name == "preDebugBuild" }.configureEach {
    dependsOn(buildRustAndroidDebug)
}
tasks.matching { it.name == "preProfileBuild" }.configureEach {
    dependsOn(buildRustAndroidRelease)
}
tasks.matching { it.name == "preReleaseBuild" }.configureEach {
    dependsOn(buildRustAndroidRelease)
}
