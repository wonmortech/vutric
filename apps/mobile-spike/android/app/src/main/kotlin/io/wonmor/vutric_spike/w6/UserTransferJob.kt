package io.wonmor.vutric_spike.w6

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.Build
import android.os.SystemClock
import androidx.annotation.RequiresApi
import io.wonmor.vutric_spike.MainActivity
import io.wonmor.vutric_spike.R

/**
 * F7 (ADR-0130): on Android 14+ a transfer keeps going with the app closed as a **user-initiated data transfer job**,
 * which has no fixed time cap (Android 15 caps the `dataSync` foreground service at 6 hours a day) and shows in the
 * system's task manager as a transfer. W1's `TransferService` (ADR-0012 decision 3) stays the path on Android 7-13
 * and wherever the job cannot run; [update] is called first and answers whether the job took the transfer.
 *
 * As with the service, the bytes move in Rust in this process: the job only holds the process at transfer priority
 * and carries the progress notification. It keeps no transfer state of its own.
 */
@RequiresApi(34)
class UserTransferJob : JobService() {
    override fun onStartJob(params: JobParameters): Boolean {
        active = this
        current = params
        startedAt = SystemClock.elapsedRealtime()
        setNotification(params, NOTICE_ID, notice(this, lastTitle, lastText, lastPermille), JOB_END_NOTIFICATION_POLICY_REMOVE)
        // A stop that came while the job was waiting to start.
        if (!wanted) finish()
        return true
    }

    /** The system ended it (the user stopped it in the task manager, or the network went away). */
    override fun onStopJob(params: JobParameters): Boolean {
        active = null
        current = null
        if (params.stopReason == JobParameters.STOP_REASON_USER) {
            // The user pressed Stop in the task manager: the transfer itself is cancelled (Dart hears it), and no
            // keep-alive comes back behind their back.
            stoppedByUser = true
            W6.emit(mapOf("kind" to "w6.transfer", "state" to "stopped_by_user"))
        } else {
            // Network gone or a quota: the next update falls back to the foreground service.
            lostJob = true
        }
        return false
    }

    private fun finish() {
        val p = current ?: return
        active = null
        current = null
        jobFinished(p, false)
    }

    companion object {
        private const val JOB_ID = 7102
        private const val NOTICE_ID = 7101
        private const val CHANNEL = "transfers"

        /** How long a scheduled job may take to start before the foreground service gets the transfer instead. */
        private const val START_GRACE_MS = 5_000L

        @Volatile private var active: UserTransferJob? = null
        @Volatile private var current: JobParameters? = null
        @Volatile private var scheduledAt = 0L
        @Volatile private var startedAt = 0L
        @Volatile private var wanted = false
        @Volatile private var stoppedByUser = false
        @Volatile private var lostJob = false
        @Volatile private var lastTitle = "Vutric"
        @Volatile private var lastText = ""
        @Volatile private var lastPermille = -1

        /** Whether the job holds a transfer right now (MainActivity keeps the hotspot up for it, as for the service). */
        val isRunning: Boolean get() = active != null

        /**
         * Takes the transfer when it can: Android 14+, a validated network (the job needs a network constraint, so
         * hotspot, Aware and Direct transfers stay on the service), and either already running or scheduled now,
         * while the app is in front. Answers false to let `TransferService` handle it.
         */
        fun update(context: Context, title: String, text: String, permille: Int): Boolean {
            if (Build.VERSION.SDK_INT < 34) return false
            lastTitle = title
            lastText = text
            lastPermille = permille
            val job = active
            val params = current
            if (job != null && params != null) {
                job.setNotification(params, NOTICE_ID, notice(job, title, text, permille), JobService.JOB_END_NOTIFICATION_POLICY_REMOVE)
                return true
            }
            val now = SystemClock.elapsedRealtime()
            val choice = KeepAlive.choose(
                sdk = Build.VERSION.SDK_INT,
                validatedNetwork = validatedNetwork(context),
                scheduled = wanted && !lostJob,
                waitedMs = if (scheduledAt == 0L) 0 else now - scheduledAt,
                graceMs = START_GRACE_MS,
                stoppedByUser = stoppedByUser,
                lostJob = lostJob,
            )
            return when (choice) {
                KeepAlive.Choice.WAIT, KeepAlive.Choice.NONE -> true
                KeepAlive.Choice.SERVICE -> {
                    if (wanted) cancel(context)
                    false
                }
                KeepAlive.Choice.SCHEDULE -> schedule(context, now)
            }
        }

        /** The transfer ended: the job finishes and its notification goes away. */
        fun stop(context: Context) {
            if (Build.VERSION.SDK_INT < 34) return
            stoppedByUser = false
            lostJob = false
            if (!wanted && active == null) return
            wanted = false
            active?.finish()
            cancel(context)
        }

        private fun schedule(context: Context, now: Long): Boolean {
            val scheduler = context.getSystemService(JobScheduler::class.java) ?: return false
            val info = JobInfo.Builder(JOB_ID, ComponentName(context, UserTransferJob::class.java))
                .setUserInitiated(true)
                .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                .build()
            // Refused when the app is not visible or the permission is missing: the service takes it.
            val ok = runCatching { scheduler.schedule(info) == JobScheduler.RESULT_SUCCESS }.getOrDefault(false)
            if (ok) {
                wanted = true
                scheduledAt = now
            }
            return ok
        }

        private fun cancel(context: Context) {
            wanted = false
            scheduledAt = 0L
            runCatching { context.getSystemService(JobScheduler::class.java)?.cancel(JOB_ID) }
        }

        private fun validatedNetwork(context: Context): Boolean {
            val cm = context.getSystemService(ConnectivityManager::class.java) ?: return false
            val caps = cm.getNetworkCapabilities(cm.activeNetwork ?: return false) ?: return false
            return caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
                caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
        }

        private fun notice(context: Context, title: String, text: String, permille: Int): Notification {
            val nm = context.getSystemService(NotificationManager::class.java)
            // The same channel as TransferService, so the user has one switch for transfer notifications.
            if (nm.getNotificationChannel(CHANNEL) == null) {
                nm.createNotificationChannel(
                    NotificationChannel(CHANNEL, context.getString(R.string.w6_transfer_channel), NotificationManager.IMPORTANCE_LOW).apply {
                        description = context.getString(R.string.w6_transfer_channel_desc)
                        setShowBadge(false)
                    },
                )
            }
            val open = PendingIntent.getActivity(
                context,
                0,
                Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            return Notification.Builder(context, CHANNEL)
                .setSmallIcon(android.R.drawable.stat_sys_upload)
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

/** The pure choice between the Android 14 job and W1's foreground service (unit-tested, no Android). */
object KeepAlive {
    /** SCHEDULE a job, WAIT for the scheduled one, hand it to the SERVICE, or NONE (the user stopped it). */
    enum class Choice { SCHEDULE, WAIT, SERVICE, NONE }

    fun choose(
        sdk: Int,
        validatedNetwork: Boolean,
        scheduled: Boolean,
        waitedMs: Long,
        graceMs: Long,
        stoppedByUser: Boolean = false,
        lostJob: Boolean = false,
    ): Choice = when {
        sdk < 34 -> Choice.SERVICE
        // The user stopped it in the task manager: no job and no service behind their back.
        stoppedByUser -> Choice.NONE
        // The system ended the job (network, quota): the service is the fallback.
        lostJob -> Choice.SERVICE
        scheduled && waitedMs < graceMs -> Choice.WAIT
        // Scheduled but never started (no network the scheduler accepts): the service holds it instead.
        scheduled -> Choice.SERVICE
        !validatedNetwork -> Choice.SERVICE
        else -> Choice.SCHEDULE
    }
}
