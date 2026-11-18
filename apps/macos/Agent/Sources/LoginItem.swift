// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0

import ServiceManagement

/// Start at login through `SMAppService.mainApp` (macOS 13+): the system lists Vutric in
/// System Settings → General → Login Items, where the user can also turn it off.
enum LoginItem {
    static var isOn: Bool {
        SMAppService.mainApp.status == .enabled
    }

    /// The system asked the user to approve it in System Settings; until then it does not start at login.
    static var needsApproval: Bool {
        SMAppService.mainApp.status == .requiresApproval
    }

    static func set(_ on: Bool) throws {
        if on {
            try SMAppService.mainApp.register()
        } else {
            try SMAppService.mainApp.unregister()
        }
    }
}
