// The Dart side of the Kotlin modules (ADR-0009 part 2, ADR-0012), over one MethodChannel. Only UI events and OS
// results cross it (files shared from another app, the Quick Settings tile, a picked folder, hotspot credentials,
// the progress notification); transfers themselves stay in Rust. Every call is a no-op off Android.

import 'dart:async';
import 'dart:io';

import 'package:flutter/services.dart';

/// Files another app shared into Vutric, or a tap on the Quick Settings tile.
sealed class AndroidEvent {
  const AndroidEvent();
}

class SharedFiles extends AndroidEvent {
  const SharedFiles(this.paths, this.names);
  final List<String> paths;
  final List<String> names;
}

class ReceiveTile extends AndroidEvent {
  const ReceiveTile();
}

/// A notification posted on this phone that passed the Companion filter (A12).
class PhoneNotice extends AndroidEvent {
  const PhoneNotice({
    required this.app,
    required this.package,
    required this.title,
    required this.text,
    required this.at,
  });
  final String app;
  final String package;
  final String title;
  final String text;
  final DateTime at;
}

/// A LocalSend or KDE Connect device seen on this Wi-Fi (A13).
class ForeignDevice extends AndroidEvent {
  const ForeignDevice({
    required this.app,
    required this.id,
    required this.name,
    required this.type,
    required this.host,
    required this.port,
  });
  final String app;
  final String id;
  final String name;
  final String type;
  final String host;
  final int port;

  /// One entry per app and device id.
  String get key => '$app:$id';
}

/// A folder picked through the system (SAF): the tree URI and a name to show.
typedef SafFolder = ({String uri, String label});

/// This phone's local-only hotspot.
typedef HotspotCredentials = ({String ssid, String password, String security});

class AndroidBridge {
  AndroidBridge._() {
    if (Platform.isAndroid) _ch.setMethodCallHandler(_onCall);
  }
  static final AndroidBridge instance = AndroidBridge._();

  static const _ch = MethodChannel('vutric/android');
  final _events = StreamController<AndroidEvent>.broadcast();

  /// Share and tile events while the app runs (the cold-start one comes from [takeStartEvent]).
  Stream<AndroidEvent> get events => _events.stream;

  Future<dynamic> _onCall(MethodCall call) async {
    if (call.method != 'event') return;
    final e = _parse(call.arguments);
    if (e != null) _events.add(e);
  }

  AndroidEvent? _parse(Object? raw) {
    if (raw is! Map) return null;
    switch (raw['kind']) {
      case 'receive':
        return const ReceiveTile();
      case 'notice':
        return PhoneNotice(
          app: '${raw['app'] ?? ''}',
          package: '${raw['package'] ?? ''}',
          title: '${raw['title'] ?? ''}',
          text: '${raw['text'] ?? ''}',
          at: DateTime.fromMillisecondsSinceEpoch(raw['at'] is int ? raw['at'] as int : 0),
        );
      case 'foreign':
        final id = '${raw['id'] ?? ''}';
        if (id.isEmpty) return null;
        return ForeignDevice(
          app: '${raw['app'] ?? ''}',
          id: id,
          name: '${raw['name'] ?? ''}',
          type: '${raw['type'] ?? ''}',
          host: '${raw['host'] ?? ''}',
          port: raw['port'] is int ? raw['port'] as int : 0,
        );
      case 'share':
        final files = (raw['files'] as List? ?? const []).whereType<Map>().toList();
        if (files.isEmpty) return null;
        return SharedFiles([for (final f in files) '${f['path']}'], [for (final f in files) '${f['name']}']);
    }
    return null;
  }

  /// The intent that started the app (Share or the tile), once.
  Future<AndroidEvent?> takeStartEvent() async {
    if (!Platform.isAndroid) return null;
    try {
      return _parse(await _ch.invokeMethod<Object?>('takeStartIntent'));
    } on PlatformException {
      return null;
    }
  }

  // ── TransferJob: the foreground service and its notification ───────────────────────────────────────────────

  DateTime _lastUpdate = DateTime.fromMillisecondsSinceEpoch(0);

  /// Starts or updates the progress notification; at most twice a second unless [force].
  Future<void> transferUpdate(String title, String text, {double? fraction, bool force = false}) async {
    if (!Platform.isAndroid) return;
    final now = DateTime.now();
    if (!force && now.difference(_lastUpdate) < const Duration(milliseconds: 500)) return;
    _lastUpdate = now;
    try {
      await _ch.invokeMethod<void>('transferUpdate', {
        'title': title,
        'text': text,
        'permille': fraction == null ? -1 : (fraction * 1000).round(),
      });
    } on PlatformException {
      // No notification: the transfer still runs while the app is in front.
    }
  }

  Future<void> transferStop() async {
    if (!Platform.isAndroid) return;
    _lastUpdate = DateTime.fromMillisecondsSinceEpoch(0);
    try {
      await _ch.invokeMethod<void>('transferStop');
    } on PlatformException {
      // Already stopped.
    }
  }

  /// A heads-up notification for something that arrived while Vutric is not on screen (A11).
  /// Returns whether the user can see it: false when notifications are off (or off this platform), so a photo
  /// must not be reported as shown.
  Future<bool> notifyIncoming(String title, String text, {Uint8List? jpeg}) async {
    if (!Platform.isAndroid) return false;
    try {
      return await _ch.invokeMethod<bool>('notifyIncoming', {'title': title, 'text': text, 'jpeg': jpeg}) ?? false;
    } on PlatformException {
      return false;
    }
  }

  // ── SafStore ───────────────────────────────────────────────────────────────────────────────────────────────

  Future<SafFolder?> pickFolder() async {
    if (!Platform.isAndroid) return null;
    final r = await _ch.invokeMapMethod<String, String>('pickFolder');
    final uri = r?['uri'];
    if (uri == null) return null;
    return (uri: uri, label: r?['label'] ?? 'Papka');
  }

  Future<bool> folderUsable(String uri) async {
    if (!Platform.isAndroid) return false;
    try {
      return await _ch.invokeMethod<bool>('folderUsable', {'uri': uri}) ?? false;
    } on PlatformException {
      return false;
    }
  }

  Future<void> releaseFolder(String uri) async {
    if (Platform.isAndroid) await _ch.invokeMethod<void>('releaseFolder', {'uri': uri});
  }

  /// Moves finished files into the folder; returns how many moved (the rest stay in Vutric's own folder).
  Future<int> moveToFolder(String uri, List<String> paths) async {
    if (!Platform.isAndroid || paths.isEmpty) return 0;
    try {
      return await _ch.invokeMethod<int>('moveToFolder', {'uri': uri, 'paths': paths}) ?? 0;
    } on PlatformException {
      return 0; // Nothing moved; the files stay in Vutric's own folder.
    }
  }

  // ── Companion (A12) and ForeignDiscovery (A13) ─────────────────────────────────────────────────────────────

  /// Whether Vutric has notification access; with [ask], opens the system page when it has not.
  Future<bool> notificationAccess({bool ask = false}) async {
    if (!Platform.isAndroid) return false;
    try {
      return await _ch.invokeMethod<bool>('notificationAccess', {'ask': ask}) ?? false;
    } on PlatformException {
      return false;
    }
  }

  /// Turns mirroring on or off for the listener. Only the packages in [apps] are mirrored; an empty list mirrors
  /// nothing (the safer default until the owner says otherwise, owner question 2026-11-18).
  Future<void> companionMirror(bool on, List<String> apps) async {
    if (Platform.isAndroid) await _ch.invokeMethod<void>('companionMirror', {'on': on, 'apps': apps});
  }

  /// Apps with a launcher icon (not Vutric), sorted by name, for the Companion allowlist picker.
  Future<List<({String package, String label})>> installedApps() async {
    if (!Platform.isAndroid) return const [];
    try {
      final raw = await _ch.invokeListMethod<Map<Object?, Object?>>('installedApps') ?? const [];
      return [
        for (final m in raw)
          if ('${m['package'] ?? ''}'.isNotEmpty) (package: '${m['package']}', label: '${m['label'] ?? m['package']}'),
      ];
    } on PlatformException {
      return const [];
    }
  }

  /// Starts the `:compat` service (C10, ADR-0066) and returns this process's end of the socket pair for Rust's
  /// `compatStart`, or null when the service could not start (then only discovery runs).
  Future<int?> compatOpen() async {
    if (!Platform.isAndroid) return null;
    try {
      return await _ch.invokeMethod<int?>('compatOpen');
    } on PlatformException {
      return null;
    }
  }

  Future<void> compatClose() async {
    if (Platform.isAndroid) await _ch.invokeMethod<void>('compatClose');
  }

  Future<void> foreignStart() async {
    if (Platform.isAndroid) await _ch.invokeMethod<void>('foreignStart');
  }

  Future<void> foreignStop() async {
    if (Platform.isAndroid) await _ch.invokeMethod<void>('foreignStop');
  }

  // ── LinkManager: local-only hotspot ────────────────────────────────────────────────────────────────────────

  /// Opens the hotspot; throws a [PlatformException] whose code says why not (`permission`, `disallowed`, …).
  /// `security` is `wpa2`, `wpa3` or `open`, as the pairing link carries it (ADR-0058).
  Future<HotspotCredentials> hotspotStart() async {
    final r = await _ch.invokeMapMethod<String, String>('hotspotStart');
    return (ssid: r?['ssid'] ?? '', password: r?['password'] ?? '', security: r?['security'] ?? 'wpa2');
  }

  Future<void> hotspotStop() async {
    if (Platform.isAndroid) await _ch.invokeMethod<void>('hotspotStop');
  }

  /// Joins the hotspot a scanned link carries and routes this app's new sockets over it (Android 10+). Throws a
  /// [PlatformException]: `unsupported` (older Android: join in system settings), `unavailable` (refused or not
  /// found), `bad_network`, `permission`.
  Future<void> hotspotJoin({
    required String ssid,
    required String password,
    required String security,
    bool hidden = false,
  }) async {
    if (!Platform.isAndroid) throw PlatformException(code: 'unsupported');
    await _ch.invokeMethod<bool>('hotspotJoin', {
      'ssid': ssid,
      'password': password,
      'security': security,
      'hidden': hidden,
    });
  }

  Future<void> hotspotLeave() async {
    if (Platform.isAndroid) await _ch.invokeMethod<void>('hotspotLeave');
  }
}
