package io.wonmor.vutric_spike.w6

import android.content.Context
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.wifi.aware.AttachCallback
import android.net.wifi.aware.DiscoverySession
import android.net.wifi.aware.DiscoverySessionCallback
import android.net.wifi.aware.PeerHandle
import android.net.wifi.aware.PublishConfig
import android.net.wifi.aware.PublishDiscoverySession
import android.net.wifi.aware.SubscribeConfig
import android.net.wifi.aware.SubscribeDiscoverySession
import android.net.wifi.aware.WifiAwareManager
import android.net.wifi.aware.WifiAwareNetworkInfo
import android.net.wifi.aware.WifiAwareNetworkSpecifier
import android.net.wifi.aware.WifiAwareSession
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.annotation.RequiresApi
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.Executors

/**
 * R6 Wi-Fi Aware (NAN), Android ⇄ Android with no router and no hotspot (ADR-0131).
 *
 * - **Listener** ([publish]): publishes service `vutric` with the caller's service info (at most 255 bytes; W3's
 *   codec decides what is in it), waits for a subscriber's hello, opens a `ServerSocket`, and answers the data path
 *   request with its port (`WifiAwareNetworkSpecifier.setPort`).
 * - **Dialer** ([subscribe]): finds the service, says hello, requests the data path, reads the peer's IPv6 and port
 *   from `WifiAwareNetworkInfo` and connects through that `Network`'s socket factory (the link-local address is only
 *   reachable through it).
 *
 * Both ends hand the connected socket to [LinkSockets]. The data path is encrypted with the PSK when the caller gives
 * one (8-63 characters, e.g. from the pairing secret); the Vutric handshake on top authenticates the peer either way.
 *
 * Discovery runs on Android 8+, the socket part needs Android 10+ (`WifiAwareNetworkInfo`), so [supported] says 10+.
 * Events: `{"kind": "w6.aware", "state": ...}` with `attached`, `publishing`, `subscribing`, `discovered`,
 * `connecting`, `failed` (+ `reason`), `stopped`.
 */
@RequiresApi(29)
object AwareLink {
    const val SERVICE = "vutric"
    private val HELLO = byteArrayOf(0x56, 0x55, 0x01)
    private const val DATA_PATH_TIMEOUT_MS = 20_000

    private val main = Handler(Looper.getMainLooper())
    private val io = Executors.newCachedThreadPool { r -> Thread(r, "w6-aware").apply { isDaemon = true } }
    private var session: WifiAwareSession? = null
    private var discovery: DiscoverySession? = null
    private var server: ServerSocket? = null
    private val callbacks = mutableListOf<ConnectivityManager.NetworkCallback>()
    private var cm: ConnectivityManager? = null

    fun supported(context: Context): Boolean =
        Build.VERSION.SDK_INT >= 29 && context.packageManager.hasSystemFeature(PackageManager.FEATURE_WIFI_AWARE)

    /** Supported and on right now (Aware is off while the phone is a hotspot, or in some power modes). */
    fun available(context: Context): Boolean =
        supported(context) && context.getSystemService(WifiAwareManager::class.java)?.isAvailable == true

    fun publish(context: Context, info: ByteArray, psk: String?) {
        val p = LinkRules.psk(psk) ?: return fail("psk")
        attach(context) { s ->
            val config = PublishConfig.Builder()
                .setServiceName(SERVICE)
                .setServiceSpecificInfo(info.copyOf(info.size.coerceAtMost(LinkRules.MAX_SERVICE_INFO)))
                .build()
            s.publish(
                config,
                object : DiscoverySessionCallback() {
                    override fun onPublishStarted(d: PublishDiscoverySession) {
                        discovery = d
                        emit("publishing")
                    }

                    override fun onMessageReceived(peer: PeerHandle, message: ByteArray) {
                        if (message.contentEquals(HELLO)) answer(context, discovery ?: return, peer, p)
                    }

                    override fun onSessionTerminated() = emit("stopped")
                },
                main,
            )
        }
    }

    fun subscribe(context: Context, psk: String?) {
        val p = LinkRules.psk(psk) ?: return fail("psk")
        attach(context) { s ->
            s.subscribe(
                SubscribeConfig.Builder().setServiceName(SERVICE).build(),
                object : DiscoverySessionCallback() {
                    private var asked = false

                    override fun onSubscribeStarted(d: SubscribeDiscoverySession) {
                        discovery = d
                        emit("subscribing")
                    }

                    override fun onServiceDiscovered(peer: PeerHandle, serviceInfo: ByteArray, matchFilter: List<ByteArray>) {
                        W6.emit(mapOf("kind" to "w6.aware", "state" to "discovered", "info" to serviceInfo))
                        if (asked) return
                        asked = true
                        discovery?.sendMessage(peer, 1, HELLO)
                        dial(context, discovery ?: return, peer, p)
                    }

                    override fun onMessageSendFailed(messageId: Int) {
                        asked = false
                    }

                    override fun onSessionTerminated() = emit("stopped")
                },
                main,
            )
        }
    }

    fun stop() {
        main.post {
            cm?.let { c -> callbacks.forEach { runCatching { c.unregisterNetworkCallback(it) } } }
            callbacks.clear()
            runCatching { server?.close() }
            server = null
            runCatching { discovery?.close() }
            discovery = null
            runCatching { session?.close() }
            session = null
        }
    }

    private fun attach(context: Context, then: (WifiAwareSession) -> Unit) {
        val app = context.applicationContext
        cm = app.getSystemService(ConnectivityManager::class.java)
        val wa = app.getSystemService(WifiAwareManager::class.java)
        if (!supported(app) || wa == null) return fail("unsupported")
        if (!wa.isAvailable) return fail("unavailable")
        session?.let { return then(it) }
        try {
            wa.attach(
                object : AttachCallback() {
                    override fun onAttached(s: WifiAwareSession) {
                        session = s
                        emit("attached")
                        then(s)
                    }

                    override fun onAttachFailed() = fail("attach")
                },
                main,
            )
        } catch (_: SecurityException) {
            fail("permission")
        }
    }

    /** Listener: a server socket first, then the data path request that carries its port. */
    private fun answer(context: Context, d: DiscoverySession, peer: PeerHandle, psk: LinkRules.Psk) {
        val ss = server ?: runCatching { ServerSocket(0) }.getOrNull()?.also { server = it } ?: return fail("socket")
        val spec = WifiAwareNetworkSpecifier.Builder(d, peer).apply {
            psk.value?.let { setPskPassphrase(it) }
            setPort(ss.localPort)
        }.build()
        request(spec) { _, _ -> }
        io.execute {
            val s = runCatching { ss.accept() }.getOrNull() ?: return@execute
            LinkSockets.hand("aware", "listener", s)
        }
    }

    /** Dialer: the data path, then a socket to the peer's address through that network. */
    private fun dial(context: Context, d: DiscoverySession, peer: PeerHandle, psk: LinkRules.Psk) {
        emit("connecting")
        val spec = WifiAwareNetworkSpecifier.Builder(d, peer).apply { psk.value?.let { setPskPassphrase(it) } }.build()
        var done = false
        request(spec) { network, caps ->
            val info = caps.transportInfo as? WifiAwareNetworkInfo ?: return@request
            val addr = info.peerIpv6Addr ?: return@request
            if (done || info.port <= 0) return@request
            done = true
            io.execute {
                val s: Socket? = runCatching { network.socketFactory.createSocket(addr, info.port) }.getOrNull()
                if (s == null) fail("connect") else LinkSockets.hand("aware", "dialer", s)
            }
        }
    }

    private fun request(spec: WifiAwareNetworkSpecifier, onCaps: (Network, NetworkCapabilities) -> Unit) {
        val c = cm ?: return fail("unsupported")
        val req = NetworkRequest.Builder()
            .addTransportType(NetworkCapabilities.TRANSPORT_WIFI_AWARE)
            .setNetworkSpecifier(spec)
            .build()
        val cb = object : ConnectivityManager.NetworkCallback() {
            override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) = onCaps(network, caps)

            override fun onUnavailable() = fail("data_path")

            override fun onLost(network: Network) = emit("lost")
        }
        callbacks += cb
        try {
            c.requestNetwork(req, cb, DATA_PATH_TIMEOUT_MS)
        } catch (_: SecurityException) {
            fail("permission")
        }
    }

    private fun emit(state: String) = W6.emit(mapOf("kind" to "w6.aware", "state" to state))

    private fun fail(reason: String) = W6.emit(mapOf("kind" to "w6.aware", "state" to "failed", "reason" to reason))
}

/** Pure rules shared by the Wi-Fi Aware and Wi-Fi Direct links (unit-tested). */
object LinkRules {
    /** Most service-specific bytes a Wi-Fi Aware publish carries (the spec limit is 255). */
    const val MAX_SERVICE_INFO = 255

    /** Wi-Fi Direct group owner listens here first; the joiner tries it unless the link says otherwise. */
    const val DIRECT_PORT = 47613

    /** A checked PSK: `value` null means an open data path (the Vutric handshake still authenticates). */
    data class Psk(val value: String?)

    /** Null when [psk] is given but not 8-63 printable ASCII characters (WPA rules). */
    fun psk(psk: String?): Psk? = when {
        psk.isNullOrEmpty() -> Psk(null)
        psk.length !in 8..63 -> null
        psk.any { it.code !in 0x20..0x7e } -> null
        else -> Psk(psk)
    }

    private const val ALPHABET = "abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789"

    /** Wi-Fi Direct group credentials: `DIRECT-xy` + our tag (Android requires the prefix), and a 16-char passphrase. */
    fun directCredentials(random: java.util.Random): Pair<String, String> {
        fun pick(n: Int) = (1..n).map { ALPHABET[random.nextInt(ALPHABET.length)] }.joinToString("")
        return "DIRECT-${pick(2)}-Vutric" to pick(16)
    }

    /** Whether [name] / [pass] are accepted by `WifiP2pConfig.Builder` (prefix, length rules). */
    fun validDirect(name: String, pass: String): Boolean =
        name.length in 9..32 && name.startsWith("DIRECT-") && name[7].isLetterOrDigit() && name[8].isLetterOrDigit() &&
            psk(pass)?.value != null
}
