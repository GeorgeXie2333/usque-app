package io.github.georgexie2333.usque

import java.net.URI
import java.util.Locale

/**
 * Process-local gate for the experimental Cloudflare WARP protocol callback.
 * It deliberately has no serialization surface, so process death invalidates
 * every pending login and every unconsumed Access assertion.
 */
internal class ZeroTrustCallbackSession {
    private var expectedTeam: String? = null
    private var pendingCallback: String? = null

    fun begin(team: String): String {
        val normalized = normalizeTeam(team)
        expectedTeam = normalized
        pendingCallback = null
        return "https://$normalized.cloudflareaccess.com/warp"
    }

    fun accept(callbackUri: String): Boolean {
        val expected = expectedTeam ?: return false
        if (callbackUri.length !in 1..MAX_CALLBACK_CHARS) return false
        val callback = runCatching { URI(callbackUri) }.getOrNull() ?: return false
        val query = callback.rawQuery?.split('&') ?: return false
        val token = query.singleOrNull()?.split('=', limit = 2) ?: return false
        if (
            callback.scheme != "com.cloudflare.warp" ||
            callback.host?.lowercase(Locale.ROOT) != "$expected.cloudflareaccess.com" ||
            callback.rawPath != "/auth" ||
            callback.rawUserInfo != null ||
            callback.port != -1 ||
            callback.rawFragment != null ||
            token.size != 2 ||
            token[0] != "token" ||
            token[1].isEmpty()
        ) {
            return false
        }
        pendingCallback = callbackUri
        expectedTeam = null
        return true
    }

    fun consume(): String? = pendingCallback.also { pendingCallback = null }

    fun cancel() {
        expectedTeam = null
        pendingCallback = null
    }

    internal companion object {
        const val MAX_CALLBACK_CHARS = 64 * 1024

        fun normalizeTeam(value: String): String {
            val team = value.trim().lowercase(Locale.ROOT)
            require(team.length in 1..63)
            require(team.first().isLetterOrDigit() && team.last().isLetterOrDigit())
            require(team.all { it in 'a'..'z' || it.isDigit() || it == '-' })
            return team
        }
    }
}
