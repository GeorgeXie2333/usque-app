package io.github.georgexie2333.usque

import io.flutter.plugins.sharedpreferences.SharedPreferencesApi
import io.flutter.plugins.sharedpreferences.SharedPreferencesAsyncApi
import io.flutter.plugins.sharedpreferences.SharedPreferencesPigeonOptions
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Proxy
import java.util.concurrent.CancellationException
import kotlin.coroutines.Continuation
import kotlin.coroutines.EmptyCoroutineContext
import kotlin.coroutines.intrinsics.COROUTINE_SUSPENDED
import kotlin.coroutines.startCoroutine

class TrackedPreferencesApiTest {
    private class Completion<T> : Continuation<T> {
        override val context = EmptyCoroutineContext
        var result: Result<T>? = null

        override fun resumeWith(result: Result<T>) {
            check(this.result == null) { "Call completed twice" }
            this.result = result
        }
    }

    private fun <T> start(operation: suspend () -> T): Completion<T> = Completion<T>().also(operation::startCoroutine)

    private class PendingCall(
        val name: String,
        val arguments: List<Any?>,
        val continuation: Continuation<Any?>,
    )

    private class DeferredApi<T>(
        type: Class<T>,
        admitted: () -> Unit = {},
    ) {
        val pending = mutableListOf<PendingCall>()
        val api: T =
            requireNotNull(
                type.cast(
                    Proxy.newProxyInstance(type.classLoader, arrayOf(type)) { _, method, args ->
                        admitted()
                        @Suppress("UNCHECKED_CAST")
                        val continuation = args.last() as Continuation<Any?>
                        pending.add(PendingCall(method.name, args.dropLast(1), continuation))
                        COROUTINE_SUSPENDED
                    },
                ),
            )
    }

    @Test
    fun everyLegacyJniEntryHoldsWorkBeforeTheDelegateSuspends() {
        checkEveryEntry(SharedPreferencesApi::class.java, ::TrackedPreferencesApi)
    }

    @Test
    fun everyAsyncJniEntryHoldsWorkBeforeTheDelegateSuspends() {
        checkEveryEntry(SharedPreferencesAsyncApi::class.java, ::TrackedPreferencesAsyncApi)
    }

    private fun <T> checkEveryEntry(
        type: Class<T>,
        wrap: (T, TrackedPreferencesWork) -> T,
    ) {
        var leases = 0
        val work =
            TrackedPreferencesWork {
                leases++
                AutoCloseable { leases-- }
            }
        val delegate = DeferredApi(type) { assertEquals(1, leases) }
        val api = wrap(delegate.api, work)
        for (method in type.declaredMethods) {
            val completion = Completion<Any?>()
            val arguments =
                method.parameterTypes.dropLast(1).map { parameter ->
                    when (parameter) {
                        String::class.java -> "flutter.setting"
                        Boolean::class.javaPrimitiveType -> true
                        Long::class.javaPrimitiveType -> 42L
                        Double::class.javaPrimitiveType -> 1.25
                        List::class.java -> listOf("flutter.setting")
                        SharedPreferencesPigeonOptions::class.java -> SharedPreferencesPigeonOptions("settings", false)
                        else -> error("Add a fixture for the new preferences argument: $parameter")
                    }
                }
            assertSame(COROUTINE_SUSPENDED, method.invoke(api, *arguments.toTypedArray(), completion))
            assertNull(completion.result)
            assertEquals(1, leases)
            val pending = delegate.pending.removeAt(0)
            assertEquals(method.name, pending.name)
            assertEquals(arguments, pending.arguments)
            val result: Any? =
                when {
                    method.name == "getAll" -> mapOf("flutter.setting" to "saved")
                    type == SharedPreferencesApi::class.java -> true
                    method.name == "getKeys" -> listOf("flutter.setting")
                    method.name.startsWith("get") -> null
                    else -> Unit
                }
            pending.continuation.resumeWith(Result.success(result))
            assertNotNull(completion.result)
            assertEquals(result, completion.result!!.getOrThrow())
            assertEquals(0, leases)
        }
    }

    @Test
    fun acceptedWritePreventsExitAcrossDetachAndLaterCallsAreRejected() {
        var exit: (() -> Unit)? = null
        var exits = 0
        val coordinator =
            UiProcessExitCoordinator(
                schedule = { _, action ->
                    exit = action
                    UiExitCancellation { exit = null }
                },
                terminate = { exits++ },
            )
        val activity = Any()
        coordinator.activityCreated(activity)
        val work = TrackedPreferencesWork(coordinator::holdBackgroundWork)
        val delegate = DeferredApi(SharedPreferencesApi::class.java)
        val api = TrackedPreferencesApi(delegate.api, work)
        val accepted = start { api.setString("flutter.setting", "saved") }
        assertNull(accepted.result)
        work.stopAccepting()
        coordinator.activityDestroyed(
            activity,
            isMainActivity = true,
            isFinishing = true,
            isChangingConfigurations = false,
        )
        assertNull(exit)
        val rejected = start { api.setString("flutter.setting", "too late") }
        assertTrue(rejected.result!!.exceptionOrNull() is IllegalStateException)
        assertEquals(1, delegate.pending.size)
        delegate.pending
            .single()
            .continuation
            .resumeWith(Result.success(true))
        assertTrue(accepted.result!!.getOrThrow())
        assertEquals(0, exits)
        assertNotNull(exit)
        exit!!.invoke()
        assertEquals(1, exits)
    }

    @Test
    fun failingAndCancelledSuspendedWritesReleaseOnlyTheirOwnLeases() {
        var leases = 0
        val work =
            TrackedPreferencesWork {
                leases++
                AutoCloseable { leases-- }
            }
        val delegate = DeferredApi(SharedPreferencesAsyncApi::class.java)
        val api = TrackedPreferencesAsyncApi(delegate.api, work)
        val options = SharedPreferencesPigeonOptions(useDataStore = true)
        val failed = start { api.setString("first", "value", options) }
        val cancelled = start { api.setString("second", "value", options) }
        assertEquals(2, leases)
        work.stopAccepting()
        val failure = IllegalStateException("disk failure")
        delegate.pending[0].continuation.resumeWith(Result.failure(failure))
        assertSame(failure, failed.result!!.exceptionOrNull())
        assertEquals(1, leases)
        val cancellation = CancellationException("cancelled")
        delegate.pending[1].continuation.resumeWith(Result.failure(cancellation))
        assertSame(cancellation, cancelled.result!!.exceptionOrNull())
        assertEquals(0, leases)
    }

    @Test
    fun immediateCompletionAndFailureReleaseWork() {
        var leases = 0
        val work =
            TrackedPreferencesWork {
                leases++
                AutoCloseable { leases-- }
            }
        val completed =
            start {
                work.run {
                    assertEquals(1, leases)
                    42
                }
            }
        assertEquals(42, completed.result!!.getOrThrow())
        assertEquals(0, leases)
        val failure = IllegalArgumentException("rejected value")
        val failed = start { work.run { throw failure } }
        assertSame(failure, failed.result!!.exceptionOrNull())
        assertEquals(0, leases)
    }

    @Test
    fun detachingOneEngineDoesNotStopAnotherEngine() {
        val first = TrackedPreferencesWork { AutoCloseable { } }
        val second = TrackedPreferencesWork { AutoCloseable { } }
        first.stopAccepting()
        assertTrue(start { first.run { true } }.result!!.isFailure)
        assertFalse(start { second.run { false } }.result!!.getOrThrow())
    }
}
