// The widget extension (backlog I2, F8, F9): the "PC'ga yuborish" widget and the transfer Live Activity. It reads
// only the App Group snapshot, never Rust: a widget has a few MB of memory and must draw in milliseconds.

import ActivityKit
import SwiftUI
import VutricKit
import WidgetKit

@main
struct VutricWidgets: WidgetBundle {
  var body: some Widget {
    SendWidget()
    TransferLiveActivity()
  }
}

// MARK: "PC'ga yuborish"

struct SendEntry: TimelineEntry {
  let date: Date
  let device: SharedDevice?
}

struct SendProvider: TimelineProvider {
  func placeholder(in context: Context) -> SendEntry {
    SendEntry(date: Date(), device: nil)
  }

  func getSnapshot(in context: Context, completion: @escaping (SendEntry) -> Void) {
    completion(entry())
  }

  func getTimeline(in context: Context, completion: @escaping (Timeline<SendEntry>) -> Void) {
    // The app reloads the timeline whenever the paired list changes; the hourly refresh is only a fallback.
    completion(Timeline(entries: [entry()], policy: .after(Date().addingTimeInterval(3600))))
  }

  private func entry() -> SendEntry {
    SendEntry(date: Date(), device: SharedStore.app()?.snapshot()?.lastComputer)
  }
}

struct SendWidget: Widget {
  var body: some WidgetConfiguration {
    StaticConfiguration(kind: "io.wonmor.vutric.send", provider: SendProvider()) { entry in
      SendWidgetView(entry: entry)
    }
    .configurationDisplayName(L10n.text("widget.send.title"))
    .description(L10n.text("widget.send.description"))
    .supportedFamilies([.systemSmall, .accessoryCircular, .accessoryRectangular])
  }
}

struct SendWidgetView: View {
  let entry: SendEntry
  @Environment(\.widgetFamily) private var family

  private var subtitle: String { entry.device?.name ?? L10n.text("widget.send.none") }

  var body: some View {
    content
      .widgetURL(VutricURL.send(device: entry.device?.id))
      .accessibilityElement(children: .combine)
      .accessibilityLabel(L10n.text("widget.send.title") + ", " + subtitle)
      .widgetBackground()
  }

  @ViewBuilder private var content: some View {
    switch family {
    case .accessoryCircular:
      Image(systemName: "arrow.up.circle.fill").font(.title)
    case .accessoryRectangular:
      VStack(alignment: .leading) {
        Text(L10n.text("widget.send.title")).font(.headline)
        Text(subtitle).font(.caption).lineLimit(1)
      }
    default:
      VStack(alignment: .leading, spacing: 6) {
        Image(systemName: "arrow.up.circle.fill").font(.largeTitle).foregroundColor(.accentColor)
        Spacer(minLength: 0)
        Text(L10n.text("widget.send.title")).font(.headline).lineLimit(2)
        Text(subtitle).font(.caption).foregroundColor(.secondary).lineLimit(1)
      }
      .frame(maxWidth: .infinity, alignment: .leading)
    }
  }
}

// MARK: Live Activity

struct TransferLiveActivity: Widget {
  var body: some WidgetConfiguration {
    ActivityConfiguration(for: TransferAttributes.self) { context in
      TransferLockView(state: context.state)
        .padding(16)
    } dynamicIsland: { context in
      DynamicIsland {
        DynamicIslandExpandedRegion(.leading) {
          DirectionIcon(state: context.state).font(.title2)
        }
        DynamicIslandExpandedRegion(.trailing) {
          Text(context.state.percentText).font(.title3.monospacedDigit())
        }
        DynamicIslandExpandedRegion(.bottom) {
          TransferLockView(state: context.state)
        }
      } compactLeading: {
        DirectionIcon(state: context.state)
      } compactTrailing: {
        Text(context.state.percentText).font(.caption.monospacedDigit())
      } minimal: {
        DirectionIcon(state: context.state)
      }
    }
  }
}

struct DirectionIcon: View {
  let state: TransferState

  private var name: String {
    switch state.phase {
    case .done: return "checkmark.circle.fill"
    case .failed: return "xmark.circle.fill"
    case .waiting, .running: return state.sending ? "arrow.up.circle.fill" : "arrow.down.circle.fill"
    }
  }

  var body: some View {
    Image(systemName: name).accessibilityHidden(true)
  }
}

struct TransferLockView: View {
  let state: TransferState
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      Text(state.title).font(.headline).lineLimit(1)
      if let f = state.fraction {
        ProgressView(value: f)
      } else {
        ProgressView().progressViewStyle(.linear)
      }
      Text(state.detail).font(.caption.monospacedDigit()).foregroundColor(.secondary).lineLimit(1)
    }
    .contentTransition(reduceMotion ? .identity : .opacity)
    .accessibilityElement(children: .combine)
  }
}

extension View {
  /// iOS 17 draws widgets on its own background and shows a placeholder without this.
  @ViewBuilder func widgetBackground() -> some View {
    if #available(iOS 17.0, *) {
      containerBackground(.fill.tertiary, for: .widget)
    } else {
      padding(12)
    }
  }
}
