package io.github.georgexie2333.usque

import org.json.JSONObject

/** HTTP/SOCKS startup captures routed traffic before waiting for an uplink. */
internal object ProxyChainVpnLifecycle {
    data class FailurePolicy(
        val protectedCapture: Boolean,
        val killSwitch: Boolean,
    )

    fun failurePolicy(
        profileJson: String?,
        inheritedCapture: Boolean,
        inheritedKillSwitch: Boolean,
    ): FailurePolicy {
        val profile = runCatching { JSONObject(requireNotNull(profileJson)) }.getOrNull()
        // Proxy-only targets are valid replacements of an inherited interface.
        // Parsing them as AndroidVpnProfile would discard their failure policy.
        val targetKillSwitch = runCatching { requireNotNull(profile).getBoolean("kill_switch") }.getOrDefault(true)
        return FailurePolicy(
            protectedCapture = inheritedCapture || (profile?.let { ChainProfileFields.proxy(it) } == true),
            killSwitch = inheritedKillSwitch || targetKillSwitch,
        )
    }

    sealed interface Startup<out T> {
        data class Ready<T>(
            val descriptor: T,
        ) : Startup<T>

        data object Cancelled : Startup<Nothing>

        data object WaitingForNetwork : Startup<Nothing>

        data object TunUnavailable : Startup<Nothing>
    }

    fun <T : Any> prepare(
        proxyChain: Boolean,
        isCurrent: () -> Boolean,
        awaitPhysicalNetwork: () -> Boolean,
        establishTun: () -> T?,
    ): Startup<T> {
        if (!isCurrent()) return Startup.Cancelled
        if (!proxyChain && !awaitPhysicalNetwork()) return Startup.WaitingForNetwork
        if (!isCurrent()) return Startup.Cancelled
        val descriptor = establishTun() ?: return Startup.TunUnavailable
        if (!isCurrent()) return Startup.Cancelled
        if (proxyChain && !awaitPhysicalNetwork()) return Startup.WaitingForNetwork
        return if (isCurrent()) Startup.Ready(descriptor) else Startup.Cancelled
    }

    // Called only for terminal failures, after the native stop has completed.
    // The owner callback must also compare the descriptor before closing it.
    fun releaseAfterStop(
        proxyChain: Boolean,
        killSwitch: Boolean,
        confirmed: Boolean,
        isCurrent: () -> Boolean,
        releaseOwnedTun: () -> Unit,
    ) {
        if (proxyChain && !killSwitch && confirmed && isCurrent()) releaseOwnedTun()
    }
}
