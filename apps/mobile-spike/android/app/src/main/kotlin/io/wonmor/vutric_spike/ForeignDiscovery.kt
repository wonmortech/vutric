package io.wonmor.vutric_spike

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Message
import android.os.Messenger
import java.net.DatagramPacket
import java.net.InetAddress
import java.net.MulticastSocket
import java.util.concurrent.atomic.AtomicBoolean

/**
 * ForeignDiscovery (ADR-0010 part 1, backlog A13): LocalSend and KDE Connect devices on this Wi-Fi, found while a
 * Vutric screen asks for them (never in the background, never announcing this phone).
 *
 * - **KDE Connect** announces `_kdeconnect._udp` over mDNS. Android's `NsdManager` reads the records; we only take
 *   four TXT values (`id`, `name`, `type`, `protocol`), each cut to 64 characters.
 * - **LocalSend** announces a JSON datagram on `224.0.0.167:53317`. Following the ADR-0010 rule that foreign
 *   bytes are parsed before authentication only in a low-privilege process, this class never parses it: the raw
 *   datagram (4 KiB at most) goes to [ForeignParser], an `isolatedProcess` service with no permissions and no
 *   network, which answers with a few capped strings.
 *
 * This is only the list in the "Qanday ulanamiz?" sheet. Sending to these devices, receiving from them and
 * LocalSend's register handshake run in the W3 adapters inside [CompatService] (`:compat` process, ADR-0066), opened
 * from that list. Both bind UDP 53317 with address reuse, so they can run at the same time. Each device found is
 * handed to Flutter as an event: `{kind: "foreign", app, id, name, type, host, port}`.
 */
object ForeignDiscovery {
    @Volatile
    var sink: ((Map<String, Any>) -> Unit)? = null

    private const val KDE_SERVICE = "_kdeconnect._udp."
    private const val LOCALSEND_GROUP = "224.0.0.167"
    private const val LOCALSEND_PORT = 53317
    private const val MAX_DATAGRAM = 4096
    private const val MAX_FIELD = 64

    private var nsd: NsdManager? = null
    private var kdeListener: NsdManager.DiscoveryListener? = null
    private var lock: WifiManager.MulticastLock? = null
    private var socket: MulticastSocket? = null
    private var parser: Messenger? = null
    private var bound: ServiceConnection? = null
    private val running = AtomicBoolean(false)
    private val main = Handler(Looper.getMainLooper())

    fun start(context: Context) {
        if (!running.compareAndSet(false, true)) return
        val wifi = context.getSystemService(Context.WIFI_SERVICE) as WifiManager
        lock = wifi.createMulticastLock("vutric-foreign").apply {
            setReferenceCounted(false)
            acquire()
        }
        startKde(context)
        startLocalSend(context)
    }

    fun stop() {
        if (!running.compareAndSet(true, false)) return
        kdeListener?.let { l -> runCatching { nsd?.stopServiceDiscovery(l) } }
        kdeListener = null
        socket?.close()
        socket = null
        lock?.release()
        lock = null
        parser = null
    }

    /** Unbinds the parser service; called when the app's context goes away. */
    fun unbind(context: Context) {
        bound?.let { runCatching { context.unbindService(it) } }
        bound = null
    }

    private fun emit(event: Map<String, Any>) = main.post { sink?.invoke(event) }

    private fun cap(s: String?): String = s.orEmpty().filter { !it.isISOControl() }.take(MAX_FIELD)

    // ── KDE Connect over mDNS ────────────────────────────────────────────────────────────────────────────────

    private fun startKde(context: Context) {
        val m = context.getSystemService(Context.NSD_SERVICE) as NsdManager
        nsd = m
        val listener = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(serviceType: String) {}
            override fun onDiscoveryStopped(serviceType: String) {}
            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {}
            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {}
            override fun onServiceLost(info: NsdServiceInfo) {}
            override fun onServiceFound(info: NsdServiceInfo) {
                resolve(m, info)
            }
        }
        kdeListener = listener
        runCatching { m.discoverServices(KDE_SERVICE, NsdManager.PROTOCOL_DNS_SD, listener) }
    }

    @Suppress("DEPRECATION") // resolveService is the only resolver below Android 14; one at a time is enough here.
    private fun resolve(m: NsdManager, info: NsdServiceInfo) {
        runCatching {
            m.resolveService(
                info,
                object : NsdManager.ResolveListener {
                    override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {}
                    override fun onServiceResolved(s: NsdServiceInfo) {
                        val txt = s.attributes
                        fun value(k: String) = cap(txt[k]?.let { String(it, Charsets.UTF_8) })
                        val id = value("id").ifEmpty { cap(s.serviceName) }
                        emit(
                            mapOf(
                                "kind" to "foreign",
                                "app" to "kdeconnect",
                                "id" to id,
                                "name" to value("name").ifEmpty { cap(s.serviceName) },
                                "type" to value("type"),
                                "host" to (s.host?.hostAddress ?: ""),
                                "port" to s.port,
                            ),
                        )
                    }
                },
            )
        }
    }

    // ── LocalSend multicast, parsed in the isolated process ─────────────────────────────────────────────────

    private fun startLocalSend(context: Context) {
        val conn = object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName, service: IBinder) {
                parser = Messenger(service)
            }

            override fun onServiceDisconnected(name: ComponentName) {
                parser = null
            }
        }
        if (bound == null &&
            context.bindService(Intent(context, ForeignParser::class.java), conn, Context.BIND_AUTO_CREATE)
        ) {
            bound = conn
        }
        val replies = Messenger(
            Handler(Looper.getMainLooper()) { msg ->
                val b = msg.data
                val id = cap(b.getString("id"))
                if (id.isNotEmpty() && running.get()) {
                    emit(
                        mapOf(
                            "kind" to "foreign",
                            "app" to "localsend",
                            "id" to id,
                            "name" to cap(b.getString("name")),
                            "type" to cap(b.getString("type")),
                            "host" to cap(b.getString("host")),
                            "port" to b.getInt("port"),
                        ),
                    )
                }
                true
            },
        )
        Thread {
            val s = runCatching {
                MulticastSocket(LOCALSEND_PORT).apply {
                    reuseAddress = true
                    @Suppress("DEPRECATION")
                    joinGroup(InetAddress.getByName(LOCALSEND_GROUP))
                    soTimeout = 1000
                }
            }.getOrNull() ?: return@Thread
            socket = s
            val buf = ByteArray(MAX_DATAGRAM + 1)
            while (running.get()) {
                val p = DatagramPacket(buf, buf.size)
                try {
                    s.receive(p)
                } catch (_: java.net.SocketTimeoutException) {
                    continue
                } catch (_: Exception) {
                    break
                }
                // Anything longer than the cap is dropped unread, as the desktop adapter does (ADR-0061).
                if (p.length > MAX_DATAGRAM) continue
                val target = parser ?: continue
                val msg = Message.obtain(null, ForeignParser.PARSE_LOCALSEND).apply {
                    data = Bundle().apply {
                        putByteArray("bytes", p.data.copyOf(p.length))
                        putString("host", p.address?.hostAddress ?: "")
                    }
                    replyTo = replies
                }
                runCatching { target.send(msg) }
            }
            runCatching { s.close() }
        }.apply {
            name = "vutric-foreign"
            isDaemon = true
            start()
        }
    }
}
