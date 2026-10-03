package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class InitialIdentityCandidateTest {
    @Test
    fun encryptedRecordPayloadRoundTripsAndClearsDecodedSecrets() {
        val request =
            InitialIdentityCoordinator.Request(
                "00000000-0000-4000-8000-000000000001",
                "00000000-0000-4000-8000-000000000002",
                "zeroTrust",
                "example-team",
            )
        val candidate =
            InitialIdentityCandidate(
                request,
                byteArrayOf(1, 2),
                byteArrayOf(3, 4),
                null,
                JSONObject().put("endpoint_ipv4", "162.159.197.2").put("endpoint_ipv6", "2606:4700:102::2"),
            )
        val encoded = candidate.encode()
        val decoded = InitialIdentityCandidate.decode(encoded)
        assertEquals(request, decoded.request)
        assertArrayEquals(candidate.identity, decoded.identity)
        assertEquals("162.159.197.2", decoded.endpoints.getString("endpoint_ipv4"))
        decoded.close()
        assertTrue(decoded.identity.all { it == 0.toByte() })
        assertTrue(decoded.metadata.all { it == 0.toByte() })
        encoded.fill(0)
        candidate.close()
    }

    @Test
    fun malformedOrTruncatedCandidateCannotBeRecovered() {
        for (bytes in listOf(byteArrayOf(), byteArrayOf(0, 0, 0, 1), ByteArray(3 * 256 * 1024 + 4097))) {
            assertTrue(runCatching { InitialIdentityCandidate.decode(bytes) }.isFailure)
        }
    }
}
