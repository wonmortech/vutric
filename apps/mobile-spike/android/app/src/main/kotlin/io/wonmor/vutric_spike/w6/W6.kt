package io.wonmor.vutric_spike.w6

import android.app.Activity
import android.os.Build
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel

/**
 * The W6 door on W1's single `vutric/android` channel (ADR-0130). `MainActivity` forwards every method it does not
 * know to [handle], and every permission result to [onPermissionsResult]; events go to Dart through [sink] as
 * `{"kind": "w6.<module>", ...}` maps on the channel's `event` method. Kotlin keeps no app state here either: the
 * trust list, the ring rules and the routes stay in Rust.
 *
 * | Method | Arguments | Answer |
 * |---|---|---|
 * | `w6.permissions` | `feature` (`aware`, `direct`, `ble`, `ble_scan`, `ble_advertise`, `ring`, `hotspot`), `ask` | `{granted, missing}` (after the prompt when `ask`) |
 * | `w6.shortcutsPublish` | `devices: [{id, name, kind}]`, most recent first | number published |
 * | `w6.shortcutsClear` / `w6.shortcutUsed` | — / `id` | null |
 * | `w6.ringStart` / `w6.ringStop` | `secs`, `from` / — | null; events `w6.ring` `found` / `timeout` |
 * | `w6.awareAvailable` / `w6.awarePublish` / `w6.awareSubscribe` / `w6.awareStop` | — / `info`, `psk` / `psk` / — | bool / null |
 * | `w6.directHost` / `w6.directJoin` / `w6.directStop` | — / `ssid`, `password`, `port` / — | `{ssid, password, security, port}` / null |
 * | `w6.bleStart` / `w6.bleStop` / `w6.bleConnect` / `w6.bleSend` / `w6.bleDisconnect` | `advertise`, `scan` / — / `address` / `address`, `bytes` / `address` | null |
 * | `w6.linkSink` | `mode` (`dart` once W3 #23 lands, `close` = TEMP default) | null |
 */
object W6 {
    const val REQUEST_PERMISSIONS = 4730

    /** Set by MainActivity while the engine is attached; null otherwise (events are dropped then). */
    @Volatile var sink: ((Map<String, Any?>) -> Unit)? = null

    private var pendingPermissions: Pair<String, MethodChannel.Result>? = null
    private var activity: Activity? = null

    fun emit(event: Map<String, Any?>) {
        sink?.invoke(event)
    }

    /** Answers true when the call was a W6 method (handled here), false to let MainActivity say notImplemented. */
    fun handle(activity: Activity, call: MethodCall, result: MethodChannel.Result): Boolean {
        if (!call.method.startsWith("w6.")) return false
        this.activity = activity
        val ctx = activity.applicationContext
        try {
            when (call.method) {
                "w6.permissions" -> permissions(activity, call.argument<String>("feature") ?: "", call.argument<Boolean>("ask") == true, result)
                "w6.shortcutsPublish" -> {
                    val devices = (call.argument<List<Map<String, Any?>>>("devices") ?: emptyList()).map {
                        ShareShortcuts.Device(it["id"] as? String ?: "", it["name"] as? String ?: "", it["kind"] as? String ?: "desktop")
                    }
                    result.success(ShareShortcuts.publish(ctx, devices))
                }
                "w6.shortcutsClear" -> {
                    ShareShortcuts.clear(ctx)
                    result.success(null)
                }
                "w6.shortcutUsed" -> {
                    call.argument<String>("id")?.let { ShareShortcuts.used(ctx, it) }
                    result.success(null)
                }
                "w6.ringStart" -> {
                    Ringer.start(ctx, call.argument<Int>("secs") ?: RingPlan.DEFAULT_SECS, call.argument<String>("from"))
                    result.success(null)
                }
                "w6.ringStop" -> {
                    Ringer.stop(ctx, remote = true)
                    result.success(null)
                }
                "w6.awareAvailable" -> result.success(Build.VERSION.SDK_INT >= 29 && AwareLink.available(ctx))
                "w6.awarePublish" -> needs29(result) {
                    AwareLink.publish(ctx, call.argument<ByteArray>("info") ?: ByteArray(0), call.argument<String>("psk"))
                }
                "w6.awareSubscribe" -> needs29(result) { AwareLink.subscribe(ctx, call.argument<String>("psk")) }
                "w6.awareStop" -> needs29(result) { AwareLink.stop() }
                "w6.directHost" -> if (Build.VERSION.SDK_INT >= 29) {
                    DirectLink.host(ctx, { result.success(it) }, { result.error(it, it, null) })
                } else {
                    result.error("unsupported", "unsupported", null)
                }
                "w6.directJoin" -> if (Build.VERSION.SDK_INT >= 29) {
                    DirectLink.join(
                        ctx,
                        call.argument<String>("ssid") ?: "",
                        call.argument<String>("password") ?: "",
                        call.argument<Int>("port") ?: LinkRules.DIRECT_PORT,
                    ) { emit(mapOf("kind" to "w6.direct", "state" to "failed", "reason" to it)) }
                    // Progress, failures and the socket come as w6.direct events; the call itself answers at once.
                    result.success(null)
                } else {
                    result.error("unsupported", "unsupported", null)
                }
                "w6.directStop" -> needs29(result) { DirectLink.stop() }
                "w6.bleStart" -> {
                    BleNearby.start(ctx, call.argument<Boolean>("advertise") ?: true, call.argument<Boolean>("scan") ?: true)
                    result.success(null)
                }
                "w6.bleStop" -> {
                    BleNearby.stop()
                    result.success(null)
                }
                "w6.bleConnect" -> {
                    BleNearby.connect(ctx, call.argument<String>("address") ?: "")
                    result.success(null)
                }
                "w6.bleSend" -> {
                    BleNearby.send(call.argument<String>("address") ?: "", call.argument<ByteArray>("bytes") ?: ByteArray(0))
                    result.success(null)
                }
                "w6.bleDisconnect" -> {
                    BleNearby.disconnect(call.argument<String>("address") ?: "")
                    result.success(null)
                }
                "w6.linkSink" -> {
                    LinkSockets.sink = if (call.argument<String>("mode") == "dart") LinkSockets.toDart else LinkSockets.closeAndReport
                    result.success(null)
                }
                else -> result.notImplemented()
            }
        } catch (e: Exception) {
            result.error("w6", e.javaClass.simpleName, null)
        }
        return true
    }

    private fun needs29(result: MethodChannel.Result, block: () -> Unit) {
        if (Build.VERSION.SDK_INT < 29) return result.error("unsupported", "unsupported", null)
        block()
        // A second answer (from a failed link below) is never sent: link failures arrive as events.
        result.success(null)
    }

    private fun permissions(activity: Activity, feature: String, ask: Boolean, result: MethodChannel.Result) {
        val missing = W6Permissions.missing(activity, feature)
        if (missing.isEmpty() || !ask) return result.success(mapOf("granted" to missing.isEmpty(), "missing" to missing))
        // One W6 prompt at a time: a second ask answers the first with its current state.
        pendingPermissions?.let { (f, r) -> r.success(mapOf("granted" to false, "missing" to W6Permissions.missing(activity, f))) }
        pendingPermissions = feature to result
        activity.requestPermissions(missing.toTypedArray(), REQUEST_PERMISSIONS)
    }

    /** MainActivity forwards its permission results here; true when it was ours. */
    fun onPermissionsResult(requestCode: Int): Boolean {
        if (requestCode != REQUEST_PERMISSIONS) return false
        val (feature, r) = pendingPermissions ?: return true
        pendingPermissions = null
        val a = activity ?: return true.also { r.success(mapOf("granted" to false, "missing" to emptyList<String>())) }
        val missing = W6Permissions.missing(a, feature)
        r.success(mapOf("granted" to missing.isEmpty(), "missing" to missing))
        return true
    }

    /** MainActivity.onDestroy: radios and the ring stop when the activity really finishes. */
    fun onDestroy(activity: Activity) {
        if (this.activity === activity) this.activity = null
        sink = null
        if (activity.isFinishing) {
            BleNearby.stop()
            if (Build.VERSION.SDK_INT >= 29) {
                AwareLink.stop()
                DirectLink.stop()
            }
        }
    }
}
