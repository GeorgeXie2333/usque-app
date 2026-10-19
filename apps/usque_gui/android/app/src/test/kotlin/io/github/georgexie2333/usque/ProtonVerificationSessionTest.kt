package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.security.MessageDigest

class ProtonVerificationSessionTest {
    private var clock = 2_000_000_000_000L
    private val challenge = "synthetic-private-challenge"
    private val original = "https://verify.proton.me/?token=$challenge&embed=true"
    private val message =
        """
        {"type":"HUMAN_VERIFICATION_SUCCESS",
         "payload":{"type":"captcha","token":"synthetic-private-result"}}
        """.trimIndent()

    private fun handoff(url: String = original): ByteArray =
        JSONObject()
            .put("job_id", "synthetic-job")
            .put("challenge_id", "synthetic-challenge-id")
            .put("url", url)
            .put("expires_at", clock + 300000)
            .put("methods", listOf("captcha"))
            .put(
                "challenge_sha256",
                MessageDigest.getInstance("SHA-256").digest(challenge.toByteArray()).joinToString("") {
                    "%02x".format(it.toInt() and 255)
                },
            ).toString()
            .toByteArray()

    private fun session(): ProtonVerificationSession = ProtonVerificationSession.parse(handoff()) { clock }

    @Test
    fun validResultKeepsOriginalJsonAndIsConsumedOnlyOnce() {
        val session = session()
        assertTrue(session.matches(original))
        assertArrayEquals(message.toByteArray(), session.accept(original, message))
        assertNull(session.accept(original, message))
    }

    @Test
    fun rejectsOtherOriginTokenDuplicateTokenAndUserInfo() {
        val session = session()
        for (url in listOf(
            "https://verify.proton.me.attacker.invalid/?token=$challenge",
            "https://attacker.invalid/?token=$challenge",
            "https://user@verify.proton.me/?token=$challenge",
            "https://verify.proton.me:444/?token=$challenge",
            "http://verify.proton.me/?token=$challenge",
            "https://verify.proton.me/?token=another",
            "$original&token=$challenge",
            "$original&%74oken=$challenge",
            "$original#fragment",
        )) {
            assertFalse(session.matches(url))
            assertNull(session.accept(url, message))
        }
        assertNotNull(session.accept(original, message))
    }

    @Test
    fun nativeExpiryRejectsLateResultWithoutAcceptingASecondPayload() {
        val session = session()
        clock += 300000
        assertTrue(session.expired())
        assertFalse(session.matches(original))
        assertNull(session.accept(original, message))
    }

    @Test
    fun wrongEventSchemaMethodControlCharactersAndOversizeAreRejected() {
        val session = session()
        for (invalid in listOf(
            "not-json",
            message.replace("HUMAN_VERIFICATION_SUCCESS", "HUMAN_VERIFICATION_ERROR"),
            message.replace("captcha", "sms"),
            message.replace("synthetic-private-result", "bad\\nresult"),
            message + " ".repeat(32769),
        )) {
            assertNull(session.accept(original, invalid))
        }
        assertNotNull(session.accept(original, message))
    }

    @Test
    fun expiredOrMismatchedHandoffCannotOpenVerification() {
        assertTrue(
            runCatching {
                ProtonVerificationSession.parse(handoff(original.replace(challenge, "another"))) { clock }
            }.isFailure,
        )
        val raw = JSONObject(handoff().toString(Charsets.UTF_8)).put("expires_at", clock)
        assertTrue(runCatching { ProtonVerificationSession.parse(raw.toString().toByteArray()) { clock } }.isFailure)
    }
}
