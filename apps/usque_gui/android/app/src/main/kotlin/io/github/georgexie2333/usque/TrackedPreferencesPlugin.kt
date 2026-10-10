package io.github.georgexie2333.usque

import io.flutter.embedding.engine.FlutterEngine
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.plugins.sharedpreferences.SharedPreferencesPlugin

/** Keeps accepted preferences writes alive while the finishing UI releases its engine. */
internal class TrackedPreferencesPlugin(
    private val engine: FlutterEngine,
) : FlutterPlugin {
    private val delegate = SharedPreferencesPlugin()
    private var messenger: TrackedPreferencesMessenger? = null
    private var trackedBinding: FlutterPlugin.FlutterPluginBinding? = null

    override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        val tracked = TrackedPreferencesMessenger(binding.binaryMessenger)
        val replacement =
            FlutterPlugin.FlutterPluginBinding(
                TrackedPreferencesContext(binding.applicationContext),
                engine,
                tracked,
                binding.textureRegistry,
                binding.platformViewRegistry,
                binding.flutterAssets,
                binding.engineGroup,
            )
        messenger = tracked
        trackedBinding = replacement
        delegate.onAttachedToEngine(replacement)
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        val tracked = messenger ?: return
        // Unregister legacy channels too: the upstream plugin only unregisters
        // its two async API families when detached.
        tracked.stopAccepting()
        try {
            trackedBinding?.let(delegate::onDetachedFromEngine)
        } finally {
            tracked.finishCleanup()
            trackedBinding = null
            messenger = null
        }
    }
}
