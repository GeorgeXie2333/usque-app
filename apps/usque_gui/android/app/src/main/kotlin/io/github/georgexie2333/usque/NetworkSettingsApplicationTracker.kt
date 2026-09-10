package io.github.georgexie2333.usque

/** Main-thread ownership of a save's runtime application, not its durable commit. */
internal class NetworkSettingsApplicationTracker {
    enum class Phase {
        IDLE,
        PERSISTING,
        RECONFIGURING,
        AWAITING_OBSERVATION,
    }

    data class Application(
        val token: Long,
        val generation: Long,
        val phase: Phase,
        val operationId: String? = null,
    )

    private var nextToken = 0L
    private var active: Application? = null

    val phase: Phase
        get() = active?.phase ?: Phase.IDLE

    val busy: Boolean
        get() = active != null

    fun begin(generation: Long): Long {
        check(!busy)
        val token = ++nextToken
        active = Application(token, generation, Phase.PERSISTING)
        return token
    }

    fun current(generation: Long): Application? = active?.takeIf { it.generation == generation }

    fun owns(
        token: Long,
        generation: Long,
    ): Boolean = current(generation)?.token == token

    // A completed operation may publish its result, but never after a new
    // reservation or cancellation has invalidated its token.
    fun isLatest(token: Long): Boolean = token == nextToken

    fun committed(
        token: Long,
        generation: Long,
        operationId: String,
    ): Boolean {
        val application = current(generation) ?: return false
        if (application.token != token || application.phase != Phase.PERSISTING) return false
        active = application.copy(phase = Phase.RECONFIGURING, operationId = operationId)
        return true
    }

    fun runtimeReplied(
        token: Long,
        generation: Long,
    ): Application? {
        val application = current(generation) ?: return null
        if (application.token != token || application.phase != Phase.RECONFIGURING) return null
        return application.copy(phase = Phase.AWAITING_OBSERVATION).also { active = it }
    }

    fun allowsObservation(generation: Long): Boolean {
        val application = active ?: return true
        return application.generation == generation && application.phase == Phase.AWAITING_OBSERVATION
    }

    fun migrateSession(
        token: Long,
        previousGeneration: Long,
        generation: Long,
    ): Boolean {
        val application = current(previousGeneration) ?: return false
        if (application.token != token || application.phase != Phase.RECONFIGURING) return false
        active = application.copy(generation = generation)
        return true
    }

    fun finish(
        token: Long,
        generation: Long,
    ): Application? {
        val application = current(generation) ?: return null
        if (application.token != token) return null
        active = null
        return application
    }

    fun cancelIfStale(generation: Long) {
        if (active?.let { it.generation != generation } == true) cancel()
    }

    fun cancel() {
        active = null
        nextToken++
    }
}
