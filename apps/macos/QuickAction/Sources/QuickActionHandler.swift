// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

import AppKit

/// The Finder Quick Action "Vutric bilan yuborish": right-click on files → Quick Actions, the preview pane's button,
/// and the Touch Bar (ADR-0115). No UI of its own: it hands the selected files to the menu-bar agent as
/// `vutric://send?path=…`, and the agent asks before sending, exactly as for the Share extension.
@objc(QuickActionHandler)
final class QuickActionHandler: NSObject, NSExtensionRequestHandling {
    func beginRequest(with context: NSExtensionContext) {
        let items = context.inputItems as? [NSExtensionItem] ?? []
        SharedItems.collect(items) { found in
            // Finder hands over files only (the activation rule); text is ignored here.
            guard let request = SendRequest(files: found.files) else {
                context.cancelRequest(withError: NSError(domain: "io.wonmor.vutric.quickaction", code: 1))
                return
            }
            NSWorkspace.shared.open(request.url)
            // An editor-role action would return modified items; a viewer returns none, so Finder changes nothing.
            context.completeRequest(returningItems: nil)
        }
    }
}
