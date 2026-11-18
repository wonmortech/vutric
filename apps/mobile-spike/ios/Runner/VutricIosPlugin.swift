// The Swift side of `lib/ios.dart` (ADR-0015), over one MethodChannel `vutric/ios` like Android's `vutric/android`
// (ADR-0012). Only UI events and OS results cross it: the App Group folder, the Keychain identity, the snapshot for
// the extensions, Live Activity progress, the hotspot join, HEIC → JPEG and the Files folder. Transfers stay in Rust.

import ActivityKit
import Flutter
import Intents
import UIKit
import UniformTypeIdentifiers
import VutricKit
import WidgetKit

final class VutricIosPlugin: NSObject, FlutterPlugin, FlutterSceneLifeCycleDelegate, UIDocumentPickerDelegate {
  private let channel: FlutterMethodChannel
  private weak var registrar: FlutterPluginRegistrar?
  private var pendingFolder: FlutterResult?
  /// Events that arrived before Dart listened (a widget tap that cold-started the app).
  private var startEvents: [[String: Any]] = []

  init(channel: FlutterMethodChannel, registrar: FlutterPluginRegistrar) {
    self.channel = channel
    self.registrar = registrar
  }

  static func register(with registrar: FlutterPluginRegistrar) {
    let channel = FlutterMethodChannel(name: "vutric/ios", binaryMessenger: registrar.messenger())
    let plugin = VutricIosPlugin(channel: channel, registrar: registrar)
    registrar.addMethodCallDelegate(plugin, channel: channel)
    registrar.addSceneDelegate(plugin)
  }

  private var store: SharedStore? { SharedStore.app() }
  private var keychain: IdentityKeychain { IdentityKeychain(accessGroup: store == nil ? nil : SharedStore.groupId()) }

  // MARK: Scene events (widget taps, App Intents)

  @objc(scene:willConnectToSession:options:)
  func scene(
    _ scene: UIScene, willConnectTo session: UISceneSession, options connectionOptions: UIScene.ConnectionOptions?
  ) -> Bool {
    for c in connectionOptions?.urlContexts ?? [] {
      if let h = VutricURL.handoff(from: c.url) { startEvents.append(h.event) }
    }
    return false
  }

  @objc(scene:openURLContexts:)
  func scene(_ scene: UIScene, openURLContexts URLContexts: Set<UIOpenURLContext>) -> Bool {
    var handled = false
    for c in URLContexts {
      if let h = VutricURL.handoff(from: c.url) {
        channel.invokeMethod("event", arguments: h.event)
        handled = true
      }
    }
    return handled
  }

  @objc(sceneDidBecomeActive:)
  func sceneDidBecomeActive(_ scene: UIScene) {
    for h in store?.takeHandoffs() ?? [] {
      channel.invokeMethod("event", arguments: h.event)
    }
  }

  // MARK: Calls from Dart

  func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    let args = call.arguments as? [String: Any] ?? [:]
    switch call.method {
    case "groupDir":
      result(store?.rustDir.path)
    case "identity":
      result(keychain.read().map { FlutterStandardTypedData(bytes: $0) })
    case "storeIdentity":
      guard let key = (args["key"] as? FlutterStandardTypedData)?.data else {
        result(FlutterError(code: "bad_key", message: nil, details: nil))
        return
      }
      do {
        try keychain.write(key)
        result(true)
      } catch {
        result(false)
      }
    case "syncShared":
      syncShared(args)
      result(nil)
    case "takeStartEvents":
      result(startEvents + (store?.takeHandoffs().map(\.event) ?? []))
      startEvents.removeAll()
    case "transferUpdate":
      if #available(iOS 16.2, *) { TransferActivityController.shared.update(state(args)) }
      result(nil)
    case "transferStop":
      if #available(iOS 16.2, *) { TransferActivityController.shared.end(args.isEmpty ? nil : state(args)) }
      result(nil)
    case "hotspotJoin":
      HotspotJoin.join(
        ssid: args["ssid"] as? String ?? "", password: args["password"] as? String ?? "",
        security: args["security"] as? String ?? "wpa2", hidden: args["hidden"] as? Bool ?? false
      ) { answer in
        DispatchQueue.main.async {
          result(answer == .joined ? true : FlutterError(code: answer.rawValue, message: nil, details: nil))
        }
      }
    case "hotspotLeave":
      if let ssid = args["ssid"] as? String { HotspotJoin.leave(ssid: ssid) }
      result(nil)
    case "jpegFromHeic":
      result(jpegFromHeic(args["path"] as? String ?? ""))
    case "pickFolder":
      pickFolder(result)
    case "folderUsable":
      result(bookmark(args).map(FolderBookmark.usable) ?? false)
    case "moveToFolder":
      guard let b = bookmark(args) else {
        result(0)
        return
      }
      let urls = (args["paths"] as? [String] ?? []).map { URL(fileURLWithPath: $0) }
      DispatchQueue.global(qos: .utility).async {
        let n = (try? FolderBookmark.move(urls, into: b)) ?? 0
        DispatchQueue.main.async { result(n) }
      }
    default:
      result(FlutterMethodNotImplemented)
    }
  }

  private func state(_ a: [String: Any]) -> TransferState {
    func u64(_ k: String) -> UInt64 { UInt64(max(0, (a[k] as? NSNumber)?.int64Value ?? 0)) }
    func u32(_ k: String) -> UInt32 { UInt32(clamping: (a[k] as? NSNumber)?.int64Value ?? 0) }
    return TransferState(
      phase: TransferState.Phase(rawValue: a["phase"] as? String ?? "") ?? .running,
      device: a["device"] as? String ?? "", sending: a["sending"] as? Bool ?? true,
      bytesDone: u64("bytesDone"), bytesTotal: u64("bytesTotal"), filesDone: u32("filesDone"),
      filesTotal: u32("filesTotal"))
  }

  /// The snapshot the extensions read, plus the Share sheet's device suggestions (F2): one donated
  /// `INSendMessageIntent` per paired device, whose conversation id is the device id.
  private func syncShared(_ a: [String: Any]) {
    let devices = (a["devices"] as? [[String: Any]] ?? []).compactMap { d -> SharedDevice? in
      guard let id = d["id"] as? String, id.count == 64 else { return nil }
      return SharedDevice(
        id: id, name: d["name"] as? String ?? "", platform: d["platform"] as? String ?? "",
        code: d["code"] as? String ?? "", lastSeen: UInt64((d["lastSeen"] as? NSNumber)?.int64Value ?? 0))
    }
    let snapshot = SharedSnapshot(
      deviceName: a["deviceName"] as? String ?? "", devices: devices, heicToJpeg: a["heicToJpeg"] as? Bool ?? false)
    try? store?.save(snapshot)
    WidgetCenter.shared.reloadAllTimelines()
    INInteraction.deleteAll { _ in
      for d in snapshot.devices.prefix(8) {
        let person = INPerson(
          personHandle: INPersonHandle(value: d.id, type: .unknown), nameComponents: nil, displayName: d.name,
          image: nil, contactIdentifier: nil, customIdentifier: d.id)
        let intent = INSendMessageIntent(
          recipients: [person], outgoingMessageType: .outgoingMessageText, content: nil,
          speakableGroupName: INSpeakableString(spokenPhrase: d.name), conversationIdentifier: d.id,
          serviceName: nil, sender: nil, attachments: nil)
        let interaction = INInteraction(intent: intent, response: nil)
        interaction.direction = .outgoing
        interaction.donate(completion: nil)
      }
    }
  }

  private func jpegFromHeic(_ path: String) -> [String: String]? {
    let src = URL(fileURLWithPath: path)
    guard HeicConverter.isHeic(src.lastPathComponent) else { return nil }
    let name = HeicConverter.jpegName(for: src.lastPathComponent)
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("heic", isDirectory: true)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let dest = FolderBookmark.freeName(name, in: dir) { FileManager.default.fileExists(atPath: $0.path) }
    guard (try? HeicConverter.convert(src, to: dest)) != nil else { return nil }
    return ["path": dest.path, "name": name]
  }

  private func bookmark(_ a: [String: Any]) -> Data? {
    (a["bookmark"] as? String).flatMap { Data(base64Encoded: $0) }
  }

  // MARK: Files folder (I7)

  private func pickFolder(_ result: @escaping FlutterResult) {
    guard pendingFolder == nil, let vc = registrar?.viewController ?? topController() else {
      result(nil)
      return
    }
    pendingFolder = result
    let picker = UIDocumentPickerViewController(forOpeningContentTypes: [.folder])
    picker.delegate = self
    picker.allowsMultipleSelection = false
    vc.present(picker, animated: true)
  }

  private func topController() -> UIViewController? {
    let scene = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first
    var vc = scene?.windows.first(where: \.isKeyWindow)?.rootViewController
    while let next = vc?.presentedViewController { vc = next }
    return vc
  }

  func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
    let result = pendingFolder
    pendingFolder = nil
    guard let url = urls.first, let data = try? FolderBookmark.make(for: url) else {
      result?(nil)
      return
    }
    result?(["bookmark": data.base64EncodedString(), "label": url.lastPathComponent])
  }

  func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
    pendingFolder?(nil)
    pendingFolder = nil
  }
}

/// The transfer Live Activity (I2, F9): started on the first progress event, updated at most twice a second, ended
/// with the last state and dismissed a few seconds later.
@available(iOS 16.2, *)
final class TransferActivityController {
  static let shared = TransferActivityController()

  private var activity: Activity<TransferAttributes>?
  private var gate = UpdateGate()

  func update(_ s: TransferState) {
    guard ActivityAuthorizationInfo().areActivitiesEnabled, gate.allows(s) else { return }
    let content = ActivityContent(state: s, staleDate: nil)
    if let a = activity {
      Task { await a.update(content) }
    } else {
      activity = try? Activity<TransferAttributes>.request(
        attributes: TransferAttributes(device: s.device, sending: s.sending), content: content, pushType: nil)
    }
  }

  func end(_ s: TransferState?) {
    guard let a = activity else { return }
    activity = nil
    gate = UpdateGate()
    let last = s ?? a.content.state
    let content = ActivityContent(state: last, staleDate: nil)
    Task { await a.end(content, dismissalPolicy: .after(Date().addingTimeInterval(4))) }
  }
}
