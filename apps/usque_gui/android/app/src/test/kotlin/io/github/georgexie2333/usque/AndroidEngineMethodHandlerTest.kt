package io.github.georgexie2333.usque

import android.content.ServiceConnection
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.util.concurrent.Executor

class AndroidEngineMethodHandlerTest {
    private lateinit var scheduler: ImmediateScheduler
    private lateinit var controlClient: VpnControlClient
    private lateinit var endpoint: RecordingEndpoint
    private lateinit var activityCommands: RecordingActivityCommands
    private lateinit var engineBridge: FakeEngineBridge
    private lateinit var handler: AndroidEngineMethodHandler

    @Before
    fun setUp() {
        scheduler = ImmediateScheduler()
        endpoint = RecordingEndpoint()
        controlClient =
            VpnControlClient(
                scheduler = scheduler,
                serviceBinder = { _: ServiceConnection -> true },
                serviceUnbinder = { },
                endpointFromBinder = { _, _ -> error("unused") },
            )
        controlClient.attachEndpointForTest(endpoint)
        activityCommands = RecordingActivityCommands()
        engineBridge = FakeEngineBridge()
        handler =
            AndroidEngineMethodHandler(
                profileConfigPath = "/tmp/profiles-v2.json",
                identityStore = NoopIdentityStore(),
                identityExecutor = Executor { it.run() },
                mainScheduler = scheduler,
                controlClient = controlClient,
                activityCommands = activityCommands,
                engineBridge = engineBridge,
                maintenanceBridge =
                    object : AndroidEngineMethodHandler.MaintenanceBridge {
                        override fun checkForUpdates(manual: Boolean): Map<String, Any?> = mapOf("manual" to manual)

                        override fun clearLocalState() = Unit
                    },
                warpSecretOkCode = 0,
            )
    }

    @Test
    fun dispatchesSnapshotToControlClient() {
        val result = RecordingResult()
        handler.handle(MethodCall("snapshot", null), result)
        assertEquals(listOf(UsqueVpnService.MSG_SNAPSHOT), endpoint.whats)
        assertNull(result.errorCode)
    }

    @Test
    fun dispatchesDisconnectAndCancelsVpnPermission() {
        val result = RecordingResult()
        handler.handle(MethodCall("disconnect", null), result)
        assertEquals(1, activityCommands.cancelCount)
        assertEquals("VPN_PERMISSION_CANCELLED", activityCommands.lastCancelCode)
        assertEquals(listOf(UsqueVpnService.MSG_DISCONNECT), endpoint.whats)
    }

    @Test
    fun dispatchesPauseCaptivePortal() {
        val result = RecordingResult()
        handler.handle(MethodCall("pauseCaptivePortal", mapOf("seconds" to 120)), result)
        assertEquals(listOf(UsqueVpnService.MSG_PAUSE_CAPTIVE_PORTAL), endpoint.whats)
        assertEquals(120, endpoint.lastExtras?.get("seconds"))
    }

    @Test
    fun rejectsInvalidPauseCaptivePortalRange() {
        val result = RecordingResult()
        handler.handle(MethodCall("pauseCaptivePortal", mapOf("seconds" to 0)), result)
        assertEquals("INVALID_ARGUMENT", result.errorCode)
        assertTrue(endpoint.whats.isEmpty())
    }

    @Test
    fun clearAllDataRequiresConfirmation() {
        val result = RecordingResult()
        handler.handle(MethodCall("clearAllData", mapOf("confirmed" to false)), result)
        assertEquals("CONFIRMATION_REQUIRED", result.errorCode)
        assertTrue(endpoint.whats.isEmpty())
    }

    @Test
    fun clearAllDataDispatchesWhenConfirmed() {
        val result = RecordingResult()
        handler.handle(MethodCall("clearAllData", mapOf("confirmed" to true)), result)
        assertEquals(1, activityCommands.cancelCount)
        assertEquals(listOf(UsqueVpnService.MSG_CLEAR_ALL_DATA), endpoint.whats)
        assertEquals(true, endpoint.lastExtras?.get("confirmed"))
    }

    @Test
    fun connectValidatesEngineReadyAndMode() {
        engineBridge.ready = false
        val unavailable = RecordingResult()
        handler.handle(
            MethodCall("connect", mapOf("mode" to "vpn", "id" to "p1")),
            unavailable,
        )
        assertEquals("ENGINE_UNAVAILABLE", unavailable.errorCode)

        engineBridge.ready = true
        val badMode = RecordingResult()
        handler.handle(MethodCall("connect", mapOf("mode" to "wireguard")), badMode)
        assertEquals("INVALID_PROFILE", badMode.errorCode)

        val ok = RecordingResult()
        handler.handle(
            MethodCall("connect", mapOf("mode" to "socks5", "id" to "p1")),
            ok,
        )
        assertEquals(1, activityCommands.connectCount)
        assertEquals("socks5", activityCommands.lastMode)
        assertTrue(activityCommands.lastProfileJson!!.contains("socks5"))
    }

    @Test
    fun unknownMethodIsNotImplemented() {
        val result = RecordingResult()
        handler.handle(MethodCall("noSuchMethod", null), result)
        assertTrue(result.notImplementedCalled)
        assertEquals(1, result.completionCount)
    }

    @Test
    fun exportDiagnosticsDelegatesToActivity() {
        val result = RecordingResult()
        handler.handle(MethodCall("exportDiagnostics", null), result)
        assertEquals(1, activityCommands.diagnosticsCount)
    }

    @Test
    fun provisionIdentityRequiresTerms() {
        val result = RecordingResult()
        handler.handle(MethodCall("provisionIdentity", mapOf("terms_accepted" to false)), result)
        assertEquals("TERMS_NOT_ACCEPTED", result.errorCode)
    }

    private class RecordingActivityCommands : AndroidEngineMethodHandler.ActivityCommands {
        var cancelCount = 0
        var lastCancelCode: String? = null
        var connectCount = 0
        var lastProfileJson: String? = null
        var lastMode: String? = null
        var diagnosticsCount = 0

        override fun cancelPendingVpnConnection(
            code: String,
            message: String,
        ) {
            cancelCount += 1
            lastCancelCode = code
        }

        override fun connectAfterValidation(
            profileJson: String,
            mode: String,
            result: MethodChannel.Result,
        ) {
            connectCount += 1
            lastProfileJson = profileJson
            lastMode = mode
            result.success(mapOf("phase" to "preparing"))
        }

        override fun selectDiagnosticsDestination(result: MethodChannel.Result) {
            diagnosticsCount += 1
            result.success(null)
        }
    }

    private class FakeEngineBridge : AndroidEngineMethodHandler.EngineBridge {
        var ready = true
        var linked = true

        override fun isLinked(): Boolean = linked

        override fun isReady(): Boolean = ready

        override fun applyProfileCommand(
            configPath: String,
            requestJson: String,
        ): String? = """{"profiles":[]}"""

        override fun registerConsumerWarp(locale: String): ByteArray? = byteArrayOf(1)

        override fun validateWarpSecret(secret: ByteArray): Int = 0
    }

    private class NoopIdentityStore : AndroidEngineMethodHandler.IdentityStore {
        override fun put(
            profileId: String,
            record: SecureIdentityStore.Record,
            value: ByteArray,
        ) = Unit

        override fun get(
            profileId: String,
            record: SecureIdentityStore.Record,
        ): ByteArray? = null

        override fun deleteIdentity(profileId: String) = Unit

        override fun clearAll() = Unit
    }

    private class RecordingEndpoint : VpnControlClient.ControlEndpoint {
        val whats = mutableListOf<Int>()
        var lastExtras: Map<String, Any?>? = null

        override fun send(
            what: Int,
            requestId: Int,
            extras: Map<String, Any?>?,
        ): Boolean {
            whats.add(what)
            lastExtras = extras
            return true
        }
    }

    private class ImmediateScheduler : VpnControlClient.MainScheduler {
        override fun post(action: () -> Unit) = action()

        override fun postDelayed(
            delayMillis: Long,
            token: Any,
            action: () -> Unit,
        ) = Unit

        override fun cancel(token: Any) = Unit
    }

    private class RecordingResult : MethodChannel.Result {
        var completionCount = 0
        var successValue: Any? = null
        var errorCode: String? = null
        var notImplementedCalled = false

        override fun success(result: Any?) {
            completionCount += 1
            successValue = result
        }

        override fun error(
            errorCode: String,
            errorMessage: String?,
            errorDetails: Any?,
        ) {
            completionCount += 1
            this.errorCode = errorCode
        }

        override fun notImplemented() {
            completionCount += 1
            notImplementedCalled = true
        }
    }
}
