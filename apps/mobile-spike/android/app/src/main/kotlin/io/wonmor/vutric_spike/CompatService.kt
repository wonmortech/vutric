package io.wonmor.vutric_spike

import android.app.Service
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.net.wifi.WifiManager
import android.os.Binder
import android.os.IBinder
import android.os.Parcel
import android.os.ParcelFileDescriptor
import java.util.concurrent.atomic.AtomicBoolean

/**
 * The foreign-app adapters (LocalSend, KDE Connect, magic-wormhole) on the phone (ADR-0066, backlog C10).
 *
 * They parse bytes from devices nobody has authenticated, so they do not run in the app process. This service runs
 * in its own non-exported `:compat` process (same UID: a separate address space with nothing of Vutric's in it).
 * The app process makes a socket pair, keeps one end and hands the other over in one Binder call ([SERVE]); the
 * service runs the Rust loop on it until the app sends `Stop` or closes its end, then stops itself. It never sees
 * the Vutric identity, the trust list or the settings, only what the app writes on the socket.
 */
class CompatService : Service() {
    private val running = AtomicBoolean(false)

    private val binder = object : Binder() {
        override fun onTransact(code: Int, data: Parcel, reply: Parcel?, flags: Int): Boolean {
            if (code != SERVE) return super.onTransact(code, data, reply, flags)
            val pfd = data.readFileDescriptor() ?: return false
            serve(pfd)
            return true
        }
    }

    override fun onBind(intent: Intent): IBinder = binder

    private fun serve(pfd: ParcelFileDescriptor) {
        if (!running.compareAndSet(false, true)) {
            // One loop per process; a second socket is closed, so its app side sees the loop is not there.
            runCatching { pfd.close() }
            return
        }
        val fd = pfd.detachFd()
        Thread {
            // The adapters announce and listen on multicast (LocalSend 224.0.0.167, KDE Connect broadcast).
            val wifi = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
            val lock = wifi.createMulticastLock("vutric-compat").apply {
                setReferenceCounted(false)
                acquire()
            }
            try {
                runCatching { CompatNative.serve(fd) }
            } finally {
                lock.release()
                running.set(false)
                stopSelf()
            }
        }.apply {
            name = "vutric-compat"
            start()
        }
    }

    companion object {
        /** The one Binder call: the parcel carries the service's end of the socket pair. */
        const val SERVE = IBinder.FIRST_CALL_TRANSACTION
    }
}

/** The Rust entry of the `:compat` process (`crates/vutric-mobile/src/compat_service.rs`). */
object CompatNative {
    init {
        System.loadLibrary("vutric_mobile")
    }

    /** Runs the adapter loop on [fd] (a connected stream socket it takes over) until it ends; 0 when clean. */
    @JvmStatic
    external fun serve(fd: Int): Int
}

/**
 * The app process's side: binds [CompatService], gives it one end of a fresh socket pair, and returns the other
 * end's descriptor for Rust (`compat_start`), which then owns it.
 */
object CompatHost {
    private var conn: ServiceConnection? = null

    fun open(context: Context, done: (Int?) -> Unit) {
        close(context)
        val pair = runCatching { ParcelFileDescriptor.createSocketPair() }.getOrNull()
        if (pair == null) {
            done(null)
            return
        }
        val (mine, theirs) = pair[0] to pair[1]
        var answered = false
        val c = object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName, service: IBinder) {
                if (answered) return
                answered = true
                val p = Parcel.obtain()
                val ok = try {
                    p.writeFileDescriptor(theirs.fileDescriptor)
                    service.transact(CompatService.SERVE, p, null, 0)
                } catch (_: Exception) {
                    false
                } finally {
                    p.recycle()
                    // The service has its own copy now (Binder duplicates it); ours is closed.
                    runCatching { theirs.close() }
                }
                if (ok) {
                    done(mine.detachFd())
                } else {
                    runCatching { mine.close() }
                    done(null)
                }
            }

            override fun onServiceDisconnected(name: ComponentName) {}
        }
        val bound = runCatching {
            context.bindService(Intent(context, CompatService::class.java), c, Context.BIND_AUTO_CREATE)
        }.getOrDefault(false)
        if (bound) {
            conn = c
        } else {
            runCatching { mine.close() }
            runCatching { theirs.close() }
            done(null)
        }
    }

    fun close(context: Context) {
        conn?.let { runCatching { context.unbindService(it) } }
        conn = null
    }
}
