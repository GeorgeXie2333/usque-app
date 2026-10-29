package io.github.georgexie2333.usque

/** Settings observation must precede native failure recovery, even without UI clients. */
internal class ServiceSnapshotPublisher(
    private val observeSettings: () -> Unit,
    private val publishSnapshot: () -> Unit,
) {
    /** Keep the publication callback so a later broadcast cannot observe the same snapshot twice. */
    fun observeNativeSnapshot(): () -> Unit {
        observeSettings()
        return publishSnapshot
    }

    fun broadcast() {
        observeNativeSnapshot().invoke()
    }
}
