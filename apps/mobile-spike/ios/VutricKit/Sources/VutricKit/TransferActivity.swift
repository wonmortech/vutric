// The Live Activity's attributes (backlog I2), shared by the app (which starts and updates it) and the widget
// extension (which draws it). ActivityKit exists only on iOS, so the type is absent from macOS builds.

#if os(iOS) && canImport(ActivityKit)
  import ActivityKit

  @available(iOS 16.1, *)
  public struct TransferAttributes: ActivityAttributes {
    public typealias ContentState = TransferState

    /// The other device's name when the transfer started.
    public var device: String
    public var sending: Bool

    public init(device: String, sending: Bool) {
      self.device = device
      self.sending = sending
    }
  }
#endif
