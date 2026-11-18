package io.wonmor.vutric_spike

import android.app.Notification
import android.content.ComponentName
import android.content.Context
import android.provider.Settings
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification

/**
 * Companion (ADR-0009 part 2, backlog A12): notifications mirrored to the paired computer. Opt-in twice: the user
 * turns it on in Vutric, and Android asks for notification access on its own settings page. Off by default.
 *
 * The listener keeps no state of its own. The switch and the allowed apps live in a small preferences file the
 * Dart side writes; only apps on that list are mirrored, and an empty list mirrors nothing. Each notification that
 * passes is handed to Flutter as an event while the app process runs; Dart sends it to a paired device only
 * (`Message::Notice`, ADR-0065).
 * Ongoing notifications (music, calls, our own progress), group summaries and Vutric's own are never mirrored,
 * and the text is cut short before it leaves this class.
 */
class CompanionListener : NotificationListenerService() {
    override fun onNotificationPosted(sbn: StatusBarNotification) {
        if (!CompanionPrefs.mirroring(this)) return
        if (sbn.packageName == packageName || sbn.isOngoing) return
        val n = sbn.notification ?: return
        if (n.flags and Notification.FLAG_GROUP_SUMMARY != 0) return
        // Only apps the user picked; an empty list mirrors nothing (the safer default, owner question 2026-11-18).
        if (sbn.packageName !in CompanionPrefs.apps(this)) return
        val extras = n.extras ?: return
        val title = extras.getCharSequence(Notification.EXTRA_TITLE)?.toString().orEmpty().take(MAX_TITLE)
        val text = extras.getCharSequence(Notification.EXTRA_TEXT)?.toString().orEmpty().take(MAX_TEXT)
        if (title.isEmpty() && text.isEmpty()) return
        val app = runCatching {
            packageManager.getApplicationLabel(packageManager.getApplicationInfo(sbn.packageName, 0)).toString()
        }.getOrDefault(sbn.packageName)
        CompanionPrefs.sink?.invoke(
            mapOf(
                "kind" to "notice",
                "package" to sbn.packageName,
                "app" to app.take(MAX_TITLE),
                "title" to title,
                "text" to text,
                "at" to sbn.postTime,
            ),
        )
    }

    companion object {
        private const val MAX_TITLE = 80
        private const val MAX_TEXT = 400
    }
}

object CompanionPrefs {
    private const val PREFS = "vutric_companion"

    /** Set by MainActivity while Flutter runs; null otherwise (nothing is kept for later). */
    @Volatile
    var sink: ((Map<String, Any>) -> Unit)? = null

    fun mirroring(context: Context): Boolean =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean("on", false)

    fun apps(context: Context): Set<String> =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getStringSet("apps", emptySet()) ?: emptySet()

    fun configure(context: Context, on: Boolean, apps: List<String>) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .putBoolean("on", on)
            .putStringSet("apps", apps.toSet())
            .apply()
    }

    /** Whether the user gave Vutric notification access in system settings. */
    fun hasAccess(context: Context): Boolean {
        val me = ComponentName(context, CompanionListener::class.java).flattenToString()
        val enabled = Settings.Secure.getString(context.contentResolver, "enabled_notification_listeners") ?: ""
        return enabled.split(':').any { it == me }
    }
}
