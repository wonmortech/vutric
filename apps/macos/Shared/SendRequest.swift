// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

import Foundation

/// Files handed to the menu-bar agent by the Share extension, the Services menu or another app, as
/// `vutric://send?path=/abs/a.jpg&path=/abs/b.pdf` (ADR-0112).
///
/// The URL only proposes files. The agent always asks the user which device gets them before anything is sent, so
/// a link opened by any other app cannot send a file silently.
struct SendRequest: Equatable {
    static let scheme = "vutric"
    static let host = "send"
    /// One Share action hands over at most this many files; the Share extension's activation rule uses the same.
    static let maxFiles = 1000

    let files: [URL]

    init?(files: [URL]) {
        let clean = files.filter { $0.isFileURL && $0.path.hasPrefix("/") }
        guard !clean.isEmpty, clean.count <= Self.maxFiles else { return nil }
        self.files = clean
    }

    /// Parses an incoming URL. Anything that is not exactly our scheme and host, has no absolute paths, or has too
    /// many, is refused.
    init?(url: URL) {
        guard url.scheme?.lowercased() == Self.scheme, url.host?.lowercased() == Self.host,
              let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems
        else { return nil }
        let paths = items.filter { $0.name == "path" }.compactMap(\.value)
        guard paths.count <= Self.maxFiles else { return nil }
        let urls = paths
            .filter { $0.hasPrefix("/") && !$0.contains("\u{0}") }
            .map { URL(fileURLWithPath: $0).standardizedFileURL }
        self.init(files: urls)
    }

    var url: URL {
        var c = URLComponents()
        c.scheme = Self.scheme
        c.host = Self.host
        c.queryItems = files.map { URLQueryItem(name: "path", value: $0.path) }
        // URLComponents leaves "+" alone, which some parsers read as a space; encode it.
        c.percentEncodedQuery = c.percentEncodedQuery?.replacingOccurrences(of: "+", with: "%2B")
        return c.url!
    }
}
