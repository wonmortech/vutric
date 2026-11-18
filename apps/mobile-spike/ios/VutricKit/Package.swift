// swift-tools-version:5.9
// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

// What the app, the Share Extension and the widget extension share on iOS (ADR-0015): the App Group store, the
// shared Keychain identity, Bonjour discovery, HEIC → JPEG, Files bookmarks, the hotspot join and the UI strings.
// No Flutter and no Rust here, so its tests run on the simulator in seconds.

import PackageDescription

let package = Package(
  name: "VutricKit",
  defaultLocalization: "uz",
  platforms: [.iOS(.v15), .macOS(.v12)],
  products: [
    .library(name: "VutricKit", targets: ["VutricKit"]),
  ],
  targets: [
    .target(
      name: "VutricKit",
      resources: [.process("Resources")]
    ),
    .testTarget(
      name: "VutricKitTests",
      dependencies: ["VutricKit"]
    ),
  ]
)
