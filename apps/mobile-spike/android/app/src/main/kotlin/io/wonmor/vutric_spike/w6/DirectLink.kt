package io.wonmor.vutric_spike.w6

import android.annotation.SuppressLint
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.net.wifi.p2p.WifiP2pConfig
import android.net.wifi.p2p.WifiP2pInfo
import android.net.wifi.p2p.WifiP2pManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.annotation.RequiresApi
import androidx.core.content.ContextCompat
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.security.SecureRandom
import java.util.concurrent.Executors

/**
 * R7 Wi-Fi Direct (ADR-0131): Android ⇄ Android with no router, and a Windows or Linux PC as a legacy client.
 *
 * - **Host** ([host]): this phone becomes the group owner of a group with our own name and passphrase
 *   (`WifiP2pConfig.Builder`, Android 10+, so neither side sees a system "accept connection" dialog). The answer has
 *   the same `{ssid, password, security}` shape as W1's hotspot (`LinkManager.start`), so the pairing link and the
 *   `WIFI:` QR of ADR-0058 carry it unchanged; a PC joins it as an ordinary Wi-Fi network. It listens on
 *   [LinkRules.DIRECT_PORT] (or a free port when that is taken, returned as `port`) for a phone that joins over P2P.
 * - **Joiner** ([join]): joins the group by name and passphrase and connects to the group owner's address.
 *
 * The socket goes to [LinkSockets]. The group owner address is on Android's local-network route, so iroh's own
 * UDP socket may reach it too; W3 #23 asks which of the two paths the engine takes.
 *
 * Events: `{"kind": "w6.direct", "state": ...}` with `hosting` (+ credentials), `joining`, `joined`, `failed`
 * (+ `reason`), `stopped`.
 */
@RequiresApi(29)
@SuppressLint("MissingPermission") // W6.permissions asks NEARBY_WIFI_DEVICES (13+) or location (10-12) first
object DirectLink {
    private const val CONNECT_TRIES = 10
    private const val CONNECT_GAP_MS = 500L

    private val main = Handler(Looper.getMainLooper())
    private val io = Executors.newCachedThreadPool { r -> Thread(r, "w6-direct").apply { isDaemon = true } }
    private var manager: WifiP2pManager? = null
    private var channel: WifiP2pManager.Channel? = null
    private var server: ServerSocket? = null
    private var receiver: BroadcastReceiver? = null
    private var appContext: Context? = null

    fun supported(context: Context): Boolean =
        Build.VERSION.SDK_INT >= 29 && context.packageManager.hasSystemFeature(PackageManager.FEATURE_WIFI_DIRECT)

    /** Calls [done] with `{ssid, password, security, port}` once the group is up, or [failed] with a reason code. */
    fun host(context: Context, done: (Map<String, Any>) -> Unit, failed: (String) -> Unit) {
        val (m, ch) = open(context) ?: return failed("unsupported")
        val (name, pass) = LinkRules.directCredentials(SecureRandom())
        val config = WifiP2pConfig.Builder()
            .setNetworkName(name)
            .setPassphrase(pass)
            .enablePersistentMode(false)
            .setGroupOperatingBand(WifiP2pConfig.GROUP_OWNER_BAND_AUTO)
            .build()
        val created = object : WifiP2pManager.ActionListener {
            override fun onSuccess() {
                val ss = listen() ?: return failed("socket")
                val out = mapOf("ssid" to name, "password" to pass, "security" to "wpa2", "port" to ss.localPort)
                W6.emit(mapOf("kind" to "w6.direct", "state" to "hosting") + out)
                io.execute {
                    while (!ss.isClosed) {
                        val s = runCatching { ss.accept() }.getOrNull() ?: break
                        LinkSockets.hand("direct", "listener", s)
                    }
                }
                done(out)
            }

            override fun onFailure(reason: Int) = failed(reasonCode(reason))
        }
        // A group left over from an earlier run would make createGroup fail with BUSY: remove it first, then create.
        val removed = object : WifiP2pManager.ActionListener {
            override fun onSuccess() = m.createGroup(ch, config, created)

            override fun onFailure(reason: Int) = m.createGroup(ch, config, created)
        }
        m.removeGroup(ch, removed)
    }

    /** Joins the group from a scanned link and connects to the group owner on [port]. */
    fun join(context: Context, ssid: String, password: String, port: Int, failed: (String) -> Unit) {
        if (!LinkRules.validDirect(ssid, password)) return failed("bad_network")
        val (m, ch) = open(context) ?: return failed("unsupported")
        val app = context.applicationContext
        watchConnection(app, m, ch, if (port in 1..65535) port else LinkRules.DIRECT_PORT)
        W6.emit(mapOf("kind" to "w6.direct", "state" to "joining"))
        m.connect(
            ch,
            WifiP2pConfig.Builder().setNetworkName(ssid).setPassphrase(password).build(),
            object : WifiP2pManager.ActionListener {
                override fun onSuccess() {}

                override fun onFailure(reason: Int) = failed(reasonCode(reason))
            },
        )
    }

    fun stop() {
        main.post {
            runCatching { server?.close() }
            server = null
            val ctx = appContext
            receiver?.let { r -> runCatching { ctx?.unregisterReceiver(r) } }
            receiver = null
            val m = manager
            val ch = channel
            if (m != null && ch != null) {
                m.cancelConnect(ch, null)
                m.removeGroup(ch, null)
                runCatching { ch.close() }
            }
            channel = null
            W6.emit(mapOf("kind" to "w6.direct", "state" to "stopped"))
        }
    }

    private fun open(context: Context): Pair<WifiP2pManager, WifiP2pManager.Channel>? {
        if (!supported(context)) return null
        val app = context.applicationContext
        appContext = app
        val m = manager ?: app.getSystemService(WifiP2pManager::class.java)?.also { manager = it } ?: return null
        val ch = channel ?: m.initialize(app, Looper.getMainLooper()) { channel = null }?.also { channel = it } ?: return null
        return m to ch
    }

    private fun listen(): ServerSocket? {
        server?.let { if (!it.isClosed) return it }
        val ss = runCatching { ServerSocket().apply { reuseAddress = true; bind(InetSocketAddress(LinkRules.DIRECT_PORT)) } }
            .getOrElse { runCatching { ServerSocket(0) }.getOrNull() }
        server = ss
        return ss
    }

    private fun watchConnection(app: Context, m: WifiP2pManager, ch: WifiP2pManager.Channel, port: Int) {
        receiver?.let { runCatching { app.unregisterReceiver(it) } }
        var connected = false
        val r = object : BroadcastReceiver() {
            override fun onReceive(c: Context, intent: Intent) {
                if (intent.action != WifiP2pManager.WIFI_P2P_CONNECTION_CHANGED_ACTION || connected) return
                m.requestConnectionInfo(ch) { info: WifiP2pInfo? ->
                    val owner = info?.groupOwnerAddress
                    if (info == null || !info.groupFormed || info.isGroupOwner || owner == null || connected) return@requestConnectionInfo
                    connected = true
                    W6.emit(mapOf("kind" to "w6.direct", "state" to "joined", "address" to (owner.hostAddress ?: "")))
                    io.execute { connect(owner, port) }
                }
            }
        }
        receiver = r
        ContextCompat.registerReceiver(app, r, IntentFilter(WifiP2pManager.WIFI_P2P_CONNECTION_CHANGED_ACTION), ContextCompat.RECEIVER_NOT_EXPORTED)
    }

    /** The group owner's server may come up a moment after the group: a few short retries. */
    private fun connect(owner: java.net.InetAddress, port: Int) {
        repeat(CONNECT_TRIES) {
            val s = runCatching { Socket().apply { connect(InetSocketAddress(owner, port), 3_000) } }.getOrNull()
            if (s != null) {
                LinkSockets.hand("direct", "dialer", s)
                return
            }
            Thread.sleep(CONNECT_GAP_MS)
        }
        W6.emit(mapOf("kind" to "w6.direct", "state" to "failed", "reason" to "connect"))
    }

    private fun reasonCode(reason: Int): String = when (reason) {
        WifiP2pManager.P2P_UNSUPPORTED -> "unsupported"
        WifiP2pManager.BUSY -> "busy"
        WifiP2pManager.NO_SERVICE_REQUESTS -> "no_service"
        else -> "generic"
    }
}
