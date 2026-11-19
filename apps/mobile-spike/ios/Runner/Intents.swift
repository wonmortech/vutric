// App Intents (backlog I3, the iOS half of F10): "Qurilmaga yuborish" for Siri, Shortcuts, automations and the
// Action button. The send runs in this process through Rust's share API (ADR-0015) with its own short-lived peer,
// so it works with no Flutter screen open and never disturbs the app's session.

import AppIntents
import Foundation
import VutricKit

@available(iOS 16.0, *)
struct DeviceEntity: AppEntity {
  static var typeDisplayRepresentation: TypeDisplayRepresentation = "intent.device.type"
  static var defaultQuery = DeviceQuery()

  var id: String
  var name: String

  var displayRepresentation: DisplayRepresentation {
    DisplayRepresentation(title: "\(name)")
  }

  init(_ d: SharedDevice) {
    id = d.id
    name = d.name
  }
}

/// The paired devices from the App Group snapshot (no Rust needed to list them).
@available(iOS 16.0, *)
struct DeviceQuery: EntityQuery {
  private var all: [DeviceEntity] { (SharedStore.app()?.snapshot()?.devices ?? []).map(DeviceEntity.init) }

  func entities(for identifiers: [DeviceEntity.ID]) async throws -> [DeviceEntity] {
    all.filter { identifiers.contains($0.id) }
  }

  func suggestedEntities() async throws -> [DeviceEntity] {
    all
  }
}

@available(iOS 16.0, *)
struct IntentFailure: Error, CustomLocalizedStringResourceConvertible {
  let text: String
  var localizedStringResource: LocalizedStringResource { LocalizedStringResource(stringLiteral: text) }
}

@available(iOS 16.0, *)
struct SendFilesIntent: AppIntent {
  static var title: LocalizedStringResource = "intent.send.title"
  static var description = IntentDescription("intent.send.description")

  @Parameter(title: "intent.send.device")
  var device: DeviceEntity

  @Parameter(title: "intent.send.files")
  var files: [IntentFile]

  static var parameterSummary: some ParameterSummary {
    Summary("\(\.$files) → \(\.$device)")
  }

  func perform() async throws -> some IntentResult & ProvidesDialog {
    let store: SharedStore
    do {
      store = try RustShare.open()
    } catch {
      throw IntentFailure(text: RustShare.message(error, device: device.name))
    }
    let dir = try store.newOutboxFolder()
    defer { try? FileManager.default.removeItem(at: dir) }
    var copied: [(url: URL, name: String)] = []
    for f in files {
      let name = f.filename.isEmpty ? "file" : f.filename
      let dest = FolderBookmark.freeName(name, in: dir) { FileManager.default.fileExists(atPath: $0.path) }
      if let src = f.fileURL {
        let scoped = src.startAccessingSecurityScopedResource()
        defer { if scoped { src.stopAccessingSecurityScopedResource() } }
        try FileManager.default.copyItem(at: src, to: dest)
      } else {
        try f.data.write(to: dest)
      }
      copied.append((dest, dest.lastPathComponent))
    }
    guard !copied.isEmpty else { throw IntentFailure(text: L10n.text("intent.error.no_files")) }
    let nearby = await Self.nearby()
    switch await RustShare.send(to: device.id, nearby: nearby, files: copied, progress: { _ in }) {
    case let .success(r):
      let text = L10n.text("intent.done", ["count": "\(r.files)", "device": device.name])
      return .result(dialog: IntentDialog(stringLiteral: text))
    case let .failure(e):
      throw IntentFailure(text: RustShare.message(e, device: device.name))
    }
  }

  /// Two seconds of Bonjour, so a computer on this Wi-Fi is dialed directly.
  private static func nearby() async -> [NearbyService] {
    let browser = NearbyBrowser()
    browser.start { _ in }
    try? await Task.sleep(nanoseconds: 2_000_000_000)
    let found: [NearbyService] = await withCheckedContinuation { cont in
      browser.queue.async { cont.resume(returning: browser.services) }
    }
    browser.stop()
    return found
  }
}

/// Opens Vutric on the send screen, aimed at a device (the Action button, or a widget-like shortcut).
@available(iOS 16.0, *)
struct OpenSendIntent: AppIntent {
  static var title: LocalizedStringResource = "intent.open.title"
  static var description = IntentDescription("intent.open.description")
  static var openAppWhenRun = true

  @Parameter(title: "intent.send.device")
  var device: DeviceEntity?

  func perform() async throws -> some IntentResult {
    try? SharedStore.app()?.push(Handoff(kind: .send, deviceId: device?.id))
    return .result()
  }
}

@available(iOS 16.0, *)
struct VutricShortcuts: AppShortcutsProvider {
  static var appShortcuts: [AppShortcut] {
    AppShortcut(
      intent: SendFilesIntent(),
      phrases: ["\(.applicationName) bilan yuborish", "Send with \(.applicationName)"],
      shortTitle: "intent.send.title",
      systemImageName: "arrow.up.circle"
    )
    AppShortcut(
      intent: OpenSendIntent(),
      phrases: ["\(.applicationName)’da yuborish", "Open \(.applicationName) to send"],
      shortTitle: "intent.open.title",
      systemImageName: "paperplane"
    )
  }
}
