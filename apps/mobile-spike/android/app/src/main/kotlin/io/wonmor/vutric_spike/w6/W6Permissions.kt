package io.wonmor.vutric_spike.w6

import android.content.Context
import android.content.pm.PackageManager
import android.os.Build

/**
 * The runtime permissions each W6 feature needs, and nothing more (ADR-0130 table). Asked at the tap that needs
 * them, never at launch. `NEARBY_WIFI_DEVICES` and `BLUETOOTH_SCAN` are declared `neverForLocation`, so on Android
 * 12/13+ none of these features needs location.
 */
object W6Permissions {
    private const val NEARBY_WIFI = "android.permission.NEARBY_WIFI_DEVICES"
    private const val FINE_LOCATION = "android.permission.ACCESS_FINE_LOCATION"
    private const val BT_SCAN = "android.permission.BLUETOOTH_SCAN"
    private const val BT_ADVERTISE = "android.permission.BLUETOOTH_ADVERTISE"
    private const val BT_CONNECT = "android.permission.BLUETOOTH_CONNECT"
    private const val NOTIFICATIONS = "android.permission.POST_NOTIFICATIONS"

    /** Runtime permissions [feature] needs on [sdk]; unknown features need nothing. */
    fun needed(feature: String, sdk: Int): List<String> = when (feature) {
        "hotspot", "aware", "direct" -> listOf(if (sdk >= 33) NEARBY_WIFI else FINE_LOCATION)
        "ble_scan" -> if (sdk >= 31) listOf(BT_SCAN, BT_CONNECT) else listOf(FINE_LOCATION)
        "ble_advertise" -> if (sdk >= 31) listOf(BT_ADVERTISE, BT_CONNECT) else emptyList()
        "ble" -> (needed("ble_scan", sdk) + needed("ble_advertise", sdk)).distinct()
        "ring" -> if (sdk >= 33) listOf(NOTIFICATIONS) else emptyList()
        else -> emptyList()
    }

    fun missing(context: Context, feature: String): List<String> =
        needed(feature, Build.VERSION.SDK_INT).filter { context.checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
}
