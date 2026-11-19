// The Share Extension's screen: the paired devices (44 pt rows), then one status line with a progress bar. Motion
// is a short fade, and none at all with Reduce Motion.

import SwiftUI
import UIKit
import VutricKit

@objc(ShareViewController)
final class ShareViewController: UIViewController {
  private var model: ShareModel?

  override func viewDidLoad() {
    super.viewDidLoad()
    let model = ShareModel(context: extensionContext)
    self.model = model
    let host = UIHostingController(rootView: ShareView(model: model))
    addChild(host)
    host.view.frame = view.bounds
    host.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
    view.addSubview(host.view)
    host.didMove(toParent: self)
    model.start()
  }
}

struct ShareView: View {
  @ObservedObject var model: ShareModel
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  var body: some View {
    NavigationView {
      content
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .navigationTitle(L10n.text("share.title"))
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
          ToolbarItem(placement: .cancellationAction) {
            Button(L10n.text(isFinal ? "share.close" : "share.cancel")) {
              if case .done = model.stage { model.finish() } else { model.cancel() }
            }
            .frame(minWidth: 44, minHeight: 44)
          }
        }
    }
    .navigationViewStyle(.stack)
    .animation(reduceMotion ? nil : .easeOut(duration: 0.2), value: model.stage)
  }

  private var isFinal: Bool {
    switch model.stage {
    case .done, .failed, .notSetUp: return true
    default: return false
    }
  }

  @ViewBuilder private var content: some View {
    switch model.stage {
    case .loading:
      ProgressView()
    case .notSetUp:
      Notice(icon: "exclamationmark.circle", text: L10n.text("share.not_set_up"))
    case .pick where model.devices.isEmpty:
      Notice(icon: "desktopcomputer", text: L10n.text("share.empty"))
    case .pick:
      List(model.devices, id: \.id) { d in
        Button {
          model.send(d)
        } label: {
          DeviceRow(device: d)
        }
        .frame(minHeight: 44)
      }
      .listStyle(.insetGrouped)
    case let .connecting(name):
      Status(title: L10n.text("share.connecting", ["device": name]), progress: nil)
    case let .sending(name):
      Status(title: L10n.text("share.sending", ["device": name]), progress: model.progress)
    case let .done(count):
      Notice(icon: "checkmark.circle.fill", text: L10n.text("share.done", ["count": "\(count)"]))
    case let .failed(message):
      Notice(icon: "xmark.octagon", text: message)
    }
  }
}

private struct DeviceRow: View {
  let device: ShareDevice

  private var symbol: String {
    ["android", "ios"].contains(device.platform) ? "iphone" : "desktopcomputer"
  }

  var body: some View {
    HStack(spacing: 12) {
      Image(systemName: symbol)
        .font(.title3)
        .frame(width: 32)
        .accessibilityHidden(true)
      VStack(alignment: .leading, spacing: 2) {
        Text(device.name).font(.body).foregroundColor(.primary)
        Text(device.code).font(.caption).foregroundColor(.secondary)
      }
      Spacer(minLength: 0)
    }
    .contentShape(Rectangle())
    .accessibilityElement(children: .combine)
  }
}

private struct Status: View {
  let title: String
  let progress: ShareProgress?

  var body: some View {
    VStack(spacing: 16) {
      Text(title).font(.headline).multilineTextAlignment(.center)
      if let p = progress, p.bytesTotal > 0 {
        let f = min(1, Double(p.bytesDone) / Double(p.bytesTotal))
        ProgressView(value: f)
        Text(
          L10n.text(
            "share.progress",
            ["done": "\(p.filesDone)", "total": "\(p.filesTotal)", "percent": "\(Int(f * 100))%"])
        )
        .font(.footnote.monospacedDigit())
        .foregroundColor(.secondary)
      } else {
        ProgressView()
      }
    }
    .padding(24)
  }
}

private struct Notice: View {
  let icon: String
  let text: String

  var body: some View {
    VStack(spacing: 12) {
      Image(systemName: icon).font(.largeTitle).foregroundColor(.accentColor).accessibilityHidden(true)
      Text(text).font(.body).multilineTextAlignment(.center)
    }
    .padding(24)
  }
}
