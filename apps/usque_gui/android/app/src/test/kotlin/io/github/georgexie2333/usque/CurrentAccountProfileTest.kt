package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class CurrentAccountProfileTest {
    @Test
    fun manualRetryUsesCurrentActiveAccountRegardlessOfAutoConnect() {
        val catalog = """{"active_profile_id":"b","profiles":[
            {"id":"a","auto_connect":true},{"id":"b","auto_connect":false}]}"""
        assertEquals("b", JSONObject(CurrentAccountProfile.read(catalog)!!).getString("id"))
        assertNull(
            CurrentAccountProfile.read(
                catalog.replace("\"active_profile_id\":\"b\"", "\"active_profile_id\":\"deleted\""),
            ),
        )
    }
}
