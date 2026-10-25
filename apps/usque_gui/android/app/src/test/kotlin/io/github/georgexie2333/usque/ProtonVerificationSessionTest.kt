package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element
import java.io.File
import java.net.URI
import java.security.MessageDigest
import javax.xml.parsers.DocumentBuilderFactory

class ProtonVerificationSessionTest {
    @Test
    fun onlyTheReservedProxyProbeNamespaceAllowsCleartext() {
        val root =
            generateSequence(File(requireNotNull(System.getProperty("user.dir"))).absoluteFile) { it.parentFile }
                .first { File(it, "apps/usque_gui/pubspec.yaml").isFile }
        val source = File(root, "apps/usque_gui/android/app/src/main")
        val parser = DocumentBuilderFactory.newInstance().newDocumentBuilder()
        val manifest = parser.parse(File(source, "AndroidManifest.xml"))
        val application = manifest.getElementsByTagName("application").item(0) as Element
        assertEquals("@xml/network_security_config", application.getAttribute("android:networkSecurityConfig"))
        val policy = parser.parse(File(source, "res/xml/network_security_config.xml"))
        val base = policy.getElementsByTagName("base-config").item(0) as Element
        assertEquals("false", base.getAttribute("cleartextTrafficPermitted"))
        val configs = policy.getElementsByTagName("domain-config")
        assertEquals(1, configs.length)
        assertEquals("true", (configs.item(0) as Element).getAttribute("cleartextTrafficPermitted"))
        val domains = policy.getElementsByTagName("domain")
        assertEquals(1, domains.length)
        val domain = domains.item(0) as Element
        assertEquals("usque-proton-proxy.invalid", domain.textContent)
        assertEquals("true", domain.getAttribute("includeSubdomains"))
        val probe = ProtonVerificationSession.Proxy(12345, "a".repeat(32), "b".repeat(64), "c".repeat(32))
        for (url in listOf(probe.probe(), probe.probe("ready"))) {
            assertEquals("http", URI(url).scheme)
            assertEquals("${probe.nonce}.${domain.textContent}", URI(url).host)
        }
    }

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
    fun proxyHandoffIsVersionedBoundedAndNeverChangesTheChallengeOrigin() {
        val value = JSONObject(handoff().toString(Charsets.UTF_8)).put("version", 2).put("route_epoch", 1)
        val proxy =
            JSONObject()
                .put(
                    "port",
                    12345,
                ).put("username", "a".repeat(32))
                .put("password", "b".repeat(64))
                .put("nonce", "c".repeat(32))
        value.put("proxy", proxy)
        val session = ProtonVerificationSession.parse(value.toString().toByteArray()) { clock }
        assertTrue(session.matches(original))
        assertTrue(session.proxy?.origin == "http://127.0.0.1:12345")
        assertFalse(session.matches(session.proxy?.probe()))
        assertTrue(JSONObject(session.networkFailure().toString(Charsets.UTF_8)).getInt("route_epoch") == 1)
        proxy.put("port", 65536)
        assertTrue(runCatching { ProtonVerificationSession.parse(value.toString().toByteArray()) { clock } }.isFailure)
        proxy.put("port", 12345)
        value.put("version", 1)
        assertTrue(runCatching { ProtonVerificationSession.parse(value.toString().toByteArray()) { clock } }.isFailure)
    }

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
