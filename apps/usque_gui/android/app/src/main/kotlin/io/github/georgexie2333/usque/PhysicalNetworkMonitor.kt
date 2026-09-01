package io.github.georgexie2333.usque

import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Handler
import java.net.Inet4Address
import java.net.Inet6Address
import java.net.InetAddress
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

internal const val FAMILY_IPV4 = 0x1
internal const val FAMILY_IPV6 = 0x2

/**
 * Tracks non-VPN physical networks, schedules reselection, and owns the underlying-network
 * generation counter used by the Rust engine. Selection ranking is delegated to the pure
 * [choosePhysicalNetwork] helper.
 */
internal class PhysicalNetworkMonitor(
    private val mainHandler: Handler,
    private val listener: Listener,
    private val selectionDelayMillis: Long = 100L,
) {
    interface Listener {
        /**
         * Called only when the selected underlying network or family mask actually changes.
         * [generation] is the new network-restart generation after the bump.
         */
        fun onUnderlyingNetworkChanged(
            selectedNetwork: Network?,
            selectedFamilyMask: Int,
            generation: Long,
        )
    }

    private data class NetworkCandidate(
        val capabilities: NetworkCapabilities? = null,
        val linkProperties: LinkProperties? = null,
        val blocked: Boolean = false,
    )

    private val availableNetworks = ConcurrentHashMap<Network, NetworkCandidate>()
    private val underlyingNetwork = AtomicReference<Network?>(null)
    private val underlyingFamilyMask = AtomicInteger()
    private val underlyingDnsServers = AtomicReference<List<InetAddress>>(emptyList())
    private val networkRestartGeneration = NetworkRestartGeneration()
    private val generationNetworks = GenerationNetworkHistory<Network>()
    private val networkSelectionTask = Runnable(::selectUnderlyingNetwork)

    private val networkCallback =
        object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                availableNetworks.putIfAbsent(network, NetworkCandidate())
                scheduleSelection()
            }

            override fun onCapabilitiesChanged(
                network: Network,
                networkCapabilities: NetworkCapabilities,
            ) {
                availableNetworks.compute(network) { _, previous ->
                    (previous ?: NetworkCandidate()).copy(capabilities = networkCapabilities)
                }
                scheduleSelection()
            }

            override fun onLinkPropertiesChanged(
                network: Network,
                linkProperties: LinkProperties,
            ) {
                availableNetworks.compute(network) { _, previous ->
                    (previous ?: NetworkCandidate()).copy(linkProperties = linkProperties)
                }
                scheduleSelection()
            }

            override fun onBlockedStatusChanged(
                network: Network,
                blocked: Boolean,
            ) {
                availableNetworks.compute(network) { _, previous ->
                    (previous ?: NetworkCandidate()).copy(blocked = blocked)
                }
                scheduleSelection()
            }

            override fun onLost(network: Network) {
                availableNetworks.remove(network)
                scheduleSelection()
            }
        }

    fun register(connectivityManager: ConnectivityManager) {
        val request =
            NetworkRequest
                .Builder()
                .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)
                .build()
        connectivityManager.registerNetworkCallback(request, networkCallback)
    }

    fun unregister(connectivityManager: ConnectivityManager) {
        mainHandler.removeCallbacks(networkSelectionTask)
        try {
            connectivityManager.unregisterNetworkCallback(networkCallback)
        } catch (_: IllegalArgumentException) {
            // The callback may already have been revoked while the process exited.
        }
        generationNetworks.clear()
    }

    fun underlyingNetwork(): Network? = underlyingNetwork.get()

    fun underlyingFamilyMask(): Int = underlyingFamilyMask.get()

    fun underlyingDnsServers(): List<InetAddress> = underlyingDnsServers.get()

    fun generation(): Long = networkRestartGeneration.get()

    fun bumpGeneration(): Long {
        val generation = networkRestartGeneration.bump()
        generationNetworks.record(generation, underlyingNetwork.get())
        return generation
    }

    fun networkForGeneration(generation: Long): Network? = generationNetworks.get(generation)

    fun scheduleSelection() {
        mainHandler.removeCallbacks(networkSelectionTask)
        mainHandler.postDelayed(networkSelectionTask, selectionDelayMillis)
    }

    fun cancelScheduledSelection() {
        mainHandler.removeCallbacks(networkSelectionTask)
    }

    /**
     * Blocks until a usable underlying network is selected or [waitMillis] elapses.
     * Returns false if [isCurrent] becomes false while waiting.
     */
    fun awaitPhysicalNetwork(
        isCurrent: () -> Boolean,
        waitMillis: Long,
        requireDns: Boolean = false,
    ): Boolean {
        val deadline = System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(waitMillis)
        while (System.nanoTime() < deadline) {
            if (!isCurrent()) return false
            if (
                underlyingNetwork.get() != null &&
                (!requireDns || underlyingDnsServers.get().isNotEmpty())
            ) {
                return true
            }
            try {
                Thread.sleep(50)
            } catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
                return false
            }
        }
        return underlyingNetwork.get() != null &&
            (!requireDns || underlyingDnsServers.get().isNotEmpty()) &&
            isCurrent()
    }

    fun selectUnderlyingNetwork() {
        val candidates =
            availableNetworks.entries
                .mapNotNull { (network, candidate) ->
                    val capabilities = candidate.capabilities ?: return@mapNotNull null
                    if (
                        candidate.blocked ||
                        !capabilities.hasCapability(
                            NetworkCapabilities.NET_CAPABILITY_INTERNET,
                        ) ||
                        !capabilities.hasCapability(
                            NetworkCapabilities.NET_CAPABILITY_NOT_VPN,
                        )
                    ) {
                        return@mapNotNull null
                    }
                    val mask = familyMask(candidate.linkProperties)
                    if (mask == 0) return@mapNotNull null
                    network to
                        PhysicalNetworkCandidate(
                            handle = network.networkHandle,
                            score = networkScore(capabilities),
                            familyMask = mask,
                        )
                }
        val current = underlyingNetwork.get()
        val selection =
            choosePhysicalNetwork(
                currentHandle = current?.networkHandle,
                candidates = candidates.map { it.second },
            )
        val selectedNetwork =
            candidates
                .firstOrNull { it.second.handle == selection?.handle }
                ?.first
        val selectedFamilyMask = selection?.familyMask ?: 0
        val selectedDnsServers =
            selectedNetwork
                ?.let(availableNetworks::get)
                ?.linkProperties
                ?.dnsServers
                ?.distinctBy(::dnsServerKey)
                ?.sortedBy(::dnsServerKey)
                ?.take(8)
                ?: emptyList()
        val previousNetwork = underlyingNetwork.getAndSet(selectedNetwork)
        val previousFamilyMask = underlyingFamilyMask.getAndSet(selectedFamilyMask)
        val previousDnsServers = underlyingDnsServers.getAndSet(selectedDnsServers)
        // Single source of truth with unit tests: handle + family-mask comparison.
        val generation =
            networkRestartGeneration.bumpIfChanged(
                hasUnderlyingSelectionChanged(
                    previousHandle = previousNetwork?.networkHandle,
                    previousFamilyMask = previousFamilyMask,
                    selectedHandle = selectedNetwork?.networkHandle,
                    selectedFamilyMask = selectedFamilyMask,
                    previousDnsServers = previousDnsServers.map(::dnsServerKey),
                    selectedDnsServers = selectedDnsServers.map(::dnsServerKey),
                ),
            ) ?: return
        generationNetworks.record(generation, selectedNetwork)
        listener.onUnderlyingNetworkChanged(selectedNetwork, selectedFamilyMask, generation)
    }
}

/**
 * Returns true when the selected physical network handle or address-family mask changed.
 * Used by the monitor and covered by unit tests for generation bump conditions.
 */
internal fun hasUnderlyingSelectionChanged(
    previousHandle: Long?,
    previousFamilyMask: Int,
    selectedHandle: Long?,
    selectedFamilyMask: Int,
    previousDnsServers: List<String> = emptyList(),
    selectedDnsServers: List<String> = emptyList(),
): Boolean =
    previousHandle != selectedHandle ||
        previousFamilyMask != selectedFamilyMask ||
        previousDnsServers != selectedDnsServers

private fun dnsServerKey(address: InetAddress): String =
    when (address) {
        is Inet6Address -> "${address.hostAddress?.substringBefore('%')}|${address.scopeId}"
        else -> "${address.hostAddress}|0"
    }

internal fun familyMask(linkProperties: LinkProperties?): Int {
    if (linkProperties == null) return FAMILY_IPV4 or FAMILY_IPV6
    var mask = 0
    for (route in linkProperties.routes) {
        if (!route.isDefaultRoute) continue
        when (route.destination.address) {
            is Inet4Address -> mask = mask or FAMILY_IPV4
            is Inet6Address -> mask = mask or FAMILY_IPV6
        }
    }
    return mask
}

internal fun networkScore(capabilities: NetworkCapabilities): Int {
    var score =
        if (capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)) {
            100
        } else {
            0
        }
    score +=
        when {
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> 40
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> 30
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> 20
            else -> 10
        }
    return score
}

/**
 * Generation counter used when disconnect/clear paths force a transport rebuild
 * independent of ConnectivityManager callbacks. Covered by unit tests.
 */
internal class NetworkRestartGeneration(
    initial: Long = 0L,
) {
    private val value = AtomicLong(initial)

    fun get(): Long = value.get()

    fun bump(): Long = value.incrementAndGet()

    /**
     * Bumps and returns the new generation only when [changed] is true; otherwise null.
     */
    fun bumpIfChanged(changed: Boolean): Long? = if (changed) bump() else null
}

/** Keeps only the exact current and immediately previous generation binding. */
internal class GenerationNetworkHistory<T> {
    private val entries = ArrayDeque<Pair<Long, T?>>(2)

    @Synchronized
    fun record(
        generation: Long,
        value: T?,
    ) {
        val newest = maxOf(generation, entries.firstOrNull()?.first ?: generation)
        entries.removeAll { it.first == generation || it.first < newest - 1 }
        if (generation < newest - 1) return
        if (generation == newest) {
            entries.addFirst(generation to value)
        } else {
            entries.addLast(generation to value)
        }
        while (entries.size > 2) {
            entries.removeLast()
        }
    }

    @Synchronized
    fun get(generation: Long): T? = entries.firstOrNull { it.first == generation }?.second

    @Synchronized
    fun clear() {
        entries.clear()
    }

    @Synchronized
    internal fun retainedGenerations(): List<Long> = entries.map { it.first }
}
