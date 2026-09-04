package io.github.georgexie2333.usque

import android.os.Bundle
import android.os.Handler
import android.os.Message
import org.json.JSONObject
import java.util.concurrent.Executor
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

/** One bounded probe job, serialized with native connection lifecycle jobs. */
internal class ServiceDiagnosticProbes(
    private val service: UsqueVpnService,
    private val executor: Executor,
    private val handler: Handler,
    private val profile: () -> String?,
    private val loadSecret: (String, String) -> ByteArray?,
    private val runtimeBusy: () -> Boolean,
    private val vpnProtected: () -> Boolean,
) {
    private data class Pending(
        val id: Int,
        val cancelled: AtomicBoolean = AtomicBoolean(),
    )

    private val pending = AtomicReference<Pending?>()
    private var cachedProfile: String? = null
    private var cachedValid: Boolean? = null

    fun configuration(): Pair<String, String> {
        val json = profile() ?: return "unknown" to "unavailable"
        if (json.length > 256 * 1024) return "unknown" to "invalid"
        if (json != cachedProfile) {
            cachedProfile = json
            cachedValid = NativeEngine.validateDiagnosticProfile(json)
        }
        val mode =
            runCatching {
                JSONObject(json).optJSONObject("direct_dns")?.optString("mode") ?: "physicalSystem"
            }.getOrDefault("unknown")
        if (mode in setOf("doh", "dot") &&
            !NetworkQualityFields.capabilities(NativeEngine.capabilities()).getValue("encrypted_direct_dns")
        ) {
            return mode to "unsupported"
        }
        return (mode.takeIf { it in setOf("physicalSystem", "doh", "dot") } ?: "unknown") to
            when (cachedValid) {
                true -> "valid"
                false -> "invalid"
                null -> "unavailable"
            }
    }

    fun start(request: Message) {
        val id = request.arg1
        val kind = request.data.getString("probe_kind")
        val reply = request.replyTo ?: return

        fun respond(json: String?) {
            runCatching {
                reply.send(
                    Message.obtain(null, UsqueVpnService.MSG_DIAGNOSTIC_PROBE, id, 0).apply {
                        data = Bundle().apply { putString("probe_result", json ?: "{\"code\":\"not_applicable\"}") }
                    },
                )
            }
        }
        val json = profile()
        if (id <= 0 || kind !in setOf("dns", "h3") || json == null || json.length > 256 * 1024 ||
            (kind == "h3" && runtimeBusy())
        ) {
            respond(null)
            return
        }
        val operation = Pending(id)
        if (!pending.compareAndSet(null, operation)) {
            respond(null)
            return
        }
        if (!NativeEngine.prepareDiagnosticProbe(id)) {
            pending.compareAndSet(operation, null)
            respond(null)
            return
        }
        val timeout = Runnable { cancel(id) }
        handler.postDelayed(timeout, 3_900L)
        try {
            executor.execute {
                var secret = ByteArray(0)
                var result: String? = null
                try {
                    if (!operation.cancelled.get() && (kind != "h3" || !runtimeBusy())) {
                        if (kind == "h3") {
                            val profileId = JSONObject(json).optString("id")
                            secret = loadSecret(profileId, json) ?: ByteArray(0)
                        }
                        if (!operation.cancelled.get()) {
                            result =
                                NativeEngine.diagnosticProbe(
                                    id,
                                    requireNotNull(kind),
                                    json,
                                    secret,
                                    service,
                                    vpnProtected(),
                                )
                        }
                    }
                } catch (_: Exception) {
                    result = "{\"code\":\"failed\"}"
                } finally {
                    secret.fill(0)
                    NativeEngine.cancelDiagnosticProbe(id)
                    pending.compareAndSet(operation, null)
                    handler.removeCallbacks(timeout)
                }
                if (operation.cancelled.get()) result = "{\"code\":\"cancelled\"}"
                respond(result)
            }
        } catch (_: Exception) {
            NativeEngine.cancelDiagnosticProbe(id)
            pending.compareAndSet(operation, null)
            handler.removeCallbacks(timeout)
            respond("{\"code\":\"failed\"}")
        }
    }

    fun cancel(id: Int = 0) {
        pending.get()?.takeIf { id == 0 || it.id == id }?.let {
            it.cancelled.set(true)
            NativeEngine.cancelDiagnosticProbe(it.id)
        }
    }
}
