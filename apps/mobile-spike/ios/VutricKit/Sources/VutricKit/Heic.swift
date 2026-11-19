// HEIC → JPEG before sending (backlog I6): Windows opens JPEG everywhere, HEIC only with an extra codec. ImageIO
// re-encodes the first image and keeps its metadata (orientation, date, place); the original stays untouched.

import Foundation
import ImageIO

public enum HeicConverter {
  public enum Failure: Error, Equatable {
    case unreadable
    case notWritten
  }

  /// Whether `name` is a HEIC or HEIF photo by its extension.
  public static func isHeic(_ name: String) -> Bool {
    let ext = (name as NSString).pathExtension.lowercased()
    return ext == "heic" || ext == "heif"
  }

  /// `IMG_0001.HEIC` → `IMG_0001.jpg`.
  public static func jpegName(for name: String) -> String {
    // Same rule as Dart's `heicAsJpegName`: cut at the last dot, so ".HEIC" (no name) becomes "photo.jpg"; NSString
    // would treat it as a hidden file with no extension.
    let base = name.lastIndex(of: ".").map { String(name[..<$0]) } ?? name
    return (base.isEmpty ? "photo" : base) + ".jpg"
  }

  /// Writes a JPEG of `source`'s first image to `destination` at `quality` (0…1).
  public static func convert(_ source: URL, to destination: URL, quality: Double = 0.9) throws {
    guard let src = CGImageSourceCreateWithURL(source as CFURL, nil), CGImageSourceGetCount(src) > 0 else {
      throw Failure.unreadable
    }
    guard
      let dst = CGImageDestinationCreateWithURL(destination as CFURL, "public.jpeg" as CFString, 1, nil)
    else { throw Failure.notWritten }
    let options: [CFString: Any] = [kCGImageDestinationLossyCompressionQuality: min(1, max(0, quality))]
    CGImageDestinationAddImageFromSource(dst, src, 0, options as CFDictionary)
    guard CGImageDestinationFinalize(dst) else { throw Failure.notWritten }
  }
}
