package io.github.georgexie2333.usque

import org.json.JSONObject

/** Error retained after a failed connection uses the full disconnect path. */
internal data class ConnectionFailure(
    val code: String,
    val message: String,
    val gateStatus: String? = null,
    val details: ServiceSnapshotState.FailureFields? = null,
) {
    companion object {
        /**
         * Resolves a failed JNI start/reconfigure/attach code against the native snapshot.
         * Only adopt structured error fields when native actually entered `error`; a live
         * `reconnecting` or `connected` snapshot carries transient status that must never
         * replace a local command rejection such as [NativeEngine.ERROR_ALREADY_RUNNING].
         */
        fun fromNativeStartResult(
            result: Int,
            nativeSnapshot: JSONObject?,
        ): ConnectionFailure {
            val errorSnapshot = nativeSnapshot?.takeIf { it.optString("phase") == "error" }
            val structuredCode = errorSnapshot?.optNullableString("error_code")
            val structuredMessage = errorSnapshot?.optNullableString("warning")
            val fallback =
                when (result) {
                    NativeEngine.ERROR_INVALID_WARP_SECRET -> {
                        ConnectionFailure(
                            "IDENTITY_INVALID",
                            "The stored WARP identity was rejected.",
                        )
                    }

                    NativeEngine.ERROR_ALREADY_RUNNING -> {
                        ConnectionFailure(
                            "ANDROID_RUNTIME_FAILED",
                            "Another native data channel is already running.",
                        )
                    }

                    NativeEngine.ERROR_INVALID_PROFILE -> {
                        ConnectionFailure(
                            "ANDROID_RUNTIME_FAILED",
                            "The Rust engine rejected this profile.",
                        )
                    }

                    NativeEngine.ERROR_PLATFORM_FAILURE -> {
                        ConnectionFailure(
                            "ANDROID_RUNTIME_FAILED",
                            "Android could not initialize the native runtime.",
                        )
                    }

                    NativeEngine.ERROR_TRANSPORT_FAILURE -> {
                        ConnectionFailure(
                            "MASQUE_CONNECT_FAILED",
                            "The MASQUE endpoint could not be reached with HTTP/3 or HTTP/2.",
                        )
                    }

                    NativeEngine.ERROR_TUN_FAILURE -> {
                        ConnectionFailure(
                            "ANDROID_RUNTIME_FAILED",
                            "The Rust engine could not own the VPN interface.",
                        )
                    }

                    else -> {
                        ConnectionFailure(
                            "ANDROID_RUNTIME_FAILED",
                            "The native engine rejected the network request ($result).",
                        )
                    }
                }
            return ConnectionFailure(
                code = structuredCode?.take(64) ?: fallback.code,
                message = structuredMessage?.take(512) ?: fallback.message,
                gateStatus =
                    VpnGateFields.status(errorSnapshot?.optJSONObject("vpn_gate"))?.let {
                        JSONObject(it).toString()
                    },
                details = errorSnapshot?.let { ServiceSnapshotState.fromNativeJson(it).failure },
            )
        }
    }
}
