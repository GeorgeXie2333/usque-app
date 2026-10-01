package io.github.georgexie2333.usque

import android.system.ErrnoException
import java.util.IdentityHashMap

// OsConstants.ENONET needs API 31. Android's ARM/x86 NDK asm headers include
// asm-generic/errno.h, where this native errno is 64 on every supported ABI.
private const val ANDROID_ENONET = 64

internal object UnderlyingSocketBindingResult {
    const val BOUND = 0
    const val STALE = 1
    const val REJECTED = 2
}

/** Protects and binds only the selected generation; rejected sockets never gain a fallback route. */
internal class UnderlyingSocketBinding<N : Any, D : Any>(
    private val destroyed: () -> Boolean,
    private val currentGeneration: () -> Long,
    private val networkForGeneration: (Long) -> N?,
    private val networkHandle: (N) -> Long,
    private val protect: (Int) -> Boolean,
    private val duplicate: (Int) -> D,
    private val bindToNetwork: (N, D) -> Unit,
    private val close: (D) -> Unit,
    private val isNetworkGoneException: (Exception) -> Boolean = ::isUnderlyingNetworkGone,
) {
    fun bind(
        descriptor: Int,
        expectedGeneration: Long,
        requireVpnProtection: Boolean,
    ): Int {
        if (descriptor < 0 || expectedGeneration < 0 || destroyed()) return UnderlyingSocketBindingResult.REJECTED
        if (currentGeneration() != expectedGeneration) return UnderlyingSocketBindingResult.STALE
        val network = networkForGeneration(expectedGeneration) ?: return UnderlyingSocketBindingResult.STALE
        if (requireVpnProtection) {
            val protected =
                try {
                    protect(descriptor)
                } catch (_: Exception) {
                    false
                }
            // Protection failures remain terminal even if the network changed at the same time.
            if (!protected) return UnderlyingSocketBindingResult.REJECTED
        }
        val protectedStatus = bindingStatus(expectedGeneration, network)
        if (protectedStatus != UnderlyingSocketBindingResult.BOUND) return protectedStatus
        val copy =
            try {
                duplicate(descriptor)
            } catch (_: Exception) {
                return UnderlyingSocketBindingResult.REJECTED
            }
        var result: Int
        try {
            result = bindingStatus(expectedGeneration, network)
            if (result == UnderlyingSocketBindingResult.BOUND) {
                result =
                    try {
                        bindToNetwork(network, copy)
                        bindingStatus(expectedGeneration, network)
                    } catch (error: Exception) {
                        val status = bindingStatus(expectedGeneration, network)
                        when {
                            status != UnderlyingSocketBindingResult.BOUND -> status
                            isNetworkGoneException(error) -> UnderlyingSocketBindingResult.STALE
                            else -> UnderlyingSocketBindingResult.REJECTED
                        }
                    }
            }
        } finally {
            try {
                close(copy)
            } catch (_: Exception) {
                result = UnderlyingSocketBindingResult.REJECTED
            }
        }
        if (destroyed()) return UnderlyingSocketBindingResult.REJECTED
        return if (result == UnderlyingSocketBindingResult.BOUND) bindingStatus(expectedGeneration, network) else result
    }

    private fun bindingStatus(
        expectedGeneration: Long,
        network: N,
    ): Int =
        when {
            destroyed() -> {
                UnderlyingSocketBindingResult.REJECTED
            }

            currentGeneration() != expectedGeneration -> {
                UnderlyingSocketBindingResult.STALE
            }

            networkForGeneration(expectedGeneration)?.let(networkHandle) != networkHandle(network) -> {
                UnderlyingSocketBindingResult.STALE
            }

            else -> {
                UnderlyingSocketBindingResult.BOUND
            }
        }
}

/** Network.bindSocket preserves netd's errno in a SocketException cause. Only ENONET proves loss. */
internal fun isUnderlyingNetworkGone(error: Throwable): Boolean =
    hasNetworkBindingErrno(error, ANDROID_ENONET) { cause -> (cause as? ErrnoException)?.errno }

/** Kept platform-free so errno matching and malformed cause chains can be tested on the host. */
internal fun hasNetworkBindingErrno(
    error: Throwable,
    expectedErrno: Int,
    errnoOf: (Throwable) -> Int?,
): Boolean {
    val visited = IdentityHashMap<Throwable, Boolean>()
    var cause: Throwable? = error
    while (cause != null && visited.put(cause, true) == null) {
        val errno = errnoOf(cause)
        if (errno != null) return errno == expectedErrno
        cause = cause.cause
    }
    return false
}
