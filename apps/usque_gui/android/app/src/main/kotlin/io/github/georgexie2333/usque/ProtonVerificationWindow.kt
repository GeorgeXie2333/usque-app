package io.github.georgexie2333.usque

import android.annotation.SuppressLint
import android.app.Activity
import android.app.Dialog
import android.content.res.Configuration
import android.net.Uri
import android.net.http.SslError
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.webkit.ConsoleMessage
import android.webkit.CookieManager
import android.webkit.GeolocationPermissions
import android.webkit.HttpAuthHandler
import android.webkit.PermissionRequest
import android.webkit.RenderProcessGoneDetail
import android.webkit.SslErrorHandler
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebMessage
import android.webkit.WebMessagePort
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebStorage
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import android.widget.ImageButton
import androidx.core.net.toUri
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.webkit.ProxyConfig
import androidx.webkit.ProxyController
import androidx.webkit.WebViewFeature
import io.flutter.plugin.common.MethodChannel
import java.io.ByteArrayInputStream
import java.security.SecureRandom
import java.util.concurrent.atomic.AtomicBoolean

/** Only the official puzzle; no JavaScript interface is exposed to subframes.
 *
 * Android WebView uses this app's default profile. Open/close clear cookies and
 * WebStorage; this is not an ephemeral profile or process-death cleanup proof.
 */
internal class ProtonVerificationWindow(
    private val activity: Activity,
) {
    private val handler = Handler(Looper.getMainLooper())
    private var dialog: Dialog? = null

    @Volatile private var webView: WebView? = null

    @Volatile private var result: MethodChannel.Result? = null
    private var closing = false
    private var session: ProtonVerificationSession? = null
    private var proxyApplied = false
    private var ownsLease = false
    private var probeStage = 0
    private var loadDeadline = 0L
    private var loaded = false

    companion object {
        private val processLease = AtomicBoolean(false)
    }

    private enum class Failure {
        EXPIRED,
        NAVIGATION,
        TLS,
        BRIDGE,
        LOAD,
        DOWNLOAD,
        HOST_STOPPED,
    }

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
                if (!loaded && loadDeadline != 0L && SystemClock.elapsedRealtime() >= loadDeadline) {
                    fail(Failure.LOAD)
                } else if (!active()) {
                    fail(Failure.EXPIRED)
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
        if (result != null || closing) {
            reply.error("PROTON_VERIFICATION_BUSY", "Verification is already open.", null)
            return
        }
        val session = runCatching { ProtonVerificationSession.parse(input) }.getOrNull()
        if (session == null) {
            reply.error("PROTON_VERIFICATION_INVALID", "Verification expired or is invalid.", null)
            return
        }
        if (!processLease.compareAndSet(false, true)) {
            reply.error("PROTON_VERIFICATION_BUSY", "Verification is already open.", null)
            return
        }
        ownsLease = true
        this.session = session
        loaded = false
        probeStage = if (session.proxy == null) 0 else 1
        val remaining = (session.expiresAt - System.currentTimeMillis()).coerceIn(0, 300000)
        if (remaining == 0L) {
            processLease.set(false)
            ownsLease = false
            this.session = null
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

            fun currentWindow(): Boolean = result === reply && webView === browser
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
            browser.setDownloadListener { _, _, _, _, _ -> if (currentWindow()) fail(Failure.DOWNLOAD) }
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
                    override fun onRenderProcessGone(
                        view: WebView,
                        detail: RenderProcessGoneDetail,
                    ): Boolean {
                        // finish invalidates the bridge immediately, then destroys
                        // this view and clears its proxy before releasing the lease.
                        // A stale callback must not replace an already accepted result.
                        if (currentWindow()) fail(Failure.LOAD)
                        return true
                    }

                    override fun shouldOverrideUrlLoading(
                        view: WebView,
                        request: WebResourceRequest,
                    ): Boolean {
                        if (!currentWindow()) return true
                        if (!active()) {
                            fail(Failure.EXPIRED)
                            return true
                        }
                        val proxy = session.proxy
                        if (probeStage != 0 && proxy != null) {
                            if (probeStage == 2 && request.isForMainFrame && request.url.toString() == proxy.ready) {
                                probeStage = 0
                                loadDeadline = SystemClock.elapsedRealtime() + 15000
                                view.loadUrl(session.url)
                                return true
                            }
                            return !proxy.matches(request.url.toString())
                        }
                        if (request.isForMainFrame &&
                            !session.matches(request.url.toString())
                        ) {
                            // Block the attempted navigation while keeping the
                            // bound puzzle alive, including any queued result.
                            return true
                        }
                        return false
                    }

                    override fun shouldInterceptRequest(
                        view: WebView?,
                        request: WebResourceRequest,
                    ): WebResourceResponse? =
                        if (currentWindow() && active() && probeStage != 0 &&
                            session.proxy?.matches(request.url.toString()) == true
                        ) {
                            null
                        } else if (!currentWindow() || !active() || request.url.scheme != "https" ||
                            request.url.host !in setOf("verify.proton.me", "verify-api.proton.me", "account.proton.me")
                        ) {
                            WebResourceResponse("text/plain", "UTF-8", ByteArrayInputStream(ByteArray(0)))
                        } else {
                            null
                        }

                    override fun onReceivedHttpAuthRequest(
                        view: WebView?,
                        callback: HttpAuthHandler,
                        host: String?,
                        realm: String?,
                    ) {
                        val proxy = session.proxy
                        if (currentWindow() && active() && proxyApplied && proxy != null && host == "127.0.0.1" &&
                            realm == proxy.nonce
                        ) {
                            callback.proceed(proxy.username, proxy.password)
                        } else {
                            callback.cancel()
                        }
                    }

                    override fun onReceivedError(
                        view: WebView,
                        request: WebResourceRequest,
                        error: WebResourceError,
                    ) {
                        if (!currentWindow()) return
                        val proxy = session.proxy
                        if (probeStage == 1 && proxy != null && request.isForMainFrame &&
                            request.url.toString() == proxy.https
                        ) {
                            probeStage = 2
                            view.loadUrl(proxy.probe("ready"))
                            return
                        }
                        if (error.errorCode in
                            setOf(ERROR_HOST_LOOKUP, ERROR_CONNECT, ERROR_TIMEOUT, ERROR_FAILED_SSL_HANDSHAKE, ERROR_IO)
                        ) {
                            fail(Failure.LOAD)
                        }
                    }

                    override fun onReceivedSslError(
                        view: WebView?,
                        callback: SslErrorHandler,
                        error: SslError?,
                    ) {
                        callback.cancel()
                        if (currentWindow()) fail(Failure.TLS)
                    }

                    override fun onPageStarted(
                        view: WebView?,
                        url: String?,
                        favicon: android.graphics.Bitmap?,
                    ) {
                        if (!currentWindow()) return
                        if (probeStage != 0 && session.proxy?.matches(url) == true) return
                        generation++
                        closeNativePort()
                        installedGeneration = -1
                        if (!active()) {
                            fail(Failure.EXPIRED)
                        } else if (!session.matches(url)) {
                            fail(Failure.NAVIGATION)
                        }
                    }

                    override fun onPageFinished(
                        view: WebView,
                        url: String?,
                    ) {
                        if (!currentWindow()) return
                        if (!active()) {
                            fail(Failure.EXPIRED)
                            return
                        }
                        if (!session.matches(url) || probeStage != 0) return
                        loaded = true
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
                                            fail(Failure.EXPIRED)
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
                                            fail(Failure.BRIDGE)
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
                                        fail(if (!active()) Failure.EXPIRED else Failure.NAVIGATION)
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
                                    fail(Failure.BRIDGE)
                                }
                            }
                        } catch (_: Exception) {
                            fail(Failure.BRIDGE)
                        }
                    }
                }
            val modal = Dialog(activity, R.style.ProtonVerificationTheme)
            dialog = modal
            modal.setContentView(R.layout.proton_verification)
            val content = modal.findViewById<View>(R.id.proton_verification_content)
            ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
                val safeArea =
                    insets.getInsets(
                        WindowInsetsCompat.Type.systemBars() or
                            WindowInsetsCompat.Type.displayCutout() or
                            WindowInsetsCompat.Type.ime(),
                    )
                view.setPadding(safeArea.left, safeArea.top, safeArea.right, safeArea.bottom)
                WindowInsetsCompat.CONSUMED
            }
            modal.findViewById<FrameLayout>(R.id.proton_verification_browser).addView(
                browser,
                ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT),
            )
            modal.findViewById<ImageButton>(R.id.proton_verification_close).apply {
                val label = AndroidLocaleController.getString(activity, android.R.string.cancel)
                contentDescription = label
                tooltipText = label
                setOnClickListener { if (currentWindow()) cancel() }
            }
            // Page content must never measure or resize the native window.
            // Keep the close control outside the WebView and inside safe insets.
            modal.window?.let { WindowCompat.setDecorFitsSystemWindows(it, false) }
            modal.setCanceledOnTouchOutside(false)
            modal.setOnCancelListener { if (currentWindow()) cancel() }
            modal.show()
            modal.window?.let { window ->
                window.setLayout(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
                val nightMode = activity.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK
                val light = nightMode != Configuration.UI_MODE_NIGHT_YES
                WindowCompat.getInsetsController(window, content).apply {
                    isAppearanceLightStatusBars = light
                    isAppearanceLightNavigationBars = light
                }
            }
            ViewCompat.requestApplyInsets(content)
            handler.postDelayed(timeout, remaining.coerceAtMost(250))
            CookieManager.getInstance().removeAllCookies {
                if (result === reply && webView === browser && generation == opening) {
                    if (!active() || session.expired()) {
                        fail(Failure.EXPIRED)
                        return@removeAllCookies
                    }
                    try {
                        val proxy = session.proxy
                        if (proxy == null) {
                            loadDeadline = SystemClock.elapsedRealtime() + 15000
                            browser.loadUrl(session.url)
                        } else if (WebViewFeature.isFeatureSupported(WebViewFeature.PROXY_OVERRIDE)) {
                            proxyApplied = true
                            loadDeadline = SystemClock.elapsedRealtime() + 5000
                            ProxyController.getInstance().setProxyOverride(
                                ProxyConfig
                                    .Builder()
                                    .addProxyRule(proxy.origin)
                                    .removeImplicitRules()
                                    .build(),
                                { handler.post(it) },
                                { if (currentWindow() && active()) browser.loadUrl(proxy.probe()) },
                            )
                        } else {
                            fail(Failure.BRIDGE)
                        }
                    } catch (_: Exception) {
                        fail(Failure.LOAD)
                    }
                }
            }
        } catch (_: Exception) {
            fail(Failure.LOAD)
        }
    }

    private fun closeNativePort() {
        val port = nativePort
        nativePort = null
        runCatching { port?.close() }
    }

    fun cancel() = finish(null)

    fun hostStopped() = fail(Failure.HOST_STOPPED)

    private fun fail(reason: Failure) {
        val current = session
        if (current?.version == 2 && current.proxy == null && reason in setOf(Failure.LOAD, Failure.TLS)) {
            val bytes = current.networkFailure()
            try {
                finish(bytes)
            } finally {
                bytes.fill(0)
            }
        } else {
            finish(null, reason)
        }
    }

    private fun finish(
        value: ByteArray?,
        failure: Failure? = null,
    ) {
        val reply = result ?: return
        result = null
        closing = true
        session = null
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
        // The message callback clears its input before this posted cleanup runs.
        val response = value?.copyOf()
        val cleanup =
            Runnable {
                // Leave the WebMessage callback before destroying its WebView.
                // A failed cleanup step cannot replace the accepted result.
                runCatching { modal?.dismiss() }
                runCatching { browser?.stopLoading() }
                runCatching { browser?.clearHistory() }
                runCatching { browser?.clearCache(true) }
                runCatching { (browser?.parent as? ViewGroup)?.removeView(browser) }
                runCatching { browser?.removeAllViews() }
                runCatching { browser?.destroy() }
                runCatching { CookieManager.getInstance().removeAllCookies(null) }
                runCatching { WebStorage.getInstance().deleteAllData() }
                val delivered = AtomicBoolean(false)
                val cleanupTimeout =
                    Runnable {
                        if (delivered.compareAndSet(false, true)) {
                            response?.fill(0)
                            reply.error(
                                "PROTON_VERIFICATION_UNAVAILABLE",
                                "Verification proxy cleanup timed out.",
                                null,
                            )
                        }
                    }
                val complete =
                    Runnable {
                        closing = false
                        if (ownsLease) {
                            ownsLease = false
                            processLease.set(false)
                        }
                        handler.removeCallbacks(cleanupTimeout)
                        if (!delivered.compareAndSet(false, true)) return@Runnable
                        try {
                            if (failure == null) {
                                reply.success(response)
                            } else {
                                reply.error(
                                    "PROTON_VERIFICATION_${failure.name}",
                                    "Verification could not complete.",
                                    null,
                                )
                            }
                        } finally {
                            response?.fill(0)
                        }
                    }
                if (proxyApplied) {
                    proxyApplied = false
                    // Keep the process lease until WebView confirms removal.
                    handler.postDelayed(cleanupTimeout, 5000)
                    runCatching {
                        if (WebViewFeature.isFeatureSupported(WebViewFeature.PROXY_OVERRIDE)) {
                            ProxyController.getInstance().clearProxyOverride(
                                { handler.post(it) },
                                complete,
                            )
                        } else {
                            error("Proxy override unavailable")
                        }
                    }.onFailure {
                        handler.removeCallbacks(cleanupTimeout)
                        response?.fill(0)
                        if (delivered.compareAndSet(false, true)) {
                            reply.error("PROTON_VERIFICATION_UNAVAILABLE", "Verification proxy cleanup failed.", null)
                        }
                    }
                } else {
                    complete.run()
                }
            }
        if (!runCatching { handler.post(cleanup) }.getOrDefault(false)) cleanup.run()
    }
}
