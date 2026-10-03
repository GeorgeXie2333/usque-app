package io.github.georgexie2333.usque

import org.json.JSONObject
import java.nio.channels.FileChannel
import java.nio.channels.OverlappingFileLockException
import java.nio.file.Files
import java.nio.file.Paths
import java.nio.file.StandardOpenOption
import java.util.UUID

/** Durable operation journal plus a process-independent lease, separate from account replacement. */
internal class InitialIdentityCoordinator(
    private val command: (JSONObject) -> JSONObject,
    private val lease: () -> AutoCloseable?,
    private val ready: (JSONObject, String) -> Boolean,
    private val hasMaterial: (String) -> Boolean,
    private val recover: (Request, JSONObject, (JSONObject) -> Unit) -> Boolean = { _, _, _ -> false },
) {
    data class Request(
        val operationId: String,
        val profileId: String,
        val method: String,
        val organization: String?,
        val resumeOnly: Boolean = false,
    ) {
        fun json(command: String): JSONObject =
            JSONObject()
                .put("command", command)
                .put("operation_id", operationId)
                .put("profile_id", profileId)
                .put("method", method)
                .put("organization", organization)
    }

    class Failure(
        val code: String,
        val uncertain: Boolean = false,
    ) : Exception(code)

    fun status(profileId: String): Map<String, Any?> {
        val held = lease()
        return try {
            val catalog = read(profileId)
            state(catalog, profileId, held == null)
        } finally {
            held?.close()
        }
    }

    fun initialize(
        request: Request,
        register: ((JSONObject) -> Unit) -> Unit,
    ): Map<String, Any?> {
        val held = lease() ?: return state(read(request.profileId), request.profileId, true)
        held.use {
            var catalog = read(request.profileId)
            if (catalog.optString("active_profile_id") != request.profileId) {
                throw Failure("INITIAL_IDENTITY_PROFILE_CHANGED")
            }
            val journal = catalog.optJSONObject("initial_identity_operation")
            var committed = false
            val commit: (JSONObject) -> Unit = { completion ->
                val finish = request.json("finish_initial_identity").put("phase", "completed")
                for (key in listOf("endpoint_ipv4", "endpoint_ipv6")) {
                    if (completion.has(key)) finish.put(key, completion.getString(key))
                }
                try {
                    command(finish)
                    committed = true
                } catch (_: Exception) {
                    val readback =
                        try {
                            read(request.profileId)
                        } catch (_: Exception) {
                            throw Failure("INITIAL_IDENTITY_COMMIT_FAILED", uncertain = true)
                        }
                    val operation = readback.optJSONObject("initial_identity_operation")
                    if (operation?.optString("operation_id") == request.operationId &&
                        operation.optString("profile_id") == request.profileId &&
                        operation.optString("phase") == "completed" && ready(readback, request.profileId)
                    ) {
                        committed = true
                    } else {
                        throw Failure("INITIAL_IDENTITY_COMMIT_FAILED")
                    }
                }
            }
            try {
                if (recover(request, catalog, commit)) {
                    return result(request.operationId, request.profileId, "completed", reused = true)
                }
            } catch (error: Failure) {
                if (error.uncertain) return result(request.operationId, request.profileId, "pending", error.code)
                throw error
            }
            if (ready(catalog, request.profileId)) {
                val profiles = catalog.optJSONArray("profiles")
                val profile =
                    profiles?.let { values ->
                        (0 until values.length())
                            .map { values.getJSONObject(it) }
                            .firstOrNull { it.optString("id") == request.profileId }
                    }
                if (journal?.optString("phase") == "pending" &&
                    profile?.optString("identity_provider").isNullOrBlank() &&
                    journal.optString("profile_id") == request.profileId
                ) {
                    val finish =
                        JSONObject()
                            .put("command", "finish_initial_identity")
                            .put("operation_id", journal.getString("operation_id"))
                            .put("profile_id", request.profileId)
                            .put("phase", "completed")
                    if (journal.optString("method") == "zeroTrust") {
                        finish
                            .put("endpoint_ipv4", profile!!.getString("endpoint_v4"))
                            .put("endpoint_ipv6", profile.getString("endpoint_v6"))
                    }
                    command(finish)
                }
                return result(request.operationId, request.profileId, "completed", reused = true)
            }
            if (request.resumeOnly) return state(catalog, request.profileId, false)
            if (hasMaterial(request.profileId)) {
                return result(request.operationId, request.profileId, "failed", "INITIAL_IDENTITY_REPAIR_REQUIRED")
            }
            if (journal?.optString("operation_id") == request.operationId) {
                return state(catalog, request.profileId, false)
            }
            if (journal?.optString("phase") == "pending") {
                command(
                    JSONObject()
                        .put("command", "finish_initial_identity")
                        .put("operation_id", journal.getString("operation_id"))
                        .put("profile_id", journal.getString("profile_id"))
                        .put("phase", "interrupted"),
                )
            }
            catalog = command(request.json("begin_initial_identity").put("owner_epoch", UUID.randomUUID().toString()))
            val started =
                catalog.optJSONObject("initial_identity_operation")
                    ?: throw Failure("INITIAL_IDENTITY_STATE_FAILED")
            if (started.optString("operation_id") != request.operationId || started.optString("phase") != "pending") {
                throw Failure("INITIAL_IDENTITY_STATE_FAILED")
            }
            try {
                register(commit)
                if (!committed) throw Failure("INITIAL_IDENTITY_COMMIT_FAILED", uncertain = true)
                return result(request.operationId, request.profileId, "completed")
            } catch (error: Exception) {
                if ((error as? Failure)?.uncertain == true) {
                    return result(request.operationId, request.profileId, "pending", "INITIAL_IDENTITY_COMMIT_FAILED")
                }
                val code =
                    (error as? Failure)?.code
                        ?: "REGISTRATION_FAILED"
                val journalCode =
                    if (code in
                        setOf("INITIAL_IDENTITY_REPAIR_REQUIRED", "INITIAL_IDENTITY_COMMIT_FAILED")
                    ) {
                        code
                    } else {
                        "REGISTRATION_FAILED"
                    }
                runCatching {
                    val current = read(request.profileId).optJSONObject("initial_identity_operation")
                    if (current?.optString("operation_id") == request.operationId &&
                        current.optString("phase") == "pending"
                    ) {
                        command(
                            request
                                .json(
                                    "finish_initial_identity",
                                ).put("phase", "failed")
                                .put("error_code", journalCode),
                        )
                    }
                }
                return result(request.operationId, request.profileId, "failed", code)
            }
        }
    }

    private fun read(profileId: String): JSONObject =
        command(JSONObject().put("command", "get_initial_identity_state").put("profile_id", profileId))

    private fun state(
        catalog: JSONObject,
        profileId: String,
        busy: Boolean,
    ): Map<String, Any?> {
        val journal =
            catalog
                .optJSONObject("initial_identity_operation")
                ?.takeIf { it.optString("profile_id") == profileId }
        if (busy) return result(journal?.optString("operation_id"), profileId, "pending")
        if (ready(catalog, profileId)) {
            return result(journal?.optString("operation_id"), profileId, "completed", reused = true)
        }
        if (journal?.optString("phase") == "pending") {
            return result(journal.optString("operation_id"), profileId, "interrupted", "INITIAL_IDENTITY_INTERRUPTED")
        }
        if (journal?.optString("phase") == "completed" || hasMaterial(profileId)) {
            return result(journal?.optString("operation_id"), profileId, "failed", "INITIAL_IDENTITY_REPAIR_REQUIRED")
        }
        return result(
            journal?.optString("operation_id"),
            profileId,
            journal?.optString("phase") ?: "idle",
            journal?.optString("error_code")?.takeIf { it.isNotBlank() && it != "null" },
        )
    }

    private fun result(
        operationId: String?,
        profileId: String,
        phase: String,
        errorCode: String? = null,
        reused: Boolean = false,
    ): Map<String, Any?> =
        mapOf(
            "operation_id" to operationId,
            "profile_id" to profileId,
            "phase" to phase,
            "error_code" to errorCode,
            "reused" to reused,
        )

    companion object {
        fun acquire(configPath: String): AutoCloseable? {
            val config = Paths.get(configPath).toAbsolutePath()
            val name =
                config.fileName.toString().substringBeforeLast('.', config.fileName.toString()) +
                    ".initial-identity.lock"
            val path = config.resolveSibling(name)
            Files.createDirectories(path.parent)
            val channel = FileChannel.open(path, StandardOpenOption.CREATE, StandardOpenOption.WRITE)
            try {
                val lock =
                    try {
                        channel.tryLock()
                    } catch (_: OverlappingFileLockException) {
                        null
                    }
                if (lock == null) {
                    channel.close()
                    return null
                }
                return AutoCloseable {
                    try {
                        lock.release()
                    } finally {
                        channel.close()
                    }
                }
            } catch (error: Exception) {
                channel.close()
                throw error
            }
        }
    }
}
