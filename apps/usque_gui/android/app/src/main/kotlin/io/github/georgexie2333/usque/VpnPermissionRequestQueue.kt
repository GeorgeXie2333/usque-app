package io.github.georgexie2333.usque

import io.flutter.plugin.common.MethodChannel

internal class VpnPermissionRequestQueue {
    data class Pending(
        val profileJson: String,
        val result: MethodChannel.Result,
    )

    private var pending: Pending? = null

    val hasPending: Boolean
        get() = pending != null

    fun offer(
        profileJson: String,
        result: MethodChannel.Result,
    ): Boolean {
        if (pending != null) return false
        pending = Pending(profileJson, result)
        return true
    }

    fun take(): Pending? = pending.also { pending = null }

    fun cancel(
        code: String,
        message: String,
    ): Boolean {
        val request = take() ?: return false
        request.result.error(code, message, null)
        return true
    }
}
