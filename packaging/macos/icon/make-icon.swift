// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0
//
// Draws the macOS app icon and the Finder Quick Action template image from the brand mark (ADR-0116), so no icon
// binary is checked in: the source is `design/claude-design/logo-mark.svg` plus the colours below.
//
//   xcrun swift packaging/macos/icon/make-icon.swift design/claude-design/logo-mark.svg OUT_DIR
//
// Writes OUT_DIR/AppIcon.iconset/icon_*.png (for iconutil), OUT_DIR/QuickActionTemplate.png and @2x, and
// OUT_DIR/preview.png (1024 px, for review).

import AppKit
import Foundation

// Brand colours (site tokens, design/claude-design/README.md): Vutric Blue for the mark, cobalt for its shade,
// sand for the tile.
let vutricBlue = NSColor(srgbRed: 0x29 / 255, green: 0x69 / 255, blue: 0xFF / 255, alpha: 1)
let cobalt700 = NSColor(srgbRed: 0x1A / 255, green: 0x45 / 255, blue: 0xB8 / 255, alpha: 1)
let sand50 = NSColor(srgbRed: 0xF7 / 255, green: 0xF2 / 255, blue: 0xE9 / 255, alpha: 1)
let sand200 = NSColor(srgbRed: 0xE6 / 255, green: 0xDB / 255, blue: 0xC7 / 255, alpha: 1)

// MARK: - the mark, read from the SVG

struct Mark {
    let viewBox: CGRect
    let path: CGPath
}

/// Parses the `viewBox` and every `<path d="…">` of the logo. Only the absolute commands the file uses (M, L, H, V,
/// C, Z) are accepted; anything else stops the build, so a redrawn logo cannot be rendered half right.
func loadMark(_ url: URL) throws -> Mark {
    let svg = try String(contentsOf: url, encoding: .utf8)
    func attr(_ name: String, in s: Substring) -> [String] {
        var out: [String] = []
        var rest = s[...]
        while let r = rest.range(of: " \(name)=\"") {
            let after = rest[r.upperBound...]
            guard let end = after.firstIndex(of: "\"") else { break }
            out.append(String(after[..<end]))
            rest = after[end...]
        }
        return out
    }
    guard let vb = attr("viewBox", in: svg[...]).first?.split(separator: " ").compactMap({ Double($0) }), vb.count == 4
    else { throw Failure("no viewBox in \(url.path)") }
    let path = CGMutablePath()
    let ds = attr("d", in: svg[...])
    guard !ds.isEmpty else { throw Failure("no <path d> in \(url.path)") }
    for d in ds { try addPath(d, to: path) }
    return Mark(viewBox: CGRect(x: vb[0], y: vb[1], width: vb[2], height: vb[3]), path: path)
}

struct Failure: Error, CustomStringConvertible {
    let description: String
    init(_ d: String) { description = d }
}

func addPath(_ d: String, to path: CGMutablePath) throws {
    var tokens: [String] = []
    var cur = ""
    for ch in d {
        if ch.isLetter {
            if !cur.isEmpty { tokens.append(cur); cur = "" }
            tokens.append(String(ch))
        } else if ch == " " || ch == "," {
            if !cur.isEmpty { tokens.append(cur); cur = "" }
        } else if ch == "-", !cur.isEmpty, cur.last != "e" {
            tokens.append(cur); cur = "-"
        } else {
            cur.append(ch)
        }
    }
    if !cur.isEmpty { tokens.append(cur) }
    var i = 0
    var cmd = ""
    var p = CGPoint.zero
    func num() throws -> CGFloat {
        guard i < tokens.count, let v = Double(tokens[i]) else { throw Failure("bad number in path at token \(i)") }
        i += 1
        return CGFloat(v)
    }
    while i < tokens.count {
        if tokens[i].first!.isLetter { cmd = tokens[i]; i += 1 }
        switch cmd {
        case "M": p = CGPoint(x: try num(), y: try num()); path.move(to: p); cmd = "L"
        case "L": p = CGPoint(x: try num(), y: try num()); path.addLine(to: p)
        case "H": p.x = try num(); path.addLine(to: p)
        case "V": p.y = try num(); path.addLine(to: p)
        case "C":
            let c1 = CGPoint(x: try num(), y: try num())
            let c2 = CGPoint(x: try num(), y: try num())
            p = CGPoint(x: try num(), y: try num())
            path.addCurve(to: p, control1: c1, control2: c2)
        case "Z", "z": path.closeSubpath(); cmd = ""
        default: throw Failure("unsupported path command \(cmd)")
        }
    }
}

// MARK: - drawing

func bitmap(_ px: Int, _ draw: (CGContext, CGFloat) -> Void) -> NSBitmapImageRep {
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8, samplesPerPixel: 4,
        hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    rep.size = NSSize(width: px, height: px)
    NSGraphicsContext.saveGraphicsState()
    let ctx = NSGraphicsContext(bitmapImageRep: rep)!
    NSGraphicsContext.current = ctx
    ctx.cgContext.clear(CGRect(x: 0, y: 0, width: px, height: px))
    draw(ctx.cgContext, CGFloat(px))
    NSGraphicsContext.restoreGraphicsState()
    return rep
}

/// Places the mark (SVG coordinates, y down) centred in `box` (y up).
func markTransform(_ mark: Mark, in box: CGRect) -> CGAffineTransform {
    let s = min(box.width / mark.viewBox.width, box.height / mark.viewBox.height)
    let w = mark.viewBox.width * s, h = mark.viewBox.height * s
    let x = box.midX - w / 2, y = box.midY - h / 2
    return CGAffineTransform(translationX: x, y: y + h)
        .scaledBy(x: s, y: -s)
        .translatedBy(x: -mark.viewBox.minX, y: -mark.viewBox.minY)
}

/// macOS icon grid (Big Sur and later): an 824 pt tile in a 1024 pt canvas, corner radius 185.4, with a soft shadow.
func appIcon(_ mark: Mark, px: Int) -> NSBitmapImageRep {
    bitmap(px) { cg, size in
        let k = size / 1024
        let tile = CGRect(x: 100 * k, y: 100 * k, width: 824 * k, height: 824 * k)
        let shape = CGPath(roundedRect: tile, cornerWidth: 185.4 * k, cornerHeight: 185.4 * k, transform: nil)
        cg.saveGState()
        cg.setShadow(offset: CGSize(width: 0, height: -10 * k), blur: 24 * k,
                     color: NSColor.black.withAlphaComponent(0.28).cgColor)
        cg.addPath(shape)
        cg.setFillColor(sand50.cgColor)
        cg.fillPath()
        cg.restoreGState()

        cg.saveGState()
        cg.addPath(shape)
        cg.clip()
        let sand = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB),
                              colors: [sand50.cgColor, sand200.cgColor] as CFArray, locations: [0, 1])!
        cg.drawLinearGradient(sand, start: CGPoint(x: 0, y: tile.maxY), end: CGPoint(x: 0, y: tile.minY), options: [])

        var t = markTransform(mark, in: tile.insetBy(dx: 190 * k, dy: 170 * k))
        let glyph = mark.path.copy(using: &t)!
        cg.addPath(glyph)
        cg.clip()
        let blue = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB),
                              colors: [vutricBlue.cgColor, cobalt700.cgColor] as CFArray, locations: [0.35, 1])!
        let bounds = glyph.boundingBox
        cg.drawLinearGradient(blue, start: CGPoint(x: 0, y: bounds.maxY), end: CGPoint(x: 0, y: bounds.minY),
                              options: [])
        cg.restoreGState()
    }
}

/// A template image (black on transparent) for Finder's Quick Action button and the Touch Bar.
func template(_ mark: Mark, px: Int) -> NSBitmapImageRep {
    bitmap(px) { cg, size in
        var t = markTransform(mark, in: CGRect(x: 0, y: 0, width: size, height: size).insetBy(dx: size * 0.1, dy: size * 0.08))
        cg.addPath(mark.path.copy(using: &t)!)
        cg.setFillColor(NSColor.black.cgColor)
        cg.fillPath()
    }
}

func write(_ rep: NSBitmapImageRep, _ url: URL) throws {
    guard let png = rep.representation(using: .png, properties: [:]) else { throw Failure("PNG encode failed") }
    try png.write(to: url)
}

// MARK: - main

let args = CommandLine.arguments
guard args.count == 3 else {
    FileHandle.standardError.write("usage: make-icon.swift LOGO.svg OUT_DIR\n".data(using: .utf8)!)
    exit(2)
}
do {
    let mark = try loadMark(URL(fileURLWithPath: args[1]))
    let out = URL(fileURLWithPath: args[2], isDirectory: true)
    let set = out.appendingPathComponent("AppIcon.iconset", isDirectory: true)
    try FileManager.default.createDirectory(at: set, withIntermediateDirectories: true)
    for pt in [16, 32, 128, 256, 512] {
        try write(appIcon(mark, px: pt), set.appendingPathComponent("icon_\(pt)x\(pt).png"))
        try write(appIcon(mark, px: pt * 2), set.appendingPathComponent("icon_\(pt)x\(pt)@2x.png"))
    }
    try write(template(mark, px: 32), out.appendingPathComponent("QuickActionTemplate.png"))
    try write(template(mark, px: 64), out.appendingPathComponent("QuickActionTemplate@2x.png"))
    try write(appIcon(mark, px: 1024), out.appendingPathComponent("preview.png"))
    print("icons written to \(out.path) (mark bounds \(mark.path.boundingBox))")
} catch {
    FileHandle.standardError.write("make-icon: \(error)\n".data(using: .utf8)!)
    exit(1)
}
