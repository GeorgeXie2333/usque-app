package io.github.georgexie2333.usque

/** Main-thread owner of a chain rebuild; socket protection remains native's responsibility. */
internal class ChainNetworkRecovery(
    private val suspendSession: () -> Unit,
    private val stop: ((Boolean) -> Unit) -> Unit,
    private val schedule: (Long, () -> Unit) -> Unit,
    private val restart: () -> Unit,
    private val cleanupFailed: () -> Unit,
) {
    private var revision = 0L
    private var networkRevision = 0L
    private var online = false
    private var stopped = false
    private var connecting = false
    var active = false
        private set

    fun networkChanged(
        chainRunning: Boolean,
        networkPresent: Boolean,
    ): Boolean {
        if (!chainRunning && !active) return false
        online = networkPresent
        networkRevision++
        if (!active || connecting) {
            active = true
            stopped = false
            connecting = false
            val owner = ++revision
            // Invalidate snapshots and cut native ingress before notifying it
            // of the physical change that would otherwise terminate the chain.
            suspendSession()
            stop { confirmed ->
                if (!active || revision != owner) return@stop
                if (!confirmed) {
                    cancel()
                    cleanupFailed()
                } else {
                    stopped = true
                    scheduleRestart()
                }
            }
        } else {
            scheduleRestart()
        }
        return true
    }

    private fun scheduleRestart() {
        if (!stopped || !online || connecting) return
        val owner = revision
        val network = networkRevision
        schedule(250L) {
            if (active && stopped && online && !connecting && revision == owner && networkRevision == network) {
                connecting = true
                restart()
            }
        }
    }

    /** Manual connection, disconnect, terminal failure and destruction revoke all pending work. */
    fun cancel() {
        revision++
        active = false
        connecting = false
        stopped = false
    }
}
