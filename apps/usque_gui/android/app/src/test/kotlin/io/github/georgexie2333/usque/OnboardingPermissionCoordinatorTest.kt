package io.github.georgexie2333.usque

import io.flutter.plugin.common.MethodChannel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OnboardingPermissionCoordinatorTest {
    @Test
    fun preparationRequestsVpnThenOptionalNotificationWithoutConnecting() {
        val host = Host()
        val coordinator = OnboardingPermissionCoordinator(host)
        val result = Result()
        coordinator.prepare(result)
        assertEquals(listOf("vpn"), host.launches)
        host.vpn = true
        coordinator.finishVpn(true)
        assertEquals(listOf("vpn", "notification"), host.launches)
        coordinator.finishNotification()
        assertEquals(1, result.completions)
        assertEquals(mapOf("vpnGranted" to true, "notification" to "notGranted"), result.value)
        assertEquals(0, host.connections)
    }

    @Test
    fun deniedOrUnverifiedVpnNeverRequestsNotifications() {
        for (reportedGrant in listOf(false, true)) {
            val host = Host()
            val coordinator = OnboardingPermissionCoordinator(host)
            val result = Result()
            coordinator.prepare(result)
            coordinator.finishVpn(reportedGrant)
            assertEquals(false, (result.value as Map<*, *>)["vpnGranted"])
            assertEquals(listOf("vpn"), host.launches)
            assertEquals(1, result.completions)
        }
    }

    @Test
    fun preparedVpnAndPreviouslyDeniedNotificationDoNotPromptAgain() {
        val host =
            Host().apply {
                vpn = true
                requested = true
            }
        val coordinator = OnboardingPermissionCoordinator(host)
        coordinator.prepare(Result())
        coordinator.connect(Result(), true) { host.connections++ }
        assertTrue(host.launches.isEmpty())
        assertEquals(1, host.connections)
        host.vpn = false
        coordinator.connect(Result(), true) { host.connections++ }
        assertEquals(listOf("vpn"), host.launches)
        assertEquals(1, host.connections)
    }

    @Test
    fun cancellationKeepsOldDialogReservedUntilItsCallback() {
        val host = Host()
        val coordinator = OnboardingPermissionCoordinator(host)
        val first = Result()
        coordinator.connect(first, true) { host.connections++ }
        coordinator.cancelConnection("VPN_PERMISSION_CANCELLED", "cancelled")
        val second = Result()
        coordinator.prepare(second)
        assertEquals("VPN_PERMISSION_IN_PROGRESS", second.code)
        host.vpn = true
        coordinator.finishVpn(true)
        assertEquals(1, first.completions)
        assertEquals(0, host.connections)
        val third = Result()
        host.requested = true
        coordinator.prepare(third)
        coordinator.finishVpn(true)
        assertEquals(1, third.completions)
    }

    @Test
    fun notificationLaunchFailureAndDenialDoNotGateConnection() {
        val host =
            Host().apply {
                vpn = true
                failNotification = true
            }
        val coordinator = OnboardingPermissionCoordinator(host)
        coordinator.connect(Result(), true) { host.connections++ }
        assertEquals(1, host.connections)
        assertFalse(host.requested)
        host.failNotification = false
        coordinator.connect(Result(), true) { host.connections++ }
        coordinator.finishNotification()
        assertEquals(2, host.connections)
    }

    @Test
    fun destroyCompletesOnceAndLateCallbacksCannotConnect() {
        val host = Host()
        val coordinator = OnboardingPermissionCoordinator(host)
        val result = Result()
        coordinator.connect(result, true) { host.connections++ }
        coordinator.destroy()
        host.vpn = true
        coordinator.finishVpn(true)
        coordinator.destroy()
        assertEquals(1, result.completions)
        assertEquals(0, host.connections)
    }

    @Test
    fun recreatedActivityDrainsItsOldDialogBeforeAcceptingAnotherRequest() {
        val host = Host()
        val original = OnboardingPermissionCoordinator(host)
        original.prepare(Result())
        val saved = original.outstandingDialog()
        original.destroy()
        val restored = OnboardingPermissionCoordinator(host)
        restored.restoreOutstandingDialog(saved)
        val busy = Result()
        restored.prepare(busy)
        assertEquals("VPN_PERMISSION_IN_PROGRESS", busy.code)
        restored.finishVpn(false)
        restored.prepare(Result())
        assertEquals(listOf("vpn", "vpn"), host.launches)
    }

    @Test
    fun proxyOnlyConnectionDoesNotInspectOrRequestVpn() {
        val host =
            Host().apply {
                failVpnInspection = true
                requested = true
            }
        OnboardingPermissionCoordinator(host).connect(Result(), false) { host.connections++ }
        assertEquals(1, host.connections)
        assertTrue(host.launches.isEmpty())
    }

    private class Host : OnboardingPermissionCoordinator.Host {
        var failVpnInspection = false
        var vpn = false
        var requested = false
        var failNotification = false
        var connections = 0
        val launches = mutableListOf<String>()

        override fun vpnGranted(): Boolean {
            if (failVpnInspection) error("VPN inspection unavailable")
            return vpn
        }

        override fun notificationState(): String = if (requested) "notGranted" else "notRequested"

        override fun launchVpn() {
            launches.add("vpn")
        }

        override fun launchNotification() {
            if (failNotification) error("unavailable")
            requested = true
            launches.add("notification")
        }
    }

    private class Result : MethodChannel.Result {
        var completions = 0
        var value: Any? = null
        var code: String? = null

        override fun success(result: Any?) {
            completions++
            value = result
        }

        override fun error(
            errorCode: String,
            errorMessage: String?,
            errorDetails: Any?,
        ) {
            completions++
            code =
                errorCode
        }

        override fun notImplemented() {
            completions++
        }
    }
}
