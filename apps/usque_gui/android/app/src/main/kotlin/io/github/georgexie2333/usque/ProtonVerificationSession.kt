package io.github.georgexie2333.usque

import org.json.JSONObject
import java.net.URI
import java.net.URLDecoder
import java.security.MessageDigest

/** Pure, one-use policy shared by the native window and host-side fixtures. */
internal class ProtonVerificationSession private constructor(
    val url: String,
    val expiresAt: Long,
    val version: Int,
    val routeEpoch: Int,
    val proxy: Proxy?,
    private val digest: String,
    private val methods: Set<String>,
    private val now: () -> Long,
) {
    internal data class Proxy(
        val port: Int,
        val username: String,
        val password: String,
        val nonce: String,
    ) {
        val origin get() = "http://127.0.0.1:$port"

        fun probe(path: String = "probe") = "http://$nonce.usque-proton-proxy.invalid/$path"

        val https get() = "https://$nonce.usque-proton-proxy.invalid/"
        val ready: String
            get() {
                val proof =
                    MessageDigest
                        .getInstance("SHA-256")
                        .digest(password.toByteArray(Charsets.US_ASCII))
                        .joinToString("") { "%02x".format(it.toInt() and 255) }
                return "usque-proxy-ready://$nonce/$proof"
            }

        fun matches(url: String?) = url == probe() || url == probe("ready") || url == https
    }

    fun networkFailure(): ByteArray =
        JSONObject()
            .put(
                "type",
                "USQUE_VERIFICATION_NETWORK_FAILED",
            ).put("route_epoch", routeEpoch)
            .toString()
            .toByteArray(Charsets.UTF_8)

    private var used = false

    fun expired(): Boolean = now() >= expiresAt

    fun matches(candidate: String?): Boolean =
        runCatching {
            require(!expired() && candidate != null && candidate.length <= 16384)
            val uri = URI(candidate)
            require(
                uri.scheme == "https" && uri.host == "verify.proton.me" && uri.port == -1 && uri.rawUserInfo == null,
            )
            require(uri.rawPath in setOf("", "/") && uri.rawFragment == null)
            val tokens =
                uri.rawQuery.orEmpty().split('&').map { it.split('=', limit = 2) }.filter {
                    URLDecoder.decode(it[0], "UTF-8") ==
                        "token"
                }
            require(tokens.size == 1 && tokens[0].size == 2)
            val token = URLDecoder.decode(tokens[0][1], "UTF-8")
            require(token.isNotEmpty() && token.length <= 8192)
            val actual =
                MessageDigest.getInstance("SHA-256").digest(token.toByteArray(Charsets.UTF_8)).joinToString("") {
                    "%02x".format(it.toInt() and 255)
                }
            MessageDigest.isEqual(actual.toByteArray(Charsets.US_ASCII), digest.toByteArray(Charsets.US_ASCII))
        }.getOrDefault(false)

    fun accept(
        candidate: String?,
        message: String,
    ): ByteArray? {
        if (used || !matches(candidate) || message.length > 32768) return null
        return runCatching {
            val bytes = message.toByteArray(Charsets.UTF_8)
            require(bytes.size <= 32768)
            val value = JSONObject(message)
            require(value.getString("type") == "HUMAN_VERIFICATION_SUCCESS")
            val payload = value.getJSONObject("payload")
            require(payload.getString("type") in methods)
            val token = payload.getString("token")
            require(token.length in 1..8192 && token.all { it.code in 33..126 })
            used = true
            bytes
        }.getOrNull()
    }

    fun script(nonce: String): String {
        val settings =
            JSONObject()
                .put(
                    "nonce",
                    nonce,
                ).put("digest", digest)
                .put("expires", expiresAt)
                .put("methods", methods.toList())
                .toString()
        return """
            (() => {
              'use strict';
              const s = $settings;
              const origin = 'https://verify.proton.me';
              const captchaOrigin = 'https://verify-api.proton.me';
              const initial = new URL(window.location.href);
              const tokens = initial.searchParams.getAll('token');
              const same = () => {
                try {
                  const page = new URL(window.location.href), values = page.searchParams.getAll('token');
                  return window.top === window && page.origin === origin && values.length === 1 && values[0] === tokens[0] && Date.now() < s.expires;
                } catch (_) { return false; }
              };
              if (tokens.length !== 1 || !same()) return;
              const initialize = async event => {
                if (event.data !== s.nonce || event.ports.length !== 1 || !same()) return;
                window.removeEventListener('message', initialize);
                const port = event.ports[0];
                const bytes = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(tokens[0]));
                const hash = Array.from(new Uint8Array(bytes), v => v.toString(16).padStart(2,'0')).join('');
                if (hash !== s.digest || !same()) { port.close(); return; }
                let used = false;
                const stop = () => { used = true; clearTimeout(timer); window.removeEventListener('message', receive); port.close(); };
                const receive = e => {
                  if (used) return;
                  if (!same()) { stop(); return; }
                  let message;
                  if (e.origin === origin && e.source === window && typeof e.data === 'string' && e.data.length <= 32768) {
                    message = e.data;
                  } else if (e.origin === captchaOrigin && s.methods.includes('captcha') && e.data && typeof e.data === 'object' && !Array.isArray(e.data) && e.data.type === 'pm_captcha' && typeof e.data.token === 'string' && /^[\x21-\x7e]{1,8192}$/.test(e.data.token)) {
                    const page = new URL(window.location.href);
                    if (page.searchParams.getAll('embed').length !== 1 || page.searchParams.get('embed') !== 'true') return;
                    // The official embedded page forwards this token unchanged.
                    // Receive it directly as well, before relying on its second
                    // asynchronous top-level dispatch. Never expose a JS interface.
                    const frames = Array.from(document.querySelectorAll('iframe')).filter(frame => {
                      try {
                        const u = new URL(frame.src), values = u.searchParams.getAll('Token'), force = u.searchParams.getAll('ForceWebMessaging');
                        return u.origin === captchaOrigin && !u.username && !u.password && u.pathname === '/core/v4/captcha' && !u.hash && values.length === 1 && values[0] === tokens[0] && force.length === 1 && force[0] === '1';
                      } catch (_) { return false; }
                    });
                    if (frames.length !== 1 || e.source !== frames[0].contentWindow) return;
                    message = JSON.stringify({type:'HUMAN_VERIFICATION_SUCCESS',payload:{token:e.data.token,type:'captcha'}});
                  } else { return; }
                  let data; try { data = JSON.parse(message); } catch (_) { return; }
                  if (!data || data.type !== 'HUMAN_VERIFICATION_SUCCESS' || !data.payload || !s.methods.includes(data.payload.type) || typeof data.payload.token !== 'string' || !/^[\x21-\x7e]{1,8192}$/.test(data.payload.token)) return;
                  used = true; clearTimeout(timer); window.removeEventListener('message', receive);
                  // Native retains and closes only its own message-port endpoint.
                  port.postMessage(message);
                };
                const timer = setTimeout(stop, Math.max(0, s.expires-Date.now()));
                window.addEventListener('message', receive);
              };
              window.addEventListener('message', initialize);
            })();
            """.trimIndent()
    }

    companion object {
        fun parse(
            bytes: ByteArray,
            now: () -> Long = System::currentTimeMillis,
        ): ProtonVerificationSession {
            require(bytes.size in 1..65536)
            val value = JSONObject(bytes.toString(Charsets.UTF_8))
            require(value.getString("job_id").isNotEmpty() && value.getString("challenge_id").isNotEmpty())
            val expiry = value.getLong("expires_at")
            require(expiry > now() && expiry - now() <= 300000)
            val digest = value.getString("challenge_sha256")
            require(digest.matches(Regex("[0-9a-f]{64}")))
            val values = value.getJSONArray("methods")
            require(values.length() in 1..16)
            val methods = (0 until values.length()).map { values.getString(it) }.toSet()
            require(methods.isNotEmpty() && methods.all { it in setOf("captcha", "sms", "email") })
            val version = value.optInt("version", 1)
            require(version in 1..2)
            val epoch = value.optInt("route_epoch", 0)
            require(epoch in 0..1 && (version == 2 || epoch == 0))
            val proxy =
                if (epoch == 1) {
                    val item = value.getJSONObject("proxy")
                    val port = item.getInt("port")

                    fun hex(
                        key: String,
                        size: Int,
                    ): String = item.getString(key).also { require(it.matches(Regex("[0-9a-f]{$size}"))) }
                    require(port in 1..65535 && item.length() == 4)
                    Proxy(port, hex("username", 32), hex("password", 64), hex("nonce", 32))
                } else {
                    require(!value.has("proxy"))
                    null
                }
            return ProtonVerificationSession(
                value.getString("url"),
                expiry,
                version,
                epoch,
                proxy,
                digest,
                methods,
                now,
            ).also {
                require(it.matches(it.url))
            }
        }
    }
}
