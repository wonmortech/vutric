// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

import Foundation

/// Foreground LocalSend and Quick Share (C1/C3 on macOS, ADR-0117): runs the bundled `vutric-mac-compat` helper while
/// the user has the menu switch on. The helper starts the `vutric-compat` adapter process and cleans everything it
/// reports, so the strings here are safe to show; paths it reports are files it already moved into Downloads.
final class CompatLink {
    struct Device: Equatable {
        let id: String
        let alias: String
    }

    var onEvent: (([String: Any]) -> Void)?
    private(set) var devices: [Device] = []
    var isRunning: Bool { process?.isRunning ?? false }

    private var process: Process?
    private var input: FileHandle?
    private var nextRequest: UInt64 = 1

    static var helperURL: URL {
        Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/vutric-mac-compat")
    }

    static var isBundled: Bool {
        FileManager.default.isExecutableFile(atPath: helperURL.path)
            && FileManager.default.isExecutableFile(
                atPath: Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/vutric-compat").path)
    }

    /// Starts the helper; `false` when this build has none.
    @discardableResult
    func start() -> Bool {
        guard process == nil, Self.isBundled else { return isRunning }
        let p = Process()
        p.executableURL = Self.helperURL
        p.arguments = ["--alias", Host.current().localizedName ?? "Mac"]
        let toHelper = Pipe(), fromHelper = Pipe()
        p.standardInput = toHelper
        p.standardOutput = fromHelper
        p.standardError = FileHandle.nullDevice
        p.terminationHandler = { [weak self] _ in
            DispatchQueue.main.async {
                self?.process = nil
                self?.input = nil
                self?.devices = []
                self?.onEvent?(["kind": "stopped"])
            }
        }
        do { try p.run() } catch { return false }
        process = p
        input = toHelper.fileHandleForWriting
        read(fromHelper.fileHandleForReading)
        return true
    }

    /// Closing stdin makes the helper stop its adapter and exit.
    func stop() {
        send(["op": "stop"])
        try? input?.close()
        input = nil
    }

    func decide(session: String, accept: Bool) {
        send(["op": "decide", "session": session, "accept": accept])
    }

    func announce() {
        send(["op": "announce"])
    }

    func send(files: [URL], to device: Device) {
        send(["op": "send", "request_id": nextRequest, "device": device.id, "paths": files.map(\.path)])
        nextRequest += 1
    }

    private func send(_ obj: [String: Any]) {
        guard let input, var data = try? JSONSerialization.data(withJSONObject: obj) else { return }
        data.append(0x0A)
        try? input.write(contentsOf: data)
    }

    private func read(_ handle: FileHandle) {
        DispatchQueue.global(qos: .utility).async { [weak self] in
            var buffer = Data()
            while true {
                let chunk = handle.availableData
                if chunk.isEmpty { return }
                buffer.append(chunk)
                // The helper caps its lines; anything this long is not from it.
                if buffer.count > 1 << 20 { buffer.removeAll() }
                while let nl = buffer.firstIndex(of: 0x0A) {
                    let line = buffer[buffer.startIndex..<nl]
                    buffer.removeSubrange(buffer.startIndex...nl)
                    guard let obj = (try? JSONSerialization.jsonObject(with: line)) as? [String: Any] else { continue }
                    DispatchQueue.main.async { self?.handle(obj) }
                }
            }
        }
    }

    private func handle(_ e: [String: Any]) {
        if e["kind"] as? String == "devices" {
            devices = (e["devices"] as? [[String: Any]] ?? []).compactMap {
                guard let id = $0["id"] as? String, let alias = $0["alias"] as? String else { return nil }
                return Device(id: id, alias: alias)
            }
        }
        onEvent?(e)
    }
}
