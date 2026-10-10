package io.github.georgexie2333.usque

import io.flutter.embedding.engine.FlutterEngine
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.plugins.sharedpreferences.SharedPreferencesPlugin
import io.flutter.plugins.urllauncher.UrlLauncherPlugin

/**
 * The default Flutter Fragment disables automatic registration and delegates it
 * to MainActivity. Avoid installing untracked preferences handlers even briefly
 * while Dart is running. The generated-registrant contract test keeps this table
 * complete when the project's Android plugin dependencies change.
 */
internal object AndroidUiPluginRegistration {
    private val factories: Map<Class<out FlutterPlugin>, (FlutterEngine) -> FlutterPlugin> =
        linkedMapOf(
            SharedPreferencesPlugin::class.java to ::TrackedPreferencesPlugin,
            UrlLauncherPlugin::class.java to { UrlLauncherPlugin() },
        )

    val generatedPluginClassNames: Set<String>
        get() = factories.keys.map { it.name }.toSet()

    fun registerWith(engine: FlutterEngine) {
        factories.values.forEach { create -> engine.plugins.add(create(engine)) }
    }
}
