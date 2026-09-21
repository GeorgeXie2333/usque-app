package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

class SharedProxyCredentialsTest {
    private val record = SecureIdentityStore.Record.PROXY_PASSWORD
    private val catalog = JSONObject("""{"profiles":[{"id":"a"},{"id":"b"}]}""")

    @Test
    fun sharedPasswordSurvivesAccountDeletionAndRotation() {
        val store = Store()
        store.put("a", record, "old".toByteArray())
        assertArrayEquals("old".toByteArray(), SharedProxyCredentials.read(store, catalog))
        store.deleteIdentity("a")
        assertArrayEquals("old".toByteArray(), SharedProxyCredentials.read(store, catalog))
        SharedProxyCredentials.save(store, catalog, "new".toByteArray()) { }
        assertArrayEquals("new".toByteArray(), SharedProxyCredentials.read(store, catalog))
        SharedProxyCredentials.save(store, catalog, byteArrayOf()) { }
        assertNull(SharedProxyCredentials.read(store, catalog))
    }

    @Test
    fun conflictingLegacyPasswordsArePreservedUntilExplicitReplacement() {
        val store = Store()
        store.put("a", record, byteArrayOf(1))
        store.put("b", record, byteArrayOf(2))
        assertThrows(IllegalStateException::class.java) { SharedProxyCredentials.read(store, catalog) }
        assertNull(store.get(SharedProxyCredentials.SHARED_ID, record))
        assertArrayEquals(byteArrayOf(1), store.get("a", record))
        SharedProxyCredentials.save(store, catalog, byteArrayOf(3)) { }
        assertArrayEquals(byteArrayOf(3), SharedProxyCredentials.read(store, catalog))
        assertNull(store.get("a", record))
        assertNull(store.get("b", record))
    }

    @Test
    fun secretFailureNeverEnablesUsername() {
        var wroteUsername = false
        val store = Store().apply { failWrite = true }
        assertThrows(IllegalStateException::class.java) {
            SharedProxyCredentials.save(store, catalog, byteArrayOf(1)) { wroteUsername = true }
        }
        assertFalse(wroteUsername)
    }

    private class Store : AndroidEngineMethodHandler.IdentityStore {
        val values = mutableMapOf<Pair<String, SecureIdentityStore.Record>, ByteArray>()
        var failWrite = false

        override fun put(
            profileId: String,
            record: SecureIdentityStore.Record,
            value: ByteArray,
        ) {
            check(!failWrite)
            values[profileId to record] = value.copyOf()
        }

        override fun get(
            profileId: String,
            record: SecureIdentityStore.Record,
        ) = values[profileId to record]?.copyOf()

        override fun delete(
            profileId: String,
            record: SecureIdentityStore.Record,
        ) {
            values.remove(profileId to record)
        }

        override fun deleteIdentity(profileId: String) {
            values.keys.removeAll { it.first == profileId }
        }

        override fun clearAll() {
            values.clear()
        }
    }
}
