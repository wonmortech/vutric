// Joining the hotspot a scanned pairing link carries (backlog I5, ADR-0005, ADR-0058) with NEHotspotConfiguration.
// The answer codes are the ones Android's LinkManager gives Dart (`unavailable`, `bad_network`, `permission`,
// `unsupported`), so the Flutter screen words both platforms the same way.

import Foundation

/// The link's `security` field (ADR-0058).
public enum HotspotSecurity: String, Sendable {
  case wpa2
  case wpa3
  case open

  /// WPA2 and WPA3 personal take the same passphrase setup on iOS (it picks the protocol); open has none.
  public var needsPassphrase: Bool { self != .open }
}

public enum HotspotAnswer: String, Equatable, Sendable {
  case joined
  case unavailable
  case badNetwork = "bad_network"
  case permission
  case unsupported
}

public enum HotspotRequest {
  /// Checks a request before iOS sees it: SSID 1…32 bytes, WPA passphrase 8…63 characters.
  public static func check(ssid: String, password: String, security: String) -> HotspotAnswer? {
    guard let sec = HotspotSecurity(rawValue: security) else { return .badNetwork }
    let ssidBytes = ssid.utf8.count
    guard ssidBytes >= 1, ssidBytes <= 32 else { return .badNetwork }
    if sec.needsPassphrase && !(8...63).contains(password.count) { return .badNetwork }
    return nil
  }

  /// `NEHotspotConfigurationError` raw codes → our answer. `alreadyAssociated` (13) means joined.
  public static func answer(forErrorCode code: Int) -> HotspotAnswer {
    switch code {
    case 13: return .joined  // alreadyAssociated
    case 0...6, 15: return .badNetwork  // invalid, invalid SSID, passphrase, EAP settings, SSID prefix
    case 10: return .permission  // systemConfiguration: a profile manages Wi-Fi
    default: return .unavailable  // userDenied, internal, pending, not in foreground, …
    }
  }
}

#if os(iOS) && canImport(NetworkExtension)
  import NetworkExtension

  public enum HotspotJoin {
    /// Joins for this app only (`joinOnce`): iOS drops the network when the app leaves the screen for long, and it
    /// never lands in the saved networks. Checks afterwards that the phone is really on that SSID, because iOS can
    /// report success without joining.
    public static func join(
      ssid: String, password: String, security: String, hidden: Bool,
      completion: @escaping (HotspotAnswer) -> Void
    ) {
      if let bad = HotspotRequest.check(ssid: ssid, password: password, security: security) {
        completion(bad)
        return
      }
      let sec = HotspotSecurity(rawValue: security) ?? .wpa2
      let config =
        sec.needsPassphrase
        ? NEHotspotConfiguration(ssid: ssid, passphrase: password, isWEP: false)
        : NEHotspotConfiguration(ssid: ssid)
      config.joinOnce = true
      config.hidden = hidden
      NEHotspotConfigurationManager.shared.apply(config) { error in
        if let error = error as NSError? {
          let a =
            error.domain == NEHotspotConfigurationErrorDomain
            ? HotspotRequest.answer(forErrorCode: error.code) : .unavailable
          guard a == .joined else {
            completion(a)
            return
          }
        }
        NEHotspotNetwork.fetchCurrent { current in
          completion(current?.ssid == ssid ? .joined : .unavailable)
        }
      }
    }

    public static func leave(ssid: String) {
      NEHotspotConfigurationManager.shared.removeConfiguration(forSSID: ssid)
    }
  }
#endif
