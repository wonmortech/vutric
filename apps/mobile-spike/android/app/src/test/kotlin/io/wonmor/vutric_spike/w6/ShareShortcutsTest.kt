package io.wonmor.vutric_spike.w6

import android.content.Context
import android.content.Intent
import androidx.core.content.pm.ShortcutManagerCompat
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ShareShortcutsTest {
    private val ctx: Context get() = RuntimeEnvironment.getApplication()
    private val office = ShareShortcuts.Device("k1", "Ofis kompyuteri", "desktop")
    private val phone = ShareShortcuts.Device("k2", "Ali telefoni", "phone")

    @Test fun planDropsBadAndDuplicateDevicesAndKeepsTheOrder() {
        val plan = ShareShortcuts.plan(listOf(office, office, ShareShortcuts.Device("", "x", "phone"), phone), 4)
        assertEquals(listOf("k1", "k2"), plan.map { it.id })
        assertEquals(1, ShareShortcuts.plan(listOf(office, phone), 1).size)
    }

    @Test fun longNamesAreCut() {
        assertEquals("Ofis", ShareShortcuts.shortLabel("  Ofis "))
        assertEquals(24, ShareShortcuts.shortLabel("x".repeat(40)).length)
        assertEquals("Vutric", ShareShortcuts.shortLabel(" "))
    }

    @Test fun theShareIntentNamesTheDevice() {
        val i = Intent(Intent.ACTION_SEND).putExtra(ShortcutManagerCompat.EXTRA_SHORTCUT_ID, ShareShortcuts.shortcutId("k1"))
        assertEquals("k1", ShareShortcuts.deviceOf(i))
        assertNull(ShareShortcuts.deviceOf(Intent(Intent.ACTION_SEND)))
        assertNull(ShareShortcuts.deviceOf(Intent(Intent.ACTION_SEND).putExtra(ShortcutManagerCompat.EXTRA_SHORTCUT_ID, "other")))
        assertNull(ShareShortcuts.deviceOf(null))
    }

    @Test fun publishFollowsTheTrustList() {
        assertEquals(2, ShareShortcuts.publish(ctx, listOf(office, phone)))
        var ids = ShortcutManagerCompat.getDynamicShortcuts(ctx).map { it.id }.toSet()
        assertEquals(setOf("device:k1", "device:k2"), ids)
        val s = ShortcutManagerCompat.getDynamicShortcuts(ctx).first { it.id == "device:k1" }
        assertTrue(s.categories!!.contains(ShareShortcuts.CATEGORY))
        assertEquals("Ofis kompyuteri", s.shortLabel.toString())

        // The phone was forgotten: its shortcut goes with the next list.
        ShareShortcuts.publish(ctx, listOf(office))
        ids = ShortcutManagerCompat.getDynamicShortcuts(ctx).map { it.id }.toSet()
        assertEquals(setOf("device:k1"), ids)

        ShareShortcuts.clear(ctx)
        assertTrue(ShortcutManagerCompat.getDynamicShortcuts(ctx).isEmpty())
    }
}
