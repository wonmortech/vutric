package io.wonmor.vutric_spike.w6

import android.app.Activity
import android.content.pm.PackageManager
import android.os.Bundle
import android.util.TypedValue
import android.widget.ScrollView
import android.widget.TextView
import io.wonmor.vutric_spike.R

/**
 * The M3 probe screen (ADR-0134), registered in the **debug** manifest only. Started from a PC:
 *
 * ```
 * adb shell am start -n io.wonmor.vutric_spike/.w6.M3ProbeActivity --es band 5 --ei seconds 120
 * ```
 *
 * It asks the hotspot permission, runs [HotspotBandProbe] and prints the host address, the two ports and finally
 * the JSON file's path (also to logcat, tag `VutricM3`).
 */
class M3ProbeActivity : Activity() {
    private lateinit var log: TextView
    private var started = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        log = TextView(this).apply {
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 15f)
            setPadding(32, 48, 32, 48)
            setTextIsSelectable(true)
        }
        setContentView(ScrollView(this).apply { addView(log) })
        line(getString(R.string.w6_m3_title))
        val missing = W6Permissions.missing(this, "hotspot")
        if (missing.isEmpty()) go() else requestPermissions(missing.toTypedArray(), REQUEST)
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != REQUEST) return
        if (grantResults.isNotEmpty() && grantResults.all { it == PackageManager.PERMISSION_GRANTED }) go() else line(getString(R.string.w6_m3_no_permission))
    }

    private fun go() {
        if (started) return
        started = true
        val band = intent.getStringExtra("band") ?: "default"
        val seconds = intent.getIntExtra("seconds", 120)
        HotspotBandProbe.run(this, band, seconds, { runOnUiThread { line(it) } }) { file ->
            val msg = if (file == null) getString(R.string.w6_m3_failed) else getString(R.string.w6_m3_saved, file.absolutePath)
            android.util.Log.i("VutricM3", msg)
            runOnUiThread { line(msg) }
        }
    }

    private fun line(s: String) {
        log.append(s)
        log.append("\n")
    }

    private companion object {
        const val REQUEST = 4731
    }
}
