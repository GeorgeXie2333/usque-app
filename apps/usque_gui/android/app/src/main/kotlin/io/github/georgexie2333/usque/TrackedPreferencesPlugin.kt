package io.github.georgexie2333.usque

import io.flutter.embedding.engine.FlutterEngine
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.plugins.sharedpreferences.LegacySharedPreferencesPlugin
import io.flutter.plugins.sharedpreferences.SharedPreferencesApiRegistrar
import io.flutter.plugins.sharedpreferences.SharedPreferencesAsyncApiRegistrar
import io.flutter.plugins.sharedpreferences.SharedPreferencesBackend
import io.flutter.plugins.sharedpreferences.SharedPreferencesPlugin

/** Keeps accepted preferences writes alive while the finishing UI releases its engine. */
internal class TrackedPreferencesPlugin(
    private val engine: FlutterEngine,
) : FlutterPlugin {
    private val delegate = SharedPreferencesPlugin()
    private val legacy = LegacySharedPreferencesPlugin()
    private var work: TrackedPreferencesWork? = null
    private var legacyApi: TrackedPreferencesApi? = null
    private var dataStoreApi: TrackedPreferencesAsyncApi? = null
    private var sharedPreferencesApi: TrackedPreferencesAsyncApi? = null
    private var trackedBinding: FlutterPlugin.FlutterPluginBinding? = null

    override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        val tracked = TrackedPreferencesWork()
        val context = TrackedPreferencesContext(binding.applicationContext)
        val replacement =
            FlutterPlugin.FlutterPluginBinding(
                context,
                engine,
                binding.binaryMessenger,
                binding.textureRegistry,
                binding.platformViewRegistry,
                binding.flutterAssets,
                binding.engineGroup,
            )
        work = tracked
        trackedBinding = replacement
        delegate.onAttachedToEngine(replacement)
        legacy.onAttachedToEngine(replacement)
        // Public registrars select the latest registration for this engine.
        // Install wrappers before Dart starts; keep upstream initialization and
        // storage formats, without reaching into the delegate's private fields.
        legacyApi =
            TrackedPreferencesApi(legacy, tracked).also {
                SharedPreferencesApiRegistrar().register(it, replacement)
            }
        dataStoreApi =
            TrackedPreferencesAsyncApi(delegate, tracked).also {
                SharedPreferencesAsyncApiRegistrar().register(it, replacement, "data_store")
            }
        sharedPreferencesApi =
            TrackedPreferencesAsyncApi(SharedPreferencesBackend(context), tracked).also {
                SharedPreferencesAsyncApiRegistrar().register(it, replacement, "shared_preferences")
            }
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        val replacement = trackedBinding ?: return
        work?.stopAccepting()
        try {
            // Remove the underlying registrations while the stopped wrappers
            // still shadow them, then unregister the wrappers themselves.
            delegate.onDetachedFromEngine(replacement)
            legacy.onDetachedFromEngine(replacement)
        } finally {
            legacyApi?.let { SharedPreferencesApiRegistrar().unregister(it, replacement) }
            dataStoreApi?.let { SharedPreferencesAsyncApiRegistrar().unregister(it, replacement, "data_store") }
            sharedPreferencesApi?.let {
                SharedPreferencesAsyncApiRegistrar().unregister(
                    it,
                    replacement,
                    "shared_preferences",
                )
            }
            legacyApi = null
            dataStoreApi = null
            sharedPreferencesApi = null
            trackedBinding = null
            work = null
        }
    }
}
