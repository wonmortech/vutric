// One transfer's progress as the Live Activity, the widget and the Share Extension show it (backlog I2). The app
// feeds it from the same Rust progress events the Flutter sheet uses (about every 150 ms; the activity is updated
// at most twice a second).

import Foundation

public struct TransferState: Codable, Equatable, Hashable, Sendable {
  public enum Phase: String, Codable, Sendable {
    case waiting
    case running
    case done
    case failed
  }

  public var phase: Phase
  /// The other device's name.
  public var device: String
  /// This phone sends (true) or receives.
  public var sending: Bool
  public var bytesDone: UInt64
  public var bytesTotal: UInt64
  public var filesDone: UInt32
  public var filesTotal: UInt32

  public init(
    phase: Phase, device: String, sending: Bool, bytesDone: UInt64 = 0, bytesTotal: UInt64 = 0,
    filesDone: UInt32 = 0, filesTotal: UInt32 = 0
  ) {
    self.phase = phase
    self.device = device
    self.sending = sending
    self.bytesDone = bytesDone
    self.bytesTotal = bytesTotal
    self.filesDone = filesDone
    self.filesTotal = filesTotal
  }

  /// 0…1, or nil while the size is unknown (the bar is then indeterminate).
  public var fraction: Double? {
    guard bytesTotal > 0 else { return phase == .done ? 1 : nil }
    return min(1, Double(bytesDone) / Double(bytesTotal))
  }

  /// "42%", or "" while unknown.
  public var percentText: String {
    guard let f = fraction else { return "" }
    return "\(Int((f * 100).rounded(.down)))%"
  }

  /// The headline: `activity.sending`, `activity.receiving`, `activity.done` or `activity.failed`.
  public var titleKey: String {
    switch phase {
    case .waiting, .running: return sending ? "activity.sending" : "activity.receiving"
    case .done: return "activity.done"
    case .failed: return "activity.failed"
    }
  }

  public var title: String { L10n.text(titleKey, ["device": device]) }

  /// "3 / 10 fayl · 1.2 GB / 4 GB".
  public var detail: String {
    L10n.text(
      "activity.detail",
      [
        "done": "\(filesDone)", "total": "\(filesTotal)",
        "bytes": ByteText.of(bytesDone), "size": ByteText.of(bytesTotal),
      ])
  }
}

/// Sizes exactly as the Flutter app writes them (`sizeText` in `transfer_ui.dart`): 1000-based, KB whole, MB one
/// decimal, GB two.
public enum ByteText {
  public static func of(_ bytes: UInt64) -> String {
    let b = Double(bytes)
    if b < 1e3 { return "\(bytes) B" }
    if b < 1e6 { return String(format: "%.0f KB", b / 1e3) }
    if b < 1e9 { return String(format: "%.1f MB", b / 1e6) }
    return String(format: "%.2f GB", b / 1e9)
  }
}

/// Lets a stream of progress through at most every `interval`, plus every phase change (the Live Activity has
/// an update budget; ActivityKit throttles apps that update too often).
public struct UpdateGate {
  public let interval: TimeInterval
  private var last: Date?
  private var lastPhase: TransferState.Phase?

  public init(interval: TimeInterval = 0.5) {
    self.interval = interval
  }

  public mutating func allows(_ state: TransferState, at now: Date = Date()) -> Bool {
    let ok = last.map { state.phase != lastPhase || now.timeIntervalSince($0) >= interval } ?? true
    if ok {
      last = now
      lastPhase = state.phase
    }
    return ok
  }
}
