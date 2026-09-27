package io.github.georgexie2333.usque

internal data class NativeSessionSnapshot<T>(
    val generation: Long,
    val value: T,
)

/** Capture ownership before the active check and any potentially blocking JNI read. */
internal fun <T> readNativeSessionSnapshot(
    generation: () -> Long,
    active: () -> Boolean,
    read: () -> T?,
): NativeSessionSnapshot<T>? {
    val owner = generation()
    if (!active()) return null
    return read()?.let { NativeSessionSnapshot(owner, it) }
}
