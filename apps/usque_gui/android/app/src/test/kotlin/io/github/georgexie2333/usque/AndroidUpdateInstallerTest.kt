package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidUpdateInstallerTest {
    @Test
    fun `lifecycle invalidation rejects stale update callbacks`() {
        val gate = UpdateInstallOperationGate()
        val first = gate.begin()

        assertTrue(gate.isActive(first))
        gate.invalidateAll()
        assertFalse(gate.isActive(first))

        val second = gate.begin()
        assertTrue(gate.isActive(second))
        assertFalse(gate.isActive(first))
        gate.finish(second)
        assertFalse(gate.isActive(second))
    }

    @Test
    fun `request metadata is decoded without losing 64-bit size`() {
        val request =
            AndroidUpdateInstaller.fromArguments(
                mapOf(
                    "path" to "/private/cache/usque-v0.2.4-android-arm64-v8a.apk",
                    "version" to "v0.2.4",
                    "package" to
                        mapOf(
                            "name" to "usque-v0.2.4-android-arm64-v8a.apk",
                            "size" to 4_294_967_296L,
                            "sha256" to "a5".repeat(32),
                            "platform" to "android",
                            "variant" to "arm64-v8a",
                        ),
                ),
            )

        assertEquals(4_294_967_296L, request.size)
        assertEquals("arm64-v8a", request.variant)
        assertEquals("v0.2.4", request.version)
    }

    @Test
    fun `runtime ABI mapping never falls back to universal`() {
        assertEquals("arm64-v8a", AndroidUpdateInstaller.variantForAbi("arm64-v8a"))
        assertEquals("x86_64", AndroidUpdateInstaller.variantForAbi("x86_64"))
        assertEquals("armeabi-v7a", AndroidUpdateInstaller.variantForAbi("armeabi-v7a"))
        assertNull(AndroidUpdateInstaller.variantForAbi("riscv64"))
        assertNull(AndroidUpdateInstaller.variantForAbi("universal"))
    }

    @Test
    fun `APK native entry must match the selected ABI`() {
        val entries = sequenceOf("lib/arm64-v8a/libusque_android.so", "assets/flutter_assets/a")
        assertTrue(AndroidUpdateInstaller.containsNativeVariant(entries, "arm64-v8a"))
        assertFalse(
            AndroidUpdateInstaller.containsNativeVariant(
                sequenceOf("lib/arm64-v8a/libusque_android.so"),
                "x86_64",
            ),
        )
    }
}
