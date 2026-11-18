// Runs on the iOS simulator in CI (`xcodebuild test`, ios-spike.yml). Everything here works without the App
// Group, the Keychain group or a network, so a failure is a real bug, not a missing entitlement.

import CoreGraphics
import ImageIO
import XCTest

@testable import VutricKit

final class SharedStoreTests: XCTestCase {
  private var dir: URL!
  private var store: SharedStore!

  override func setUpWithError() throws {
    dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    store = SharedStore(root: dir)
  }

  override func tearDownWithError() throws {
    try? FileManager.default.removeItem(at: dir)
  }

  private func device(_ id: Character, _ platform: String, seen: UInt64) -> SharedDevice {
    SharedDevice(id: String(repeating: id, count: 64), name: "D\(id)", platform: platform, code: "ABCD-EFGH",
      lastSeen: seen)
  }

  func testSnapshotRoundTripsNewestFirst() throws {
    XCTAssertNil(store.snapshot())
    let s = SharedSnapshot(
      deviceName: "Nosir iPhone",
      devices: [device("a", "android", seen: 5), device("b", "windows", seen: 9), device("c", "linux", seen: 7)],
      heicToJpeg: true)
    try store.save(s)
    let back = try XCTUnwrap(store.snapshot())
    XCTAssertEqual(back, s)
    XCTAssertEqual(back.devices.map(\.lastSeen), [9, 7, 5])
    XCTAssertEqual(back.lastComputer?.platform, "windows")
  }

  func testLastComputerSkipsPhones() {
    let s = SharedSnapshot(deviceName: "x", devices: [device("a", "android", seen: 9)], heicToJpeg: false)
    XCTAssertNil(s.lastComputer)
  }

  func testRustFolderIsCreatedInsideTheGroup() {
    let rust = store.rustDir
    var isDir: ObjCBool = false
    XCTAssertTrue(FileManager.default.fileExists(atPath: rust.path, isDirectory: &isDir))
    XCTAssertTrue(isDir.boolValue)
    XCTAssertEqual(rust.deletingLastPathComponent().standardizedFileURL, dir.standardizedFileURL)
  }

  func testHandoffsAreTakenOnceAndOldOnesDropped() throws {
    let now = Date()
    try store.push(Handoff(kind: .send, deviceId: nil, at: now.addingTimeInterval(-3600)))
    try store.push(Handoff(kind: .send, deviceId: String(repeating: "a", count: 64), at: now))
    let got = store.takeHandoffs(now: now)
    XCTAssertEqual(got.count, 1)
    XCTAssertEqual(got.first?.deviceId, String(repeating: "a", count: 64))
    XCTAssertEqual(got.first?.event["kind"] as? String, "send")
    XCTAssertTrue(store.takeHandoffs(now: now).isEmpty)
  }

  func testHandoffsKeepOnlyTheLastEight() throws {
    let now = Date()
    for _ in 0..<20 { try store.push(Handoff(kind: .send, deviceId: nil, at: now)) }
    XCTAssertEqual(store.takeHandoffs(now: now).count, 8)
  }

  func testOutboxFoldersAreFreshAndOldOnesCleaned() throws {
    let a = try store.newOutboxFolder()
    let b = try store.newOutboxFolder()
    XCTAssertNotEqual(a, b)
    store.cleanOutbox(olderThan: 3600, now: Date())
    XCTAssertTrue(FileManager.default.fileExists(atPath: a.path))
    store.cleanOutbox(olderThan: 0, now: Date().addingTimeInterval(10))
    XCTAssertFalse(FileManager.default.fileExists(atPath: a.path))
    XCTAssertFalse(FileManager.default.fileExists(atPath: b.path))
  }

  func testAppStoreNeedsTheGroupKey() {
    // The test bundle has no VutricAppGroup key: the extensions must get nil, not a crash.
    XCTAssertNil(SharedStore.app(bundle: Bundle(for: SharedStoreTests.self)))
  }
}

final class URLTests: XCTestCase {
  func testSendURLRoundTrips() throws {
    let id = String(repeating: "f", count: 64)
    let url = VutricURL.send(device: id)
    XCTAssertEqual(url.absoluteString, "vutric://send?device=\(id)")
    XCTAssertEqual(VutricURL.handoff(from: url)?.deviceId, id)
    XCTAssertEqual(VutricURL.handoff(from: VutricURL.send(device: nil))?.kind, .send)
  }

  func testForeignOrBadURLsAreIgnored() throws {
    XCTAssertNil(VutricURL.handoff(from: URL(string: "https://send?device=x")!))
    XCTAssertNil(VutricURL.handoff(from: URL(string: "vutric://pair")!))
    // A device that is not 64 hex is dropped, the send itself still opens.
    let h = try XCTUnwrap(VutricURL.handoff(from: URL(string: "vutric://send?device=../../etc")!))
    XCTAssertNil(h.deviceId)
  }
}

final class TransferStateTests: XCTestCase {
  func testFractionAndPercent() {
    var t = TransferState(phase: .running, device: "PC", sending: true, bytesDone: 0, bytesTotal: 0)
    XCTAssertNil(t.fraction)
    XCTAssertEqual(t.percentText, "")
    t.bytesTotal = 1000
    t.bytesDone = 425
    XCTAssertEqual(t.fraction ?? -1, 0.425, accuracy: 1e-9)
    XCTAssertEqual(t.percentText, "42%")
    t.bytesDone = 5000
    XCTAssertEqual(t.fraction, 1)
    t = TransferState(phase: .done, device: "PC", sending: true)
    XCTAssertEqual(t.fraction, 1)
  }

  func testTitleKeysFollowPhaseAndDirection() {
    XCTAssertEqual(TransferState(phase: .running, device: "", sending: true).titleKey, "activity.sending")
    XCTAssertEqual(TransferState(phase: .waiting, device: "", sending: false).titleKey, "activity.receiving")
    XCTAssertEqual(TransferState(phase: .done, device: "", sending: false).titleKey, "activity.done")
    XCTAssertEqual(TransferState(phase: .failed, device: "", sending: true).titleKey, "activity.failed")
  }

  func testBytesAreWrittenLikeTheFlutterApp() {
    XCTAssertEqual(ByteText.of(999), "999 B")
    XCTAssertEqual(ByteText.of(1_500), "2 KB")
    XCTAssertEqual(ByteText.of(12_300_000), "12.3 MB")
    XCTAssertEqual(ByteText.of(4_250_000_000), "4.25 GB")
  }

  func testGateLetsPhaseChangesThroughAndThrottlesTheRest() {
    var g = UpdateGate(interval: 0.5)
    let t0 = Date()
    let run = TransferState(phase: .running, device: "", sending: true)
    XCTAssertTrue(g.allows(run, at: t0))
    XCTAssertFalse(g.allows(run, at: t0.addingTimeInterval(0.1)))
    XCTAssertTrue(g.allows(run, at: t0.addingTimeInterval(0.6)))
    var done = run
    done.phase = .done
    XCTAssertTrue(g.allows(done, at: t0.addingTimeInterval(0.61)))
  }
}

final class NearbyTests: XCTestCase {
  func testAddressesAreWrittenForRust() {
    XCTAssertEqual(NearbyAddress.text(host: "192.168.1.24", port: 51000), "192.168.1.24:51000")
    XCTAssertEqual(NearbyAddress.text(host: "fe80::1%en0", port: 51000), "[fe80::1]:51000")
    XCTAssertEqual(NearbyAddress.text(host: "2001:db8::5", port: 7), "[2001:db8::5]:7")
    XCTAssertNil(NearbyAddress.text(host: "printer.local", port: 80))
    XCTAssertNil(NearbyAddress.text(host: "192.168.1.300", port: 80))
    XCTAssertNil(NearbyAddress.text(host: "10.0.0.1", port: 0))
  }

  func testServiceTypeMatchesInfoPlist() {
    // NSBonjourServices in Runner and VutricShare lists exactly this; iroh announces `_vutric._udp.local.`.
    XCTAssertEqual(NearbyBrowser.serviceType, "_vutric._udp")
  }
}

final class HotspotTests: XCTestCase {
  func testRequestsAreCheckedBeforeIOSSeesThem() {
    XCTAssertNil(HotspotRequest.check(ssid: "DIRECT-vutric", password: "12345678", security: "wpa2"))
    XCTAssertNil(HotspotRequest.check(ssid: "Cafe", password: "", security: "open"))
    XCTAssertEqual(HotspotRequest.check(ssid: "", password: "12345678", security: "wpa2"), .badNetwork)
    XCTAssertEqual(
      HotspotRequest.check(ssid: String(repeating: "x", count: 33), password: "12345678", security: "wpa3"),
      .badNetwork)
    XCTAssertEqual(HotspotRequest.check(ssid: "A", password: "short", security: "wpa2"), .badNetwork)
    XCTAssertEqual(HotspotRequest.check(ssid: "A", password: "12345678", security: "wep"), .badNetwork)
  }

  func testErrorCodesMapToTheAndroidAnswers() {
    XCTAssertEqual(HotspotRequest.answer(forErrorCode: 13), .joined)
    XCTAssertEqual(HotspotRequest.answer(forErrorCode: 2), .badNetwork)
    XCTAssertEqual(HotspotRequest.answer(forErrorCode: 7), .unavailable)
    XCTAssertEqual(HotspotRequest.answer(forErrorCode: 10), .permission)
    XCTAssertEqual(HotspotRequest.answer(forErrorCode: 999), .unavailable)
    XCTAssertEqual(HotspotAnswer.badNetwork.rawValue, "bad_network")
  }
}

final class HeicTests: XCTestCase {
  private func writeImage(_ url: URL, type: String) -> Bool {
    let w = 64
    let h = 48
    guard
      let ctx = CGContext(
        data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
    else { return false }
    ctx.setFillColor(CGColor(red: 0.2, green: 0.4, blue: 0.9, alpha: 1))
    ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
    guard let image = ctx.makeImage(),
      let dst = CGImageDestinationCreateWithURL(url as CFURL, type as CFString, 1, nil)
    else { return false }
    CGImageDestinationAddImage(dst, image, nil)
    return CGImageDestinationFinalize(dst)
  }

  private func check(_ source: URL) throws {
    let out = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".jpg")
    defer { try? FileManager.default.removeItem(at: out) }
    try HeicConverter.convert(source, to: out)
    let data = try Data(contentsOf: out)
    XCTAssertEqual(Array(data.prefix(2)), [0xFF, 0xD8])
    let src = try XCTUnwrap(CGImageSourceCreateWithURL(out as CFURL, nil))
    let props = try XCTUnwrap(CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any])
    XCTAssertEqual(props[kCGImagePropertyPixelWidth] as? Int, 64)
    XCTAssertEqual(props[kCGImagePropertyPixelHeight] as? Int, 48)
  }

  func testNames() {
    XCTAssertTrue(HeicConverter.isHeic("IMG_0001.HEIC"))
    XCTAssertTrue(HeicConverter.isHeic("a.heif"))
    XCTAssertFalse(HeicConverter.isHeic("a.jpg"))
    XCTAssertEqual(HeicConverter.jpegName(for: "IMG_0001.HEIC"), "IMG_0001.jpg")
    XCTAssertEqual(HeicConverter.jpegName(for: ".HEIC"), "photo.jpg")
  }

  func testAPngBecomesAJpegOfTheSameSize() throws {
    let src = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".png")
    defer { try? FileManager.default.removeItem(at: src) }
    XCTAssertTrue(writeImage(src, type: "public.png"))
    try check(src)
  }

  func testAHeicBecomesAJpeg() throws {
    let src = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".heic")
    defer { try? FileManager.default.removeItem(at: src) }
    guard writeImage(src, type: "public.heic") else {
      throw XCTSkip("this simulator has no HEIC encoder")
    }
    try check(src)
  }

  func testUnreadableInputThrows() {
    let src = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".heic")
    try? Data("not an image".utf8).write(to: src)
    defer { try? FileManager.default.removeItem(at: src) }
    let out = src.deletingPathExtension().appendingPathExtension("jpg")
    XCTAssertThrowsError(try HeicConverter.convert(src, to: out))
  }
}

final class FolderTests: XCTestCase {
  func testFreeNamesFollowTheFilesApp() {
    let dir = URL(fileURLWithPath: "/x", isDirectory: true)
    let taken: Set<String> = ["/x/a.jpg", "/x/a (2).jpg", "/x/notes"]
    let exists: (URL) -> Bool = { taken.contains($0.path) }
    XCTAssertEqual(FolderBookmark.freeName("b.jpg", in: dir, exists: exists).path, "/x/b.jpg")
    XCTAssertEqual(FolderBookmark.freeName("a.jpg", in: dir, exists: exists).path, "/x/a (3).jpg")
    XCTAssertEqual(FolderBookmark.freeName("notes", in: dir, exists: exists).path, "/x/notes (2)")
  }

  func testBookmarkMovesFilesAndKeepsBoth() throws {
    let fm = FileManager.default
    let base = fm.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
    let target = base.appendingPathComponent("target", isDirectory: true)
    let inbox = base.appendingPathComponent("inbox", isDirectory: true)
    try fm.createDirectory(at: target, withIntermediateDirectories: true)
    try fm.createDirectory(at: inbox, withIntermediateDirectories: true)
    defer { try? fm.removeItem(at: base) }
    try Data("old".utf8).write(to: target.appendingPathComponent("a.txt"))
    let file = inbox.appendingPathComponent("a.txt")
    try Data("new".utf8).write(to: file)
    let bookmark = try FolderBookmark.make(for: target)
    XCTAssertTrue(FolderBookmark.usable(bookmark))
    XCTAssertEqual(try FolderBookmark.move([file], into: bookmark), 1)
    XCTAssertFalse(fm.fileExists(atPath: file.path))
    XCTAssertEqual(try String(contentsOf: target.appendingPathComponent("a.txt"), encoding: .utf8), "old")
    XCTAssertEqual(try String(contentsOf: target.appendingPathComponent("a (2).txt"), encoding: .utf8), "new")
    try fm.removeItem(at: target)
    XCTAssertFalse(FolderBookmark.usable(bookmark))
  }
}

final class KeychainTests: XCTestCase {
  func testIdentityRoundTripsWhenTheKeychainIsThere() throws {
    let k = IdentityKeychain(accessGroup: nil)
    k.delete()
    let key = Data((0..<32).map { UInt8($0) })
    do {
      try k.write(key)
    } catch IdentityKeychain.Failure.status(let s) where s == errSecMissingEntitlement || s == errSecNotAvailable {
      throw XCTSkip("no Keychain for this test host (\(s))")
    }
    XCTAssertEqual(k.read(), key)
    let other = Data(repeating: 7, count: 32)
    try k.write(other)
    XCTAssertEqual(k.read(), other)
    k.delete()
    XCTAssertNil(k.read())
    XCTAssertThrowsError(try k.write(Data(count: 31)))
  }
}

final class L10nTests: XCTestCase {
  func testEveryKeyHasUzbekAndEnglish() {
    for key in L10n.keys {
      XCTAssertNotNil(L10n.line(key, language: "uz"), "no Uzbek line for \(key)")
      XCTAssertNotNil(L10n.line(key, language: "en"), "no English line for \(key)")
    }
  }

  func testPlaceholdersAreFilled() {
    let s = L10n.text("share.sending", ["device": "Ofis kompyuteri"])
    XCTAssertTrue(s.contains("Ofis kompyuteri"), s)
    XCTAssertFalse(s.contains("{device}"), s)
  }

  func testMissingKeysShowThemselves() {
    XCTAssertEqual(L10n.text("no.such.key"), "no.such.key")
  }
}
