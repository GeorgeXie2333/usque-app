package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class ProtonClientContextTest {
    @Test
    fun androidFactsMatchTheSharedRustWireFixture() {
        val facts =
            ProtonClientContext.Facts(
                osRelease = "15",
                brand = "google",
                model = "Pixel 8",
                locale = "ja",
                device = "shiba",
                isTv = false,
                appLanguage = "ja",
                timezone = "Asia/Tokyo",
                regionCode = "JP",
                timezoneOffset = -540,
                rooted = false,
                fontScale = 1.25f,
                storageBytes = 128_999_999_999L,
                darkMode = true,
                keyboards = listOf("com.example.synthetic.keyboard"),
            )
        val expected =
            requireNotNull(javaClass.getResourceAsStream("/proton_android_client.json")).bufferedReader().use {
                JSONObject(it.readText())
            }
        val actual = JSONObject(ProtonClientContext.encode(facts))
        assertSameFields(expected, actual)
        assertEquals(
            expected.getJSONObject("device").getLong("deviceName"),
            actual.getJSONObject("device").getLong("deviceName"),
        )
        assertEquals(128.0, actual.getJSONObject("device").getDouble("storageCapacity"), 0.0)
        assertFalse(actual.toString().contains("shiba"))
        assertFalse(actual.toString().contains("serial"))
    }

    @Test
    fun deviceHashUsesSignedUtf8BytesAndDoesNotReduceTheSum() {
        assertEquals(73_362_205_250L, ProtonClientContext.deviceHash("Pixel 8/google shiba"))
        assertEquals(-670L, ProtonClientContext.deviceHash("é"))
        assertEquals(76_413_035_754L, ProtonClientContext.deviceHash("模型😀/brand device"))
    }

    private fun assertSameFields(
        expected: JSONObject,
        actual: JSONObject,
    ) {
        val keys = expected.keys().asSequence().toSet()
        assertEquals(keys, actual.keys().asSequence().toSet())
        for (key in keys) {
            when (val value = expected.get(key)) {
                is JSONObject -> assertSameFields(value, actual.getJSONObject(key))
                is JSONArray -> assertEquals(value.toString(), actual.getJSONArray(key).toString())
                is Number -> assertEquals(value.toDouble(), actual.getDouble(key), 0.0)
                else -> assertEquals(value, actual.get(key))
            }
        }
    }
}
