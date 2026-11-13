package io.github.georgexie2333.usque

import android.content.Context
import io.flutter.embedding.android.FlutterFragment
import io.flutter.embedding.engine.FlutterEngine

/** Owns an engine whose JNI plugins are configured before its Dart entrypoint runs. */
class UsqueFlutterFragment : FlutterFragment() {
    override fun provideFlutterEngine(context: Context): FlutterEngine =
        FlutterEngine(context, flutterShellArgs.toArray(), false, shouldRestoreAndSaveState())

    // A provided engine otherwise outlives the Fragment by default. Preserve
    // the original new-engine lifetime, including plugin detach and restoration.
    override fun shouldDestroyEngineWithHost(): Boolean = true
}
