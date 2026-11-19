// Finds Vutric devices on this Wi-Fi through the system's Bonjour (backlog I4, ADR-0015). iroh's own mDNS in Rust
// needs raw multicast, which iOS allows only with the multicast entitlement; `NWBrowser` goes through
// mDNSResponder and needs only `NSBonjourServices` and the local-network permission. Each result is resolved to
// `ip:port` and handed to Rust as a dial hint (`NearbyHint`); Rust matches the instance name to the endpoint id.

import Foundation
import Network

/// One Vutric service on the network: iroh's instance name (the endpoint id in base32) and where it answers.
public struct NearbyService: Equatable, Hashable, Sendable {
  public var instance: String
  public var addrs: [String]

  public init(instance: String, addrs: [String]) {
    self.instance = instance
    self.addrs = addrs
  }
}

public enum NearbyAddress {
  /// `192.168.1.24:51000` or `[fe80::1]:51000`, the way Rust's `SocketAddr` parses it. The interface scope
  /// (`%en0`) is dropped: Rust's parser takes only numeric scopes, and iroh adds the local interface itself.
  public static func text(host: String, port: UInt16) -> String? {
    var h = host
    if let pct = h.firstIndex(of: "%") { h = String(h[..<pct]) }
    guard !h.isEmpty, port > 0 else { return nil }
    if h.contains(":") { return "[\(h)]:\(port)" }
    let parts = h.split(separator: ".")
    guard parts.count == 4, parts.allSatisfy({ UInt8($0) != nil }) else { return nil }
    return "\(h):\(port)"
  }

  static func text(_ endpoint: NWEndpoint) -> String? {
    guard case let .hostPort(host, port) = endpoint else { return nil }
    switch host {
    case let .ipv4(a): return text(host: "\(a)", port: port.rawValue)
    case let .ipv6(a): return text(host: "\(a)", port: port.rawValue)
    default: return nil
    }
  }
}

/// Browses `_vutric._udp` and keeps every service resolved. Not thread-safe: use it from one queue (`queue`).
public final class NearbyBrowser {
  public static let serviceType = "_vutric._udp"

  public let queue: DispatchQueue
  private var browser: NWBrowser?
  private var resolving: [String: NWConnection] = [:]
  private var found: [String: NearbyService] = [:]
  private var onChange: (([NearbyService]) -> Void)?

  public init(queue: DispatchQueue = DispatchQueue(label: "vutric.nearby")) {
    self.queue = queue
  }

  /// Starts browsing; `onChange` runs on `queue` with every service resolved so far.
  public func start(onChange: @escaping ([NearbyService]) -> Void) {
    queue.async { [self] in
      guard browser == nil else { return }
      self.onChange = onChange
      let b = NWBrowser(for: .bonjour(type: Self.serviceType, domain: nil), using: .udp)
      b.browseResultsChangedHandler = { [weak self] results, _ in self?.update(results) }
      b.start(queue: queue)
      browser = b
    }
  }

  public func stop() {
    queue.async { [self] in
      browser?.cancel()
      browser = nil
      resolving.values.forEach { $0.cancel() }
      resolving.removeAll()
      onChange = nil
    }
  }

  /// What is known right now (call on `queue`).
  public var services: [NearbyService] { Array(found.values) }

  private func update(_ results: Set<NWBrowser.Result>) {
    var names = Set<String>()
    for r in results {
      guard case let .service(name, _, _, _) = r.endpoint else { continue }
      names.insert(name)
      if found[name] == nil && resolving[name] == nil { resolve(name, r.endpoint) }
    }
    for gone in found.keys where !names.contains(gone) { found[gone] = nil }
    onChange?(services)
  }

  /// A UDP "connection" to a Bonjour endpoint resolves it; its path then names the address. Nothing is sent.
  private func resolve(_ name: String, _ endpoint: NWEndpoint) {
    let c = NWConnection(to: endpoint, using: .udp)
    resolving[name] = c
    c.stateUpdateHandler = { [weak self, weak c] state in
      guard let self, let c else { return }
      switch state {
      case .ready:
        if let remote = c.currentPath?.remoteEndpoint, let addr = NearbyAddress.text(remote) {
          var s = self.found[name] ?? NearbyService(instance: name, addrs: [])
          if !s.addrs.contains(addr) { s.addrs.append(addr) }
          self.found[name] = s
          self.onChange?(self.services)
        }
        c.cancel()
        self.resolving[name] = nil
      case .failed, .cancelled:
        self.resolving[name] = nil
      default:
        break
      }
    }
    c.start(queue: queue)
  }
}
