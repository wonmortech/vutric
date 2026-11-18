package io.wonmor.vutric_spike

import android.content.Context
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Handler
import android.os.Looper

/**
 * LinkManager (ADR-0009 part 2, backlog A6, route R3): no router, no internet. The phone opens a local-only
 * hotspot; the other device joins it (a Wi-Fi QR for phones, name and password for a computer), and the normal
 * Vutric pairing runs over it at full Wi-Fi speed. Wi-Fi Direct and Wi-Fi Aware come later (R6, R7).
 *
 * The only state is the OS reservation, which must be closed to turn the hotspot off.
 */
object LinkManager {
    private var reservation: WifiManager.LocalOnlyHotspotReservation? = null

    /** Calls [done] with `{ssid, password}` once the hotspot is up, or [failed] with a reason code. */
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

    @Suppress("DEPRECATION")
    private fun info(r: WifiManager.LocalOnlyHotspotReservation): Map<String, String>? {
        if (Build.VERSION.SDK_INT >= 30) {
            val c = r.softApConfiguration
            val ssid = (if (Build.VERSION.SDK_INT >= 33) c.wifiSsid?.toString() else c.ssid)?.trim('"') ?: return null
            return mapOf("ssid" to ssid, "password" to (c.passphrase ?: ""))
        }
        val c = r.wifiConfiguration ?: return null
        val ssid = c.SSID?.trim('"') ?: return null
        return mapOf("ssid" to ssid, "password" to (c.preSharedKey ?: "").trim('"'))
    }
}
