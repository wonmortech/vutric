package io.wonmor.vutric_spike.w6

import android.content.Context
import android.content.Intent
import androidx.core.content.pm.ShortcutInfoCompat
import androidx.core.content.pm.ShortcutManagerCompat
import androidx.core.graphics.drawable.IconCompat
import io.wonmor.vutric_spike.MainActivity
import io.wonmor.vutric_spike.R

/**
 * F2 (ADR-0130): every paired device as a direct target in the system Share sheet ("Ofis kompyuteri").
 *
 * Android 10+ reads the dynamic sharing shortcuts with the category of `res/xml/w6_share_targets.xml`; Android 7-9
 * gets them through androidx.sharetarget's `ChooserTargetServiceCompat` (the replacement for `ChooserTargetService`).
 * The shortcuts come from the trust list W3's engine keeps: Dart passes the whole current list every time it changes,
 * so this object keeps no state and a forgotten device disappears with the next call.
 */
object ShareShortcuts {
    const val CATEGORY = "io.wonmor.vutric.category.DEVICE_SHARE_TARGET"
    private const val PREFIX = "device:"

    /** One paired device, as Dart reads it from `devices()`: id (endpoint key), name, kind (`desktop`/`phone`). */
    data class Device(val id: String, val name: String, val kind: String)

    /** The shortcuts to publish: valid devices only, de-duplicated, at most [max], in the order given (most recent first). */
    fun plan(devices: List<Device>, max: Int): List<Device> =
        devices
            .filter { it.id.isNotBlank() && it.id.length <= 128 }
            .distinctBy { it.id }
            .take(max.coerceAtLeast(0))

    fun shortcutId(deviceId: String): String = PREFIX + deviceId

    /** The device a Share sheet shortcut stands for, or null for a plain share to Vutric. */
    fun deviceOf(intent: Intent?): String? {
        val id = intent?.getStringExtra(ShortcutManagerCompat.EXTRA_SHORTCUT_ID) ?: return null
        return if (id.startsWith(PREFIX)) id.removePrefix(PREFIX).ifBlank { null } else null
    }

    /** Short label: the Share sheet cuts long names, so it is trimmed here with an ellipsis. */
    fun shortLabel(name: String): String {
        val n = name.trim().ifEmpty { "Vutric" }
        return if (n.length <= SHORT_LABEL) n else n.take(SHORT_LABEL - 1).trimEnd() + "…"
    }

    /** Replaces every Vutric sharing shortcut with [devices]. Answers how many were published. */
    fun publish(context: Context, devices: List<Device>): Int {
        val max = ShortcutManagerCompat.getMaxShortcutCountPerActivity(context).takeIf { it > 0 } ?: DEFAULT_MAX
        val chosen = plan(devices, max)
        val shortcuts = chosen.mapIndexed { rank, d -> build(context, d, rank) }
        val keep = shortcuts.map { it.id }.toSet()
        val stale = ShortcutManagerCompat.getDynamicShortcuts(context).map { it.id }.filter { it.startsWith(PREFIX) && it !in keep }
        if (stale.isNotEmpty()) remove(context, stale)
        // add, not set: set would also drop any other dynamic shortcut the app publishes later.
        if (shortcuts.isNotEmpty() && !ShortcutManagerCompat.addDynamicShortcuts(context, shortcuts)) return 0
        return shortcuts.size
    }

    /** Removes them all (the trust list was cleared, or the user turned direct share off). */
    fun clear(context: Context) {
        val ours = ShortcutManagerCompat.getDynamicShortcuts(context).map { it.id }.filter { it.startsWith(PREFIX) }
        if (ours.isNotEmpty()) remove(context, ours)
    }

    /** Ranks a device higher in the Share sheet after a send to it. */
    fun used(context: Context, deviceId: String) {
        ShortcutManagerCompat.reportShortcutUsed(context, shortcutId(deviceId))
    }

    /** Dynamic and long-lived (cached by the Share sheet) copies both go. */
    private fun remove(context: Context, ids: List<String>) {
        ShortcutManagerCompat.removeDynamicShortcuts(context, ids)
        ShortcutManagerCompat.removeLongLivedShortcuts(context, ids)
    }

    private fun build(context: Context, d: Device, rank: Int): ShortcutInfoCompat {
        val icon = if (d.kind == "phone") R.drawable.w6_ic_phone else R.drawable.w6_ic_desktop
        // Launching the shortcut from the launcher opens the app; from the Share sheet Android sends ACTION_SEND
        // with EXTRA_SHORTCUT_ID instead, which ShareEntry and deviceOf read.
        val open = Intent(context, MainActivity::class.java)
            .setAction(Intent.ACTION_MAIN)
            .putExtra(ShortcutManagerCompat.EXTRA_SHORTCUT_ID, shortcutId(d.id))
        return ShortcutInfoCompat.Builder(context, shortcutId(d.id))
            .setShortLabel(shortLabel(d.name))
            .setLongLabel(context.getString(R.string.w6_share_long_label, d.name.trim().take(LONG_LABEL)))
            .setIcon(IconCompat.createWithResource(context, icon))
            .setIntent(open)
            .setCategories(setOf(CATEGORY))
            .setLongLived(true)
            .setRank(rank)
            .build()
    }

    private const val SHORT_LABEL = 24
    private const val LONG_LABEL = 60
    private const val DEFAULT_MAX = 4
}
