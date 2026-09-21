package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.util.concurrent.Executors

class AtomicPolicyStoreTest {
    @Test
    fun independentReadersAndWritersNeverReuseCachedPolicy() {
        val root = Files.createTempDirectory("usque-policy-test").toFile()
        try {
            val first = AtomicPolicyStore(File(root, "policy.json"))
            val second = AtomicPolicyStore(File(root, "policy.json"))
            assertFalse(first.getBoolean("enabled", false))
            second.edit {
                putBoolean("enabled", true)
                putStringSet("apps", setOf("a", "b"))
            }
            assertTrue(first.getBoolean("enabled", false))
            first.edit { putString("recovery", "new") }
            assertTrue(second.getBoolean("enabled", false))
            assertEquals(setOf("a", "b"), first.getStringSet("apps", emptySet()))
            assertEquals("new", second.getString("recovery", null))
        } finally {
            root.deleteRecursively()
        }
    }

    @Test
    fun concurrentFieldPatchesPreserveAllWritersAndClearDoesNotRemigrate() {
        val root = Files.createTempDirectory("usque-policy-test").toFile()
        val pool = Executors.newFixedThreadPool(4)
        try {
            val file = File(root, "policy.json")
            val first = AtomicPolicyStore(file) { mapOf("legacy" to "secret-free") }
            assertEquals("secret-free", first.getString("legacy", null))
            (0 until 24)
                .map { index ->
                    pool.submit {
                        AtomicPolicyStore(file).edit { putString("key$index", "$index") }
                    }
                }.forEach { it.get() }
            for (index in 0 until 24) assertEquals("$index", first.getString("key$index", null))
            first.edit { clear() }
            val reopened = AtomicPolicyStore(file) { error("must not re-import") }
            assertFalse(reopened.contains("legacy"))
            assertTrue(reopened.revision() > 24)
        } finally {
            pool.shutdownNow()
            root.deleteRecursively()
        }
    }
}
