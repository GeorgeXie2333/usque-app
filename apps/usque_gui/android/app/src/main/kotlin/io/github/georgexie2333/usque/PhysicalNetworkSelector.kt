package io.github.georgexie2333.usque

internal data class PhysicalNetworkCandidate(
    val handle: Long,
    val score: Int,
    val familyMask: Int,
)

internal data class PhysicalNetworkSelection(
    val handle: Long,
    val familyMask: Int,
)

/**
 * Chooses one non-VPN physical network deterministically. An equally-ranked
 * current network remains selected so capability callback noise cannot create
 * redundant Rust transport generations.
 */
internal fun choosePhysicalNetwork(
    currentHandle: Long?,
    candidates: List<PhysicalNetworkCandidate>,
): PhysicalNetworkSelection? {
    val usable = candidates.filter { it.familyMask != 0 }
    val maximumScore = usable.maxOfOrNull(PhysicalNetworkCandidate::score) ?: return null
    val selected =
        usable.firstOrNull { it.handle == currentHandle && it.score == maximumScore }
            ?: usable
                .asSequence()
                .filter { it.score == maximumScore }
                .minByOrNull(PhysicalNetworkCandidate::handle)
            ?: return null
    return PhysicalNetworkSelection(selected.handle, selected.familyMask)
}
