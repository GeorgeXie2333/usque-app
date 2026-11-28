package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.util.UUID

/** Mirrors Rust's largest_routing_settings: both kinds full, target budget spent. */
internal fun largestRoutingProfileJson(): String {
    val rules = JSONArray()
    for (index in 0 until 4096) {
        val high = 0x8000 or (index shr 8)
        val low = 0x8000 or (index and 0xff)
        rules.put(
            JSONObject()
                .put("id", UUID.randomUUID().toString())
                .put("kind", "cidr")
                .put("target", "2001:db8f:aaaa:bbbb:cccc:dddd:%04x:%04x/128".format(high, low))
                .put("action", "reject"),
        )
        rules.put(
            JSONObject()
                .put("id", UUID.randomUUID().toString())
                .put("kind", "domain")
                .put("target", "h%04d.%s".format(index, "a".repeat(47)))
                .put("action", "reject"),
        )
    }
    // The other profile fields are a few kilobytes at most.
    return JSONObject()
        .put("id", UUID.randomUUID().toString())
        .put("padding", "p".repeat(8 * 1024))
        .put("routing", JSONObject().put("rules", rules).put("ads_enabled", true))
        .toString()
}

class LargeIpcPayloadTest {
    private fun <T> withDirectory(action: (File) -> T): T {
        val root = Files.createTempDirectory("usque-ipc-handoff").toFile()
        try {
            return action(File(root, "ipc-handoff"))
        } finally {
            root.deleteRecursively()
        }
    }

    @Test
    fun aFullSubscriptionCatalogueDecodesBeyondTheSettingsBound() {
        // 32 lists x 512 endpoints with maximum hostnames, identifiers and notes.
        val host = "a".repeat(63) + "." + "b".repeat(63) + "." + "c".repeat(63) + "." + "d".repeat(57) + ".com"
        val note = "\u00e9".repeat(128)
        val subscriptions = JSONArray()
        for (sub in 0 until 32) {
            val endpoints = JSONArray()
            for (index in 0 until 512) {
                endpoints.put(
                    JSONObject()
                        .put("id", "host:$host:$index")
                        .put("host", JSONObject().put("kind", "domain").put("value", host))
                        .put("port", 65535)
                        .put("note", note)
                        .put("pinned", false),
                )
            }
            subscriptions.put(JSONObject().put("id", "sub-$sub").put("endpoints", endpoints))
        }
        val json = JSONObject().put("subscriptions", subscriptions).put("target", "private").toString()
        assertTrue(json.length > LargeIpcPayload.MAX_SETTINGS_CHARS)
        assertTrue(json.length <= LargeIpcPayload.MAX_SUBSCRIPTION_CHARS)
        val decoded = NetworkSettingsFields.decodeObject(json)
        assertEquals(32, (decoded["subscriptions"] as List<*>).size)
        assertFalse(decoded.containsKey("target"))
    }

    @Test
    fun largeStringsRoundTripOnceThroughAVerifiedFile() =
        withDirectory { directory ->
            val value = largestRoutingProfileJson() + "界"
            assertTrue(LargeIpcPayload.needsHandoff(value))
            assertFalse(LargeIpcPayload.needsHandoff("x".repeat(LargeIpcPayload.INLINE_LIMIT_CHARS)))
            assertFalse(LargeIpcPayload.needsHandoff(null))
            val handoff = LargeIpcPayload.write(directory, value)
            assertEquals(listOf(handoff.name), directory.list()!!.toList())
            assertEquals(value, LargeIpcPayload.read(directory, handoff.name, handoff.digest))
            assertFalse(handoff.file.exists())
            // A second read, as from a replayed message, finds nothing.
            assertNull(LargeIpcPayload.read(directory, handoff.name, handoff.digest))
        }

    @Test
    fun alteredMissingOrForeignHandoffsReadAsAbsentAndAreRemoved() =
        withDirectory { directory ->
            val altered = LargeIpcPayload.write(directory, "a".repeat(40_000))
            altered.file.writeText("b".repeat(40_000))
            assertNull(LargeIpcPayload.read(directory, altered.name, altered.digest))
            assertFalse(altered.file.exists())
            val unsigned = LargeIpcPayload.write(directory, "c".repeat(40_000))
            assertNull(LargeIpcPayload.read(directory, unsigned.name, null))
            assertFalse(unsigned.file.exists())
            assertNull(LargeIpcPayload.read(directory, "${UUID.randomUUID()}.json", altered.digest))
            val outside = File(directory.parentFile, "secret.json").apply { writeText("keep") }
            for (name in listOf("../secret.json", "secret.json", "${UUID.randomUUID()}.json/../x", null)) {
                assertNull(LargeIpcPayload.read(directory, name, altered.digest))
            }
            assertEquals("keep", outside.readText())
        }

    @Test
    fun oversizedPayloadsAreRefusedBeforeWriting() =
        withDirectory { directory ->
            assertThrows(IllegalArgumentException::class.java) {
                LargeIpcPayload.write(directory, "x".repeat(LargeIpcPayload.MAX_PAYLOAD_BYTES + 1))
            }
            assertTrue(directory.list().isNullOrEmpty())
        }

    @Test
    fun sweepRemovesOnlyUnclaimedStaleHandoffs() =
        withDirectory { directory ->
            val stale = LargeIpcPayload.write(directory, "s".repeat(40_000))
            val fresh = LargeIpcPayload.write(directory, "f".repeat(40_000))
            val now = System.currentTimeMillis()
            assertTrue(stale.file.setLastModified(now - LargeIpcPayload.STALE_MILLIS - 1_000))
            LargeIpcPayload.sweep(directory, now)
            assertFalse(stale.file.exists())
            assertTrue(fresh.file.exists())
            LargeIpcPayload.discard(listOf(fresh))
            assertFalse(fresh.file.exists())
            LargeIpcPayload.sweep(File(directory, "missing"))
        }

    @Test
    fun largestProfilesFitEveryKotlinBound() {
        val profile = largestRoutingProfileJson()
        assertTrue(profile.toByteArray(Charsets.UTF_8).size <= LargeIpcPayload.MAX_PROFILE_BYTES)
        // A settings result carries up to four profiles.
        assertTrue(profile.length * 4 <= LargeIpcPayload.MAX_SETTINGS_CHARS)
        assertEquals(
            ApplicationRoutingFields(needsDns = true, directDns = false),
            ApplicationRoutingFields.parse(JSONObject(profile)),
        )
        val tooMany = JSONObject(profile)
        val extra =
            JSONObject()
                .put("id", "x")
                .put("kind", "cidr")
                .put("target", "192.0.2.0/24")
                .put("action", "direct")
        tooMany.getJSONObject("routing").getJSONArray("rules").put(extra)
        assertThrows(IllegalArgumentException::class.java) { ApplicationRoutingFields.parse(tooMany) }
    }

    @Test
    fun recoveryPolicyHoldsTwoLargestProfiles() {
        val root = Files.createTempDirectory("usque-policy-limit").toFile()
        try {
            val file = File(root, "recovery.json")
            val profile = largestRoutingProfileJson()
            AtomicPolicyStore(file).edit {
                putString(UsqueVpnService.RECOVERY_PROFILE, profile)
                putString(UsqueVpnService.LAST_PROFILE, profile)
            }
            assertEquals(profile, AtomicPolicyStore(file).getString(UsqueVpnService.LAST_PROFILE, null))
            // Escaping grows the stored copy; keep room for the other keys.
            assertTrue(file.length() < 3 * 1024 * 1024)
        } finally {
            root.deleteRecursively()
        }
    }
}
