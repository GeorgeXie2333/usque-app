package io.github.georgexie2333.usque

import android.content.Context
import android.net.Uri
import android.os.Build
import org.json.JSONObject
import java.io.IOException
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream

internal object AndroidMaintenance {
    private const val UPDATE_PREFERENCES = "usque_update_state_v1"
    private const val UPDATE_CHECKED_AT = "checked_at_unix_millis"
    private const val UPDATE_RESULT = "result_json"
    private const val UPDATE_INTERVAL_MILLIS = 24L * 60L * 60L * 1_000L
    private const val MAX_UPDATE_RESULT_BYTES = 16 * 1024
    private const val MAX_DIAGNOSTIC_LOG_BYTES = 2 * 1024 * 1024
    private const val RELEASE_URL_PREFIX =
        "https://github.com/GeorgeXie2333/usque-app/releases/"

    fun checkForUpdates(
        context: Context,
        manual: Boolean,
    ): Map<String, Any?> {
        val preferences =
            context.getSharedPreferences(UPDATE_PREFERENCES, Context.MODE_PRIVATE)
        val now = System.currentTimeMillis()
        if (!manual) {
            val checkedAt = preferences.getLong(UPDATE_CHECKED_AT, 0L)
            val cached = preferences.getString(UPDATE_RESULT, null)
            if (
                checkedAt in 1..now &&
                now - checkedAt < UPDATE_INTERVAL_MILLIS &&
                cached != null
            ) {
                return parseUpdateResult(cached)
            }
        }

        val response =
            NativeEngine.checkForUpdates()
                ?: throw IOException("The Rust update checker is unavailable.")
        if (response.toByteArray(Charsets.UTF_8).size > MAX_UPDATE_RESULT_BYTES) {
            throw IOException("The update result exceeded the Android safety limit.")
        }
        val parsed = parseUpdateResult(response)
        if (
            !preferences
                .edit()
                .putLong(UPDATE_CHECKED_AT, now)
                .putString(UPDATE_RESULT, response)
                .commit()
        ) {
            throw IOException("Android could not persist the update-check timestamp.")
        }
        return parsed
    }

    fun writeDiagnostics(
        context: Context,
        destination: Uri,
        snapshot: Map<String, Any?>,
    ) {
        val output =
            context.contentResolver.openOutputStream(destination, "wt")
                ?: throw IOException("The selected document provider returned no output stream.")
        output.use { stream ->
            ZipOutputStream(stream.buffered()).use { archive ->
                val logs =
                    AndroidLogStore(context).diagnosticSnapshot(MAX_DIAGNOSTIC_LOG_BYTES)
                val contents =
                    mutableListOf("manifest.json", "connection-summary.json", "README.txt").apply {
                        if (logs.isNotEmpty()) add("logs/android-engine.jsonl")
                    }
                val manifest =
                    JSONObject()
                        .put("schema_version", 1)
                        .put("created_at_unix_millis", System.currentTimeMillis())
                        .put(
                            "app_version",
                            context.packageManager
                                .getPackageInfo(context.packageName, 0)
                                .versionName,
                        ).put("platform", "android")
                        .put("sdk", Build.VERSION.SDK_INT)
                        .put("supported_abis", Build.SUPPORTED_ABIS.joinToString(","))
                        .put(
                            "contents",
                            contents,
                        ).put(
                            "excluded",
                            listOf(
                                "WARP Secret",
                                "private key",
                                "access token",
                                "device ID",
                                "license",
                                "endpoint pin",
                                "exit IP addresses",
                                "listener addresses",
                                "custom endpoint and DNS addresses",
                                "split-exclusion CIDRs",
                            ),
                        )
                val connection =
                    JSONObject()
                        .put("phase", snapshot["phase"])
                        .put("transport", snapshot["transport"])
                        .put("address_family", snapshot["address_family"])
                        .put(
                            "download_bytes_per_second",
                            snapshot["download_bytes_per_second"],
                        ).put("upload_bytes_per_second", snapshot["upload_bytes_per_second"])
                        .put("downloaded_bytes", snapshot["downloaded_bytes"])
                        .put("uploaded_bytes", snapshot["uploaded_bytes"])
                        .put("reconnect_count", snapshot["reconnect_count"])
                        .put("kill_switch_state", snapshot["kill_switch_state"])
                        .put("platform_lockdown", snapshot["platform_lockdown"])
                        .put("always_on", snapshot["always_on"])
                        .put(
                            "active_listener_count",
                            (snapshot["active_listeners"] as? List<*>)?.size ?: 0,
                        ).put("exit_ipv4_observed", snapshot["exit_ipv4"] != null)
                        .put("exit_ipv6_observed", snapshot["exit_ipv6"] != null)

                archive.writeEntry("manifest.json", manifest.toString(2))
                archive.writeEntry("connection-summary.json", connection.toString(2))
                if (logs.isNotEmpty()) {
                    archive.writeEntry("logs/android-engine.jsonl", logs)
                }
                archive.writeEntry(
                    "README.txt",
                    """
                    Usque diagnostic bundle

                    This archive was created locally and is never uploaded automatically.
                    Identity secrets, cryptographic material, full network addresses, and
                    user-provided profile names are deliberately excluded.
                    """.trimIndent() + "\n",
                )
            }
        }
    }

    fun clearLocalState(context: Context) {
        check(
            context
                .getSharedPreferences(UPDATE_PREFERENCES, Context.MODE_PRIVATE)
                .edit()
                .clear()
                .commit(),
        ) {
            "Android update state could not be cleared"
        }
        AndroidLogStore(context).clear()
        FlagSvgCache(context).clear()
    }

    internal fun parseUpdateResult(json: String): Map<String, Any?> {
        val value = JSONObject(json)
        val available = value.optBoolean("available", false)
        val version = value.optString("version").takeIf { it.length in 1..64 }
        val releaseUrl =
            value
                .optString("release_url")
                .takeIf { it.length <= 512 && it.startsWith(RELEASE_URL_PREFIX) }
        if (available && (version == null || releaseUrl == null)) {
            throw IOException("The Rust update checker returned an invalid release.")
        }
        return mapOf(
            "available" to available,
            "version" to version,
            "release_url" to releaseUrl,
        )
    }

    private fun ZipOutputStream.writeEntry(
        name: String,
        contents: String,
    ) {
        putNextEntry(ZipEntry(name).apply { time = 0L })
        write(contents.toByteArray(Charsets.UTF_8))
        closeEntry()
    }
}
