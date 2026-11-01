package io.github.georgexie2333.usque

import android.app.Activity
import android.content.Context
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.os.Build
import android.view.KeyEvent
import android.view.inputmethod.InputMethodManager
import android.widget.FrameLayout
import android.window.OnBackInvokedCallback
import android.window.OnBackInvokedDispatcher
import androidx.annotation.RequiresApi
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

/** Native UI adapter only. No engine, account data or network operations. */
internal class TvInputHost(
    private val activity: Activity,
) : TvInputPolicy.Host {
    private val television =
        TvInputPolicy.isTelevision(
            activity.resources.configuration.uiMode and Configuration.UI_MODE_TYPE_MASK ==
                Configuration.UI_MODE_TYPE_TELEVISION,
            activity.packageManager.hasSystemFeature(PackageManager.FEATURE_LEANBACK),
            activity.packageManager.hasSystemFeature("amazon.hardware.fire_tv"),
        )
    private val policy = TvInputPolicy(television, this)
    private var channel: MethodChannel? = null
    private var root: FrameLayout? = null
    private var modernBack: ModernBack? = null

    fun createRoot(context: Context): FrameLayout =
        object : FrameLayout(context) {
            override fun dispatchKeyEventPreIme(event: KeyEvent): Boolean {
                if (television && Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU &&
                    event.keyCode == KeyEvent.KEYCODE_BACK &&
                    (event.action == KeyEvent.ACTION_DOWN || event.action == KeyEvent.ACTION_UP) &&
                    policy.backKey(event.action == KeyEvent.ACTION_DOWN, event.downTime, event.isCanceled)
                ) {
                    return true
                }
                return super.dispatchKeyEventPreIme(event)
            }
        }.also { view ->
            root = view
            if (television) {
                // Observe at our container, never replace FlutterView's IME or
                // animation callbacks. Always pass the insets to its children.
                ViewCompat.setOnApplyWindowInsetsListener(view) { _, insets ->
                    policy.imeVisibility(insets.isVisible(WindowInsetsCompat.Type.ime()))
                    insets
                }
            }
        }

    fun attach(engine: FlutterEngine) {
        channel = MethodChannel(engine.dartExecutor.binaryMessenger, "io.github.georgexie2333.usque/tv_input")
        channel?.setMethodCallHandler { call, result ->
            when (call.method) {
                "getEnvironment" -> {
                    result.success(mapOf("isTv" to television, "nativeBack" to television))
                }

                "beginEditing", "endEditing" -> {
                    val sessionId = call.argument<String>("sessionId")
                    if (sessionId.isNullOrEmpty() || sessionId.length > 128) {
                        result.error("INVALID_SESSION", "The input session is invalid.", null)
                    } else {
                        if (call.method == "beginEditing") policy.begin(sessionId) else policy.end(sessionId)
                        result.success(null)
                    }
                }

                else -> {
                    result.notImplemented()
                }
            }
        }
    }

    override fun registerBack(sessionId: String?) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            if (modernBack == null) modernBack = ModernBack(activity)
            modernBack?.register(sessionId) { policy.systemBack(it) }
        }
    }

    override fun hideKeyboard() {
        val token = root?.windowToken ?: return
        activity.getSystemService(InputMethodManager::class.java).hideSoftInputFromWindow(token, 0)
    }

    override fun editingEnded(
        sessionId: String,
        reason: String,
    ) {
        channel?.invokeMethod("editingEnded", mapOf("sessionId" to sessionId, "reason" to reason))
    }

    fun stopped() = policy.stopped()

    fun dispose() {
        policy.stopped()
        channel?.setMethodCallHandler(null)
        channel = null
        root?.let { ViewCompat.setOnApplyWindowInsetsListener(it, null) }
        root = null
    }

    /** Kept in a separate class so API 26 does not resolve API 33 types. */
    @RequiresApi(Build.VERSION_CODES.TIRAMISU)
    private class ModernBack(
        private val activity: Activity,
    ) {
        private var callback: OnBackInvokedCallback? = null

        fun register(
            sessionId: String?,
            onBack: (String) -> Unit,
        ) {
            callback?.let { activity.onBackInvokedDispatcher.unregisterOnBackInvokedCallback(it) }
            callback = null
            if (sessionId == null) return
            val next = OnBackInvokedCallback { onBack(sessionId) }
            callback = next
            activity.onBackInvokedDispatcher
                .registerOnBackInvokedCallback(OnBackInvokedDispatcher.PRIORITY_OVERLAY, next)
        }
    }
}
