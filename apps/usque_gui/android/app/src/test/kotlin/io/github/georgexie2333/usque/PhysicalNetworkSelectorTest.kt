package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PhysicalNetworkSelectorTest {
    @Test
    fun `keeps current network while score is tied`() {
        val selected =
            choosePhysicalNetwork(
                currentHandle = 22,
                candidates =
                    listOf(
                        PhysicalNetworkCandidate(11, 130, 3),
                        PhysicalNetworkCandidate(22, 130, 1),
                    ),
            )

        assertEquals(PhysicalNetworkSelection(22, 1), selected)
    }

    @Test
    fun `selects latest usable family and ignores route-less candidates`() {
        val selected =
            choosePhysicalNetwork(
                currentHandle = 11,
                candidates =
                    listOf(
                        PhysicalNetworkCandidate(11, 140, 0),
                        PhysicalNetworkCandidate(33, 120, 1),
                    ),
            )

        assertEquals(PhysicalNetworkSelection(33, 1), selected)
    }

    @Test
    fun `returns no physical network when every family is unavailable`() {
        assertNull(
            choosePhysicalNetwork(
                currentHandle = null,
                candidates = listOf(PhysicalNetworkCandidate(11, 140, 0)),
            ),
        )
    }
}
