// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

import Foundation

/// Every user-visible string goes through a key in `Localizable.xcstrings`: Uzbek (Latin) is the development
/// language, English the other one (CLAUDE.md, the same rule as the daemon's `i18n.rs`).
func t(_ key: String) -> String {
    NSLocalizedString(key, bundle: .main, comment: "")
}

/// A key whose value has one `%@` or `%lld` placeholder.
func t(_ key: String, _ args: CVarArg...) -> String {
    String(format: t(key), locale: Locale.current, arguments: args)
}
