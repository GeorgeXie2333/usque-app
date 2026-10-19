package io.github.georgexie2333.usque

import android.annotation.SuppressLint
import android.app.Activity
import android.app.AlertDialog
import android.net.Uri
import android.net.http.SslError
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.View
import android.webkit.ConsoleMessage
import android.webkit.CookieManager
import android.webkit.GeolocationPermissions
import android.webkit.PermissionRequest
import android.webkit.SslErrorHandler
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebMessage
import android.webkit.WebMessagePort
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebStorage
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.core.net.toUri
import io.flutter.plugin.common.MethodChannel
import java.io.ByteArrayInputStream
import java.security.SecureRandom

/** Only the official puzzle; no JavaScript interface is exposed to subframes.
 *
 * Android WebView uses this app's default profile. Open/close clear cookies and
 * WebStorage; this is not an ephemeral profile or process-death cleanup proof.
 */
internal class ProtonVerificationWindow(
    private val activity: Activity,
) {
    private val handler = Handler(Looper.getMainLooper())
    private var dialog: AlertDialog? = null
    private var webView: WebView? = null
    private var result: MethodChannel.Result? = null

    // Only the app-owned endpoint is retained; the peer becomes browser-owned
    // after postWebMessage and must never be closed through the Java wrapper.
    private var nativePort: WebMessagePort? = null
    private var installedGeneration = -1
    private var generation = 0

    @Volatile private var deadlineElapsed = 0L

    @Volatile private var deadlineWall = 0L
    private val timeout =
        object : Runnable {
            override fun run() {
                if (!active()) {
                    finish(null)
                } else {
                    handler.postDelayed(this, 250)
                }
            }
        }

    private fun active(): Boolean =
        result != null && SystemClock.elapsedRealtime() < deadlineElapsed && System.currentTimeMillis() < deadlineWall

    @SuppressLint("SetJavaScriptEnabled") // Official puzzle only; private ports, URL and origin are gated.
    fun open(
        input: ByteArray,
        reply: MethodChannel.Result,
    ) {
        if (result != null) {
            reply.error("PROTON_VERIFICATION_BUSY", "Verification is already open.", null)
            return
        }
        val session = runCatching { ProtonVerificationSession.parse(input) }.getOrNull()
        if (session == null) {
            reply.error("PROTON_VERIFICATION_INVALID", "Verification expired or is invalid.", null)
            return
        }
        val remaining = (session.expiresAt - System.currentTimeMillis()).coerceIn(0, 300000)
        if (remaining == 0L) {
            reply.error("PROTON_VERIFICATION_INVALID", "Verification expired or is invalid.", null)
            return
        }
        deadlineElapsed = SystemClock.elapsedRealtime() + remaining
        deadlineWall = session.expiresAt
        result = reply
        generation++
        val opening = generation
        try {
            WebView.setWebContentsDebuggingEnabled(false)
            val browser = WebView(activity)
            webView = browser
            browser.isSaveEnabled = false
            browser.isSaveFromParentEnabled = false
            browser.importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_NO_EXCLUDE_DESCENDANTS
            browser.settings.apply {
                javaScriptEnabled = true
                domStorageEnabled = true
                allowFileAccess = false
                allowContentAccess = false
                mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
                cacheMode = WebSettings.LOAD_NO_CACHE
                mediaPlaybackRequiresUserGesture = true
                setGeolocationEnabled(false)
                setSupportMultipleWindows(false)
            }
            CookieManager.getInstance().setAcceptThirdPartyCookies(browser, false)
            browser.setDownloadListener { _, _, _, _, _ -> finish(null) }
            browser.webChromeClient =
                object : WebChromeClient() {
                    override fun onConsoleMessage(consoleMessage: ConsoleMessage?): Boolean = true

                    override fun onPermissionRequest(request: PermissionRequest) {
                        request.deny()
                    }

                    override fun onGeolocationPermissionsShowPrompt(
                        origin: String?,
                        callback: GeolocationPermissions.Callback,
                    ) {
                        callback.invoke(origin, false, false)
                    }

                    override fun onShowFileChooser(
                        view: WebView?,
                        callback: ValueCallback<Array<Uri>>,
                        parameters: FileChooserParams?,
                    ): Boolean {
                        callback.onReceiveValue(null)
                        return true
                    }
                }
            browser.webViewClient =
                object : WebViewClient() {
                    override fun shouldOverrideUrlLoading(
                        view: WebView,
                        request: WebResourceRequest,
                    ): Boolean {
                        if (!active()) {
                            finish(null)
                            return true
                        }
                        if (request.isForMainFrame &&
                            !session.matches(request.url.toString())
                        ) {
                            finish(null)
                            return true
                        }
                        return false
                    }

                    override fun shouldInterceptRequest(
                        view: WebView?,
                        request: WebResourceRequest,
                    ): WebResourceResponse? =
                        if (!active() || request.url.scheme != "https" ||
                            request.url.host !in setOf("verify.proton.me", "verify-api.proton.me", "account.proton.me")
                        ) {
                            WebResourceResponse("text/plain", "UTF-8", ByteArrayInputStream(ByteArray(0)))
                        } else {
                            null
                        }

                    override fun onReceivedSslError(
                        view: WebView?,
                        callback: SslErrorHandler,
                        error: SslError?,
                    ) {
                        callback.cancel()
                        finish(null)
                    }

                    override fun onPageStarted(
                        view: WebView?,
                        url: String?,
                        favicon: android.graphics.Bitmap?,
                    ) {
                        generation++
                        closeNativePort()
                        installedGeneration = -1
                        if (!active() || !session.matches(url)) finish(null)
                    }

                    override fun onPageFinished(
                        view: WebView,
                        url: String?,
                    ) {
                        if (!active()) {
                            finish(null)
                            return
                        }
                        if (!session.matches(url)) return
                        if (installedGeneration == generation) return
                        installedGeneration = generation
                        val current = generation
                        try {
                            closeNativePort()
                            val channel = view.createWebMessageChannel()
                            nativePort = channel[0]
                            channel[0].setWebMessageCallback(
                                object : WebMessagePort.WebMessageCallback() {
                                    override fun onMessage(
                                        port: WebMessagePort?,
                                        message: WebMessage?,
                                    ) {
                                        if (generation != current || result == null || nativePort !== channel[0]) return
                                        if (!active()) {
                                            finish(null)
                                            return
                                        }
                                        try {
                                            val accepted = message?.data?.let { session.accept(view.url, it) } ?: return
                                            try {
                                                finish(accepted)
                                            } finally {
                                                accepted.fill(0)
                                            }
                                        } catch (_: Exception) {
                                            finish(null)
                                        }
                                    }
                                },
                            )
                            val nonce =
                                ByteArray(32).also { SecureRandom().nextBytes(it) }.joinToString("") {
                                    "%02x".format(it.toInt() and 255)
                                }
                            view.evaluateJavascript(session.script(nonce)) {
                                if (result == null || generation != current || nativePort !== channel[0]) {
                                    // This endpoint has not been transferred. Its stale
                                    // evaluation callback must not touch the next window.
                                    runCatching { channel[1].close() }
                                    return@evaluateJavascript
                                }
                                try {
                                    if (!active() || !session.matches(view.url)) {
                                        channel[1].close()
                                        finish(null)
                                    } else {
                                        view.postWebMessage(
                                            WebMessage(nonce, arrayOf(channel[1])),
                                            "https://verify.proton.me".toUri(),
                                        )
                                        // channel[1] is now owned by the page. Retain
                                        // only nativePort, including across reloads.
                                    }
                                } catch (_: Exception) {
                                    runCatching { channel[1].close() }
                                    finish(null)
                                }
                            }
                        } catch (_: Exception) {
                            finish(null)
                        }
                    }
                }
            dialog =
                AlertDialog
                    .Builder(
                        activity,
                    ).setTitle(
                        "Proton VPN",
                    ).setView(browser)
                    .setNegativeButton(android.R.string.cancel) { _, _ -> finish(null) }
                    .create()
                    .also {
                        it.setOnCancelListener { finish(null) }
                        it.show()
                    }
            handler.postDelayed(timeout, remaining.coerceAtMost(250))
            CookieManager.getInstance().removeAllCookies {
                if (result === reply && webView === browser && generation == opening) {
                    if (!active() || session.expired()) {
                        finish(null)
                        return@removeAllCookies
                    }
                    try {
                        browser.loadUrl(session.url)
                    } catch (_: Exception) {
                        finish(null)
                    }
                }
            }
        } catch (_: Exception) {
            finish(null)
        }
    }

    private fun closeNativePort() {
        val port = nativePort
        nativePort = null
        runCatching { port?.close() }
    }

    fun finish(value: ByteArray?) {
        val reply = result ?: return
        result = null
        deadlineElapsed = 0L
        deadlineWall = 0L
        generation++
        handler.removeCallbacks(timeout)
        closeNativePort()
        installedGeneration = -1
        val modal = dialog
        dialog = null
        val browser = webView
        webView = null
        // One broken platform cleanup step must not strand the pending Flutter
        // reply or prevent the remaining local cleanup attempts.
        runCatching { modal?.dismiss() }
        runCatching { browser?.stopLoading() }
        runCatching { browser?.clearHistory() }
        runCatching { browser?.clearCache(true) }
        runCatching { browser?.removeAllViews() }
        runCatching { browser?.destroy() }
        runCatching { CookieManager.getInstance().removeAllCookies(null) }
        runCatching { WebStorage.getInstance().deleteAllData() }
        reply.success(value)
    }
}
