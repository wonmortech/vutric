// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

import Foundation

/// What the menu-bar agent lists under "So'nggi qabul qilinganlar" (ADR-0119, backlog A10 on macOS): the last
/// receipts, newest first, kept in the agent's defaults. Only paths and labels: no file content, no device ids.
struct History: Codable, Equatable {
    struct Entry: Codable, Equatable {
        /// The received file, or the folder a multi-file transfer landed in.
        let path: String
        /// What the menu shows: the file name, or "3 ta fayl".
        let label: String
        let from: String?
        let at: Date
    }

    static let maxEntries = 20
    static let shownEntries = 10
    /// Receipts older than this drop off (the history row of T6 keeps 90 days).
    static let maxAge: TimeInterval = 90 * 24 * 3600

    private(set) var entries: [Entry] = []

    /// Adds a receipt; the same path again moves to the top instead of repeating.
    mutating func add(path: String, label: String, from: String?, at: Date = Date()) {
        guard path.hasPrefix("/") else { return }
        entries.removeAll { $0.path == path }
        entries.insert(Entry(path: path, label: label, from: from, at: at), at: 0)
        if entries.count > Self.maxEntries { entries.removeLast(entries.count - Self.maxEntries) }
    }

    /// What to show now: not expired, still on disk (`exists`), at most `shownEntries`.
    func shown(now: Date = Date(), exists: (String) -> Bool) -> [Entry] {
        Array(entries.filter { now.timeIntervalSince($0.at) < Self.maxAge && exists($0.path) }.prefix(Self.shownEntries))
    }

    mutating func clear() {
        entries.removeAll()
    }

    // MARK: - storage

    static let defaultsKey = "receiveHistory"

    static func load(_ defaults: UserDefaults = .standard) -> History {
        guard let data = defaults.data(forKey: defaultsKey),
              let h = try? JSONDecoder().decode(History.self, from: data)
        else { return History() }
        return h
    }

    func save(_ defaults: UserDefaults = .standard) {
        if let data = try? JSONEncoder().encode(self) { defaults.set(data, forKey: Self.defaultsKey) }
    }
}
