package io.wonmor.vutric_spike.w6

import android.app.NotificationManager
import android.content.Context
import android.os.Looper
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.time.Duration

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class RingerTest {
    private val ctx: Context get() = RuntimeEnvironment.getApplication()
    private val events = mutableListOf<Map<String, Any?>>()

    @Before fun sink() {
        W6.sink = { events += it }
    }

    @After fun clear() {
        Ringer.stop(ctx, remote = true)
        idle()
        W6.sink = null
    }

    private fun idle() = shadowOf(Looper.getMainLooper()).idle()

    private fun notices() = shadowOf(ctx.getSystemService(NotificationManager::class.java)).allNotifications

    @Test fun aRingShowsTheNoticeWithTheFoundAction() {
        Ringer.start(ctx, 10, "Ofis kompyuteri")
        idle()
        assertTrue(Ringer.isRinging)
        val n = notices().single()
        assertEquals(ctx.getString(io.wonmor.vutric_spike.R.string.w6_ring_found), n.actions.single().title.toString())
    }

    @Test fun foundTellsDartAndClearsTheNotice() {
        Ringer.start(ctx, 10, null)
        idle()
        Ringer.found(ctx)
        idle()
        assertEquals(false, Ringer.isRinging)
        assertTrue(notices().isEmpty())
        assertEquals(listOf(mapOf("kind" to "w6.ring", "state" to "found")), events)
    }

    @Test fun theSendersStopIsSilent() {
        Ringer.start(ctx, 10, null)
        idle()
        Ringer.stop(ctx, remote = true)
        idle()
        assertEquals(false, Ringer.isRinging)
        assertTrue(events.isEmpty())
    }

    @Test fun itStopsByItselfWhenTheSecondsRunOut() {
        Ringer.start(ctx, 2, null)
        idle()
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofSeconds(3))
        assertEquals(false, Ringer.isRinging)
        assertEquals(listOf(mapOf("kind" to "w6.ring", "state" to "timeout")), events)
    }
}
