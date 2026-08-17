package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class ZeroTrustCallbackSessionTest {
    @Test
    fun matchingCallbackIsConsumedOnlyOnce() {
        val session = ZeroTrustCallbackSession()
        assertEquals(
            "https://example-team.cloudflareaccess.com/warp",
            session.begin(" Example-Team "),
        )
        val callback =
            "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=assertion"
        assertTrue(session.accept(callback))
        assertEquals(callback, session.consume())
        assertNull(session.consume())
        assertFalse(session.accept(callback))
    }

    @Test
    fun callbackRequiresAnActiveSameTeamLogin() {
        val session = ZeroTrustCallbackSession()
        val callback =
            "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=assertion"
        assertFalse(session.accept(callback))
        session.begin("other-team")
        assertFalse(session.accept(callback))
        assertNull(session.consume())
    }

    @Test
    fun cancellationAndProcessReplacementDiscardState() {
        val session = ZeroTrustCallbackSession()
        session.begin("example-team")
        session.cancel()
        assertFalse(
            session.accept(
                "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=assertion",
            ),
        )

        val replacementProcess = ZeroTrustCallbackSession()
        assertNull(replacementProcess.consume())
    }

    @Test
    fun malformedCallbacksAndTeamsAreRejected() {
        assertThrows(IllegalArgumentException::class.java) {
            ZeroTrustCallbackSession.normalizeTeam("team.example")
        }
        val invalidCallbacks =
            listOf(
                "https://example-team.cloudflareaccess.com/auth?token=x",
                "com.cloudflare.warp://example-team.cloudflareaccess.com/warp?token=x",
                "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=x&token=y",
                "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?state=x",
            )
        invalidCallbacks.forEach { callback ->
            val session = ZeroTrustCallbackSession()
            session.begin("example-team")
            assertFalse(session.accept(callback))
        }
    }
}
