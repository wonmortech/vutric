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

/// A folder picked through the system (SAF): the tree URI and a name to show.
typedef SafFolder = ({String uri, String label});

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
      case 'share':
        final files = (raw['files'] as List? ?? const []).whereType<Map>().toList();
        if (files.isEmpty) return null;
        return SharedFiles(
          [for (final f in files) '${f['path']}'],
          [for (final f in files) '${f['name']}'],
        );
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
  Future<void> notifyIncoming(String title, String text, {Uint8List? jpeg}) async {
    if (!Platform.isAndroid) return;
    try {
      await _ch.invokeMethod<void>('notifyIncoming', {'title': title, 'text': text, 'jpeg': jpeg});
    } on PlatformException {
      // No notification permission: the photo is still waiting in the Qabul tab.
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

  Future<bool> folderUsable(String uri) async =>
      Platform.isAndroid && (await _ch.invokeMethod<bool>('folderUsable', {'uri': uri}) ?? false);

  Future<void> releaseFolder(String uri) async {
    if (Platform.isAndroid) await _ch.invokeMethod<void>('releaseFolder', {'uri': uri});
  }

  /// Moves finished files into the folder; returns how many moved (the rest stay in Vutric's own folder).
  Future<int> moveToFolder(String uri, List<String> paths) async {
    if (!Platform.isAndroid || paths.isEmpty) return 0;
    return await _ch.invokeMethod<int>('moveToFolder', {'uri': uri, 'paths': paths}) ?? 0;
  }

  // ── LinkManager: local-only hotspot ────────────────────────────────────────────────────────────────────────

  /// Opens the hotspot; throws a [PlatformException] whose code says why not (`permission`, `disallowed`, …).
  Future<({String ssid, String password})> hotspotStart() async {
    final r = await _ch.invokeMapMethod<String, String>('hotspotStart');
    return (ssid: r?['ssid'] ?? '', password: r?['password'] ?? '');
  }

  Future<void> hotspotStop() async {
    if (Platform.isAndroid) await _ch.invokeMethod<void>('hotspotStop');
  }
}
