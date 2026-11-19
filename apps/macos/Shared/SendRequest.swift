// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

import Foundation

/// What the Share extension, the Finder Quick Action, the Services menu or another app hands to the menu-bar agent
/// (ADR-0112, ADR-0115):
///
/// - files: `vutric://send?path=/abs/a.jpg&path=/abs/b.pdf`
/// - text or links: `vutric://send?text=…` (one item, at most 64 KiB, the cap of ADR-0065's `Clipboard` message)
///
/// The URL only proposes. The agent always asks the user before anything is sent, so a link opened by any other app
/// cannot send a file or a text silently.
struct SendRequest: Equatable {
    enum Payload: Equatable {
        case files([URL])
        case text(String)
    }

    static let scheme = "vutric"
    static let host = "send"
    /// One Share action hands over at most this many files; the extensions' activation rules use the same.
    static let maxFiles = 1000
    /// ADR-0065: `Clipboard { text }` is 1 byte to 64 KiB.
    static let maxTextBytes = 64 * 1024

    let payload: Payload

    var files: [URL] {
        if case let .files(f) = payload { return f }
        return []
    }

    var text: String? {
        if case let .text(t) = payload { return t }
        return nil
    }

    init?(files: [URL]) {
        let clean = files.filter { $0.isFileURL && $0.path.hasPrefix("/") }
        guard !clean.isEmpty, clean.count <= Self.maxFiles else { return nil }
        payload = .files(clean)
    }

    /// Text or links. NUL characters are dropped; blank or oversized text is refused (the extension says why).
    init?(text: String) {
        let t = text.replacingOccurrences(of: "\u{0}", with: "")
        guard !t.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              t.utf8.count <= Self.maxTextBytes
        else { return nil }
        payload = .text(t)
    }

    /// Parses an incoming URL. Anything that is not exactly our scheme and host, mixes files and text, has more than
    /// one text, no absolute paths, or too many, is refused.
    init?(url: URL) {
        guard url.scheme?.lowercased() == Self.scheme, url.host?.lowercased() == Self.host,
              let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems
        else { return nil }
        let paths = items.filter { $0.name == "path" }.compactMap(\.value)
        let texts = items.filter { $0.name == "text" }.compactMap(\.value)
        switch (paths.isEmpty, texts.count) {
        case (true, 1):
            self.init(text: texts[0])
        case (false, 0):
            guard paths.count <= Self.maxFiles else { return nil }
            let urls = paths
                .filter { $0.hasPrefix("/") && !$0.contains("\u{0}") }
                .map { URL(fileURLWithPath: $0).standardizedFileURL }
            self.init(files: urls)
        default:
            return nil
        }
    }

    var url: URL {
        var c = URLComponents()
        c.scheme = Self.scheme
        c.host = Self.host
        switch payload {
        case let .files(files): c.queryItems = files.map { URLQueryItem(name: "path", value: $0.path) }
        case let .text(text): c.queryItems = [URLQueryItem(name: "text", value: text)]
        }
        // URLComponents leaves "+" alone, which some parsers read as a space; encode it.
        c.percentEncodedQuery = c.percentEncodedQuery?.replacingOccurrences(of: "+", with: "%2B")
        return c.url!
    }

    /// A short, single-line preview for the confirmation: control, format and bidi-override characters removed,
    /// whitespace collapsed, at most `max` characters (the cleaning ADR-0065's `shown` does before display).
    static func preview(_ text: String, max: Int = 160) -> String {
        var out = String.UnicodeScalarView()
        for s in text.unicodeScalars {
            switch s.properties.generalCategory {
            case .control, .lineSeparator, .paragraphSeparator:
                out.append(" ")
            case .format, .surrogate, .unassigned, .privateUse:
                continue
            default:
                out.append(s)
            }
        }
        let flat = String(out).split(separator: " ", omittingEmptySubsequences: true).joined(separator: " ")
        return flat.count > max ? String(flat.prefix(max)) + "…" : flat
    }

    /// Name of the `.txt` the text goes as when the peer cannot take a `Clipboard` message:
    /// `Matn 2026-11-18 14.05.09.txt`.
    static func textFileName(at date: Date = Date()) -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd HH.mm.ss"
        return "Matn \(f.string(from: date)).txt"
    }
}
