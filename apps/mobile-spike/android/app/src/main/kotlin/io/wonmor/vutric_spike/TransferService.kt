package io.wonmor.vutric_spike

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import io.wonmor.vutric_spike.w6.UserTransferJob

/**
 * TransferJob (ADR-0009 part 2, backlog A4): keeps a big transfer going with the app in the background or the
 * screen off. The bytes move in Rust, in this same process; this service only keeps the process at foreground
 * priority and shows the progress notification. Type `dataSync` on every version (ADR-0012 explains why not the
 * Android 14 user-initiated job yet).
 *
 * Holds no transfer state: Flutter sends `update` and `stop` from the progress stream it already reads.
 */
class TransferService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            stopForegroundCompat()
            stopSelf()
            return START_NOT_STICKY
        }
        val n = build(
            this,
            intent?.getStringExtra(EXTRA_TITLE) ?: "Vutric",
            intent?.getStringExtra(EXTRA_TEXT) ?: "",
            intent?.getIntExtra(EXTRA_PERMILLE, -1) ?: -1,
        )
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            startForeground(ID, n)
        }
        // Killed by the system: the transfer died with the process, so there is nothing to come back to.
        return START_NOT_STICKY
    }

    /** Android 15 caps dataSync time; the system calls this instead of killing silently. */
    override fun onTimeout(startId: Int, fgsType: Int) {
        running = false
        stopForegroundCompat()
        stopSelf()
    }

    override fun onDestroy() {
        running = false
        super.onDestroy()
    }

    @Suppress("DEPRECATION")
    private fun stopForegroundCompat() {
        stopForeground(STOP_FOREGROUND_REMOVE)
    }

    companion object {
        private const val ID = 7101
        private const val CHANNEL = "transfers"
        private const val ACTION_STOP = "io.wonmor.vutric.TRANSFER_STOP"
        private const val EXTRA_TITLE = "title"
        private const val EXTRA_TEXT = "text"
        private const val EXTRA_PERMILLE = "permille"

        /** Set once the service is in the foreground, so updates never call startForegroundService again. */
        @Volatile private var running = false

        /** Whether a transfer still holds the foreground service (MainActivity keeps the hotspot up for it). */
        val isRunning: Boolean get() = running || UserTransferJob.isRunning

        /**
         * Starts the service (the first call, made while the app is in front) or updates its notification.
         * [permille] is 0..1000, or -1 while waiting. Updates go straight to the notification manager: Android 12+
         * refuses a background startForegroundService, and the user may have left the app mid-transfer.
         */
        fun update(context: Context, title: String, text: String, permille: Int) {
            // W6, F7 (ADR-0130): Android 14+ takes the transfer as a user-initiated job when it can.
            if (!running && UserTransferJob.update(context, title, text, permille)) return
            if (running) {
                val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
                nm.notify(ID, build(context, title, text, permille))
                return
            }
            val i = Intent(context, TransferService::class.java)
                .putExtra(EXTRA_TITLE, title)
                .putExtra(EXTRA_TEXT, text)
                .putExtra(EXTRA_PERMILLE, permille)
            try {
                if (Build.VERSION.SDK_INT >= 26) context.startForegroundService(i) else context.startService(i)
                running = true
            } catch (_: IllegalStateException) {
                // Not allowed from the background (ForegroundServiceStartNotAllowedException): the transfer goes on
                // while the app stays in front, only without the keep-alive.
            }
        }

        fun stop(context: Context) {
            UserTransferJob.stop(context)
            if (!running) return
            running = false
            context.startService(Intent(context, TransferService::class.java).setAction(ACTION_STOP))
        }

        @Suppress("DEPRECATION")
        private fun build(context: Context, title: String, text: String, permille: Int): Notification {
            val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            val b = if (Build.VERSION.SDK_INT >= 26) {
                if (nm.getNotificationChannel(CHANNEL) == null) {
                    nm.createNotificationChannel(
                        NotificationChannel(CHANNEL, "Uzatishlar", NotificationManager.IMPORTANCE_LOW).apply {
                            description = "Fayl yuborish va qabul qilish jarayoni"
                            setShowBadge(false)
                        },
                    )
                }
                Notification.Builder(context, CHANNEL)
            } else {
                Notification.Builder(context).setPriority(Notification.PRIORITY_LOW)
            }
            val open = PendingIntent.getActivity(
                context,
                0,
                Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            return b.setSmallIcon(android.R.drawable.stat_sys_upload)
                .setContentTitle(title)
                .setContentText(text)
                .setContentIntent(open)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .setCategory(Notification.CATEGORY_PROGRESS)
                .setProgress(1000, permille.coerceIn(0, 1000), permille < 0)
                .build()
        }
    }
}
