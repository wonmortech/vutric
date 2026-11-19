package io.wonmor.vutric_spike.w6

import android.annotation.SuppressLint
import android.content.Context
import android.net.wifi.SoftApConfiguration
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.SparseIntArray
import java.io.File
import java.io.InputStream
import java.net.Inet4Address
import java.net.NetworkInterface
import java.net.ServerSocket
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicLong

/**
 * M3 helper (ADR-0134): measures a LocalOnlyHotspot on 2.4 GHz versus 5 GHz on a real phone and writes one JSON
 * file per run, for W3's M3 row.
 *
 * - The band can only be **requested** on Android 16+ (`startLocalOnlyHotspotWithConfiguration` and
 *   `SoftApConfiguration.Builder.setChannels` became public in API 36; on 13-15 they are system APIs). Older phones
 *   run `default` and the JSON says the request was not possible; the band is then read on the client.
 * - Throughput needs a second device on the hotspot. The probe opens two TCP ports on the phone: `sink` counts
 *   what a client uploads, `source` sends [SOURCE_BYTES] zero bytes and closes. From a PC on the hotspot, e.g.
 *   `curl -T big.bin telnet://HOST:SINK` and `curl telnet://HOST:SOURCE -o NUL`.
 *
 * Runs for `seconds`, then closes the hotspot and writes `files/m3/m3-<band>-<epoch ms>.json` under the app's
 * external files folder (`adb pull /sdcard/Android/data/io.wonmor.vutric_spike/files/m3/`).
 */
@SuppressLint("MissingPermission") // the probe screen asks NEARBY_WIFI_DEVICES (13+) or location (8-12) first
object HotspotBandProbe {
    const val SOURCE_BYTES = 256L * 1024 * 1024

    private val main = Handler(Looper.getMainLooper())
    private val io = Executors.newCachedThreadPool { r -> Thread(r, "w6-m3").apply { isDaemon = true } }

    /** Live state for the probe screen. */
    fun interface Progress {
        fun update(line: String)
    }

    fun run(context: Context, band: String, seconds: Int, progress: Progress, done: (File?) -> Unit) {
        val app = context.applicationContext
        val wifi = app.getSystemService(WifiManager::class.java) ?: return done(null)
        val report = M3Report(
            band = M3Report.band(band),
            sdk = Build.VERSION.SDK_INT,
            device = "${Build.MANUFACTURER} ${Build.MODEL}",
            supports5 = wifi.is5GHzBandSupported,
            supports6 = Build.VERSION.SDK_INT >= 30 && wifi.is6GHzBandSupported,
            staApConcurrency = Build.VERSION.SDK_INT >= 30 && wifi.isStaApConcurrencySupported,
            seconds = seconds.coerceIn(10, 900),
        )
        val t0 = SystemClock.elapsedRealtime()
        val cb = object : WifiManager.LocalOnlyHotspotCallback() {
            override fun onStarted(reservation: WifiManager.LocalOnlyHotspotReservation) {
                report.startMs = SystemClock.elapsedRealtime() - t0
                describe(reservation, report)
                report.addresses = ipv4Addresses()
                measure(app, reservation, report, progress, done)
            }

            override fun onFailed(reason: Int) {
                report.failure = "lohs_$reason"
                main.post { done(write(app, report)) }
            }
        }
        try {
            val request = M3Report.channelsFor(report.band)
            if (request != null && Build.VERSION.SDK_INT >= 36) {
                report.bandRequested = true
                val config = SoftApConfiguration.Builder().setChannels(SparseIntArray().apply { put(request, 0) }).build()
                wifi.startLocalOnlyHotspotWithConfiguration(config, { main.post(it) }, cb)
            } else {
                report.bandRequested = false
                if (request != null) report.note = "band request needs Android 16 (API 36); ran the default band"
                wifi.startLocalOnlyHotspot(cb, main)
            }
            progress.update("hotspot: starting (${report.band})")
        } catch (e: Exception) {
            report.failure = e.javaClass.simpleName
            done(write(app, report))
        }
    }

    private fun describe(r: WifiManager.LocalOnlyHotspotReservation, report: M3Report) {
        if (Build.VERSION.SDK_INT < 30) return
        val c = r.softApConfiguration
        report.ssid = (if (Build.VERSION.SDK_INT >= 33) c.wifiSsid?.toString() else @Suppress("DEPRECATION") c.ssid)?.trim('"')
        report.security = c.securityType
        if (Build.VERSION.SDK_INT >= 36) {
            val ch = c.channels
            report.channels = (0 until ch.size()).associate { ch.keyAt(it) to ch.valueAt(it) }
        }
    }

    private fun measure(
        app: Context,
        reservation: WifiManager.LocalOnlyHotspotReservation,
        report: M3Report,
        progress: Progress,
        done: (File?) -> Unit,
    ) {
        val sink = ServerSocket(0)
        val source = ServerSocket(0)
        report.sinkPort = sink.localPort
        report.sourcePort = source.localPort
        val received = AtomicLong()
        val sent = AtomicLong()
        progress.update(
            "ssid ${report.ssid} · host ${report.addresses.joinToString()} · sink ${sink.localPort} · source ${source.localPort}",
        )
        io.execute {
            while (!sink.isClosed) {
                val s = runCatching { sink.accept() }.getOrNull() ?: break
                io.execute { s.use { timed(report.uploads) { count(it.getInputStream()).also(received::addAndGet) } } }
            }
        }
        io.execute {
            while (!source.isClosed) {
                val s = runCatching { source.accept() }.getOrNull() ?: break
                io.execute {
                    s.use {
                        timed(report.downloads) {
                            val buf = ByteArray(64 * 1024)
                            var left = SOURCE_BYTES
                            val out = it.getOutputStream()
                            while (left > 0) {
                                val n = minOf(left, buf.size.toLong()).toInt()
                                out.write(buf, 0, n)
                                left -= n
                            }
                            out.flush()
                            SOURCE_BYTES.also(sent::addAndGet)
                        }
                    }
                }
            }
        }
        main.postDelayed({
            runCatching { sink.close() }
            runCatching { source.close() }
            runCatching { reservation.close() }
            progress.update("done: up ${received.get()} B, down ${sent.get()} B")
            done(write(app, report))
        }, report.seconds * 1000L)
    }

    /** Runs [block] (returns bytes moved), and records one [M3Report.Run] with the wall time. */
    private fun timed(into: MutableList<M3Report.Run>, block: () -> Long) {
        val t = SystemClock.elapsedRealtimeNanos()
        val bytes = runCatching(block).getOrDefault(-1L)
        val ns = SystemClock.elapsedRealtimeNanos() - t
        synchronized(into) { into += M3Report.Run(bytes, ns) }
    }

    private fun count(input: InputStream): Long {
        val buf = ByteArray(64 * 1024)
        var total = 0L
        while (true) {
            val n = input.read(buf)
            if (n < 0) return total
            total += n
        }
    }

    private fun ipv4Addresses(): List<String> =
        runCatching {
            NetworkInterface.getNetworkInterfaces().toList()
                .filter { it.isUp && !it.isLoopback }
                .flatMap { i -> i.inetAddresses.toList().filterIsInstance<Inet4Address>().map { "${i.name}=${it.hostAddress}" } }
        }.getOrDefault(emptyList())

    private fun write(app: Context, report: M3Report): File? = runCatching {
        val dir = File(app.getExternalFilesDir(null) ?: app.filesDir, "m3").apply { mkdirs() }
        File(dir, "m3-${report.band}-${System.currentTimeMillis()}.json").apply { writeText(report.toJson()) }
    }.getOrNull()
}

/** One M3 run, as JSON (pure; unit-tested). */
class M3Report(
    val band: String,
    val sdk: Int,
    val device: String,
    val supports5: Boolean,
    val supports6: Boolean,
    val staApConcurrency: Boolean,
    val seconds: Int,
) {
    class Run(val bytes: Long, val nanos: Long) {
        /** Megabits per second, or -1 when nothing moved or the run failed. */
        val mbps: Double get() = if (bytes <= 0 || nanos <= 0) -1.0 else bytes * 8.0 / (nanos / 1e9) / 1e6
    }

    var bandRequested = false
    var startMs = -1L
    var ssid: String? = null
    var security = -1
    var channels: Map<Int, Int> = emptyMap()
    var addresses: List<String> = emptyList()
    var sinkPort = -1
    var sourcePort = -1
    val uploads = mutableListOf<Run>()
    val downloads = mutableListOf<Run>()
    var failure: String? = null
    var note: String? = null

    fun toJson(): String {
        val sb = StringBuilder("{")
        fun key(k: String) = sb.append('"').append(k).append("\":")
        fun str(k: String, v: String?) {
            key(k)
            if (v == null) sb.append("null") else sb.append('"').append(escape(v)).append('"')
            sb.append(',')
        }
        fun raw(k: String, v: Any) = key(k).append(v).append(',')
        fun runs(k: String, list: List<Run>) {
            key(k).append('[')
            synchronized(list) {
                list.forEachIndexed { i, r ->
                    if (i > 0) sb.append(',')
                    sb.append("{\"bytes\":").append(r.bytes).append(",\"nanos\":").append(r.nanos)
                        .append(",\"mbps\":").append(String.format(java.util.Locale.ROOT, "%.2f", r.mbps)).append('}')
                }
            }
            sb.append("],")
        }
        raw("schema", 1)
        str("measure", "M3")
        str("band", band)
        raw("band_requested", bandRequested)
        raw("sdk", sdk)
        str("device", device)
        raw("supports_5ghz", supports5)
        raw("supports_6ghz", supports6)
        raw("sta_ap_concurrency", staApConcurrency)
        raw("seconds", seconds)
        raw("start_ms", startMs)
        str("ssid", ssid)
        raw("security", security)
        key("channels").append('{').append(channels.entries.joinToString(",") { "\"${it.key}\":${it.value}" }).append("},")
        key("addresses").append('[').append(addresses.joinToString(",") { "\"${escape(it)}\"" }).append("],")
        raw("sink_port", sinkPort)
        raw("source_port", sourcePort)
        runs("uploads", uploads)
        runs("downloads", downloads)
        str("failure", failure)
        str("note", note)
        sb.setLength(sb.length - 1)
        return sb.append('}').toString()
    }

    companion object {
        /** `2.4`, `5` or `default`; anything else is `default`. */
        fun band(s: String?): String = when (s?.trim()?.lowercase()) {
            "2", "2.4", "2g", "2ghz", "2.4ghz" -> "2.4"
            "5", "5g", "5ghz" -> "5"
            else -> "default"
        }

        /** The `SoftApConfiguration.BAND_*` value to request, or null for the default band. */
        fun channelsFor(band: String): Int? = when (band) {
            "2.4" -> SoftApConfiguration.BAND_2GHZ
            "5" -> SoftApConfiguration.BAND_5GHZ
            else -> null
        }

        fun escape(s: String): String = buildString {
            s.forEach { c ->
                when {
                    c == '"' -> append("\\\"")
                    c == '\\' -> append("\\\\")
                    c == '\n' -> append("\\n")
                    c.code < 0x20 -> append(String.format(java.util.Locale.ROOT, "\\u%04x", c.code))
                    else -> append(c)
                }
            }
        }
    }
}
