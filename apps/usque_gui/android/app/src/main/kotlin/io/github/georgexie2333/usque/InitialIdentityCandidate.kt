package io.github.georgexie2333.usque

import org.json.JSONObject
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream

/** Encoded bytes are stored only as an encrypted vault record, never in the profile journal. */
internal class InitialIdentityCandidate(
    val request: InitialIdentityCoordinator.Request,
    val identity: ByteArray,
    val metadata: ByteArray,
    val license: ByteArray?,
    val endpoints: JSONObject,
) : AutoCloseable {
    fun encode(): ByteArray {
        val bytes = ByteArrayOutputStream()
        DataOutputStream(bytes).use { stream ->
            stream.writeInt(1)
            for (value in listOf(
                request.operationId,
                request.profileId,
                request.method,
                request.organization.orEmpty(),
                endpoints.optString("endpoint_ipv4"),
                endpoints.optString("endpoint_ipv6"),
            )) {
                stream.writeUTF(value)
            }
            for (value in listOf(identity, metadata, license)) {
                require(value == null || value.size <= MAX_FIELD_BYTES)
                stream.writeInt(value?.size ?: -1)
                value?.let(stream::write)
            }
        }
        return bytes.toByteArray()
    }

    override fun close() {
        identity.fill(0)
        metadata.fill(0)
        license?.fill(0)
    }

    companion object {
        private const val MAX_FIELD_BYTES = 256 * 1024

        fun decode(bytes: ByteArray): InitialIdentityCandidate {
            require(bytes.size <= 3 * MAX_FIELD_BYTES + 4096)
            val fields = mutableListOf<ByteArray?>()
            try {
                DataInputStream(ByteArrayInputStream(bytes)).use { stream ->
                    require(stream.readInt() == 1)
                    val operation = stream.readUTF()
                    val profile = stream.readUTF()
                    val method = stream.readUTF()
                    val organization = stream.readUTF().takeIf { it.isNotEmpty() }
                    val ipv4 = stream.readUTF()
                    val ipv6 = stream.readUTF()
                    repeat(3) {
                        val length = stream.readInt()
                        require(length in -1..MAX_FIELD_BYTES)
                        fields.add(if (length == -1) null else ByteArray(length).also(stream::readFully))
                    }
                    require(
                        stream.available() == 0 && fields[0]?.isNotEmpty() == true && fields[1]?.isNotEmpty() == true,
                    )
                    val endpoints = JSONObject()
                    if (ipv4.isNotEmpty() || ipv6.isNotEmpty()) {
                        require(ipv4.isNotEmpty() && ipv6.isNotEmpty() && method == "zeroTrust")
                        endpoints.put("endpoint_ipv4", ipv4).put("endpoint_ipv6", ipv6)
                    }
                    return InitialIdentityCandidate(
                        InitialIdentityCoordinator.Request(operation, profile, method, organization),
                        fields[0]!!,
                        fields[1]!!,
                        fields[2],
                        endpoints,
                    )
                }
            } catch (error: Exception) {
                fields.forEach { it?.fill(0) }
                throw error
            }
        }
    }
}
