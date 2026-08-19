package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Test

class TunRestartPolicyTest {
    private val identity =
        TunIdentity(
            profileId = "8c30b771-9ebd-457a-b67b-bbc74a1ddba6",
            mtu = 1280,
            dnsMode = "tunnel",
            dnsV4 = "1.1.1.1",
            dnsV6 = "2606:4700:4700::1111",
            allowLan = false,
            bypassCidrs = emptyList(),
        )

    @Test
    fun sameTunIdentityRetainsFdWhenKillSwitchArmed() {
        assertEquals(
            TunRestartDecision.RETAIN,
            TunRestartPolicy.decide(
                killSwitch = true,
                tunnelFrontend = true,
                hasCurrentFd = true,
                sameIdentity = identity.sameForReuse(identity),
                userRequestedDisconnect = false,
            ),
        )
    }

    @Test
    fun routeChangeEstablishesNewBeforeClose() {
        val changed = identity.copy(bypassCidrs = listOf("10.0.0.0/8"))
        assertEquals(
            TunRestartDecision.REPLACE_NEW_FIRST,
            TunRestartPolicy.decide(
                killSwitch = true,
                tunnelFrontend = true,
                hasCurrentFd = true,
                sameIdentity = identity.sameForReuse(changed),
                userRequestedDisconnect = false,
            ),
        )
    }

    @Test
    fun killSwitchOffOrUserDisconnectClosesFd() {
        assertEquals(
            TunRestartDecision.TEARDOWN,
            TunRestartPolicy.decide(
                killSwitch = false,
                tunnelFrontend = true,
                hasCurrentFd = true,
                sameIdentity = true,
                userRequestedDisconnect = false,
            ),
        )
        assertEquals(
            TunRestartDecision.TEARDOWN,
            TunRestartPolicy.decide(
                killSwitch = true,
                tunnelFrontend = true,
                hasCurrentFd = true,
                sameIdentity = true,
                userRequestedDisconnect = true,
            ),
        )
    }

    @Test
    fun nativeFailureWithKillSwitchLeavesFdConceptuallyOpen() {
        // Failure is not a user disconnect and identity is unchanged: retain.
        assertEquals(
            TunRestartDecision.RETAIN,
            TunRestartPolicy.decide(
                killSwitch = true,
                tunnelFrontend = true,
                hasCurrentFd = true,
                sameIdentity = true,
                userRequestedDisconnect = false,
            ),
        )
    }
}
