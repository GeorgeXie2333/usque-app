package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.security.GeneralSecurityException
import javax.crypto.Cipher
import javax.crypto.spec.SecretKeySpec

class ChainProfileCipherTest {
    private val id = "00000000-0000-4000-8000-000000000001"
    private val key = SecretKeySpec(ByteArray(32) { (it + 1).toByte() }, "AES")

    @Test
    fun completeProtonCatalogueRoundTripsAboveOrdinaryProfileBounds() {
        val nodes = JSONArray()
        repeat(2500) { index ->
            nodes.put(
                JSONObject()
                    .put("id", "%08x-0000-4000-8000-000000000000".format(index))
                    .put("name", "US-FREE#$index")
                    .put("country", "US")
                    .put("city", "Los Angeles")
                    .put("load", 10),
            )
        }
        val plaintext =
            JSONObject()
                .put("version", 2)
                .put("id", id)
                .put("server_catalog", nodes)
                .toString()
                .toByteArray(Charsets.UTF_8)
        assertTrue(plaintext.size > 192 * 1024)
        val sealed = ChainProfileCipherCodec.seal(id, plaintext, key)
        assertTrue(sealed.size > 256 * 1024)
        assertEquals(1.toByte(), sealed[0])
        assertEquals(plaintext.size + 29, sealed.size)
        assertArrayEquals(plaintext, ChainProfileCipherCodec.open(id, sealed, key))
    }

    @Test
    fun maximumProtonPlaintextRoundTripsWithoutChangingTheEnvelope() {
        val plaintext = ByteArray(ChainProfileCipherCodec.MAX_RECORD_PLAINTEXT_BYTES) { (it % 251).toByte() }
        val sealed = ChainProfileCipherCodec.seal(id, plaintext, key)
        assertEquals(plaintext.size + 29, sealed.size)
        assertArrayEquals(plaintext, ChainProfileCipherCodec.open(id, sealed, key))
    }

    @Test
    fun invalidPlaintextIsRejectedBeforeAccessingAndroidKeystore() {
        for (value in listOf(ByteArray(0), ByteArray(ChainProfileCipherCodec.MAX_RECORD_PLAINTEXT_BYTES + 1))) {
            assertThrows(IllegalArgumentException::class.java) { ChainProfileCipher.seal(id, value) }
            assertThrows(IllegalArgumentException::class.java) { ChainProfileCipherCodec.seal(id, value, key) }
        }
    }

    @Test
    fun malformedAndOversizeEnvelopesAreRejectedBeforeAccessingAndroidKeystore() {
        for (value in listOf(ByteArray(0), ByteArray(29), ByteArray(30), ByteArray(30) { 2 })) {
            assertThrows(IllegalArgumentException::class.java) { ChainProfileCipher.open(id, value) }
            assertThrows(IllegalArgumentException::class.java) { ChainProfileCipherCodec.open(id, value, key) }
        }
        val oversize = ByteArray(ChainProfileCipherCodec.MAX_RECORD_CIPHERTEXT_BYTES + 1).also { it[0] = 1 }
        assertThrows(IllegalArgumentException::class.java) { ChainProfileCipher.open(id, oversize) }
        assertThrows(IllegalArgumentException::class.java) { ChainProfileCipherCodec.open(id, oversize, key) }
        val maximum = ByteArray(ChainProfileCipherCodec.MAX_RECORD_CIPHERTEXT_BYTES).also { it[0] = 1 }
        assertThrows(GeneralSecurityException::class.java) { ChainProfileCipherCodec.open(id, maximum, key) }
    }

    @Test
    fun profileBindingWrongKeysAndCiphertextTamperingRemainRejected() {
        val sealed = ChainProfileCipherCodec.seal(id, "synthetic-private-record".toByteArray(), key)
        assertThrows(GeneralSecurityException::class.java) {
            ChainProfileCipherCodec.open("00000000-0000-4000-8000-000000000002", sealed, key)
        }
        assertThrows(GeneralSecurityException::class.java) {
            ChainProfileCipherCodec.open(id, sealed, SecretKeySpec(ByteArray(32) { 42 }, "AES"))
        }
        val tampered = sealed.copyOf().also { it[it.lastIndex] = (it.last().toInt() xor 1).toByte() }
        assertThrows(GeneralSecurityException::class.java) { ChainProfileCipherCodec.open(id, tampered, key) }
    }

    @Test
    fun authenticatedPlaintextAboveTheRecordLimitIsRejectedAfterDecryption() {
        val plaintext = ByteArray(ChainProfileCipherCodec.MAX_RECORD_PLAINTEXT_BYTES + 1) { 42 }
        val sealed = legacySeal(plaintext)
        assertTrue(sealed.size <= ChainProfileCipherCodec.MAX_RECORD_CIPHERTEXT_BYTES)
        assertThrows(IllegalArgumentException::class.java) { ChainProfileCipherCodec.open(id, sealed, key) }
    }

    @Test
    fun existingVersionOneEnvelopeStillOpens() {
        val plaintext = "synthetic-existing-chain-profile".toByteArray()
        assertArrayEquals(plaintext, ChainProfileCipherCodec.open(id, legacySeal(plaintext), key))
    }

    private fun legacySeal(value: ByteArray): ByteArray {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key)
        cipher.updateAAD("Usque/chain-profiles/v1/$id".toByteArray(Charsets.UTF_8))
        val ciphertext = cipher.doFinal(value)
        return try {
            byteArrayOf(1) + cipher.iv + ciphertext
        } finally {
            ciphertext.fill(0)
        }
    }
}
