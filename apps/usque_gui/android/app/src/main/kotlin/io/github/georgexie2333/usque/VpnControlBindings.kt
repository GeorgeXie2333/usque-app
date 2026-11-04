package io.github.georgexie2333.usque

import android.content.ComponentName
import android.content.ServiceConnection
import android.os.IBinder
import java.util.Collections
import java.util.IdentityHashMap

/** Observation never owns service creation; explicit work has a separate binding. */
internal class VpnControlBindings(
    private val bindService: (ServiceConnection, Boolean) -> Boolean,
    private val unbindService: (ServiceConnection) -> Unit,
    private val connected: (IBinder) -> Unit,
    private val unavailable: (unexpected: Boolean, mayRebind: Boolean) -> Unit,
) {
    private var observer: Connection? = null
    private var creator: Connection? = null
    private var currentBinder: IBinder? = null
    private val retiredBinders = Collections.newSetFromMap(IdentityHashMap<IBinder, Boolean>())

    val isBound: Boolean get() = observer != null || creator != null
    val createsService: Boolean get() = creator != null
    val hasEndpoint: Boolean get() = currentBinder != null

    fun observe() = bind(creating = false)

    fun retainService() = bind(creating = true)

    fun releaseObservation() = release(creating = false)

    fun releaseService() = release(creating = true)

    fun close() {
        releaseObservation()
        releaseService()
    }

    fun bindingDiedForTest() {
        (observer ?: creator)?.onBindingDied(null)
    }

    private fun active(connection: Connection): Boolean =
        if (connection.creating) creator === connection else observer === connection

    private fun bind(creating: Boolean) {
        if (if (creating) creator != null else observer != null) return
        val connection = Connection(creating)
        if (creating) creator = connection else observer = connection
        val accepted = runCatching { bindService(connection, creating) }.getOrDefault(false)
        if (!accepted && active(connection)) {
            // Android requires paired cleanup even when bindService returns false.
            release(creating)
        }
    }

    private fun release(
        creating: Boolean,
        unexpected: Boolean = false,
        mayRebind: Boolean = false,
    ) {
        val connection = (if (creating) creator else observer) ?: return
        if (creating) creator = null else observer = null
        if (!isBound) retiredBinders.clear()
        runCatching { unbindService(connection) }
        val replacement = creator?.binder ?: observer?.binder
        if (currentBinder !== replacement) {
            if (unexpected && isBound) currentBinder?.let { retiredBinders.add(it) }
            currentBinder = replacement
            if (replacement == null) unavailable(unexpected, mayRebind) else connected(replacement)
        } else if (mayRebind) {
            // The other binding can still own a usable endpoint.
            unavailable(false, true)
        }
    }

    private inner class Connection(
        val creating: Boolean,
    ) : ServiceConnection {
        var binder: IBinder? = null

        override fun onServiceConnected(
            name: ComponentName?,
            service: IBinder?,
        ) {
            if (!active(this)) return
            if (service == null) {
                onNullBinding(name)
                return
            }
            // Each ServiceConnection has its own Binder callback node. Its first
            // callback can arrive after the other connection replaced that actor.
            if (retiredBinders.contains(service)) return
            if (currentBinder != null && currentBinder !== service) {
                retiredBinders.add(requireNotNull(currentBinder))
                // Never fall back to the previous service when this binding ends.
                observer?.binder = null
                creator?.binder = null
            }
            binder = service
            currentBinder = service
            connected(service)
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            if (!active(this)) return
            val lost = binder
            binder = null
            if (lost != null && currentBinder === lost) {
                retiredBinders.add(lost)
                if (observer?.binder === lost) observer?.binder = null
                if (creator?.binder === lost) creator?.binder = null
                currentBinder = null
                unavailable(true, false)
            }
        }

        override fun onBindingDied(name: ComponentName?) {
            if (!active(this)) return
            release(creating, unexpected = true, mayRebind = true)
        }

        override fun onNullBinding(name: ComponentName?) {
            if (!active(this)) return
            release(creating)
            // Keep unsent request deadlines; do not spin on a null binding.
        }
    }
}
