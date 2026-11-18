// UI copy for the extensions, behind the same keys as the Flutter app's `lib/i18n.dart` (Uzbek Latin first,
// English fallback). The String Catalog is `Resources/Localizable.xcstrings`; a Dart test checks both tables agree.

import Foundation

public enum L10n {
  /// The line for `key`, with `{name}` placeholders filled from `args`; the key itself when missing, so a gap is
  /// visible in testing.
  public static func text(_ key: String, _ args: [String: String] = [:]) -> String {
    var s = Bundle.module.localizedString(forKey: key, value: key, table: "Localizable")
    for (k, v) in args {
      s = s.replacingOccurrences(of: "{\(k)}", with: v)
    }
    return s
  }

  /// The line for `key` in one language (`uz` or `en`), or nil; for tests.
  public static func line(_ key: String, language: String) -> String? {
    guard let path = Bundle.module.path(forResource: language, ofType: "lproj"),
      let bundle = Bundle(path: path)
    else { return nil }
    let s = bundle.localizedString(forKey: key, value: "\u{0}", table: "Localizable")
    return s == "\u{0}" ? nil : s
  }

  /// Every key the extensions use.
  public static let keys: [String] = [
    "share.title", "share.empty", "share.not_set_up", "share.cancel", "share.close", "share.preparing",
    "share.connecting", "share.sending", "share.progress", "share.done", "share.nothing",
    "share.error.not_paired", "share.error.unreachable", "share.error.declined", "share.error.cancelled",
    "share.error.failed", "bulk.too_big",
    "activity.sending", "activity.receiving", "activity.done", "activity.failed", "activity.detail",
    "widget.send.title", "widget.send.none", "widget.send.description", "widget.activity.description",
    "intent.done", "intent.error.no_files",
  ]
}
