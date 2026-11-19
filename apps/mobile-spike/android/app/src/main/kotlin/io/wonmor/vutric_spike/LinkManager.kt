package io.wonmor.vutric_spike

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.wifi.WifiManager
import android.net.wifi.WifiNetworkSpecifier
import android.os.Build
import android.os.Handler
import android.os.Looper

/**
 * LinkManager (ADR-0009 part 2, backlog A6, route R3): no router, no internet.
 *
 * - **Host:** the phone opens a local-only hotspot. Its name, password and security go into the pairing link
 *   (ADR-0058), so one QR both joins the network and pairs; a phone without Vutric joins from a `WIFI:` QR.
 * - **Joiner:** a phone that scanned such a link joins the network with `WifiNetworkSpecifier` (Android 10+; the
 *   system asks the user once) and binds this process to it, so the Rust sockets opened afterwards go over the
 *   hotspot even though it has no internet. Leaving unbinds and releases the request.
 *
 * Wi-Fi Direct and Wi-Fi Aware come later (R6, R7). The only state is the OS reservation and the network request,
 * both of which must be closed to undo them.
 */
object LinkManager {
    private var reservation: WifiManager.LocalOnlyHotspotReservation? = null
    private var joined: ConnectivityManager.NetworkCallback? = null

    /** Calls [done] with `{ssid, password, security}` once the hotspot is up, or [failed] with a reason code. */
    fun start(context: Context, done: (Map<String, String>) -> Unit, failed: (String) -> Unit) {
        if (Build.VERSION.SDK_INT < 26) return failed("unsupported")
        reservation?.let { r ->
            val i = info(r)
            return if (i == null) failed("no_config") else done(i)
        }
        val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        try {
            wifi.startLocalOnlyHotspot(
                object : WifiManager.LocalOnlyHotspotCallback() {
                    override fun onStarted(r: WifiManager.LocalOnlyHotspotReservation) {
                        reservation = r
                        val i = info(r)
                        if (i == null) failed("no_config") else done(i)
                    }

                    override fun onStopped() {
                        reservation = null
                    }

                    override fun onFailed(reason: Int) {
                        reservation = null
                        failed(
                            when (reason) {
                                ERROR_TETHERING_DISALLOWED -> "disallowed"
                                ERROR_INCOMPATIBLE_MODE -> "incompatible"
                                ERROR_NO_CHANNEL -> "no_channel"
                                else -> "generic"
                            },
                        )
                    }
                },
                Handler(Looper.getMainLooper()),
            )
        } catch (_: SecurityException) {
            failed("permission")
        } catch (_: IllegalStateException) {
            // Already requested by this app and not closed yet.
            failed("busy")
        }
    }

    fun stop() {
        reservation?.close()
        reservation = null
    }

    /**
     * Joins the hotspot a scanned link carries and binds this process to it. [done] once the network is up,
     * [failed] with `unsupported` (Android 9 and older: the user joins in system settings), `unavailable` (the user
     * said no, or the network was not found within 30 s), `bad_network` or `permission`. Answers exactly once.
     */
    fun join(
        context: Context,
        ssid: String,
        password: String,
        security: String,
        hidden: Boolean,
        done: () -> Unit,
        failed: (String) -> Unit,
    ) {
        if (Build.VERSION.SDK_INT < 29) return failed("unsupported")
        leave(context)
        val cm = context.applicationContext.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        val spec = try {
            WifiNetworkSpecifier.Builder().setSsid(ssid).apply {
                when (security) {
                    "wpa3" -> setWpa3Passphrase(password)
                    "wpa2" -> setWpa2Passphrase(password)
                }
                if (hidden) setIsHiddenSsid(true)
            }.build()
        } catch (_: IllegalArgumentException) {
            return failed("bad_network")
        }
        val request = NetworkRequest.Builder()
            .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
            .removeCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
            .setNetworkSpecifier(spec)
            .build()
        val main = Handler(Looper.getMainLooper())
        var answered = false
        fun answer(block: () -> Unit) = main.post {
            if (!answered) {
                answered = true
                block()
            }
        }
        val callback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                cm.bindProcessToNetwork(network)
                answer(done)
            }

            override fun onUnavailable() {
                if (joined === this) joined = null
                answer { failed("unavailable") }
            }

            override fun onLost(network: Network) {
                // The hotspot went away: sockets fall back to the default network instead of a dead one.
                cm.bindProcessToNetwork(null)
            }
        }
        joined = callback
        try {
            cm.requestNetwork(request, callback, JOIN_TIMEOUT_MS)
        } catch (_: SecurityException) {
            joined = null
            answer { failed("permission") }
        }
    }

    /** Leaves a joined hotspot: unbinds the process and releases the request (the phone may rejoin its Wi-Fi). */
    fun leave(context: Context) {
        val cb = joined ?: return
        joined = null
        val cm = context.applicationContext.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        cm.bindProcessToNetwork(null)
        runCatching { cm.unregisterNetworkCallback(cb) }
    }

    private const val JOIN_TIMEOUT_MS = 30_000

    @Suppress("DEPRECATION")
    private fun info(r: WifiManager.LocalOnlyHotspotReservation): Map<String, String>? {
        if (Build.VERSION.SDK_INT >= 30) {
            val c = r.softApConfiguration
            val ssid = (if (Build.VERSION.SDK_INT >= 33) c.wifiSsid?.toString() else c.ssid)?.trim('"') ?: return null
            // 0 open, 1 WPA2, 2 WPA3 transition (WPA2 clients still join), 3 WPA3 only.
            val security = when (c.securityType) {
                0 -> "open"
                3 -> "wpa3"
                else -> "wpa2"
            }
            return mapOf("ssid" to ssid, "password" to (c.passphrase ?: ""), "security" to security)
        }
        val c = r.wifiConfiguration ?: return null
        val ssid = c.SSID?.trim('"') ?: return null
        return mapOf("ssid" to ssid, "password" to (c.preSharedKey ?: "").trim('"'), "security" to "wpa2")
    }
}
