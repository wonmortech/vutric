package io.wonmor.vutric_spike.w6

import android.app.Activity
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.util.TypedValue
import android.view.Gravity
import android.view.WindowManager
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import io.wonmor.vutric_spike.R
import java.lang.ref.WeakReference

/**
 * The full-screen "Vutric: telefon qidirilmoqda" page (F14, ADR-0133): shown over the lock screen when the system
 * allows a full-screen intent, or when the user taps the ring notification. One big "Topdim" button. Native and
 * tiny on purpose: it must appear at once, even when the Flutter engine is not running.
 */
class RingActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        open += WeakReference(this)
        if (Build.VERSION.SDK_INT >= 27) {
            setShowWhenLocked(true)
            setTurnScreenOn(true)
        } else {
            @Suppress("DEPRECATION")
            window.addFlags(WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON)
        }
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        if (!Ringer.isRinging) {
            finish()
            return
        }
        val from = intent.getStringExtra(Ringer.EXTRA_FROM)
        val dp = { v: Float -> TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v, resources.displayMetrics).toInt() }
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            setPadding(dp(24f), dp(24f), dp(24f), dp(24f))
            setBackgroundColor(Color.rgb(0x0B, 0x16, 0x3A))
        }
        root.addView(
            TextView(this).apply {
                text = getString(R.string.w6_ring_title)
                setTextColor(Color.WHITE)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 26f)
                gravity = Gravity.CENTER
            },
        )
        root.addView(
            TextView(this).apply {
                text = if (from.isNullOrBlank()) getString(R.string.w6_ring_text_unknown) else getString(R.string.w6_ring_text, from.take(60))
                setTextColor(Color.rgb(0xD6, 0xDE, 0xF5))
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 18f)
                gravity = Gravity.CENTER
                setPadding(0, dp(12f), 0, dp(40f))
            },
        )
        root.addView(
            Button(this).apply {
                text = getString(R.string.w6_ring_found)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 22f)
                minHeight = dp(72f)
                minWidth = dp(220f)
                contentDescription = getString(R.string.w6_ring_found)
                setOnClickListener {
                    Ringer.found(this@RingActivity)
                    finish()
                }
            },
        )
        setContentView(root)
    }

    override fun onDestroy() {
        open.removeAll { it.get() == null || it.get() === this }
        super.onDestroy()
    }

    /** Back = "Topdim" too: the person is holding the phone, so it is found. */
    @Deprecated("Activity.onBackPressed is enough here; no AndroidX back dispatcher in this activity")
    @Suppress("DEPRECATION")
    override fun onBackPressed() {
        Ringer.found(this)
        super.onBackPressed()
    }

    companion object {
        private val open = mutableListOf<WeakReference<RingActivity>>()

        fun closeAll() {
            open.mapNotNull { it.get() }.forEach { a ->
                // On the lock screen, finishing is enough; the keyguard stays as it was.
                if (!a.isFinishing) a.finish()
            }
            open.clear()
        }
    }
}
