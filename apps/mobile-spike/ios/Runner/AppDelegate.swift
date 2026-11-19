import Flutter
import UIKit

@main
@objc class AppDelegate: FlutterAppDelegate, FlutterImplicitEngineDelegate {
  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  func didInitializeImplicitFlutterEngine(_ engineBridge: FlutterImplicitEngineBridge) {
    GeneratedPluginRegistrant.register(with: engineBridge.pluginRegistry)
    // The Swift modules of ADR-0015 (App Group, Keychain, Live Activity, hotspot, Files folder), on `vutric/ios`.
    if let registrar = engineBridge.pluginRegistry.registrar(forPlugin: "VutricIosPlugin") {
      VutricIosPlugin.register(with: registrar)
    }
  }
}
