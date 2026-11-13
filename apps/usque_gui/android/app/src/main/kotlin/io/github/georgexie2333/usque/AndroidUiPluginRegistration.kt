package io.github.georgexie2333.usque

import com.github.dart_lang.jni.JniPlugin
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.plugins.sharedpreferences.SharedPreferencesPlugin
import io.flutter.plugins.urllauncher.UrlLauncherPlugin

/**
 * UsqueFlutterFragment disables automatic registration and delegates it to
 * MainActivity before Dart starts. The generated-registrant contract test keeps this table
 * complete when the project's Android plugin dependencies change.
 */
internal object AndroidUiPluginRegistration {
    private val factories: Map<Class<out FlutterPlugin>, (FlutterEngine) -> FlutterPlugin> =
        linkedMapOf(
            JniPlugin::class.java to { JniPlugin() },
            SharedPreferencesPlugin::class.java to ::TrackedPreferencesPlugin,
            UrlLauncherPlugin::class.java to { UrlLauncherPlugin() },
        )

    val generatedPluginClassNames: Set<String>
        get() = factories.keys.map { it.name }.toSet()

    fun registerWith(engine: FlutterEngine) {
        check(!engine.dartExecutor.isExecutingDart) { "JNI plugins must be registered before Dart starts" }
        factories.values.forEach { create -> engine.plugins.add(create(engine)) }
    }
}
