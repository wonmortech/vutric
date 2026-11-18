// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

import AppKit

/// The menu-bar agent: the daemon role on macOS (ADR-0111). `LSUIElement` keeps it out of the Dock and the app
/// switcher; it owns the menu-bar item, the login item, notifications, and the entry points that hand files over
/// (Share extension, Services menu, `vutric://send`). Network, transfers and the overlay live in the Rust helper.
final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate {
    private let link = DaemonLink()
    private let notifier = Notifier()
    private let compat = CompatLink()
    private var history = History.load()
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

        compat.onEvent = { [weak self] e in self?.compatEvent(e) }
        if UserDefaults.standard.bool(forKey: Self.compatKey) { compat.start() }
    }

    private static let compatKey = "compatForeground"

    func applicationWillTerminate(_: Notification) {
        link.stopHelper()
        compat.stop()
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
        menu.addItem(historyMenu())
        menu.addItem(.separator())
        menu.addItem(compatToggle())
        if compat.isRunning {
            menu.addItem(compatSendMenu())
            compat.announce()
        }
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

    // MARK: - history (ADR-0119)

    private func historyMenu() -> NSMenuItem {
        let parent = entry("menu.history", nil)
        let sub = NSMenu()
        let shown = history.shown { FileManager.default.fileExists(atPath: $0) }
        if shown.isEmpty {
            let none = NSMenuItem(title: t("menu.history.none"), action: nil, keyEquivalent: "")
            none.isEnabled = false
            sub.addItem(none)
        }
        let time = DateFormatter()
        time.dateStyle = .none
        time.timeStyle = .short
        for e in shown {
            let title = [e.label, e.from, time.string(from: e.at)].compactMap { $0 }.joined(separator: " · ")
            let i = NSMenuItem(title: title, action: #selector(revealHistory(_:)), keyEquivalent: "")
            i.target = self
            i.representedObject = e.path
            i.toolTip = e.path
            sub.addItem(i)
        }
        if !shown.isEmpty {
            sub.addItem(.separator())
            sub.addItem(entry("menu.history.clear", #selector(clearHistory)))
        }
        parent.submenu = sub
        return parent
    }

    @objc private func revealHistory(_ sender: NSMenuItem) {
        guard let path = sender.representedObject as? String else { return }
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
    }

    @objc private func clearHistory() {
        history.clear()
        history.save()
    }

    private func remember(path: String, files: Int, from: String?) {
        let label = files == 1 ? (path as NSString).lastPathComponent : t("history.files", files)
        history.add(path: path, label: label, from: from.map { SendRequest.preview($0, max: 60) })
        history.save()
    }

    // MARK: - LocalSend and Quick Share (ADR-0117)

    private func compatToggle() -> NSMenuItem {
        let i = entry("menu.compat", #selector(toggleCompat))
        i.state = compat.isRunning ? .on : .off
        if !CompatLink.isBundled {
            i.action = nil
            i.toolTip = t("menu.compat.missing")
        }
        return i
    }

    private func compatSendMenu() -> NSMenuItem {
        let parent = entry("menu.compat.send", nil)
        let sub = NSMenu()
        if compat.devices.isEmpty {
            let none = NSMenuItem(title: t("menu.compat.none"), action: nil, keyEquivalent: "")
            none.isEnabled = false
            sub.addItem(none)
        }
        for (n, d) in compat.devices.enumerated() {
            let i = NSMenuItem(title: d.alias, action: #selector(sendToCompatDevice(_:)), keyEquivalent: "")
            i.target = self
            i.tag = n
            sub.addItem(i)
        }
        parent.submenu = sub
        return parent
    }

    @objc private func toggleCompat() {
        let on = !compat.isRunning
        UserDefaults.standard.set(on, forKey: Self.compatKey)
        if on {
            if !compat.start() { notifier.failed(reason: t("menu.compat.missing")) }
        } else {
            compat.stop()
        }
    }

    @objc private func sendToCompatDevice(_ sender: NSMenuItem) {
        guard compat.devices.indices.contains(sender.tag) else { return }
        let device = compat.devices[sender.tag]
        NSApp.activate(ignoringOtherApps: true)
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true
        panel.message = t("compat.send.to", device.alias)
        panel.prompt = t("send.confirm.button")
        guard panel.runModal() == .OK, let r = SendRequest(files: panel.urls) else { return }
        compat.send(files: r.files, to: device)
    }

    private func compatEvent(_ e: [String: Any]) {
        switch e["kind"] as? String {
        case "offer":
            guard let session = e["session"] as? String else { return }
            NSApp.activate(ignoringOtherApps: true)
            let alert = NSAlert()
            let via = (e["via"] as? String) == "quickshare" ? "Quick Share" : "LocalSend"
            alert.messageText = t("compat.offer.title", e["alias"] as? String ?? "?", e["files"] as? Int ?? 0)
            var lines = (e["names"] as? [String]) ?? []
            if (e["files"] as? Int ?? 0) > lines.count { lines.append("…") }
            lines.append(ByteCountFormatter.string(fromByteCount: Int64(e["bytes"] as? Int ?? 0), countStyle: .file)
                + " · " + via)
            if let code = e["code"] as? String { lines.append(t("compat.offer.code", code)) }
            alert.informativeText = lines.joined(separator: "\n")
            alert.addButton(withTitle: t("compat.offer.accept"))
            alert.addButton(withTitle: t("compat.offer.decline"))
            compat.decide(session: session, accept: alert.runModal() == .alertFirstButtonReturn)
        case "text":
            // Shown, never acted on: no link is opened; the user may copy it (ADR-0065 rules).
            guard let text = e["text"] as? String, !text.isEmpty else { return }
            NSApp.activate(ignoringOtherApps: true)
            let alert = NSAlert()
            alert.messageText = t((e["link"] as? Bool ?? false) ? "compat.link.title" : "compat.text.title")
            alert.informativeText = SendRequest.preview(text, max: 400)
            alert.addButton(withTitle: t("compat.text.copy"))
            alert.addButton(withTitle: t("compat.text.close"))
            if alert.runModal() == .alertFirstButtonReturn {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(text, forType: .string)
            }
        case "received":
            let path = e["path"] as? String ?? ""
            notifier.received(files: 1, folder: (path as NSString).deletingLastPathComponent, from: nil)
            remember(path: path, files: 1, from: nil)
        case "send_failed", "error":
            notifier.failed(reason: (e["reason"] as? String) ?? (e["message"] as? String) ?? "")
        default:
            break
        }
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

    /// Anything that did not come from our own menu is confirmed first (ADR-0112, ADR-0115).
    private func confirmAndSend(_ r: SendRequest) {
        NSApp.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        if let text = r.text {
            alert.messageText = t("send.text.confirm.title")
            alert.informativeText = SendRequest.preview(text)
        } else {
            alert.messageText = t("send.confirm.title", r.files.count)
            alert.informativeText = r.files.prefix(5).map(\.lastPathComponent).joined(separator: "\n")
                + (r.files.count > 5 ? "\n…" : "")
        }
        alert.addButton(withTitle: t("send.confirm.button"))
        alert.addButton(withTitle: t("send.confirm.cancel"))
        if alert.runModal() == .alertFirstButtonReturn { send(r) }
    }

    private func send(_ r: SendRequest) {
        guard case .running = state else {
            queued.append(r)
            return
        }
        switch r.payload {
        case let .files(files):
            // TEMP until W2 #29: not in protocol v1 yet.
            link.send(["op": "send_files", "paths": files.map(\.path)])
        case let .text(text):
            // TEMP until W2 #29 (ADR-0115): the daemon sends `Message::Clipboard` to a paired device (ADR-0065) and
            // the `.txt` written here to anything else (a guest, LocalSend, Quick Share).
            var op: [String: Any] = ["op": "send_text", "text": text]
            if let txt = try? Self.writeTextFile(text) { op["fallback_path"] = txt.path }
            link.send(op)
        }
    }

    /// `~/Library/Caches/io.wonmor.vutric/Outbox/Matn <time>.txt`; files older than a day are removed first.
    static func writeTextFile(_ text: String) throws -> URL {
        let fm = FileManager.default
        let dir = try fm.url(for: .cachesDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            .appendingPathComponent("io.wonmor.vutric/Outbox", isDirectory: true)
        try fm.createDirectory(at: dir, withIntermediateDirectories: true)
        let old = Date().addingTimeInterval(-86400)
        let keys: [URLResourceKey] = [.contentModificationDateKey]
        for f in (try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: keys)) ?? [] {
            let m = (try? f.resourceValues(forKeys: Set(keys)))?.contentModificationDate
            if let m, m < old { try? fm.removeItem(at: f) }
        }
        let url = dir.appendingPathComponent(SendRequest.textFileName())
        try Data(text.utf8).write(to: url, options: .atomic)
        return url
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
            let files = e["files"] as? Int ?? 1
            let folder = e["folder"] as? String ?? ""
            notifier.received(files: files, folder: folder, from: e["from"] as? String)
            remember(path: folder, files: files, from: e["from"] as? String)
        // ADR-0065 on macOS (ADR-0119); event names proposed to W2 on #30.
        case "notice":
            notifier.notice(
                app: e["app"] as? String ?? "", title: e["title"] as? String ?? "", text: e["text"] as? String ?? "")
        case "clipboard":
            guard let text = e["text"] as? String, !text.isEmpty else { return }
            offerCopy(text: text, from: e["from"] as? String)
        case "failed":
            notifier.failed(reason: e["reason"] as? String ?? "")
        default:
            break
        }
    }

    /// Clipboard text from a paired phone: a prompt with "Nusxalash", never written to the clipboard by itself
    /// (ADR-0065 §3).
    private func offerCopy(text: String, from: String?) {
        NSApp.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        alert.messageText = from.map { t("clipboard.title.from", SendRequest.preview($0, max: 60)) }
            ?? t("compat.text.title")
        alert.informativeText = SendRequest.preview(text, max: 400)
        alert.addButton(withTitle: t("compat.text.copy"))
        alert.addButton(withTitle: t("compat.text.close"))
        if alert.runModal() == .alertFirstButtonReturn {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
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

// A write to a helper that has just exited returns EPIPE instead of killing the agent.
signal(SIGPIPE, SIG_IGN)
let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.accessory)
app.run()
