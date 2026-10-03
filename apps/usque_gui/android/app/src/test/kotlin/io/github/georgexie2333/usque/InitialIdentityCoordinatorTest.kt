package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class InitialIdentityCoordinatorTest {
    @Test
    fun readyIdentityIsReusedWithoutRegistrationOrJournalMutation() {
        val fixture = Fixture()
        fixture.ready = true
        val state =
            fixture.coordinator.initialize(fixture.request) {
                fixture.registrations++
                it(JSONObject())
            }
        assertEquals("completed", state["phase"])
        assertEquals(true, state["reused"])
        assertEquals(0, fixture.registrations)
        assertEquals(listOf("get_initial_identity_state"), fixture.commands)
    }

    @Test
    fun registrationIsJournaledAndCommittedExactlyOnce() {
        val fixture = Fixture()
        fixture.coordinator.initialize(fixture.request) {
            fixture.registrations++
            fixture.ready = true
            it(JSONObject())
        }
        fixture.coordinator.initialize(fixture.request) {
            fixture.registrations++
            it(JSONObject())
        }
        assertEquals(1, fixture.registrations)
        assertEquals(
            listOf(
                "get_initial_identity_state",
                "begin_initial_identity",
                "finish_initial_identity",
                "get_initial_identity_state",
            ),
            fixture.commands,
        )
    }

    @Test
    fun busyLeaseCannotRegisterOrMutateJournal() {
        val fixture = Fixture()
        fixture.busy = true
        val state =
            fixture.coordinator.initialize(fixture.request) {
                fixture.registrations++
                it(JSONObject())
            }
        assertEquals("pending", state["phase"])
        assertEquals(0, fixture.registrations)
        assertEquals(listOf("get_initial_identity_state"), fixture.commands)
    }

    @Test
    fun statusOfAbandonedPendingOperationIsReadOnlyAndInterrupted() {
        val fixture = Fixture()
        fixture.journal = fixture.request.json("begin_initial_identity").put("phase", "pending")
        val state = fixture.coordinator.status(fixture.request.profileId)
        assertEquals("interrupted", state["phase"])
        assertEquals(listOf("get_initial_identity_state"), fixture.commands)
    }

    @Test
    fun completeStableVaultWinsOverAbandonedPendingJournal() {
        val fixture = Fixture()
        fixture.ready = true
        fixture.journal = fixture.request.json("begin_initial_identity").put("phase", "pending")
        val state = fixture.coordinator.status(fixture.request.profileId)
        assertEquals("completed", state["phase"])
        assertEquals(true, state["reused"])
        assertEquals(listOf("get_initial_identity_state"), fixture.commands)
    }

    @Test
    fun resumeWithoutJournalNeverReservesOrRegisters() {
        val fixture = Fixture()
        val state =
            fixture.coordinator.initialize(fixture.request.copy(resumeOnly = true)) {
                fixture.registrations++
                it(JSONObject())
            }
        assertEquals("idle", state["phase"])
        assertEquals(0, fixture.registrations)
        assertEquals(listOf("get_initial_identity_state"), fixture.commands)
    }

    @Test
    fun resumeAbandonedPendingWithoutCandidateReturnsInterruptedWithoutRegistration() {
        val fixture = Fixture()
        fixture.journal = fixture.request.json("begin_initial_identity").put("phase", "pending")
        val state =
            fixture.coordinator.initialize(fixture.request.copy(resumeOnly = true)) {
                fixture.registrations++
                it(JSONObject())
            }
        assertEquals("interrupted", state["phase"])
        assertEquals(0, fixture.registrations)
        assertEquals(listOf("get_initial_identity_state"), fixture.commands)
    }

    @Test
    fun operatingSystemLeaseRejectsOverlappingOwnerAndCanBeReacquired() {
        val directory =
            java.nio.file.Files
                .createTempDirectory("usque-initial-lease-test")
        val config = directory.resolve("profiles-v2.json").toString()
        try {
            InitialIdentityCoordinator.acquire(config)!!.use {
                assertEquals(null, InitialIdentityCoordinator.acquire(config))
            }
            InitialIdentityCoordinator.acquire(config)!!.close()
        } finally {
            java.nio.file.Files
                .deleteIfExists(directory.resolve("profiles-v2.initial-identity.lock"))
            java.nio.file.Files
                .deleteIfExists(directory)
        }
    }

    @Test
    fun partialIdentityNeverUsesReplacementOrRemoteRegistration() {
        val fixture = Fixture()
        fixture.material = true
        val state =
            fixture.coordinator.initialize(fixture.request) {
                fixture.registrations++
                it(JSONObject())
            }
        assertEquals("INITIAL_IDENTITY_REPAIR_REQUIRED", state["error_code"])
        assertEquals(0, fixture.registrations)
        assertTrue(fixture.commands.none { it.contains("replacement") })
    }

    private class Fixture {
        val request =
            InitialIdentityCoordinator.Request(
                "00000000-0000-4000-8000-000000000001",
                "00000000-0000-4000-8000-000000000002",
                "register",
                null,
            )
        var busy = false
        var ready = false
        var material = false
        var registrations = 0
        var journal: JSONObject? = null
        val commands = mutableListOf<String>()
        val coordinator =
            InitialIdentityCoordinator(
                command = { command ->
                    commands.add(command.getString("command"))
                    when (command.getString("command")) {
                        "begin_initial_identity" -> journal = JSONObject(command.toString()).put("phase", "pending")
                        "finish_initial_identity" -> journal?.put("phase", command.getString("phase"))
                    }
                    JSONObject().put("active_profile_id", request.profileId).put("initial_identity_operation", journal)
                },
                lease = { if (busy) null else AutoCloseable { } },
                ready = { _, _ -> ready },
                hasMaterial = { material },
            )
    }
}
