package io.github.georgexie2333.usque

/** UI policy kept independent of Android so Back ordering is host-testable. */
internal class TvInputPolicy(
    private val television: Boolean,
    private val host: Host,
) {
    interface Host {
        fun registerBack(sessionId: String?)

        fun hideKeyboard()

        fun editingEnded(
            sessionId: String,
            reason: String,
        )
    }

    companion object {
        fun isTelevision(
            televisionMode: Boolean,
            leanback: Boolean,
            fireTv: Boolean,
        ): Boolean = televisionMode || leanback || fireTv
    }

    private var active: String? = null
    private var sawIme = false
    private var backDownTime: Long? = null
    private var backSession: String? = null

    fun begin(sessionId: String): Boolean {
        if (!television) return false
        if (active == sessionId) return true
        finish("replaced")
        active = sessionId
        sawIme = false
        host.registerBack(sessionId)
        return true
    }

    fun end(sessionId: String) {
        if (active == sessionId) finish("released", notify = false)
    }

    fun systemBack(sessionId: String) {
        // A queued callback belongs to its original field, never the new one.
        if (active == sessionId) finish("back")
    }

    fun imeVisibility(visible: Boolean) {
        if (active == null) return
        if (visible) {
            sawIme = true
        } else if (sawIme) {
            finish("imeHidden")
        }
    }

    /** Consumes the entire physical gesture, including repeats after IME hide. */
    fun backKey(
        down: Boolean,
        downTime: Long,
        cancelled: Boolean = false,
    ): Boolean {
        if (down) {
            if (backDownTime == downTime) return true
            if (active == null) return false
            backDownTime = downTime
            backSession = active
            return true
        }
        if (backDownTime != downTime) return false
        backDownTime = null
        val session = backSession
        backSession = null
        if (!cancelled && active == session) finish("back")
        return true
    }

    fun stopped() {
        finish("lifecycle")
        backDownTime = null
        backSession = null
    }

    private fun finish(
        reason: String,
        notify: Boolean = true,
    ) {
        val sessionId = active ?: return
        active = null
        sawIme = false
        host.registerBack(null)
        host.hideKeyboard()
        if (notify) host.editingEnded(sessionId, reason)
    }
}
