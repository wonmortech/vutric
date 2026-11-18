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

if failures > 0 {
    print("\(failures) check(s) failed")
    exit(1)
}
print("SendRequest: all checks passed")
