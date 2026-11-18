// The Share Extension's state (backlog I1, F2, ADR-0015): copies what was shared into the App Group outbox while the
// user picks a device, then streams it through Rust. No Flutter engine runs here; the whole extension stays far
// below the system's memory limit for share extensions because Rust reads each file in chunks.

import Foundation
import Intents
import UniformTypeIdentifiers
import VutricKit

@MainActor
final class ShareModel: ObservableObject {
  enum Stage: Equatable {
    case loading
    case notSetUp
    case pick
    case connecting(String)
    case sending(String)
    case done(Int)
    case failed(String)
  }

  @Published private(set) var stage: Stage = .loading
  @Published private(set) var devices: [ShareDevice] = []
  @Published private(set) var progress: ShareProgress?

  private weak var context: NSExtensionContext?
  private var store: SharedStore?
  private var outbox: URL?
  private var collecting: Task<[(url: URL, name: String)], Never>?
  private let nearby = NearbyBrowser()
  private var services: [NearbyService] = []

  init(context: NSExtensionContext?) {
    self.context = context
  }

  func start() {
    do {
      store = try RustShare.open()
    } catch {
      stage = .notSetUp
      return
    }
    devices = shareDevices()
    stage = .pick
    nearby.start { [weak self] found in
      Task { @MainActor in self?.services = found }
    }
    let providers = (context?.inputItems as? [NSExtensionItem] ?? []).flatMap { $0.attachments ?? [] }
    let heic = store?.snapshot()?.heicToJpeg ?? false
    if let store, let dir = try? store.newOutboxFolder() {
      outbox = dir
      collecting = Task.detached { await Self.collect(providers, into: dir, heicToJpeg: heic) }
    }
    // A tap on a device suggestion in the Share sheet (F2) names the device: send at once.
    if let intent = context?.intent as? INSendMessageIntent, let id = intent.conversationIdentifier,
      let d = devices.first(where: { $0.id == id })
    {
      send(d)
    }
  }

  func send(_ device: ShareDevice) {
    guard stage == .pick else { return }
    stage = .connecting(device.name)
    Task {
      let files = await collecting?.value ?? []
      guard !files.isEmpty else {
        stage = .failed(L10n.text("share.nothing"))
        return
      }
      let r = await RustShare.send(to: device.id, nearby: services, files: files) { [weak self] p in
        Task { @MainActor in
          guard let self else { return }
          self.progress = p
          if case .connecting = self.stage { self.stage = .sending(device.name) }
        }
      }
      removeOutbox()
      switch r {
      case let .success(result):
        stage = .done(Int(result.files))
        try? await Task.sleep(nanoseconds: 900_000_000)
        finish()
      case let .failure(error):
        stage = .failed(RustShare.message(error, device: device.name))
      }
    }
  }

  func cancel() {
    switch stage {
    case .connecting, .sending: shareCancel()
    default: break
    }
    nearby.stop()
    removeOutbox()
    context?.cancelRequest(withError: NSError(domain: NSCocoaErrorDomain, code: NSUserCancelledError))
  }

  func finish() {
    nearby.stop()
    removeOutbox()
    context?.completeRequest(returningItems: nil)
  }

  private func removeOutbox() {
    if let outbox { try? FileManager.default.removeItem(at: outbox) }
    outbox = nil
    store?.cleanOutbox()
  }

  // MARK: Copying what was shared

  /// One file per attachment, in order. Files and photos keep their bytes (HEIC becomes JPEG when the app's
  /// setting says so); a web page becomes a small text file with its address; plain text becomes a `.txt`.
  nonisolated private static func collect(_ providers: [NSItemProvider], into dir: URL, heicToJpeg: Bool) async -> [(
    url: URL, name: String
  )] {
    var out: [(url: URL, name: String)] = []
    for (i, p) in providers.enumerated() {
      if let file = await copyFile(p, into: dir) {
        out.append(heicToJpeg ? jpegIfHeic(file, in: dir) : file)
      } else if let text = await loadText(p) {
        let name = text.isLink ? "link-\(i + 1).txt" : "text-\(i + 1).txt"
        let url = dir.appendingPathComponent(name)
        if (try? Data(text.value.utf8).write(to: url)) != nil { out.append((url, name)) }
      }
    }
    return out
  }

  /// The most specific data type the provider has, skipping links and plain text (handled as text).
  nonisolated private static func dataType(of p: NSItemProvider) -> String? {
    p.registeredTypeIdentifiers.first { id in
      guard let t = UTType(id) else { return false }
      return t.conforms(to: .data) && !t.conforms(to: .url) && !t.conforms(to: .plainText)
    } ?? p.registeredTypeIdentifiers.first { UTType($0)?.conforms(to: .fileURL) == true }
  }

  nonisolated private static func copyFile(_ p: NSItemProvider, into dir: URL) async -> (url: URL, name: String)? {
    guard let type = dataType(of: p) else { return nil }
    return await withCheckedContinuation { cont in
      _ = p.loadFileRepresentation(forTypeIdentifier: type) { url, _ in
        // `url` is deleted when this returns: copy now (APFS clones it, no bytes are read).
        guard let url else {
          cont.resume(returning: nil)
          return
        }
        let ext = url.pathExtension
        var name = p.suggestedName ?? url.lastPathComponent
        if (name as NSString).pathExtension.isEmpty && !ext.isEmpty { name += ".\(ext)" }
        let fm = FileManager.default
        let dest = FolderBookmark.freeName(name, in: dir) { fm.fileExists(atPath: $0.path) }
        do {
          try fm.copyItem(at: url, to: dest)
          cont.resume(returning: (dest, dest.lastPathComponent))
        } catch {
          cont.resume(returning: nil)
        }
      }
    }
  }

  nonisolated private static func jpegIfHeic(_ file: (url: URL, name: String), in dir: URL) -> (url: URL, name: String) {
    guard HeicConverter.isHeic(file.name) else { return file }
    let name = HeicConverter.jpegName(for: file.name)
    let dest = FolderBookmark.freeName(name, in: dir) { FileManager.default.fileExists(atPath: $0.path) }
    guard (try? HeicConverter.convert(file.url, to: dest)) != nil else { return file }
    try? FileManager.default.removeItem(at: file.url)
    return (dest, dest.lastPathComponent)
  }

  nonisolated private static func loadText(_ p: NSItemProvider) async -> (value: String, isLink: Bool)? {
    for (type, isLink) in [(UTType.url, true), (UTType.plainText, false)]
    where p.hasItemConformingToTypeIdentifier(type.identifier) {
      let item: NSSecureCoding? = await withCheckedContinuation { cont in
        p.loadItem(forTypeIdentifier: type.identifier, options: nil) { item, _ in cont.resume(returning: item) }
      }
      if let url = item as? URL { return (url.absoluteString, isLink) }
      if let s = item as? String { return (s, isLink) }
    }
    return nil
  }
}
