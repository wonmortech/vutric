// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

import AppKit

/// The big windows (settings, history, devices) are W2's Slint process `vutric-app` (ADR-0030, ADR-0121): it runs
/// only while a window is open and talks to the daemon over the same IPC socket. The agent starts it with
/// `--page <name>`; while it is up, the agent brings it forward instead of starting a second one.
final class WindowLink {
    enum Page: String {
        case settings, history, devices
    }

    private var process: Process?

    static var helperURL: URL {
        Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/vutric-app")
    }

    static var isBundled: Bool {
        FileManager.default.isExecutableFile(atPath: helperURL.path)
    }

    var isRunning: Bool { process?.isRunning ?? false }

    /// Opens `page`. Returns false when the window app is not bundled or does not start.
    @discardableResult
    func open(_ page: Page) -> Bool {
        guard Self.isBundled else { return false }
        if let p = process, p.isRunning {
            // TEMP until vutric-app has a single-instance channel (W2): the open window comes forward as it is.
            NSRunningApplication(processIdentifier: p.processIdentifier)?.activate(options: [])
            return true
        }
        let p = Process()
        p.executableURL = Self.helperURL
        p.arguments = ["--page", page.rawValue]
        var env = ProcessInfo.processInfo.environment
        // ADR-0030: the windows use Slint's software renderer (6 MB private memory instead of 350 MB with OpenGL).
        if env["SLINT_BACKEND"] == nil { env["SLINT_BACKEND"] = "winit-software" }
        env["VUTRIC_IPC"] = DaemonLink.socketPath
        p.environment = env
        p.terminationHandler = { [weak self] _ in
            DispatchQueue.main.async { self?.process = nil }
        }
        do {
            try p.run()
        } catch {
            return false
        }
        process = p
        return true
    }

    /// Closes the window app with the agent (it exits on SIGTERM like on window close).
    func stop() {
        if let p = process, p.isRunning { p.terminate() }
        process = nil
    }
}
