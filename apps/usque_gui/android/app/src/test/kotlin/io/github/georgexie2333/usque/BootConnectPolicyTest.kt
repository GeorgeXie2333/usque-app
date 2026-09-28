package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class BootConnectPolicyTest {
    @Test
    fun bootUsesCurrentActiveAccountAndItsCurrentFlag() {
        val catalog = """{"active_profile_id":"new","profiles":[
            {"id":"old","auto_connect":true},{"id":"new","auto_connect":false}]}"""
        assertNull(BootConnectPolicy.profile(catalog))
        val updated = JSONObject(catalog)
        updated.getJSONArray("profiles").getJSONObject(1).put("auto_connect", true)
        assertEquals("new", JSONObject(BootConnectPolicy.profile(updated.toString())!!).getString("id"))
        updated.put("active_profile_id", "deleted")
        assertNull(BootConnectPolicy.profile(updated.toString()))
    }
}
