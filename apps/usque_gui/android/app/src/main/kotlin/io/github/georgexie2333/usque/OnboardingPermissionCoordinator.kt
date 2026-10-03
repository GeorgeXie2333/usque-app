package io.github.georgexie2333.usque

import io.flutter.plugin.common.MethodChannel

/** Serializes system dialogs; preparing permission never supplies a connection continuation. */
internal class OnboardingPermissionCoordinator(
    private val host: Host,
) {
    interface Host {
        fun vpnGranted(): Boolean

        fun notificationState(): String

        fun launchVpn()

        fun launchNotification()
    }

    private enum class Dialog { NONE, VPN, NOTIFICATION }

    private data class Pending(
        val result: MethodChannel.Result,
        val requireVpn: Boolean,
        val connect: (() -> Unit)?,
    )

    private var pending: Pending? = null
    private var dialog = Dialog.NONE
    private var destroyed = false

    fun outstandingDialog(): String? = dialog.takeIf { it != Dialog.NONE }?.name

    fun restoreOutstandingDialog(value: String?) {
        if (pending == null && dialog == Dialog.NONE) {
            dialog = Dialog.entries.firstOrNull { it.name == value } ?: Dialog.NONE
        }
    }

    fun status(): Map<String, Any> = mapOf("vpnGranted" to host.vpnGranted(), "notification" to notificationState())

    private fun notificationState(): String = runCatching { host.notificationState() }.getOrDefault("notGranted")

    fun prepare(result: MethodChannel.Result) = begin(result, true, null)

    fun connect(
        result: MethodChannel.Result,
        requireVpn: Boolean,
        continuation: () -> Unit,
    ) = begin(result, requireVpn, continuation)

    private fun begin(
        result: MethodChannel.Result,
        requireVpn: Boolean,
        continuation: (() -> Unit)?,
    ) {
        if (destroyed) {
            result.error("VPN_PERMISSION_CANCELLED", "The Android UI closed.", null)
            return
        }
        if (pending != null || dialog != Dialog.NONE) {
            result.error("VPN_PERMISSION_IN_PROGRESS", "Another permission request is in progress.", null)
            return
        }
        pending = Pending(result, requireVpn, continuation)
        try {
            if (requireVpn && !host.vpnGranted()) {
                dialog = Dialog.VPN
                host.launchVpn()
            } else {
                requestNotification()
            }
        } catch (_: Exception) {
            dialog = Dialog.NONE
            val request = pending.also { pending = null }
            request?.result?.error("VPN_PERMISSION_LAUNCH_FAILED", "Android could not request VPN permission.", null)
        }
    }

    fun finishVpn(granted: Boolean) {
        if (dialog != Dialog.VPN || destroyed) return
        dialog = Dialog.NONE
        if (pending == null) return
        try {
            if (!granted || !host.vpnGranted()) {
                complete(vpnDenied = true)
            } else {
                requestNotification()
            }
        } catch (_: Exception) {
            val request = pending.also { pending = null }
            request?.result?.error("VPN_PERMISSION_LAUNCH_FAILED", "Android could not verify VPN permission.", null)
        }
    }

    private fun requestNotification() {
        if (pending == null) return
        if (notificationState() == "notRequested") {
            dialog = Dialog.NOTIFICATION
            try {
                host.launchNotification()
            } catch (_: Exception) {
                dialog = Dialog.NONE
                complete()
            }
        } else {
            complete()
        }
    }

    fun finishNotification() {
        if (dialog != Dialog.NOTIFICATION || destroyed) return
        dialog = Dialog.NONE
        complete()
    }

    private fun complete(vpnDenied: Boolean = false) {
        val request = pending.also { pending = null } ?: return
        if (request.connect != null && !request.requireVpn) {
            request.connect.invoke()
            return
        }
        try {
            val value = status().toMutableMap()
            if (vpnDenied) value["vpnGranted"] = false
            if (request.connect == null) {
                request.result.success(value)
            } else if (request.requireVpn && value["vpnGranted"] != true) {
                request.result.error("VPN_PERMISSION_DENIED", "VPN permission was not granted.", null)
            } else {
                request.connect.invoke()
            }
        } catch (_: Exception) {
            request.result.error("VPN_PERMISSION_LAUNCH_FAILED", "Android could not verify permissions.", null)
        }
    }

    fun cancelConnection(
        code: String,
        message: String,
    ) {
        if (pending?.connect == null) return
        cancel(code, message)
    }

    private fun cancel(
        code: String,
        message: String,
    ) {
        val request = pending.also { pending = null } ?: return
        // Keep the dialog occupied: ActivityResult callbacks cannot identify an old request.
        request.result.error(code, message, null)
    }

    fun destroy() {
        destroyed = true
        cancel("VPN_PERMISSION_CANCELLED", "The Android UI closed before permissions were granted.")
    }
}
