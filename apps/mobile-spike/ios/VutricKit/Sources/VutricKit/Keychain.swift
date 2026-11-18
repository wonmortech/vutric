// This phone's identity key in the Keychain, shared by the app and its extensions (backlog I8, ADR-0015). The
// access group is the App Group id: iOS counts application groups as Keychain access groups, so one capability
// covers both and no Team ID prefix has to be guessed at sideload time.

import Foundation
import Security

public struct IdentityKeychain {
  public static let service = "io.wonmor.vutric.identity"
  public static let account = "device"

  /// nil: the app's default group (used when the App Group is missing; the extensions then cannot read it).
  public let accessGroup: String?

  public init(accessGroup: String?) {
    self.accessGroup = accessGroup
  }

  public enum Failure: Error, Equatable {
    case badKey
    case status(OSStatus)
  }

  private var query: [String: Any] {
    var q: [String: Any] = [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: Self.service,
      kSecAttrAccount as String: Self.account,
    ]
    if let accessGroup { q[kSecAttrAccessGroup as String] = accessGroup }
    return q
  }

  /// The 32-byte key, or nil when none is stored (or this signature has no access to the group).
  public func read() -> Data? {
    var q = query
    q[kSecReturnData as String] = true
    q[kSecMatchLimit as String] = kSecMatchLimitOne
    var out: CFTypeRef?
    guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let data = out as? Data,
      data.count == 32
    else { return nil }
    return data
  }

  /// Stores (or replaces) the key. Readable after the first unlock, never synced to iCloud or other devices.
  public func write(_ key: Data) throws {
    guard key.count == 32 else { throw Failure.badKey }
    let attrs: [String: Any] = [
      kSecValueData as String: key,
      kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
    ]
    var status = SecItemUpdate(query as CFDictionary, attrs as CFDictionary)
    if status == errSecItemNotFound {
      var add = query
      add.merge(attrs) { $1 }
      status = SecItemAdd(add as CFDictionary, nil)
    }
    guard status == errSecSuccess else { throw Failure.status(status) }
  }

  public func delete() {
    SecItemDelete(query as CFDictionary)
  }
}
