package io.wonmor.vutric_spike

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.graphics.BitmapFactory
import android.os.Build

/**
 * Receive while Vutric is not on screen (backlog A11): a heads-up notification with the photo's thumbnail, or the
 * file offer, instead of drawing over other apps (no SYSTEM_ALERT_WINDOW). A tap opens the Qabul tab. Stateless.
 */
object IncomingNotice {
    private const val CHANNEL = "incoming"
    private const val ID = 7102

    /**
     * Posts the notice; returns whether the user can actually see it. False when notifications are off for the app
     * or for this channel: the caller must then not report the photo as shown.
     */
    @Suppress("DEPRECATION")
    fun show(context: Context, title: String, text: String, jpeg: ByteArray?): Boolean {
        val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val b = if (Build.VERSION.SDK_INT >= 26) {
            if (nm.getNotificationChannel(CHANNEL) == null) {
                nm.createNotificationChannel(
                    NotificationChannel(CHANNEL, "Kelgan rasm va fayllar", NotificationManager.IMPORTANCE_HIGH).apply {
                        description = "Boshqa qurilma rasm otganda yoki fayl yuborganda"
                    },
                )
            }
            Notification.Builder(context, CHANNEL)
        } else {
            Notification.Builder(context).setPriority(Notification.PRIORITY_HIGH).setDefaults(Notification.DEFAULT_ALL)
        }
        val open = PendingIntent.getActivity(
            context,
            2,
            Intent(context, MainActivity::class.java)
                .setAction(MainActivity.ACTION_RECEIVE)
                .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        b.setSmallIcon(android.R.drawable.stat_sys_download_done)
            .setContentTitle(title)
            .setContentText(text)
            .setContentIntent(open)
            .setAutoCancel(true)
            .setCategory(Notification.CATEGORY_MESSAGE)
        // The preview is already a small JPEG (preview-sized by the sender); decode it at most ~512 px wide.
        val bmp = jpeg?.let {
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeByteArray(it, 0, it.size, bounds)
            var sample = 1
            while (bounds.outWidth / (sample * 2) >= 512) sample *= 2
            BitmapFactory.decodeByteArray(it, 0, it.size, BitmapFactory.Options().apply { inSampleSize = sample })
        }
        if (bmp != null) {
            b.setLargeIcon(bmp).setStyle(Notification.BigPictureStyle().bigPicture(bmp).setSummaryText(text))
        }
        nm.notify(ID, b.build())
        return visible(nm)
    }

    private fun visible(nm: NotificationManager): Boolean {
        if (!nm.areNotificationsEnabled()) return false
        if (Build.VERSION.SDK_INT >= 26) {
            val ch = nm.getNotificationChannel(CHANNEL) ?: return false
            if (ch.importance == NotificationManager.IMPORTANCE_NONE) return false
        }
        return true
    }
}
