// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

import AppKit

/// The menu-bar agent: the daemon role on macOS (ADR-0111). `LSUIElement` keeps it out of the Dock and the app
/// switcher; it owns the menu-bar item, the login item, notifications, and the entry points that hand files over
/// (Share extension, Services menu, `vutric://send`). Network, transfers and the overlay live in the Rust helper.
final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate {
    private let link = DaemonLink()
    private let notifier = Notifier()
    private var item: NSStatusItem!
    private var state: DaemonLink.State = .starting
    private var queued: [SendRequest] = []

    func applicationDidFinishLaunching(_: Notification) {
        item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        item.button?.image = NSImage(systemSymbolName: "paperplane.circle", accessibilityDescription: "Vutric")
        item.button?.image?.isTemplate = true
        let menu = NSMenu()
        menu.delegate = self
        item.menu = menu

        notifier.setUp()
        NSApp.servicesProvider = self
        firstLaunchLoginItem()

        link.onState = { [weak self] s in self?.update(s) }
        link.onEvent = { [weak self] e in self?.event(e) }
        link.start()
    }

    func applicationWillTerminate(_: Notification) {
        link.stopHelper()
    }

    func application(_: NSApplication, open urls: [URL]) {
        for url in urls {
            if let r = SendRequest(url: url) { confirmAndSend(r) }
        }
    }

    // MARK: - menu

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let status = NSMenuItem(title: statusLine(), action: nil, keyEquivalent: "")
        status.isEnabled = false
        menu.addItem(status)
        menu.addItem(.separator())
        menu.addItem(entry("menu.connect", #selector(connectPhone)))
        menu.addItem(entry("menu.send", #selector(pickAndSend)))
        menu.addItem(.separator())
        let paused: Bool
        if case let .running(p, _, _) = state { paused = p } else { paused = false }
        menu.addItem(entry(paused ? "menu.resume" : "menu.pause", #selector(togglePause)))
        let login = entry("menu.login", #selector(toggleLogin))
        login.state = LoginItem.isOn ? .on : .off
        menu.addItem(login)
        let settings = entry("menu.settings", nil)
        settings.toolTip = t("menu.settings.later")
        menu.addItem(settings)
        menu.addItem(.separator())
        menu.addItem(entry("menu.quit", #selector(quit)))
        let running: Bool
        if case .running = state { running = true } else { running = false }
        for i in menu.items where [#selector(connectPhone), #selector(togglePause)].contains(i.action) {
            i.isEnabled = running
        }
    }

    private func entry(_ key: String, _ action: Selector?) -> NSMenuItem {
        let i = NSMenuItem(title: t(key), action: action, keyEquivalent: "")
        i.target = self
        return i
    }

    private func statusLine() -> String {
        switch state {
        case .starting: return t("status.starting")
        case .missing: return t("status.missing")
        case let .failed(why): return t("status.failed", why)
        case let .running(paused, transfers, online):
            if paused { return t("status.paused") }
            if transfers > 0 { return t("status.transfers", transfers) }
            return online ? t("status.ready.online") : t("status.ready.lan")
        }
    }

    @objc private func connectPhone() {
        link.send(["op": "show"])
    }

    @objc private func togglePause() {
        guard case let .running(paused, _, _) = state else { return }
        link.send(["op": "pause", "paused": !paused])
    }

    @objc private func toggleLogin() {
        do {
            try LoginItem.set(!LoginItem.isOn)
            if LoginItem.needsApproval { SMAppServiceHint.show() }
        } catch {
            notifier.failed(reason: error.localizedDescription)
        }
    }

    @objc private func pickAndSend() {
        NSApp.activate(ignoringOtherApps: true)
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = true
        panel.prompt = t("send.confirm.button")
        guard panel.runModal() == .OK, let r = SendRequest(files: panel.urls) else { return }
        send(r)
    }

    @objc private func quit() {
        link.send(["op": "quit"])
        DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(300)) { NSApp.terminate(nil) }
    }

    // MARK: - handing files over

    /// Services menu ("Vutric bilan yuborish" on files in Finder). Declared in Info.plist `NSServices`.
    @objc func sendFiles(_ pboard: NSPasteboard, userData _: String?, error _: AutoreleasingUnsafeMutablePointer<NSString?>) {
        let urls = pboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL]
        if let urls, let r = SendRequest(files: urls) { confirmAndSend(r) }
    }

    /// Anything that did not come from our own menu is confirmed first (ADR-0112).
    private func confirmAndSend(_ r: SendRequest) {
        NSApp.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        alert.messageText = t("send.confirm.title", r.files.count)
        alert.informativeText = r.files.prefix(5).map(\.lastPathComponent).joined(separator: "\n")
            + (r.files.count > 5 ? "\n…" : "")
        alert.addButton(withTitle: t("send.confirm.button"))
        alert.addButton(withTitle: t("send.confirm.cancel"))
        if alert.runModal() == .alertFirstButtonReturn { send(r) }
    }

    private func send(_ r: SendRequest) {
        guard case .running = state else {
            queued.append(r)
            return
        }
        // Not in protocol v1 yet: W2 #29.
        link.send(["op": "send_files", "paths": r.files.map(\.path)])
    }

    // MARK: - daemon

    private func update(_ s: DaemonLink.State) {
        state = s
        item.button?.appearsDisabled = !(s.isRunning)
        if s.isRunning {
            queued.forEach(send)
            queued.removeAll()
        }
    }

    private func event(_ e: [String: Any]) {
        switch e["event"] as? String {
        case "received":
            notifier.received(
                files: e["files"] as? Int ?? 1, folder: e["folder"] as? String ?? "", from: nil)
        case "failed":
            notifier.failed(reason: e["reason"] as? String ?? "")
        default:
            break
        }
    }

    private func firstLaunchLoginItem() {
        let key = "loginItemOffered"
        guard !UserDefaults.standard.bool(forKey: key) else { return }
        UserDefaults.standard.set(true, forKey: key)
        // Same default as the Windows daemon (autostart on); the menu has the switch.
        try? LoginItem.set(true)
    }
}

extension DaemonLink.State {
    var isRunning: Bool {
        if case .running = self { return true }
        return false
    }
}

/// Points the user at System Settings when macOS wants the login item approved there.
enum SMAppServiceHint {
    static func show() {
        let alert = NSAlert()
        alert.messageText = t("login.approve.title")
        alert.informativeText = t("login.approve.body")
        alert.runModal()
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.accessory)
app.run()
