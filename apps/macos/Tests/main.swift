// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

// Plain executable tests (no Xcode project, no XCTest): built with swiftc next to the files under test and run in
// CI by packaging/macos/build-app.sh --test.

import Foundation

var failures = 0

func check(_ ok: Bool, _ what: String, line: Int = #line) {
    if !ok {
        failures += 1
        print("FAIL line \(line): \(what)")
    }
}

// Round trip, with characters that need escaping in a query.
let files = ["/Users/a/Rasmlar/qish kuni.jpg", "/tmp/a+b&c=d?.pdf", "/Users/a/O'zbek/fayl#1.txt"]
    .map { URL(fileURLWithPath: $0) }
let req = SendRequest(files: files)!
let back = SendRequest(url: req.url)
check(back?.files.map(\.path) == files.map(\.path), "round trip: \(String(describing: back?.files))")

// Only our scheme and host.
check(SendRequest(url: URL(string: "https://send?path=/a")!) == nil, "other scheme refused")
check(SendRequest(url: URL(string: "vutric://pair?path=/a")!) == nil, "other host refused")
check(SendRequest(url: URL(string: "VUTRIC://SEND?path=/a")!) != nil, "scheme and host are case-insensitive")

// Relative paths and empty requests are refused.
check(SendRequest(url: URL(string: "vutric://send?path=etc/passwd")!) == nil, "relative path refused")
check(SendRequest(url: URL(string: "vutric://send")!) == nil, "no files refused")
check(SendRequest(files: [URL(string: "https://example.com/a")!]) == nil, "non-file URL refused")

// Dot segments are resolved, so the confirmation shows the real file.
let dotted = SendRequest(url: URL(string: "vutric://send?path=/Users/a/../b/c.txt")!)
check(dotted?.files.first?.path == "/Users/b/c.txt", "dot segments resolved: \(String(describing: dotted))")

// The cap.
let many = (0...SendRequest.maxFiles).map { URL(fileURLWithPath: "/tmp/\($0)") }
check(SendRequest(files: many) == nil, "over the cap refused")
check(SendRequest(files: Array(many.dropLast())) != nil, "at the cap accepted")

// Text and links (ADR-0115).
let text = "Salom, dunyo! a+b=c & d?e #f\nhttps://vutric.wonmor.io/?q=1&r=2"
let treq = SendRequest(text: text)!
check(SendRequest(url: treq.url)?.text == text, "text round trip: \(treq.url)")
check(SendRequest(url: treq.url)?.files == [], "a text request has no files")
check(SendRequest(text: "   \n\t ") == nil, "blank text refused")
check(SendRequest(text: "a\u{0}b")?.text == "ab", "NUL dropped")
let atCap = String(repeating: "x", count: SendRequest.maxTextBytes)
check(SendRequest(text: atCap) != nil, "64 KiB accepted")
check(SendRequest(text: atCap + "x") == nil, "over 64 KiB refused")
check(SendRequest(text: String(repeating: "\u{02BB}", count: SendRequest.maxTextBytes / 2 + 1)) == nil,
      "the cap counts UTF-8 bytes, not characters")
check(SendRequest(url: URL(string: "vutric://send?text=a&text=b")!) == nil, "two texts refused")
check(SendRequest(url: URL(string: "vutric://send?text=a&path=/tmp/a")!) == nil, "text and files mixed refused")
check(SendRequest(url: URL(string: "vutric://send?text=")!) == nil, "empty text refused")

// Preview: no controls, no bidi overrides, one line, bounded.
let cleaned = SendRequest.preview("a\u{202E}b\nc\u{7}d")
check(cleaned == "ab c d", "preview cleaned: \(cleaned)")
check(SendRequest.preview(String(repeating: "y", count: 500), max: 10) == "yyyyyyyyyy…", "preview bounded")
check(SendRequest.textFileName(at: Date(timeIntervalSince1970: 0)).hasPrefix("Matn 19"), "txt name")
check(SendRequest.textFileName().hasSuffix(".txt"), "txt suffix")

// What a Share or Quick Action invocation becomes (ADR-0115).
var shared = SharedItems()
check(shared.request() == .failure(.nothing), "nothing shared")
shared.texts = ["  Vutric  ", "Vutric"]
shared.links = [URL(string: "https://vutric.wonmor.io/")!]
check((try? shared.request().get())?.text == "Vutric\nhttps://vutric.wonmor.io/", "title and link, de-duplicated")
shared.files = [URL(fileURLWithPath: "/tmp/a.txt")]
check((try? shared.request().get())?.files.map(\.path) == ["/tmp/a.txt"], "files win over text")
check(SharedItems(texts: [String(repeating: "z", count: SendRequest.maxTextBytes + 1)]).request() == .failure(.tooLong),
      "too long is told apart from nothing")

if failures > 0 {
    print("\(failures) check(s) failed")
    exit(1)
}
print("SendRequest, SharedItems: all checks passed")
