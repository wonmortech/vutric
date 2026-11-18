// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

import AppKit

/// "Vutric bilan yuborish" in the macOS Share menu: files (Finder, Photos exports), and text or links (Safari, Notes,
/// any app that shares them).
///
/// The extension is sandboxed and has no network of its own: it collects what was shared and hands it to the
/// menu-bar agent as `vutric://send?path=…` or `vutric://send?text=…` (ADR-0112, ADR-0115). The agent confirms and
/// asks the daemon to send.
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
        let items = extensionContext?.inputItems as? [NSExtensionItem] ?? []
        SharedItems.collect(items) { [weak self] found in self?.handOver(found) }
    }

    private func handOver(_ found: SharedItems) {
        switch found.request() {
        case let .success(request):
            label.stringValue = request.text == nil
                ? t("share.handing", request.files.count) : t("share.handing.text")
            NSWorkspace.shared.open(request.url)
            extensionContext?.completeRequest(returningItems: nil)
        case let .failure(why):
            label.stringValue = t(why == .tooLong ? "share.toolong" : "share.nothing")
            let error = NSError(domain: "io.wonmor.vutric.share", code: why == .tooLong ? 2 : 1)
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.6) {
                self.extensionContext?.cancelRequest(withError: error)
            }
        }
    }
}
