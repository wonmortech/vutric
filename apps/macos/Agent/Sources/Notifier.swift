// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

import AppKit
import UserNotifications

/// "Tushdi" notifications with the same two buttons as the Windows toast: Ochish and Papkada ko'rsatish.
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    private static let category = "vutric.received"
    private static let openAction = "vutric.open"
    private static let revealAction = "vutric.reveal"

    private let center = UNUserNotificationCenter.current()

    func setUp() {
        center.delegate = self
        let open = UNNotificationAction(identifier: Self.openAction, title: t("notify.action.open"))
        let reveal = UNNotificationAction(identifier: Self.revealAction, title: t("notify.action.reveal"))
        center.setNotificationCategories([
            UNNotificationCategory(identifier: Self.category, actions: [open, reveal], intentIdentifiers: []),
        ])
        center.requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    /// Files landed in `folder` (the daemon's `received` event).
    func received(files: Int, folder: String, from: String?) {
        let c = UNMutableNotificationContent()
        c.title = t("notify.received.title")
        c.body = from.map { t("notify.received.body.from", files, $0) } ?? t("notify.received.body", files)
        c.categoryIdentifier = Self.category
        c.userInfo = ["folder": folder]
        c.sound = .default
        center.add(UNNotificationRequest(identifier: UUID().uuidString, content: c, trigger: nil))
    }

    func failed(reason: String) {
        let c = UNMutableNotificationContent()
        c.title = t("notify.failed.title")
        c.body = reason
        center.add(UNNotificationRequest(identifier: UUID().uuidString, content: c, trigger: nil))
    }

    func userNotificationCenter(
        _: UNUserNotificationCenter,
        willPresent _: UNNotification,
        withCompletionHandler done: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        done([.banner, .sound])
    }

    func userNotificationCenter(
        _: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler done: @escaping () -> Void
    ) {
        defer { done() }
        guard let folder = response.notification.request.content.userInfo["folder"] as? String else { return }
        let url = URL(fileURLWithPath: folder)
        switch response.actionIdentifier {
        case Self.revealAction:
            NSWorkspace.shared.activateFileViewerSelecting([url])
        case Self.openAction, UNNotificationDefaultActionIdentifier:
            NSWorkspace.shared.open(url)
        default:
            break
        }
    }
}
