package io.wonmor.vutric_spike.w6

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.media.MediaPlayer
import android.media.RingtoneManager
import android.media.ToneGenerator
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.VibrationAttributes
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import io.wonmor.vutric_spike.R

/**
 * F14 "ring my phone", Android side (ADR-0133, W3's `Message::Ring` of ADR-0069, issue #18).
 *
 * Dart hands over a ring that W3's engine already admitted (`ring::accepts` + `RingLimit`). Kotlin then:
 * - plays the alarm sound on the **alarm stream** (`USAGE_ALARM`), which sounds in silent and vibrate mode; Do Not
 *   Disturb still wins where the user's DND policy blocks alarms, and the alarm volume is raised for the ring only
 *   and put back afterwards;
 * - vibrates with alarm attributes;
 * - posts a high-priority notification with a "Topdim" action. On Android 14+ it is a full-screen intent only when the
 *   system still grants `USE_FULL_SCREEN_INTENT` (`canUseFullScreenIntent`); otherwise it stays a heads-up.
 *
 * It stops on the user's "Topdim" (event `found`, so Dart sends `Session::stop_ring()`), on the sender's `Ring::Stop`
 * (`stop(remote = true)`, no event), or when `secs` run out (event `timeout`).
 */
object Ringer {
    private const val CHANNEL = "w6_ring"
    private const val NOTICE_ID = 7131
    const val ACTION_FOUND = "io.wonmor.vutric.w6.RING_FOUND"
    const val EXTRA_FROM = "from"

    private val main = Handler(Looper.getMainLooper())
    private var player: MediaPlayer? = null
    private var tone: ToneGenerator? = null
    private var focus: AudioFocusRequest? = null
    private var restoreVolume: Int? = null
    private var ringing = false
    private var stopAt: Runnable? = null

    val isRinging: Boolean get() = ringing

    /** Starts (or restarts, with the new length) a ring for [secs] seconds; [from] is the paired device's name. */
    fun start(context: Context, secs: Int, from: String?) {
        val app = context.applicationContext
        val plan = RingPlan.of(secs)
        main.post {
            stopSound(app)
            ringing = true
            playSound(app)
            vibrate(app, plan)
            notify(app, from)
            stopAt?.let { main.removeCallbacks(it) }
            stopAt = Runnable { finish(app, RingPlan.End.TIMEOUT) }.also { main.postDelayed(it, plan.millis) }
        }
    }

    /** The sender's `Ring::Stop` ([remote] = true), or the app's own stop. */
    fun stop(context: Context, remote: Boolean = true) {
        val app = context.applicationContext
        main.post { finish(app, if (remote) RingPlan.End.REMOTE else RingPlan.End.FOUND) }
    }

    /** The user's "Topdim", from the notification action or [RingActivity]. */
    fun found(context: Context) {
        val app = context.applicationContext
        main.post { finish(app, RingPlan.End.FOUND) }
    }

    private fun finish(context: Context, end: RingPlan.End) {
        if (!ringing) return
        ringing = false
        stopAt?.let { main.removeCallbacks(it) }
        stopAt = null
        stopSound(context)
        context.getSystemService(NotificationManager::class.java)?.cancel(NOTICE_ID)
        RingActivity.closeAll()
        end.event?.let { W6.emit(mapOf("kind" to "w6.ring", "state" to it)) }
    }

    private fun alarmAttributes(): AudioAttributes =
        AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_ALARM)
            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
            .build()

    private fun playSound(context: Context) {
        val audio = context.getSystemService(AudioManager::class.java)
        if (audio != null) {
            // Loud on purpose: the phone is lost somewhere. Put back in stopSound. Refused under some DND policies.
            runCatching {
                val max = audio.getStreamMaxVolume(AudioManager.STREAM_ALARM)
                val now = audio.getStreamVolume(AudioManager.STREAM_ALARM)
                if (now < max) {
                    audio.setStreamVolume(AudioManager.STREAM_ALARM, max, 0)
                    restoreVolume = now
                }
            }
            if (Build.VERSION.SDK_INT >= 26) {
                val req = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT).setAudioAttributes(alarmAttributes()).build()
                runCatching { audio.requestAudioFocus(req) }
                focus = req
            }
        }
        val uri = soundUri(context)
        val mp = uri?.let { u ->
            runCatching {
                MediaPlayer().apply {
                    setAudioAttributes(alarmAttributes())
                    setDataSource(context, u)
                    isLooping = true
                    prepare()
                    start()
                }
            }.getOrNull()
        }
        if (mp != null) {
            player = mp
            return
        }
        // No alarm sound on this phone (some builds ship none): a loud generated tone instead.
        tone = runCatching {
            ToneGenerator(AudioManager.STREAM_ALARM, ToneGenerator.MAX_VOLUME).apply {
                startTone(ToneGenerator.TONE_CDMA_ALERT_CALL_GUARD, RingPlan.MAX_SECS * 1000)
            }
        }.getOrNull()
    }

    private fun soundUri(context: Context): Uri? =
        listOf(RingtoneManager.TYPE_ALARM, RingtoneManager.TYPE_RINGTONE, RingtoneManager.TYPE_NOTIFICATION)
            .firstNotNullOfOrNull { runCatching { RingtoneManager.getActualDefaultRingtoneUri(context, it) }.getOrNull() }

    private fun stopSound(context: Context) {
        player?.let { runCatching { it.stop() }; runCatching { it.release() } }
        player = null
        tone?.let { runCatching { it.stopTone() }; runCatching { it.release() } }
        tone = null
        val audio = context.getSystemService(AudioManager::class.java)
        restoreVolume?.let { v -> runCatching { audio?.setStreamVolume(AudioManager.STREAM_ALARM, v, 0) } }
        restoreVolume = null
        if (Build.VERSION.SDK_INT >= 26) focus?.let { f -> runCatching { audio?.abandonAudioFocusRequest(f) } }
        focus = null
        vibrator(context)?.cancel()
    }

    private fun vibrator(context: Context): Vibrator? =
        if (Build.VERSION.SDK_INT >= 31) {
            context.getSystemService(VibratorManager::class.java)?.defaultVibrator
        } else {
            @Suppress("DEPRECATION")
            context.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
        }

    private fun vibrate(context: Context, plan: RingPlan) {
        val v = vibrator(context)?.takeIf { it.hasVibrator() } ?: return
        runCatching {
            when {
                Build.VERSION.SDK_INT >= 33 -> v.vibrate(
                    VibrationEffect.createWaveform(plan.pattern, 0),
                    VibrationAttributes.createForUsage(VibrationAttributes.USAGE_ALARM),
                )
                Build.VERSION.SDK_INT >= 26 -> @Suppress("DEPRECATION") v.vibrate(VibrationEffect.createWaveform(plan.pattern, 0), alarmAttributes())
                else -> @Suppress("DEPRECATION") v.vibrate(plan.pattern, 0, alarmAttributes())
            }
        }
    }

    private fun notify(context: Context, from: String?) {
        val nm = context.getSystemService(NotificationManager::class.java) ?: return
        if (Build.VERSION.SDK_INT >= 26 && nm.getNotificationChannel(CHANNEL) == null) {
            nm.createNotificationChannel(
                NotificationChannel(CHANNEL, context.getString(R.string.w6_ring_channel), NotificationManager.IMPORTANCE_HIGH).apply {
                    description = context.getString(R.string.w6_ring_channel_desc)
                    // The sound is ours, on the alarm stream; the channel stays silent so it is not played twice.
                    setSound(null, null)
                    enableVibration(false)
                    setBypassDnd(false)
                },
            )
        }
        val text = if (from.isNullOrBlank()) {
            context.getString(R.string.w6_ring_text_unknown)
        } else {
            context.getString(R.string.w6_ring_text, from.take(60))
        }
        val flags = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        val found = PendingIntent.getBroadcast(
            context,
            0,
            Intent(context, RingStopReceiver::class.java).setAction(ACTION_FOUND),
            flags,
        )
        val screen = PendingIntent.getActivity(
            context,
            1,
            Intent(context, RingActivity::class.java).putExtra(EXTRA_FROM, from).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            flags,
        )
        val b = if (Build.VERSION.SDK_INT >= 26) {
            Notification.Builder(context, CHANNEL)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(context).setPriority(Notification.PRIORITY_MAX)
        }
        b.setSmallIcon(android.R.drawable.ic_lock_idle_alarm)
            .setContentTitle(context.getString(R.string.w6_ring_title))
            .setContentText(text)
            .setCategory(Notification.CATEGORY_ALARM)
            .setOngoing(true)
            .setAutoCancel(false)
            .setContentIntent(screen)
            .setDeleteIntent(found)
            .addAction(Notification.Action.Builder(null, context.getString(R.string.w6_ring_found), found).build())
            .setVisibility(Notification.VISIBILITY_PUBLIC)
        if (RingPlan.fullScreen(Build.VERSION.SDK_INT, canUseFullScreen(nm))) b.setFullScreenIntent(screen, true)
        runCatching { nm.notify(NOTICE_ID, b.build()) }
    }

    private fun canUseFullScreen(nm: NotificationManager): Boolean =
        if (Build.VERSION.SDK_INT >= 34) nm.canUseFullScreenIntent() else true
}

/** Pure parts of a ring (unit-tested without Android). */
data class RingPlan(val millis: Long, val pattern: LongArray) {
    enum class End(val event: String?) { FOUND("found"), TIMEOUT("timeout"), REMOTE(null) }

    override fun equals(other: Any?): Boolean = other is RingPlan && millis == other.millis && pattern.contentEquals(other.pattern)

    override fun hashCode(): Int = 31 * millis.hashCode() + pattern.contentHashCode()

    companion object {
        /** The same bounds as `vutric_core::ring::MAX_RING_SECS` and `DEFAULT_RING_SECS`. */
        const val MAX_SECS = 60
        const val DEFAULT_SECS = 30

        /** Off/on pairs: a long buzz, a short gap, repeated from index 0 until the ring stops. */
        private val PATTERN = longArrayOf(0, 900, 400, 900, 400, 1500, 800)

        fun of(secs: Int): RingPlan {
            val s = if (secs <= 0) DEFAULT_SECS else secs.coerceAtMost(MAX_SECS)
            return RingPlan(s * 1000L, PATTERN.copyOf())
        }

        /**
         * Android 14+ grants `USE_FULL_SCREEN_INTENT` by default only to calling and alarm apps; the user can revoke
         * it on any version. Without it a full-screen intent would silently become a heads-up anyway, so ask the
         * system and only set it when it is granted.
         */
        fun fullScreen(sdk: Int, granted: Boolean): Boolean = if (sdk >= 34) granted else true
    }
}

/** The "Topdim" action and the swipe-away of the ring notification. Not exported. */
class RingStopReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == Ringer.ACTION_FOUND) Ringer.found(context)
    }
}
