package io.github.georgexie2333.usque

/** Error retained after a failed connection uses the full disconnect path. */
internal data class ConnectionFailure(
    val code: String,
    val message: String,
    val gateStatus: String? = null,
    val details: ServiceSnapshotState.FailureFields? = null,
)
