// The App Group folder the app and its extensions share (backlog I8, ADR-0015). Rust keeps its settings and trust
// files in `vutric/` here, so the Share Extension's Rust sees the same paired devices as the app. Next to them, a
// small JSON snapshot lets the widget and App Intents list devices without loading Rust, a handoff file carries
// "open the app on this device" from a widget tap or an intent, and the outbox holds the Share Extension's copies.

import Foundation

/// A paired device as the extensions show it. The app writes the list after every pairing change; Rust's
/// `trusted.json` stays the truth for who may receive.
public struct SharedDevice: Codable, Equatable, Hashable, Identifiable, Sendable {
  /// The endpoint id, 64 hex.
  public var id: String
  public var name: String
  /// `android`, `ios`, `windows`, `linux`, `macos` or "".
  public var platform: String
  /// `ABCD-EFGH`.
  public var code: String
  /// Unix seconds.
  public var lastSeen: UInt64

  public init(id: String, name: String, platform: String, code: String, lastSeen: UInt64) {
    self.id = id
    self.name = name
    self.platform = platform
    self.code = code
    self.lastSeen = lastSeen
  }

  /// Whether this is a computer (the "PC'ga yuborish" widget offers only those).
  public var isComputer: Bool { ["windows", "linux", "macos"].contains(platform) }
}

/// What the app last told the extensions.
public struct SharedSnapshot: Codable, Equatable, Sendable {
  public var deviceName: String
  /// Most recently seen first.
  public var devices: [SharedDevice]
  /// Send HEIC photos as JPEG (backlog I6).
  public var heicToJpeg: Bool

  public init(deviceName: String, devices: [SharedDevice], heicToJpeg: Bool) {
    self.deviceName = deviceName
    self.devices = devices.sorted { $0.lastSeen > $1.lastSeen }
    self.heicToJpeg = heicToJpeg
  }

  /// The computer the widget sends to: the one seen most recently.
  public var lastComputer: SharedDevice? { devices.first(where: \.isComputer) }
}

/// Something an extension or a widget asks the app to do when it opens next.
public struct Handoff: Codable, Equatable, Sendable {
  public enum Kind: String, Codable, Sendable {
    /// Open the Otish tab, aimed at `deviceId` when set.
    case send
  }

  public var kind: Kind
  public var deviceId: String?
  public var at: Date

  public init(kind: Kind, deviceId: String?, at: Date = Date()) {
    self.kind = kind
    self.deviceId = deviceId
    self.at = at
  }

  /// The event the Dart side reads (`IosBridge`), same shape as Android's.
  public var event: [String: Any] {
    var e: [String: Any] = ["kind": kind.rawValue]
    if let deviceId { e["device"] = deviceId }
    return e
  }
}

public final class SharedStore {
  /// The Info.plist key every target carries with the App Group id. Re-signing can change the group, so the id
  /// is read from the bundle instead of being compiled in.
  public static let groupKey = "VutricAppGroup"

  public let root: URL
  private let fm = FileManager.default

  /// A store rooted anywhere (tests).
  public init(root: URL) {
    self.root = root
  }

  /// The App Group store, or nil when the group is missing from the signature (a sideload that dropped it): the
  /// extensions then say "open Vutric once" instead of failing later.
  public static func app(bundle: Bundle = .main) -> SharedStore? {
    guard let id = bundle.object(forInfoDictionaryKey: groupKey) as? String, !id.isEmpty,
      let url = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: id)
    else { return nil }
    return SharedStore(root: url)
  }

  /// The App Group id this bundle was built with, also the Keychain access group (ADR-0015).
  public static func groupId(bundle: Bundle = .main) -> String? {
    bundle.object(forInfoDictionaryKey: groupKey) as? String
  }

  /// Rust's settings folder (`settings.json`, `trusted.json`), created on first use.
  public var rustDir: URL { folder("vutric") }

  /// Copies of shared files waiting to be sent, one folder per share.
  public var outbox: URL { folder("outbox") }

  private func folder(_ name: String) -> URL {
    let url = root.appendingPathComponent(name, isDirectory: true)
    try? fm.createDirectory(at: url, withIntermediateDirectories: true)
    return url
  }

  private var snapshotURL: URL { root.appendingPathComponent("snapshot.json") }
  private var handoffURL: URL { root.appendingPathComponent("handoff.json") }

  private static let encoder: JSONEncoder = {
    let e = JSONEncoder()
    e.dateEncodingStrategy = .secondsSince1970
    e.outputFormatting = [.sortedKeys]
    return e
  }()

  private static let decoder: JSONDecoder = {
    let d = JSONDecoder()
    d.dateDecodingStrategy = .secondsSince1970
    return d
  }()

  private func read<T: Decodable>(_ type: T.Type, at url: URL) -> T? {
    guard let data = try? Data(contentsOf: url) else { return nil }
    return try? Self.decoder.decode(type, from: data)
  }

  private func write<T: Encodable>(_ value: T, at url: URL) throws {
    try Self.encoder.encode(value).write(to: url, options: [.atomic])
  }

  // MARK: Snapshot

  public func snapshot() -> SharedSnapshot? { read(SharedSnapshot.self, at: snapshotURL) }

  public func save(_ snapshot: SharedSnapshot) throws { try write(snapshot, at: snapshotURL) }

  // MARK: Handoff

  /// Adds a request for the app; at most the last 8 are kept, so a widget tapped many times stays small.
  public func push(_ handoff: Handoff) throws {
    var all = read([Handoff].self, at: handoffURL) ?? []
    all.append(handoff)
    try write(Array(all.suffix(8)), at: handoffURL)
  }

  /// The requests waiting for the app, oldest first, and removes them. Requests older than `maxAge` are dropped:
  /// a tap from yesterday must not send anything today.
  public func takeHandoffs(now: Date = Date(), maxAge: TimeInterval = 10 * 60) -> [Handoff] {
    let all = read([Handoff].self, at: handoffURL) ?? []
    try? fm.removeItem(at: handoffURL)
    return all.filter { now.timeIntervalSince($0.at) <= maxAge && $0.at <= now.addingTimeInterval(60) }
  }

  // MARK: Outbox

  /// A fresh folder for one share's files.
  public func newOutboxFolder() throws -> URL {
    let url = outbox.appendingPathComponent(UUID().uuidString, isDirectory: true)
    try fm.createDirectory(at: url, withIntermediateDirectories: true)
    return url
  }

  /// Removes outbox folders older than `age` (a share the system killed mid-way leaves one behind).
  public func cleanOutbox(olderThan age: TimeInterval = 24 * 3600, now: Date = Date()) {
    let keys: [URLResourceKey] = [.creationDateKey]
    guard let items = try? fm.contentsOfDirectory(at: outbox, includingPropertiesForKeys: keys) else { return }
    for item in items {
      let created = (try? item.resourceValues(forKeys: Set(keys)))?.creationDate ?? .distantPast
      if now.timeIntervalSince(created) > age { try? fm.removeItem(at: item) }
    }
  }
}

/// `vutric://send?device=<id>`: what a widget tap opens.
public enum VutricURL {
  public static let scheme = "vutric"

  public static func send(device: String?) -> URL {
    var c = URLComponents()
    c.scheme = scheme
    c.host = "send"
    if let device { c.queryItems = [URLQueryItem(name: "device", value: device)] }
    return c.url!
  }

  /// The handoff a URL asks for; nil for anything that is not ours.
  public static func handoff(from url: URL, now: Date = Date()) -> Handoff? {
    guard url.scheme?.lowercased() == scheme,
      let c = URLComponents(url: url, resolvingAgainstBaseURL: false),
      c.host == "send"
    else { return nil }
    let device = c.queryItems?.first(where: { $0.name == "device" })?.value
    let valid = device.map { $0.count == 64 && $0.allSatisfy(\.isHexDigit) } ?? true
    return Handoff(kind: .send, deviceId: valid ? device : nil, at: now)
  }
}
