// A Files folder the user picked for received files (backlog I7), kept as a bookmark so the access survives a
// restart. Rust still saves into the app's own folder and verifies; the app then moves each finished file here,
// as Android's SafStore does (ADR-0012).

import Foundation

public enum FolderBookmark {
  public enum Failure: Error, Equatable {
    case noAccess
  }

  /// The bookmark for a folder the document picker returned.
  public static func make(for url: URL) throws -> Data {
    let scoped = url.startAccessingSecurityScopedResource()
    defer { if scoped { url.stopAccessingSecurityScopedResource() } }
    return try url.bookmarkData(options: [], includingResourceValuesForKeys: nil, relativeTo: nil)
  }

  /// The folder, and a fresh bookmark when the old one went stale (iOS moved the item and wants a new one).
  public static func resolve(_ bookmark: Data) throws -> (url: URL, refreshed: Data?) {
    var stale = false
    let url = try URL(resolvingBookmarkData: bookmark, options: [], relativeTo: nil, bookmarkDataIsStale: &stale)
    return (url, stale ? (try? make(for: url)) : nil)
  }

  /// Whether the folder still exists and can be opened.
  public static func usable(_ bookmark: Data) -> Bool {
    guard let url = try? resolve(bookmark).url else { return false }
    let scoped = url.startAccessingSecurityScopedResource()
    defer { if scoped { url.stopAccessingSecurityScopedResource() } }
    var dir: ObjCBool = false
    return FileManager.default.fileExists(atPath: url.path, isDirectory: &dir) && dir.boolValue
  }

  /// `name` in `dir`, or `name (2)`, `name (3)` … when taken, the way the Files app names copies.
  public static func freeName(_ name: String, in dir: URL, exists: (URL) -> Bool) -> URL {
    let first = dir.appendingPathComponent(name)
    if !exists(first) { return first }
    let ns = name as NSString
    let base = ns.deletingPathExtension
    let ext = ns.pathExtension
    var n = 2
    while true {
      let candidate = dir.appendingPathComponent(ext.isEmpty ? "\(base) (\(n))" : "\(base) (\(n)).\(ext)")
      if !exists(candidate) { return candidate }
      n += 1
    }
  }

  /// Moves `files` into the folder (coordinated, so iCloud Drive and other providers see a whole file); returns
  /// how many moved. A file that fails stays where it was.
  public static func move(_ files: [URL], into bookmark: Data) throws -> Int {
    let dir = try resolve(bookmark).url
    let scoped = dir.startAccessingSecurityScopedResource()
    defer { if scoped { dir.stopAccessingSecurityScopedResource() } }
    let fm = FileManager.default
    var moved = 0
    for file in files {
      let dest = freeName(file.lastPathComponent, in: dir) { fm.fileExists(atPath: $0.path) }
      var coordError: NSError?
      var ok = false
      NSFileCoordinator().coordinate(writingItemAt: dest, options: .forReplacing, error: &coordError) { target in
        ok = (try? fm.copyItem(at: file, to: target)) != nil
      }
      if ok && coordError == nil {
        try? fm.removeItem(at: file)
        moved += 1
      }
    }
    return moved
  }
}
