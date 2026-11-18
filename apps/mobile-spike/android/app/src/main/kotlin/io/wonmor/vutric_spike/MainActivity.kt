package io.wonmor.vutric_spike

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.wifi.WifiManager
import android.os.Build
import androidx.lifecycle.Lifecycle
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.util.concurrent.Executors

/**
 * The one Activity. It hosts Flutter and is the door for the Kotlin modules (ADR-0009 part 2, ADR-0012):
 *
 * - `vutric/multicast`: Android's multicast lock while Vutric is in front (mDNS replies are dropped without it).
 * - `vutric/android`: Dart → Kotlin calls (transfer notification, SAF folder, hotspot) and Kotlin → Dart events
 *   (`share` with files from another app's Share menu, `receive` from the Quick Settings tile).
 *
 * Nothing here keeps app state; Rust and Flutter own it.
 */
class MainActivity : FlutterActivity() {
    private var lock: WifiManager.MulticastLock? = null
    private var channel: MethodChannel? = null
    private var pendingTree: MethodChannel.Result? = null
    private var pendingHotspot: MethodChannel.Result? = null
    private var notificationsAsked = false
    private var notificationsAsking = false
    private var hotspotRetried = false

    /** The intent that started the app (cold start from Share or the tile), until Dart takes it. */
    private var startIntent: Intent? = null
    private val io = Executors.newSingleThreadExecutor()

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        startIntent = intent
        val messenger = flutterEngine.dartExecutor.binaryMessenger
        MethodChannel(messenger, "vutric/multicast").setMethodCallHandler { call, result ->
            when (call.method) {
                "acquire" -> {
                    if (lock == null) {
                        val wifi = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
                        lock = wifi.createMulticastLock("vutric-mdns").apply {
                            setReferenceCounted(false)
                            acquire()
                        }
                    }
                    result.success(true)
                }
                "release" -> {
                    lock?.release()
                    lock = null
                    result.success(true)
                }
                else -> result.notImplemented()
            }
        }
        channel = MethodChannel(messenger, "vutric/android").apply { setMethodCallHandler(::onCall) }
    }

    private fun onCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            // The intent that started the app, once: {"kind": "share"|"receive", "files": [...]} or null.
            "takeStartIntent" -> {
                val i = startIntent
                startIntent = null
                route(i) { result.success(it) }
            }
            "transferUpdate" -> {
                askNotifications()
                TransferService.update(
                    this,
                    call.argument<String>("title") ?: "Vutric",
                    call.argument<String>("text") ?: "",
                    call.argument<Int>("permille") ?: -1,
                )
                result.success(null)
            }
            "transferStop" -> {
                TransferService.stop(this)
                result.success(null)
            }
            "pickFolder" -> {
                pendingTree?.success(null)
                pendingTree = result
                SafStore.pickTree(this)
            }
            "folderUsable" -> result.success(SafStore.usable(this, call.argument<String>("uri") ?: ""))
            "releaseFolder" -> {
                SafStore.release(this, call.argument<String>("uri") ?: "")
                result.success(null)
            }
            "moveToFolder" -> {
                val uri = call.argument<String>("uri")
                if (uri == null) {
                    result.error("args", "uri missing", null)
                    return
                }
                val paths = call.argument<List<String>>("paths") ?: emptyList()
                io.execute {
                    // Always answer: a lost reply would leave Dart's receive handler waiting forever.
                    val n = runCatching { SafStore.moveInto(applicationContext, uri, paths) }.getOrDefault(0)
                    runOnUiThread { result.success(n) }
                }
            }
            // Called while Vutric is in the background, so no permission prompt here; answers whether it is visible.
            "notifyIncoming" -> result.success(
                runCatching {
                    IncomingNotice.show(
                        this,
                        call.argument<String>("title") ?: "Vutric",
                        call.argument<String>("text") ?: "",
                        call.argument<ByteArray>("jpeg"),
                    )
                }.getOrDefault(false),
            )
            "hotspotStart" -> startHotspot(result)
            "hotspotStop" -> {
                LinkManager.stop()
                result.success(null)
            }
            else -> result.notImplemented()
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        route(intent) { event -> if (event != null) channel?.invokeMethod("event", event) }
    }

    /** Share and tile intents become one event map for Dart. Copying shared files runs off the UI thread. */
    private fun route(intent: Intent?, deliver: (Map<String, Any>?) -> Unit) {
        when {
            intent == null -> deliver(null)
            intent.action == ACTION_RECEIVE -> deliver(mapOf("kind" to "receive"))
            ShareEntry.isShare(intent) -> io.execute {
                val files = ShareEntry.files(applicationContext, intent)
                runOnUiThread { deliver(if (files.isEmpty()) null else mapOf("kind" to "share", "files" to files)) }
            }
            else -> deliver(null)
        }
    }

    @Deprecated("The platform callback is enough here; the AndroidX result API is not a dependency")
    @Suppress("DEPRECATION")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == SafStore.REQUEST_TREE) {
            val r = pendingTree
            pendingTree = null
            r?.success(if (resultCode == RESULT_OK) runCatching { SafStore.onPicked(this, data) }.getOrNull() else null)
        }
    }

    // Hotspot: needs NEARBY_WIFI_DEVICES (13+) or location (8–12), asked right when the user taps it.

    private fun hotspotPermission(): String =
        if (Build.VERSION.SDK_INT >= 33) Manifest.permission.NEARBY_WIFI_DEVICES else Manifest.permission.ACCESS_FINE_LOCATION

    private fun startHotspot(result: MethodChannel.Result, retried: Boolean = false) {
        val perm = hotspotPermission()
        if (checkSelfPermission(perm) != PackageManager.PERMISSION_GRANTED) {
            pendingHotspot?.error("busy", "busy", null)
            pendingHotspot = result
            hotspotRetried = retried
            requestPermissions(arrayOf(perm), REQUEST_HOTSPOT)
            return
        }
        LinkManager.start(this, { result.success(it) }, { result.error(it, it, null) })
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        when (requestCode) {
            REQUEST_HOTSPOT -> {
                val r = pendingHotspot ?: return
                pendingHotspot = null
                when {
                    grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED -> startHotspot(r)
                    // Empty means the request was interrupted (another dialog, a rotation), not refused: ask once more.
                    grantResults.isEmpty() && !hotspotRetried -> startHotspot(r, retried = true)
                    else -> r.error("permission", "permission", null)
                }
            }
            REQUEST_NOTIFICATIONS -> notificationsAsking = false
        }
    }

    /**
     * Android 13+: the progress notification is hidden without this; the transfer itself runs either way.
     * Asked once per launch, only while Vutric is on screen and no other permission dialog is open
     * (two requests at once cancel each other and come back empty).
     */
    private fun askNotifications() {
        if (Build.VERSION.SDK_INT < 33 || notificationsAsked || notificationsAsking) return
        if (pendingHotspot != null || !lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) return
        if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED) {
            notificationsAsked = true
            return
        }
        notificationsAsked = true
        notificationsAsking = true
        requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), REQUEST_NOTIFICATIONS)
    }

    override fun onDestroy() {
        lock?.release()
        lock = null
        // A rotation or a system-recreated activity is not "leaving": Dart's hotspotStop owns the hotspot then.
        // Only a real finish with no transfer still running closes it here.
        if (isFinishing && !TransferService.isRunning) LinkManager.stop()
        io.shutdown()
        super.onDestroy()
    }

    companion object {
        const val ACTION_RECEIVE = "io.wonmor.vutric.RECEIVE"
        private const val REQUEST_HOTSPOT = 4712
        private const val REQUEST_NOTIFICATIONS = 4713
    }
}
