// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

import AppKit
import UniformTypeIdentifiers

/// "Vutric bilan yuborish" in the macOS Share menu (Finder, Photos exports, any app that shares files).
///
/// The extension is sandboxed and has no network of its own: it collects the shared file URLs and hands them to
/// the menu-bar agent as `vutric://send?path=…` (ADR-0112). The agent confirms and asks the daemon to send.
@objc(ShareViewController)
final class ShareViewController: NSViewController {
    private let label = NSTextField(labelWithString: "")

    override var nibName: NSNib.Name? { nil }

    override func loadView() {
        let v = NSView(frame: NSRect(x: 0, y: 0, width: 320, height: 72))
        label.stringValue = t("share.collecting")
        label.alignment = .center
        label.frame = v.bounds.insetBy(dx: 16, dy: 24)
        label.autoresizingMask = [.width, .height]
        v.addSubview(label)
        view = v
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        collect { [weak self] urls in self?.handOver(urls) }
    }

    private func collect(_ done: @escaping ([URL]) -> Void) {
        let items = extensionContext?.inputItems as? [NSExtensionItem] ?? []
        let providers = items.flatMap { $0.attachments ?? [] }
            .filter { $0.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) }
        let group = DispatchGroup()
        let lock = NSLock()
        var urls: [URL] = []
        for p in providers {
            group.enter()
            p.loadItem(forTypeIdentifier: UTType.fileURL.identifier, options: nil) { item, _ in
                let url = (item as? URL) ?? (item as? Data).flatMap { URL(dataRepresentation: $0, relativeTo: nil) }
                if let url, url.isFileURL {
                    lock.lock()
                    urls.append(url)
                    lock.unlock()
                }
                group.leave()
            }
        }
        group.notify(queue: .main) { done(urls) }
    }

    private func handOver(_ urls: [URL]) {
        guard let request = SendRequest(files: urls) else {
            label.stringValue = t("share.nothing")
            let error = NSError(domain: "io.wonmor.vutric.share", code: 1)
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) {
                self.extensionContext?.cancelRequest(withError: error)
            }
            return
        }
        label.stringValue = t("share.handing", request.files.count)
        NSWorkspace.shared.open(request.url)
        extensionContext?.completeRequest(returningItems: nil)
    }
}
