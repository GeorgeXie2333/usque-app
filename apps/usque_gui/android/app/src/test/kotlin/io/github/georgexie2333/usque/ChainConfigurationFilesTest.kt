package io.github.georgexie2333.usque

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.IOException
import java.io.InputStream

class ChainConfigurationFilesTest {
    @Test
    fun readsShortChunksUntilEofAndClosesTheStream() {
        var closed = false
        val input =
            object : ByteArrayInputStream("client\n".toByteArray()) {
                override fun read(
                    buffer: ByteArray,
                    offset: Int,
                    length: Int,
                ): Int = super.read(buffer, offset, minOf(length, 2))

                override fun close() {
                    closed = true
                    super.close()
                }
            }
        val result = ChainConfigurationFiles.read("sample.ovpn") { input }
        assertEquals("sample.ovpn", result["name"])
        val bytes = result["bytes"] as ByteArray
        assertArrayEquals("client\n".toByteArray(), bytes)
        assertTrue(closed)
        ChainConfigurationFiles.clear(listOf(result))
        assertTrue(bytes.all { it == 0.toByte() })
    }

    @Test
    fun acceptsTheExactBoundaryAndRejectsOversizedEmptyAndUnavailableFiles() {
        val limit = ChainConfigurationFiles.MAX_BYTES
        assertEquals(
            limit,
            (
                ChainConfigurationFiles.read("boundary") {
                    ByteArrayInputStream(ByteArray(limit))
                }["bytes"] as ByteArray
            ).size,
        )
        assertEquals(
            "CHAIN_FILE_TOO_LARGE",
            ChainConfigurationFiles.read("large") {
                ByteArrayInputStream(
                    ByteArray(limit + 1),
                )
            }["error"],
        )
        assertEquals(
            "CHAIN_FILE_READ_FAILED",
            ChainConfigurationFiles.read("empty") {
                ByteArrayInputStream(byteArrayOf())
            }["error"],
        )
        assertEquals("CHAIN_FILE_READ_FAILED", ChainConfigurationFiles.read("missing") { null }["error"])
    }

    @Test
    fun failureIsPerFileAndExceptionDetailsNeverReachTheReply() {
        val results =
            listOf(
                ChainConfigurationFiles.read("bad") { throw IOException("private path and secret") },
                ChainConfigurationFiles.read("good") { ByteArrayInputStream("valid".toByteArray()) },
            )
        assertEquals(mapOf("name" to "bad", "error" to "CHAIN_FILE_READ_FAILED"), results.first())
        assertArrayEquals("valid".toByteArray(), results.last()["bytes"] as ByteArray)
        ChainConfigurationFiles.clear(results)
    }

    @Test
    fun zeroProgressCannotHangTheReader() {
        val result =
            ChainConfigurationFiles.read("stalled") {
                object : InputStream() {
                    override fun read(): Int = 0

                    override fun read(
                        buffer: ByteArray,
                        offset: Int,
                        length: Int,
                    ): Int = 0
                }
            }
        assertEquals("CHAIN_FILE_READ_FAILED", result["error"])
    }
}
