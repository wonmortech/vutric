// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

import Foundation
import UniformTypeIdentifiers

/// What a Share or Quick Action invocation carried, read out of its `NSExtensionItem`s (ADR-0115).
///
/// Files win: if any file URL was shared, only files are sent. Otherwise web links and plain text become one text,
/// one item per line, in the order the app gave them (a page shared from Safari arrives as its URL, often with the
/// page title as the item's text).
struct SharedItems: Equatable {
    var files: [URL] = []
    var links: [URL] = []
    var texts: [String] = []

    enum Refusal: Error, Equatable {
        case nothing
        case tooLong
    }

    func request() -> Result<SendRequest, Refusal> {
        if !files.isEmpty {
            return SendRequest(files: files).map { .success($0) } ?? .failure(.nothing)
        }
        var lines: [String] = []
        for t in texts + links.map(\.absoluteString) {
            let s = t.trimmingCharacters(in: .whitespacesAndNewlines)
            if !s.isEmpty, !lines.contains(s) { lines.append(s) }
        }
        let joined = lines.joined(separator: "\n")
        if joined.isEmpty { return .failure(.nothing) }
        if joined.utf8.count > SendRequest.maxTextBytes { return .failure(.tooLong) }
        return SendRequest(text: joined).map { .success($0) } ?? .failure(.nothing)
    }

    /// Loads every attachment, then calls `done` on the main queue. Each provider is asked for one kind only, in
    /// the order file URL, web URL, plain text.
    static func collect(_ items: [NSExtensionItem], _ done: @escaping (SharedItems) -> Void) {
        let group = DispatchGroup()
        let lock = NSLock()
        var found = SharedItems()
        func add(_ change: (inout SharedItems) -> Void) {
            lock.lock()
            change(&found)
            lock.unlock()
        }
        for item in items {
            if let text = item.attributedContentText?.string { add { $0.texts.append(text) } }
            for p in item.attachments ?? [] {
                let kind: UTType
                if p.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) {
                    kind = .fileURL
                } else if p.hasItemConformingToTypeIdentifier(UTType.url.identifier) {
                    kind = .url
                } else if p.hasItemConformingToTypeIdentifier(UTType.plainText.identifier) {
                    kind = .plainText
                } else {
                    continue
                }
                group.enter()
                p.loadItem(forTypeIdentifier: kind.identifier, options: nil) { value, _ in
                    defer { group.leave() }
                    switch kind {
                    case .plainText:
                        let s = (value as? String) ?? (value as? Data).flatMap { String(data: $0, encoding: .utf8) }
                        if let s { add { $0.texts.append(s) } }
                    default:
                        let url = (value as? URL)
                            ?? (value as? Data).flatMap { URL(dataRepresentation: $0, relativeTo: nil) }
                        guard let url else { return }
                        if url.isFileURL {
                            add { $0.files.append(url) }
                        } else if ["http", "https"].contains(url.scheme?.lowercased() ?? "") {
                            add { $0.links.append(url) }
                        }
                    }
                }
            }
        }
        group.notify(queue: .main) { done(found) }
    }
}
