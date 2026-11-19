package io.wonmor.vutric_spike.w6

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Random

class KeepAliveTest {
    private fun choose(
        sdk: Int = 34,
        validated: Boolean = true,
        scheduled: Boolean = false,
        waited: Long = 0,
        user: Boolean = false,
        lost: Boolean = false,
    ) = KeepAlive.choose(sdk, validated, scheduled, waited, 5_000, user, lost)

    @Test fun beforeAndroid14TheServiceKeepsEveryTransfer() = assertEquals(KeepAlive.Choice.SERVICE, choose(sdk = 33))

    @Test fun android14WithAValidatedNetworkSchedulesTheJob() = assertEquals(KeepAlive.Choice.SCHEDULE, choose())

    @Test fun hotspotAwareOrDirectWithoutInternetStayOnTheService() =
        assertEquals(KeepAlive.Choice.SERVICE, choose(validated = false))

    @Test fun aScheduledJobIsGivenTheGraceTimeToStart() =
        assertEquals(KeepAlive.Choice.WAIT, choose(scheduled = true, waited = 4_999))

    @Test fun aJobThatNeverStartsFallsBackToTheService() =
        assertEquals(KeepAlive.Choice.SERVICE, choose(scheduled = true, waited = 5_000))

    @Test fun theUsersStopInTheTaskManagerIsFinal() = assertEquals(KeepAlive.Choice.NONE, choose(user = true))

    @Test fun aJobTheSystemEndedFallsBackToTheService() = assertEquals(KeepAlive.Choice.SERVICE, choose(lost = true))
}

class RingPlanTest {
    @Test fun secondsFollowTheEngineBounds() {
        assertEquals(30_000L, RingPlan.of(0).millis)
        assertEquals(30_000L, RingPlan.of(-5).millis)
        assertEquals(1_000L, RingPlan.of(1).millis)
        assertEquals(60_000L, RingPlan.of(60).millis)
        assertEquals(60_000L, RingPlan.of(600).millis)
    }

    @Test fun thePatternStartsAtOnceAndBuzzesMostOfTheTime() {
        val p = RingPlan.of(10).pattern
        assertEquals(0L, p[0])
        val on = p.filterIndexed { i, _ -> i % 2 == 1 }.sum()
        val off = p.filterIndexed { i, _ -> i % 2 == 0 }.sum()
        assertTrue(on > off)
    }

    @Test fun fullScreenOnlyWhenAndroid14StillGrantsIt() {
        assertTrue(RingPlan.fullScreen(33, granted = false))
        assertTrue(RingPlan.fullScreen(34, granted = true))
        assertEquals(false, RingPlan.fullScreen(34, granted = false))
    }

    @Test fun onlyTheLocalEndsTellDart() {
        assertEquals("found", RingPlan.End.FOUND.event)
        assertEquals("timeout", RingPlan.End.TIMEOUT.event)
        assertNull(RingPlan.End.REMOTE.event)
    }
}

class LinkRulesTest {
    @Test fun pskIsOpenOrWpaShaped() {
        assertEquals(LinkRules.Psk(null), LinkRules.psk(null))
        assertEquals(LinkRules.Psk(null), LinkRules.psk(""))
        assertNull(LinkRules.psk("short"))
        assertNull(LinkRules.psk("x".repeat(64)))
        assertNull(LinkRules.psk("cafécafécafé"))
        assertEquals("12345678", LinkRules.psk("12345678")?.value)
    }

    @Test fun directCredentialsAreAcceptedByTheP2pBuilderRules() {
        val r = Random(7)
        repeat(200) {
            val (name, pass) = LinkRules.directCredentials(r)
            assertTrue(name, LinkRules.validDirect(name, pass))
            assertEquals(16, pass.length)
        }
    }

    @Test fun badDirectNetworksAreRefusedBeforeTheOsSeesThem() {
        assertEquals(false, LinkRules.validDirect("Vutric", "12345678"))
        assertEquals(false, LinkRules.validDirect("DIRECT-", "12345678"))
        assertEquals(false, LinkRules.validDirect("DIRECT-ab-Vutric", "short"))
    }
}

class BleRulesTest {
    @Test fun advertsNeverExceedTheScanResponse() {
        assertEquals(13, BleRules.fit(ByteArray(40)).size)
        assertEquals(5, BleRules.fit(ByteArray(5)).size)
    }

    @Test fun rotationIsBounded() {
        assertEquals(BleRules.DEFAULT_ROTATION_MS, BleRules.rotation(0))
        assertEquals(30_000L, BleRules.rotation(10))
        assertEquals(120_000L, BleRules.rotation(120_000))
    }

    @Test fun chunksFitTheMtuAndJoinBack() {
        val msg = ByteArray(1000) { it.toByte() }
        for (mtu in listOf(0, 23, 185, 517, 9000)) {
            val chunks = BleRules.split(msg, mtu)
            val max = mtu.coerceIn(23, 517) - 3
            assertTrue(chunks.all { it.size <= max })
            assertArrayEquals(msg, chunks.fold(ByteArray(0)) { a, c -> a + c })
        }
        assertEquals(1, BleRules.split(ByteArray(0), 23).size)
    }

    @Test fun theTempCodecRotatesAndAnswersNothing() {
        val c = TempBleCodec(Random(1))
        val a = c.advert()
        val b = c.advert()
        assertEquals(BleRules.MAX_ADVERT, a.bytes.size)
        assertTrue(!a.bytes.contentEquals(b.bytes))
        assertTrue(c.onChunk("AA:BB", byteArrayOf(1, 2)).isEmpty())
    }
}

class W6PermissionsTest {
    @Test fun wifiLinksNeedNearbyDevicesFrom13AndLocationBefore() {
        assertEquals(listOf("android.permission.NEARBY_WIFI_DEVICES"), W6Permissions.needed("aware", 33))
        assertEquals(listOf("android.permission.ACCESS_FINE_LOCATION"), W6Permissions.needed("direct", 29))
    }

    @Test fun bleNeedsTheBluetoothTrioFrom12() {
        assertEquals(
            setOf("android.permission.BLUETOOTH_SCAN", "android.permission.BLUETOOTH_CONNECT", "android.permission.BLUETOOTH_ADVERTISE"),
            W6Permissions.needed("ble", 31).toSet(),
        )
        assertEquals(listOf("android.permission.ACCESS_FINE_LOCATION"), W6Permissions.needed("ble", 30))
        assertTrue(W6Permissions.needed("ble_advertise", 30).isEmpty())
    }

    @Test fun ringOnlyNeedsNotificationsAndShortcutsNothing() {
        assertEquals(listOf("android.permission.POST_NOTIFICATIONS"), W6Permissions.needed("ring", 34))
        assertTrue(W6Permissions.needed("ring", 32).isEmpty())
        assertTrue(W6Permissions.needed("shortcuts", 36).isEmpty())
    }
}

class M3ReportTest {
    @Test fun bandNamesAreNormalised() {
        assertEquals("2.4", M3Report.band("2.4GHz"))
        assertEquals("5", M3Report.band(" 5 "))
        assertEquals("default", M3Report.band(null))
        assertEquals("default", M3Report.band("6"))
        assertNull(M3Report.channelsFor("default"))
    }

    @Test fun jsonHasEveryFieldAndEscapes() {
        val r = M3Report("5", 36, "Acme \"Phone\"", supports5 = true, supports6 = false, staApConcurrency = true, seconds = 60)
        r.ssid = "Android_1\\2"
        r.channels = mapOf(2 to 36)
        r.addresses = listOf("swlan0=192.168.42.1")
        r.uploads += M3Report.Run(125_000_000, 1_000_000_000)
        val j = r.toJson()
        assertTrue(j, j.startsWith("{\"schema\":1,\"measure\":\"M3\",\"band\":\"5\""))
        assertTrue(j, j.contains("\"device\":\"Acme \\\"Phone\\\"\""))
        assertTrue(j, j.contains("\"ssid\":\"Android_1\\\\2\""))
        assertTrue(j, j.contains("\"channels\":{\"2\":36}"))
        assertTrue(j, j.contains("\"mbps\":1000.00"))
        assertTrue(j, j.contains("\"downloads\":[]"))
        assertTrue(j, j.endsWith("\"note\":null}"))
    }

    @Test fun aFailedRunHasNoSpeed() = assertEquals(-1.0, M3Report.Run(-1, 10).mbps, 0.0)
}
