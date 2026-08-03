package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PhysicalNetworkMonitorTest {
    @Test
    fun selectionChangeDetectionDrivesGenerationBumps() {
        assertFalse(
            hasUnderlyingSelectionChanged(
                previousHandle = 11L,
                previousFamilyMask = FAMILY_IPV4 or FAMILY_IPV6,
                selectedHandle = 11L,
                selectedFamilyMask = FAMILY_IPV4 or FAMILY_IPV6,
            ),
        )
        assertTrue(
            hasUnderlyingSelectionChanged(
                previousHandle = 11L,
                previousFamilyMask = FAMILY_IPV4,
                selectedHandle = 22L,
                selectedFamilyMask = FAMILY_IPV4,
            ),
        )
        assertTrue(
            hasUnderlyingSelectionChanged(
                previousHandle = 11L,
                previousFamilyMask = FAMILY_IPV4,
                selectedHandle = 11L,
                selectedFamilyMask = FAMILY_IPV4 or FAMILY_IPV6,
            ),
        )
        assertTrue(
            hasUnderlyingSelectionChanged(
                previousHandle = null,
                previousFamilyMask = 0,
                selectedHandle = 5L,
                selectedFamilyMask = FAMILY_IPV4,
            ),
        )
    }

    @Test
    fun networkRestartGenerationBumpsOnlyWhenSelectionChanges() {
        val generation = NetworkRestartGeneration()
        assertEquals(0L, generation.get())

        assertNull(
            generation.bumpIfChanged(
                hasUnderlyingSelectionChanged(
                    previousHandle = 1L,
                    previousFamilyMask = 1,
                    selectedHandle = 1L,
                    selectedFamilyMask = 1,
                ),
            ),
        )
        assertEquals(0L, generation.get())

        assertEquals(
            1L,
            generation.bumpIfChanged(
                hasUnderlyingSelectionChanged(
                    previousHandle = 1L,
                    previousFamilyMask = 1,
                    selectedHandle = 2L,
                    selectedFamilyMask = 1,
                ),
            ),
        )
        assertEquals(1L, generation.get())

        assertEquals(
            2L,
            generation.bumpIfChanged(
                hasUnderlyingSelectionChanged(
                    previousHandle = 2L,
                    previousFamilyMask = FAMILY_IPV4,
                    selectedHandle = 2L,
                    selectedFamilyMask = FAMILY_IPV6,
                ),
            ),
        )
        assertEquals(2L, generation.get())
        assertEquals(3L, generation.bump())
        assertEquals(3L, generation.get())
    }

    @Test
    fun pureSelectorStillPrefersStableCurrentNetwork() {
        val selected =
            choosePhysicalNetwork(
                currentHandle = 22,
                candidates =
                    listOf(
                        PhysicalNetworkCandidate(11, 130, FAMILY_IPV4 or FAMILY_IPV6),
                        PhysicalNetworkCandidate(22, 130, FAMILY_IPV4),
                    ),
            )
        assertEquals(PhysicalNetworkSelection(22, FAMILY_IPV4), selected)
    }
}
