package io.github.georgexie2333.usque

/** Tracks JNI calls before the upstream suspend API dispatches disk work. */
internal class TrackedPreferencesWork(
    private val holdWork: () -> AutoCloseable = UiProcessReclaimer::holdBackgroundWork,
) {
    private var accepting = true

    @Synchronized
    private fun acquire(): AutoCloseable {
        check(accepting) { "Preferences engine has detached" }
        return holdWork()
    }

    suspend fun <T> run(operation: suspend () -> T): T = acquire().use { operation() }

    @Synchronized
    fun stopAccepting() {
        // Already accepted calls retain their leases through suspension and
        // completion. Cached JNI registrars cannot start new work after detach.
        accepting = false
    }
}
