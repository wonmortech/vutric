package io.wonmor.vutric_spike.w6

import android.annotation.SuppressLint
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothGattServer
import android.bluetooth.BluetoothGattServerCallback
import android.bluetooth.BluetoothGattService
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.ParcelUuid
import java.security.SecureRandom
import java.util.UUID

/**
 * R4, Android side (ADR-0132): BLE for finding and handshake only, never for file bytes.
 *
 * Kotlin moves bytes and nothing else. What goes into the advert and what a handshake chunk means is W3's codec
 * (`vutric-core::ble`, issue #24), reached through [BleCodec]; this file never reads a payload.
 *
 * - **Advertiser**: connectable, our service UUID in the advert, the codec's bytes (at most 13) as service data in the
 *   scan response. Restarted with fresh bytes every `rotationMs`, so the id on the air rotates (Android also rotates
 *   the random MAC by itself).
 * - **Scanner**: filters on our service UUID only, so other devices' adverts never reach the app.
 * - **GATT server**: one service, `rx` (the central writes chunks) and `tx` (we notify reply chunks, one at a time).
 * - **GATT client** ([connect]/[send]): the same characteristics from the other side, for phone ⇄ phone or a PC that
 *   advertises.
 *
 * Events: `{"kind": "w6.ble", "state": ...}` with `advertising`, `scanning`, `heard` (+ `address`, `rssi`,
 * `data`), `linked` / `unlinked` (+ `address`), `failed` (+ `reason`), `stopped`.
 */
@SuppressLint("MissingPermission") // W6.permissions asks BLUETOOTH_SCAN / ADVERTISE / CONNECT (12+) first
object BleNearby {
    val SERVICE: UUID = UUID.fromString("7144296c-8360-4d5c-bf1b-edf2e74ccebc")
    val RX: UUID = UUID.fromString("45ad6c06-d329-4bcb-9037-aad8a3c03e8e")
    val TX: UUID = UUID.fromString("8e24cee5-90ac-4895-99bc-94973a331eb8")
    private val CCCD: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
    private const val WANT_MTU = 517

    /** TEMP until W3 #24: random bytes, no resolving, no replies. W1 sets the Rust-backed codec here. */
    @Volatile var codec: BleCodec = TempBleCodec(SecureRandom())

    private val main = Handler(Looper.getMainLooper())
    private var app: Context? = null
    private var advertising: AdvertiseCallback? = null
    private var rotate: Runnable? = null
    private var scanning: ScanCallback? = null
    private var server: BluetoothGattServer? = null
    private val serverLinks = mutableMapOf<String, Link>()
    private val clientLinks = mutableMapOf<String, ClientLink>()

    /** One connected central: its MTU, whether it subscribed, and the reply chunks still to notify. */
    private class Link(val device: BluetoothDevice) {
        var mtu = 23
        var subscribed = false
        val out = ArrayDeque<ByteArray>()
        var sending = false
    }

    private class ClientLink(val gatt: BluetoothGatt) {
        var rx: BluetoothGattCharacteristic? = null
        var mtu = 23
        val out = ArrayDeque<ByteArray>()
        var writing = false
        var ready = false
    }

    fun supported(context: Context): Boolean =
        context.packageManager.hasSystemFeature(PackageManager.FEATURE_BLUETOOTH_LE)

    fun start(context: Context, advertise: Boolean, scan: Boolean) {
        val a = context.applicationContext
        app = a
        main.post {
            val adapter = a.getSystemService(BluetoothManager::class.java)?.adapter
            if (!supported(a) || adapter == null) return@post fail("unsupported")
            if (!adapter.isEnabled) return@post fail("bluetooth_off")
            try {
                if (advertise) {
                    openServer(a)
                    advertise(a)
                }
                if (scan) scan(a)
            } catch (_: SecurityException) {
                fail("permission")
            }
        }
    }

    fun stop() {
        main.post {
            val a = app ?: return@post
            val adapter = a.getSystemService(BluetoothManager::class.java)?.adapter
            runCatching {
                rotate?.let { main.removeCallbacks(it) }
                rotate = null
                advertising?.let { adapter?.bluetoothLeAdvertiser?.stopAdvertising(it) }
                advertising = null
                scanning?.let { adapter?.bluetoothLeScanner?.stopScan(it) }
                scanning = null
                server?.close()
                server = null
                serverLinks.clear()
                clientLinks.values.forEach { it.gatt.close() }
                clientLinks.clear()
            }
            emit("stopped")
        }
    }

    // Advertiser -----------------------------------------------------------------------------------------------

    private fun advertise(context: Context) {
        val adv = context.getSystemService(BluetoothManager::class.java)?.adapter?.bluetoothLeAdvertiser ?: return fail("no_advertiser")
        advertising?.let { adv.stopAdvertising(it) }
        val advert = codec.advert()
        val bytes = BleRules.fit(advert.bytes)
        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_BALANCED)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_MEDIUM)
            .setConnectable(true)
            .setTimeout(0)
            .build()
        val data = AdvertiseData.Builder().addServiceUuid(ParcelUuid(SERVICE)).setIncludeDeviceName(false).build()
        val response = AdvertiseData.Builder().addServiceData(ParcelUuid(SERVICE), bytes).setIncludeDeviceName(false).build()
        val cb = object : AdvertiseCallback() {
            override fun onStartSuccess(settingsInEffect: AdvertiseSettings) = emit("advertising")

            override fun onStartFailure(errorCode: Int) = fail("advertise_$errorCode")
        }
        advertising = cb
        adv.startAdvertising(settings, data, response, cb)
        rotate?.let { main.removeCallbacks(it) }
        rotate = Runnable { runCatching { advertise(context) } }.also { main.postDelayed(it, BleRules.rotation(advert.rotationMs)) }
    }

    // Scanner --------------------------------------------------------------------------------------------------

    private fun scan(context: Context) {
        val scanner = context.getSystemService(BluetoothManager::class.java)?.adapter?.bluetoothLeScanner ?: return fail("no_scanner")
        scanning?.let { scanner.stopScan(it) }
        val cb = object : ScanCallback() {
            override fun onScanResult(callbackType: Int, result: ScanResult) {
                val data = result.scanRecord?.getServiceData(ParcelUuid(SERVICE)) ?: ByteArray(0)
                val address = result.device.address
                runCatching { codec.onHeard(address, result.rssi, data) }
                W6.emit(mapOf("kind" to "w6.ble", "state" to "heard", "address" to address, "rssi" to result.rssi, "data" to data))
            }

            override fun onScanFailed(errorCode: Int) = fail("scan_$errorCode")
        }
        scanning = cb
        val filters = listOf(ScanFilter.Builder().setServiceUuid(ParcelUuid(SERVICE)).build())
        val settings = ScanSettings.Builder().setScanMode(ScanSettings.SCAN_MODE_BALANCED).build()
        scanner.startScan(filters, settings, cb)
        emit("scanning")
    }

    // GATT server ----------------------------------------------------------------------------------------------

    private fun openServer(context: Context) {
        if (server != null) return
        val bm = context.getSystemService(BluetoothManager::class.java) ?: return fail("unsupported")
        val s = bm.openGattServer(context, serverCallback) ?: return fail("gatt_server")
        val service = BluetoothGattService(SERVICE, BluetoothGattService.SERVICE_TYPE_PRIMARY)
        service.addCharacteristic(
            BluetoothGattCharacteristic(
                RX,
                BluetoothGattCharacteristic.PROPERTY_WRITE or BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE,
                BluetoothGattCharacteristic.PERMISSION_WRITE,
            ),
        )
        val tx = BluetoothGattCharacteristic(TX,BluetoothGattCharacteristic.PROPERTY_NOTIFY, 0)
        tx.addDescriptor(BluetoothGattDescriptor(CCCD, BluetoothGattDescriptor.PERMISSION_READ or BluetoothGattDescriptor.PERMISSION_WRITE))
        service.addCharacteristic(tx)
        s.addService(service)
        server = s
    }

    private val serverCallback = object : BluetoothGattServerCallback() {
        override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
            main.post {
                if (newState == BluetoothProfile.STATE_CONNECTED) {
                    serverLinks[device.address] = Link(device)
                    linkEvent("linked", device.address)
                } else if (newState == BluetoothProfile.STATE_DISCONNECTED) {
                    serverLinks.remove(device.address)
                    linkEvent("unlinked", device.address)
                }
            }
        }

        override fun onMtuChanged(device: BluetoothDevice, mtu: Int) {
            main.post { serverLinks[device.address]?.mtu = mtu }
        }

        override fun onDescriptorWriteRequest(
            device: BluetoothDevice,
            requestId: Int,
            descriptor: BluetoothGattDescriptor,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray?,
        ) {
            val ok = descriptor.uuid == CCCD
            if (responseNeeded) server?.sendResponse(device, requestId, if (ok) BluetoothGatt.GATT_SUCCESS else BluetoothGatt.GATT_FAILURE, 0, null)
            if (ok) main.post { serverLinks[device.address]?.subscribed = value?.firstOrNull()?.toInt() == 1 }
        }

        override fun onCharacteristicWriteRequest(
            device: BluetoothDevice,
            requestId: Int,
            characteristic: BluetoothGattCharacteristic,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray?,
        ) {
            val ok = characteristic.uuid == RX && !preparedWrite && offset == 0 && value != null
            if (responseNeeded) server?.sendResponse(device, requestId, if (ok) BluetoothGatt.GATT_SUCCESS else BluetoothGatt.GATT_FAILURE, 0, null)
            if (!ok || value == null) return
            main.post {
                val link = serverLinks[device.address] ?: Link(device).also { serverLinks[device.address] = it }
                val replies = runCatching { codec.onChunk(device.address, value) }.getOrDefault(emptyList())
                replies.forEach { link.out.addAll(BleRules.split(it, link.mtu)) }
                pumpServer(link)
            }
        }

        override fun onNotificationSent(device: BluetoothDevice, status: Int) {
            main.post {
                val link = serverLinks[device.address] ?: return@post
                link.sending = false
                pumpServer(link)
            }
        }
    }

    private fun pumpServer(link: Link) {
        if (link.sending || !link.subscribed) return
        val chunk = link.out.removeFirstOrNull() ?: return
        val s = server ?: return
        val tx = s.getService(SERVICE)?.getCharacteristic(TX) ?: return
        link.sending = true
        val sent = if (Build.VERSION.SDK_INT >= 33) {
            s.notifyCharacteristicChanged(link.device, tx, false, chunk) == BluetoothGatt.GATT_SUCCESS
        } else {
            @Suppress("DEPRECATION")
            run {
                tx.value = chunk
                s.notifyCharacteristicChanged(link.device, tx, false)
            }
        }
        if (!sent) {
            link.sending = false
            link.out.clear()
            fail("notify")
        }
    }

    // GATT client ----------------------------------------------------------------------------------------------

    /** Connects to a device the scanner heard; `linked` once its `tx` notifications are on. */
    fun connect(context: Context, address: String) {
        val a = context.applicationContext
        app = a
        main.post {
            if (clientLinks.containsKey(address)) return@post
            val adapter = a.getSystemService(BluetoothManager::class.java)?.adapter ?: return@post fail("unsupported")
            val device = runCatching { adapter.getRemoteDevice(address) }.getOrNull() ?: return@post fail("address")
            val gatt = try {
                device.connectGatt(a, false, clientCallback, BluetoothDevice.TRANSPORT_LE)
            } catch (_: SecurityException) {
                return@post fail("permission")
            } ?: return@post fail("connect")
            clientLinks[address] = ClientLink(gatt)
        }
    }

    /** Writes one codec message (already a whole message; it is split to the link's MTU here). */
    fun send(address: String, bytes: ByteArray) {
        main.post {
            val link = clientLinks[address] ?: return@post fail("not_linked")
            link.out.addAll(BleRules.split(bytes, link.mtu))
            pumpClient(link)
        }
    }

    fun disconnect(address: String) {
        main.post { clientLinks.remove(address)?.gatt?.let { runCatching { it.disconnect(); it.close() } } }
    }

    private val clientCallback = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
            main.post {
                val address = gatt.device.address
                if (newState == BluetoothProfile.STATE_CONNECTED) {
                    if (!gatt.requestMtu(WANT_MTU)) gatt.discoverServices()
                } else if (newState == BluetoothProfile.STATE_DISCONNECTED) {
                    clientLinks.remove(address)
                    runCatching { gatt.close() }
                    linkEvent("unlinked", address)
                }
            }
        }

        override fun onMtuChanged(gatt: BluetoothGatt, mtu: Int, status: Int) {
            main.post {
                clientLinks[gatt.device.address]?.mtu = if (status == BluetoothGatt.GATT_SUCCESS) mtu else 23
                gatt.discoverServices()
            }
        }

        override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
            main.post {
                val link = clientLinks[gatt.device.address] ?: return@post
                val service = gatt.getService(SERVICE) ?: return@post fail("no_service")
                val tx = service.getCharacteristic(TX) ?: return@post fail("no_service")
                link.rx = service.getCharacteristic(RX) ?: return@post fail("no_service")
                gatt.setCharacteristicNotification(tx, true)
                val cccd = tx.getDescriptor(CCCD) ?: return@post fail("no_service")
                if (Build.VERSION.SDK_INT >= 33) {
                    gatt.writeDescriptor(cccd, BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE)
                } else {
                    @Suppress("DEPRECATION")
                    run {
                        cccd.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
                        gatt.writeDescriptor(cccd)
                    }
                }
            }
        }

        override fun onDescriptorWrite(gatt: BluetoothGatt, descriptor: BluetoothGattDescriptor, status: Int) {
            main.post {
                val link = clientLinks[gatt.device.address] ?: return@post
                if (status != BluetoothGatt.GATT_SUCCESS) return@post fail("subscribe")
                link.ready = true
                linkEvent("linked", gatt.device.address)
                pumpClient(link)
            }
        }

        override fun onCharacteristicWrite(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic, status: Int) {
            main.post {
                val link = clientLinks[gatt.device.address] ?: return@post
                link.writing = false
                if (status != BluetoothGatt.GATT_SUCCESS) {
                    link.out.clear()
                    return@post fail("write")
                }
                pumpClient(link)
            }
        }

        override fun onCharacteristicChanged(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic, value: ByteArray) {
            onNotified(gatt, characteristic, value)
        }

        @Deprecated("Android 12 and older call this one")
        @Suppress("DEPRECATION")
        override fun onCharacteristicChanged(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic) {
            if (Build.VERSION.SDK_INT < 33) onNotified(gatt, characteristic, characteristic.value ?: return)
        }
    }

    private fun onNotified(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic, value: ByteArray) {
        if (characteristic.uuid != TX) return
        val chunk = value.copyOf()
        main.post {
            val address = gatt.device.address
            val replies = runCatching { codec.onChunk(address, chunk) }.getOrDefault(emptyList())
            val link = clientLinks[address] ?: return@post
            replies.forEach { link.out.addAll(BleRules.split(it, link.mtu)) }
            pumpClient(link)
        }
    }

    private fun pumpClient(link: ClientLink) {
        if (link.writing || !link.ready) return
        val rx = link.rx ?: return
        val chunk = link.out.removeFirstOrNull() ?: return
        link.writing = true
        val ok = if (Build.VERSION.SDK_INT >= 33) {
            link.gatt.writeCharacteristic(rx, chunk, BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT) == BluetoothGatt.GATT_SUCCESS
        } else {
            @Suppress("DEPRECATION")
            run {
                rx.writeType = BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
                rx.value = chunk
                link.gatt.writeCharacteristic(rx)
            }
        }
        if (!ok) {
            link.writing = false
            link.out.clear()
            fail("write")
        }
    }

    private fun linkEvent(state: String, address: String) =
        W6.emit(mapOf("kind" to "w6.ble", "state" to state, "address" to address))

    private fun emit(state: String) = W6.emit(mapOf("kind" to "w6.ble", "state" to state))

    private fun fail(reason: String) = W6.emit(mapOf("kind" to "w6.ble", "state" to "failed", "reason" to reason))
}

/** The frame codec Kotlin talks to. W3 owns the format (issue #24); Kotlin never reads the bytes. */
interface BleCodec {
    class Advert(val bytes: ByteArray, val rotationMs: Long)

    /** Service data for the scan response now (at most [BleRules.MAX_ADVERT] bytes), and when to ask again. */
    fun advert(): Advert

    /** A Vutric advert was heard. The codec decides whether it is a paired device worth connecting to. */
    fun onHeard(address: String, rssi: Int, serviceData: ByteArray)

    /** One GATT chunk from [link] (a BLE address). Answers whole reply messages to send back (split by Kotlin). */
    fun onChunk(link: String, chunk: ByteArray): List<ByteArray>
}

/**
 * TEMP until W3 #24: 13 random bytes rotated every 15 minutes. Nothing of the identity or the trust list is on
 * the air, and nothing is answered, so a stranger learns only that a Vutric phone is near.
 */
class TempBleCodec(private val random: java.util.Random) : BleCodec {
    override fun advert(): BleCodec.Advert =
        BleCodec.Advert(ByteArray(BleRules.MAX_ADVERT).also { random.nextBytes(it) }, BleRules.DEFAULT_ROTATION_MS)

    override fun onHeard(address: String, rssi: Int, serviceData: ByteArray) {}

    override fun onChunk(link: String, chunk: ByteArray): List<ByteArray> = emptyList()
}

/** Pure BLE sizing rules (unit-tested). */
object BleRules {
    /** Legacy scan response: 31 bytes, minus 2 (AD header) and 16 (128-bit service UUID). */
    const val MAX_ADVERT = 13

    /** ATT header of a write or a notification. */
    private const val ATT_HEADER = 3

    /** BLE's smallest MTU; every chunk fits it before the MTU exchange. */
    const val MIN_MTU = 23

    const val DEFAULT_ROTATION_MS = 15L * 60 * 1000
    private const val MIN_ROTATION_MS = 30_000L

    /** Never more than [MAX_ADVERT] bytes: a longer advert makes `startAdvertising` fail with DATA_TOO_LARGE. */
    fun fit(bytes: ByteArray): ByteArray = if (bytes.size <= MAX_ADVERT) bytes else bytes.copyOf(MAX_ADVERT)

    /** A codec asking for rotation faster than 30 s would only drain the battery; 0 or less means the default. */
    fun rotation(ms: Long): Long = if (ms <= 0) DEFAULT_ROTATION_MS else ms.coerceAtLeast(MIN_ROTATION_MS)

    /** Splits [bytes] into chunks of `mtu - 3` (an empty message is one empty chunk). */
    fun split(bytes: ByteArray, mtu: Int): List<ByteArray> {
        val size = (mtu.coerceIn(MIN_MTU, 517) - ATT_HEADER)
        if (bytes.isEmpty()) return listOf(ByteArray(0))
        return (bytes.indices step size).map { bytes.copyOfRange(it, minOf(it + size, bytes.size)) }
    }
}
