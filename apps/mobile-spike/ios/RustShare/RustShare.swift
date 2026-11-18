// The Swift side of `crates/vutric-mobile/src/share.rs` (ADR-0015), compiled into the app (App Intents) and the
// Share Extension next to the UniFFI bindings that `tool/ios_bindings.sh` generates into `ios/Generated/`.

import Foundation
import VutricKit

enum RustShare {
  enum OpenFailure: Error {
    /// The signature has no App Group (a sideload that dropped it).
    case noGroup
    /// The app has not run yet, so there is no identity in the shared Keychain.
    case noIdentity
  }

  /// Points Rust at the App Group's settings and trust files and hands it this phone's identity and name.
  static func open() throws -> SharedStore {
    guard let store = SharedStore.app() else { throw OpenFailure.noGroup }
    guard let key = IdentityKeychain(accessGroup: SharedStore.groupId()).read() else {
      throw OpenFailure.noIdentity
    }
    let name = store.snapshot()?.deviceName ?? ""
    try shareOpen(dir: store.rustDir.path, identity: key, name: name)
    return store
  }

  /// The line for a failed send, from the String Catalog.
  static func message(_ error: Error, device: String) -> String {
    guard let e = error as? ShareError else {
      return error is OpenFailure ? L10n.text("share.not_set_up") : L10n.text("share.error.failed")
    }
    switch e {
    case .NotSetUp: return L10n.text("share.not_set_up")
    case .NotPaired: return L10n.text("share.error.not_paired")
    case .Unreachable: return L10n.text("share.error.unreachable", ["device": device])
    case .Declined: return L10n.text("share.error.declined")
    case .Cancelled: return L10n.text("share.error.cancelled")
    case .TooBig: return L10n.text("bulk.too_big")
    case .Failed: return L10n.text("share.error.failed")
    }
  }

  static func hints(_ services: [NearbyService]) -> [NearbyHint] {
    services.map { NearbyHint(instance: $0.instance, addrs: $0.addrs) }
  }

  /// Runs `shareSend` on a background queue (it blocks until the transfer ends).
  static func send(
    to device: String, nearby: [NearbyService], files: [(url: URL, name: String)],
    progress: @escaping (ShareProgress) -> Void
  ) async -> Result<ShareResult, Error> {
    let listener = ProgressRelay(progress)
    let found = hints(nearby)
    return await withCheckedContinuation { cont in
      DispatchQueue.global(qos: .userInitiated).async {
        do {
          let r = try shareSend(
            deviceId: device, nearby: found, paths: files.map { $0.url.path }, names: files.map { $0.name },
            listener: listener)
          cont.resume(returning: .success(r))
        } catch {
          cont.resume(returning: .failure(error))
        }
      }
    }
  }
}

/// Rust calls this about every 150 ms from its own thread.
final class ProgressRelay: ShareListener, @unchecked Sendable {
  private let handler: (ShareProgress) -> Void

  init(_ handler: @escaping (ShareProgress) -> Void) {
    self.handler = handler
  }

  func onProgress(progress: ShareProgress) {
    handler(progress)
  }
}
