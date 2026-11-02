package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class PassiveControlFieldsTest {
    @Test
    fun persistedSettingsCannotConfirmAnOperationOrReplaceTheLiveEpoch() {
        val result =
            NetworkSettingsFields.persistedOnly(
                """{"source_epoch":"ui-process","sequence":9,"stored_profile":{"id":"saved"},"shared_network_profile":{"mtu":1280},"applied_profile":{"id":"live"},"session_id":"session","operation_id":"op","persisted":true,"persisted_operation_ids":["op"],"apply_status":"applied","target":{"private":true}}""",
            )
        assertEquals(setOf("runtime_observed", "stored_profile", "shared_network_profile"), result.keys)
        assertEquals(false, result["runtime_observed"])
        assertEquals("saved", (result["stored_profile"] as Map<*, *>)["id"])
    }

    @Test
    fun persistedScenesExposeSavedTemplatesWithoutRuntimeOrCommitClaims() {
        val result =
            ScenarioFields.persistedOnly(
                """{"source_epoch":"ui-process","scenarios":[{"id":"saved"}],"catalogue_revision":"revision","selection":{"scenario_id":"saved"},"current_account_id":"account","applied_account_id":"live","applied_scenario_id":"saved","applied_scene_confirmed":true,"observation_session_id":"session","persisted_operation_id":"op","unresolved_operation_id":null,"last_operation_id":"op","can_restore":true,"state":{"apply_status":"applied","applied_matches":true},"preview_target":{"private":true}}""",
            )
        assertEquals(false, result["runtime_observed"])
        assertEquals("revision", result["catalogue_revision"])
        assertEquals(listOf(mapOf("id" to "saved")), result["scenarios"])
        assertEquals(mapOf("apply_status" to "unknown"), result["state"])
        for (key in listOf(
            "source_epoch",
            "applied_account_id",
            "applied_scene_confirmed",
            "persisted_operation_id",
            "last_operation_id",
            "unresolved_operation_id",
            "can_restore",
            "preview_target",
        )) {
            assertFalse(key, result.containsKey(key))
        }
    }
}
