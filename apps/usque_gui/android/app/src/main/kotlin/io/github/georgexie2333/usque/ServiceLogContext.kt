package io.github.georgexie2333.usque

/** Frozen correlation for a runtime; cleanup keeps it across service-generation changes. */
internal data class ServiceLogContext(
    val instanceId: String?,
    val connectionGeneration: Long,
    val networkGeneration: Long,
) {
    fun replacementRequest(
        connectionGeneration: Long,
        networkGeneration: Long,
    ): ServiceLogContext = ServiceLogContext(null, connectionGeneration, networkGeneration)
}
