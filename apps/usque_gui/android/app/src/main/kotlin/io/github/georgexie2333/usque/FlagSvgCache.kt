package io.github.georgexie2333.usque

import android.content.Context
import android.util.AtomicFile
import java.io.File

internal class FlagSvgCache(context: Context) {
    private val directory =
        File(context.applicationContext.cacheDir, CACHE_DIRECTORY).apply {
            check(isDirectory || mkdirs()) { "Flag cache directory could not be created" }
        }

    fun get(countryCode: String): String? {
        val file = file(countryCode) ?: return null
        if (!file.isFile || file.length() !in 1..MAX_BYTES.toLong()) return null
        return try {
            validate(AtomicFile(file).readFully())
        } catch (_: Exception) {
            AtomicFile(file).delete()
            null
        }
    }

    fun put(countryCode: String, svg: String) {
        val file = file(countryCode) ?: return
        val bytes = svg.toByteArray(Charsets.UTF_8)
        if (bytes.size !in 1..MAX_BYTES || validate(bytes) == null) {
            bytes.fill(0)
            return
        }
        try {
            val atomic = AtomicFile(file)
            val output = atomic.startWrite()
            try {
                output.write(bytes)
                atomic.finishWrite(output)
            } catch (error: Exception) {
                atomic.failWrite(output)
                throw error
            }
        } finally {
            bytes.fill(0)
        }
    }

    fun clear() {
        directory.listFiles()?.forEach { file ->
            if (file.isFile) AtomicFile(file).delete()
        }
    }

    private fun file(countryCode: String): File? {
        val normalized = countryCode.lowercase()
        if (normalized.length != 2 || normalized.any { it !in 'a'..'z' }) return null
        return File(directory, "$normalized.svg")
    }

    private fun validate(bytes: ByteArray): String? {
        val svg = bytes.toString(Charsets.UTF_8)
        val lower = svg.lowercase()
        return svg.takeIf {
            lower.trimStart().startsWith("<svg") &&
                "<script" !in lower &&
                "<foreignobject" !in lower &&
                "<!entity" !in lower &&
                "onload=" !in lower &&
                "javascript:" !in lower &&
                "xlink:href" !in lower &&
                "href=\"http" !in lower &&
                "href='http" !in lower
        }
    }

    private companion object {
        const val CACHE_DIRECTORY = "flag-icons-7.5.0"
        const val MAX_BYTES = 64 * 1024
    }
}
