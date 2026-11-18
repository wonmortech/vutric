package io.wonmor.vutric_spike

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService

/**
 * VutricTile (ADR-0009 part 2, backlog A5): one tap in Quick Settings opens Vutric ready to receive (the Qabul tab,
 * showing this phone's QR). The tile keeps no state of its own: it is always "available", never on/off, because
 * the session lives in the app.
 */
class VutricTile : TileService() {
    override fun onStartListening() {
        qsTile?.apply {
            state = Tile.STATE_INACTIVE
            label = "Vutric: qabul"
            if (Build.VERSION.SDK_INT >= 29) subtitle = "QR kodni koʻrsatish"
            updateTile()
        }
    }

    @SuppressLint("StartActivityAndCollapseDeprecated")
    @Suppress("DEPRECATION")
    override fun onClick() {
        val open = Intent(this, MainActivity::class.java)
            .setAction(MainActivity.ACTION_RECEIVE)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        val go = Runnable {
            if (Build.VERSION.SDK_INT >= 34) {
                startActivityAndCollapse(
                    PendingIntent.getActivity(
                        this,
                        1,
                        open,
                        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                    ),
                )
            } else {
                startActivityAndCollapse(open)
            }
        }
        // On a locked phone, ask for the unlock first: the QR and the gallery are not for the lock screen.
        if (isLocked) unlockAndRun(go) else go.run()
    }
}
