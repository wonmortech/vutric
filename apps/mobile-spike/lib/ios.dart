// The Dart side of the Swift modules (ADR-0009 part 3, ADR-0015), over one MethodChannel like Android's
// (`android.dart`). Only UI events and OS results cross it: the App Group folder and the Keychain identity the
// extensions share, the snapshot they read, Live Activity progress, the hotspot join, HEIC → JPEG and the Files
// folder. Transfers stay in Rust. Every call is a no-op off iOS.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/services.dart';

/// A widget tap, an App Intent or a Share-sheet suggestion asking to send (to [device] when set).
class IosSendRequest {
  const IosSendRequest(this.device);
  final String? device;
}

/// A Files folder the user picked: the bookmark (base64) and its name.
typedef IosFolder = ({String bookmark, String label});

/// One paired device for the extensions' snapshot.
typedef IosDevice = ({String id, String name, String platform, String code, int lastSeen});

/// The Live Activity's phases (`TransferState.Phase` in Swift).
enum IosTransferPhase { waiting, running, done, failed }

/// Parses an event from Swift; null for anything malformed (the channel is ours, but never trust a shape).
IosSendRequest? parseIosEvent(Object? raw) {
  if (raw is! Map || raw['kind'] != 'send') return null;
  final d = raw['device'];
  final ok = d is String && d.length == 64 && RegExp(r'^[0-9a-fA-F]+$').hasMatch(d);
  return IosSendRequest(ok ? d : null);
}

/// The arguments of `syncShared`, also what the Swift side's `SharedSnapshot` decodes.
Map<String, Object?> iosSnapshotArgs({
  required String deviceName,
  required List<IosDevice> devices,
  required bool heicToJpeg,
}) => {
  'deviceName': deviceName,
  'heicToJpeg': heicToJpeg,
  'devices': [
    for (final d in devices)
      {'id': d.id, 'name': d.name, 'platform': d.platform, 'code': d.code, 'lastSeen': d.lastSeen},
  ],
};

class IosBridge {
  IosBridge._() {
    if (Platform.isIOS) _ch.setMethodCallHandler(_onCall);
  }
  static final IosBridge instance = IosBridge._();

  static const _ch = MethodChannel('vutric/ios');
  final _events = StreamController<IosSendRequest>.broadcast();

  Stream<IosSendRequest> get events => _events.stream;

  Future<dynamic> _onCall(MethodCall call) async {
    if (call.method != 'event') return;
    final e = parseIosEvent(call.arguments);
    if (e != null) _events.add(e);
  }

  /// What a widget tap or an intent asked for before Dart listened (a cold start).
  Future<List<IosSendRequest>> takeStartEvents() async {
    if (!Platform.isIOS) return const [];
    try {
      final raw = await _ch.invokeListMethod<Object?>('takeStartEvents') ?? const [];
      return [for (final r in raw) ?parseIosEvent(r)];
    } on PlatformException {
      return const [];
    }
  }

  // ── App Group and Keychain (I8) ────────────────────────────────────────────────────────────────────────────

  /// Rust's settings folder inside the App Group, or null when the signature has no App Group (the app then keeps
  /// its own folder and the extensions say "open Vutric once").
  Future<String?> groupDir() async {
    if (!Platform.isIOS) return null;
    try {
      return await _ch.invokeMethod<String>('groupDir');
    } on PlatformException {
      return null;
    }
  }

  Future<Uint8List?> identity() async {
    if (!Platform.isIOS) return null;
    try {
      final k = await _ch.invokeMethod<Uint8List>('identity');
      return k != null && k.length == 32 ? k : null;
    } on PlatformException {
      return null;
    }
  }

  /// Stores the identity in the shared Keychain item; false when the Keychain refused (the file copy then stays
  /// the only one, and the extensions cannot send).
  Future<bool> storeIdentity(Uint8List key) async {
    if (!Platform.isIOS) return false;
    try {
      return await _ch.invokeMethod<bool>('storeIdentity', {'key': key}) ?? false;
    } on PlatformException {
      return false;
    }
  }

  /// Writes the snapshot the Share Extension, the widget and App Intents read, and refreshes the Share sheet's
  /// device suggestions.
  Future<void> syncShared({
    required String deviceName,
    required List<IosDevice> devices,
    required bool heicToJpeg,
  }) async {
    if (!Platform.isIOS) return;
    try {
      await _ch.invokeMethod<void>(
        'syncShared',
        iosSnapshotArgs(deviceName: deviceName, devices: devices, heicToJpeg: heicToJpeg),
      );
    } on PlatformException {
      // The extensions keep the previous snapshot.
    }
  }

  // ── Live Activity (I2) ─────────────────────────────────────────────────────────────────────────────────────

  /// Starts or updates the transfer's Live Activity; Swift throttles to twice a second.
  Future<void> transferUpdate({
    required String device,
    required bool sending,
    required IosTransferPhase phase,
    int bytesDone = 0,
    int bytesTotal = 0,
    int filesDone = 0,
    int filesTotal = 0,
  }) async {
    if (!Platform.isIOS) return;
    try {
      await _ch.invokeMethod<void>('transferUpdate', {
        'device': device,
        'sending': sending,
        'phase': phase.name,
        'bytesDone': bytesDone,
        'bytesTotal': bytesTotal,
        'filesDone': filesDone,
        'filesTotal': filesTotal,
      });
    } on PlatformException {
      // No Live Activity: the sheet in the app still shows the progress.
    }
  }

  /// Ends the Live Activity, showing [phase] for a few seconds.
  Future<void> transferStop({required String device, required bool sending, required IosTransferPhase phase}) async {
    if (!Platform.isIOS) return;
    try {
      await _ch.invokeMethod<void>('transferStop', {'device': device, 'sending': sending, 'phase': phase.name});
    } on PlatformException {
      // Already ended.
    }
  }

  // ── Hotspot join (I5) ──────────────────────────────────────────────────────────────────────────────────────

  /// Joins the hotspot a scanned link carries. Throws a [PlatformException] with Android's codes: `unavailable`,
  /// `bad_network`, `permission`, `unsupported`.
  Future<void> hotspotJoin({
    required String ssid,
    required String password,
    required String security,
    bool hidden = false,
  }) async {
    if (!Platform.isIOS) throw PlatformException(code: 'unsupported');
    await _ch.invokeMethod<bool>('hotspotJoin', {
      'ssid': ssid,
      'password': password,
      'security': security,
      'hidden': hidden,
    });
  }

  Future<void> hotspotLeave(String ssid) async {
    if (Platform.isIOS) await _ch.invokeMethod<void>('hotspotLeave', {'ssid': ssid});
  }

  // ── Photos (I6) ────────────────────────────────────────────────────────────────────────────────────────────

  /// A JPEG copy of a HEIC photo in the temporary folder, or null when [path] is not HEIC or could not be read.
  Future<({String path, String name})?> jpegFromHeic(String path) async {
    if (!Platform.isIOS) return null;
    try {
      final r = await _ch.invokeMapMethod<String, String>('jpegFromHeic', {'path': path});
      final p = r?['path'];
      return p == null ? null : (path: p, name: r?['name'] ?? 'photo.jpg');
    } on PlatformException {
      return null;
    }
  }

  // ── Files folder (I7) ──────────────────────────────────────────────────────────────────────────────────────

  Future<IosFolder?> pickFolder() async {
    if (!Platform.isIOS) return null;
    final r = await _ch.invokeMapMethod<String, String>('pickFolder');
    final b = r?['bookmark'];
    if (b == null || b.isEmpty) return null;
    return (bookmark: b, label: r?['label'] ?? 'Papka');
  }

  Future<bool> folderUsable(String bookmark) async {
    if (!Platform.isIOS) return false;
    try {
      return await _ch.invokeMethod<bool>('folderUsable', {'bookmark': bookmark}) ?? false;
    } on PlatformException {
      return false;
    }
  }

  /// Moves finished files into the folder; returns how many moved (the rest stay in Vutric's own folder).
  Future<int> moveToFolder(String bookmark, List<String> paths) async {
    if (!Platform.isIOS || paths.isEmpty) return 0;
    try {
      return await _ch.invokeMethod<int>('moveToFolder', {'bookmark': bookmark, 'paths': paths}) ?? 0;
    } on PlatformException {
      return 0;
    }
  }
}

/// Moves Rust's settings and trust files from the app's own folder into the App Group once, so a phone that ran an
/// older build keeps its paired devices. Never overwrites a file already in the group.
Future<void> moveIntoGroup(Directory from, Directory to) async {
  for (final name in const ['settings.json', 'trusted.json']) {
    final src = File('${from.path}/$name');
    final dst = File('${to.path}/$name');
    if (await src.exists() && !await dst.exists()) {
      await src.copy(dst.path);
      await src.delete();
    }
  }
}

/// `IMG_0001.HEIC` → `IMG_0001.jpg`, as Swift's `HeicConverter.jpegName` names the copy.
String heicAsJpegName(String name) {
  final dot = name.lastIndexOf('.');
  final base = dot <= 0 ? (dot == 0 ? '' : name) : name.substring(0, dot);
  return '${base.isEmpty ? 'photo' : base}.jpg';
}

/// The base64 check the folder row uses before handing a bookmark to Swift.
bool looksLikeBookmark(String s) {
  if (s.isEmpty || s.length % 4 != 0) return false;
  try {
    return base64.decode(s).isNotEmpty;
  } on FormatException {
    return false;
  }
}
