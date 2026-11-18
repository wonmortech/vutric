// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

import Darwin
import Foundation

/// The agent's side of the daemon IPC (`vutric_daemon::ipc`, protocol version 1): JSON objects, one per line, over a
/// per-user Unix socket. The agent starts the bundled `vutric-daemon` helper when nothing answers (ADR-0111).
final class DaemonLink {
    static let protocolVersion = 1

    enum State: Equatable {
        case starting
        case running(paused: Bool, transfers: Int, online: Bool)
        /// No helper in the bundle and nothing listening: a build without the daemon.
        case missing
        case failed(String)
    }

    var onState: ((State) -> Void)?
    var onEvent: (([String: Any]) -> Void)?

    private let queue = DispatchQueue(label: "io.wonmor.vutric.ipc")
    private var fd: Int32 = -1
    private var helper: Process?
    private var pending: [[String: Any]] = []

    /// `$VUTRIC_IPC`, else `~/Library/Application Support/Vutric/daemon.sock` (a per-user folder, mode 0700).
    /// The daemon must listen on the same path: W2 issue `needs-w2` "daemon IPC socket path on macOS".
    static var socketPath: String {
        if let p = ProcessInfo.processInfo.environment["VUTRIC_IPC"], !p.isEmpty { return p }
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("Vutric/daemon.sock").path
    }

    func start() {
        queue.async { self.connectOrLaunch(attempt: 0) }
    }

    func send(_ request: [String: Any]) {
        queue.async {
            guard self.fd >= 0 else {
                self.pending.append(request)
                return
            }
            self.write(request)
        }
    }

    func stopHelper() {
        queue.sync { helper?.terminate() }
    }

    // MARK: - connection

    private func connectOrLaunch(attempt: Int) {
        if connect() {
            write(["op": "hello", "version": Self.protocolVersion])
            write(["op": "subscribe"])
            write(["op": "status"])
            pending.forEach(write)
            pending.removeAll()
            readLoop()
            return
        }
        if attempt == 0 {
            guard launchHelper() else {
                publish(.missing)
                return
            }
            publish(.starting)
        }
        if attempt < 40 {
            queue.asyncAfter(deadline: .now() + .milliseconds(250)) { self.connectOrLaunch(attempt: attempt + 1) }
        } else {
            publish(.failed(t("daemon.error.noanswer")))
        }
    }

    private func connect() -> Bool {
        let path = Self.socketPath
        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let capacity = MemoryLayout.size(ofValue: addr.sun_path)
        guard path.utf8.count < capacity else { return false }
        let s = socket(AF_UNIX, SOCK_STREAM, 0)
        guard s >= 0 else { return false }
        withUnsafeMutableBytes(of: &addr.sun_path) { raw in
            _ = path.withCString { strncpy(raw.baseAddress!.assumingMemoryBound(to: CChar.self), $0, capacity - 1) }
        }
        let ok = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(s, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) == 0
            }
        }
        if ok {
            var on: Int32 = 1
            setsockopt(s, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))
            fd = s
        } else {
            close(s)
        }
        return ok
    }

    /// Starts `Contents/MacOS/vutric-daemon` as a child, so macOS attributes its Local Network use to this app.
    private func launchHelper() -> Bool {
        guard helper == nil, let exe = Bundle.main.url(forAuxiliaryExecutable: "vutric-daemon") else {
            return helper != nil
        }
        try? FileManager.default.createDirectory(
            atPath: (Self.socketPath as NSString).deletingLastPathComponent,
            withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700])
        let p = Process()
        p.executableURL = exe
        p.arguments = ["--agent"]
        var env = ProcessInfo.processInfo.environment
        env["VUTRIC_IPC"] = Self.socketPath
        p.environment = env
        do {
            try p.run()
        } catch {
            return false
        }
        helper = p
        return true
    }

    private func write(_ obj: [String: Any]) {
        guard fd >= 0, var data = try? JSONSerialization.data(withJSONObject: obj) else { return }
        data.append(0x0A)
        let ok = data.withUnsafeBytes { buf -> Bool in
            var off = 0
            while off < buf.count {
                let n = Darwin.write(fd, buf.baseAddress! + off, buf.count - off)
                if n <= 0 { return false }
                off += n
            }
            return true
        }
        if !ok { drop() }
    }

    private func readLoop() {
        var buffer = Data()
        var chunk = [UInt8](repeating: 0, count: 64 * 1024)
        while fd >= 0 {
            let n = read(fd, &chunk, chunk.count)
            if n <= 0 { break }
            buffer.append(contentsOf: chunk[0..<n])
            while let nl = buffer.firstIndex(of: 0x0A) {
                let line = buffer[buffer.startIndex..<nl]
                buffer.removeSubrange(buffer.startIndex...nl)
                handle(line)
            }
            // The daemon refuses lines over 256 KiB; never buffer more than that from it either.
            if buffer.count > 256 * 1024 { break }
        }
        drop()
    }

    private func drop() {
        if fd >= 0 { close(fd) }
        fd = -1
        publish(.failed(t("daemon.error.lost")))
        queue.asyncAfter(deadline: .now() + .seconds(2)) { self.connectOrLaunch(attempt: 1) }
    }

    private func handle(_ line: Data) {
        guard let obj = (try? JSONSerialization.jsonObject(with: line)) as? [String: Any],
              let kind = obj["kind"] as? String
        else { return }
        switch kind {
        case "status":
            publish(.running(
                paused: obj["paused"] as? Bool ?? false,
                transfers: obj["transfers"] as? Int ?? 0,
                online: obj["online"] as? Bool ?? false))
        case "error":
            let reason = obj["message"] as? String ?? ""
            DispatchQueue.main.async { self.onEvent?(["event": "failed", "reason": reason]) }
        case "event":
            if (obj["event"] as? String) == "paused" { write(["op": "status"]) }
            DispatchQueue.main.async { self.onEvent?(obj) }
        default:
            break
        }
    }

    private func publish(_ s: State) {
        DispatchQueue.main.async { self.onState?(s) }
    }
}
