// Other apps on this phone's Wi-Fi and beyond (C10, C7; ADR-0066, ADR-0067): LocalSend, KDE Connect and
// magic-wormhole, through the adapters in the Android `:compat` process. Rust forwards their events as JSON lines;
// this file turns them into state the screens show, and keeps the rule that everything foreign is asked about.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';

import 'android.dart';
import 'session.dart';
import 'src/rust/api.dart' as vutric;

/// A device another app announced: LocalSend (`via` `localsend`) or KDE Connect (`kdeconnect`).
@immutable
class CompatDevice {
  const CompatDevice({
    required this.via,
    required this.name,
    required this.to,
    required this.https,
    required this.type,
    this.deviceId,
  });

  final String via;
  final String name;

  /// `ip:port` as the adapter saw it.
  final String to;
  final bool https;
  final String type;

  /// KDE Connect's device id (sending needs it).
  final String? deviceId;

  String get key => via == 'kdeconnect' && deviceId != null ? 'kdeconnect:$deviceId' : '$via:$to';
  String get host => to.contains(']') ? to.substring(0, to.lastIndexOf(':')) : to.split(':').first;
}

/// A file in a foreign offer.
typedef CompatFile = ({String id, String name, int size});

/// One event from the `:compat` process, parsed from its JSON line; null for anything malformed.
sealed class CompatEvent {
  const CompatEvent();
}

class CompatSeen extends CompatEvent {
  const CompatSeen(this.device);
  final CompatDevice device;
}

/// A device asks to pair (KDE Connect): show its name and code, then answer.
class CompatPairAsk extends CompatEvent {
  const CompatPairAsk({required this.sessionId, required this.alias, required this.code, required this.deviceId});
  final String sessionId;
  final String alias;
  final String code;
  final String deviceId;
}

class CompatPaired extends CompatEvent {
  const CompatPaired(this.deviceId, {required this.on});
  final String deviceId;
  final bool on;
}

/// Files offered by a foreign app: always asked about (ADR-0010 rule 2).
class CompatOffer extends CompatEvent {
  const CompatOffer({required this.sessionId, required this.alias, required this.via, required this.files});
  final String sessionId;
  final String alias;
  final String via;
  final List<CompatFile> files;
  int get bytes => files.fold(0, (a, f) => a + f.size);
}

class CompatText extends CompatEvent {
  const CompatText(this.text);
  final String text;
}

/// One file finished arriving; the path is already checked by Rust (inside the staging folder).
class CompatReceived extends CompatEvent {
  const CompatReceived({required this.sessionId, required this.path, required this.name});
  final String sessionId;
  final String path;
  final String name;
}

/// An offer is over (finished or cancelled by the other side).
class CompatDone extends CompatEvent {
  const CompatDone(this.sessionId, {required this.cancelled});
  final String sessionId;
  final bool cancelled;
}

/// The answer to one of our requests: `reason` null when it worked, `code` for a wormhole code.
class CompatAnswer extends CompatEvent {
  const CompatAnswer(this.requestId, {this.reason, this.code});
  final int requestId;
  final String? reason;
  final String? code;
}

class CompatClosed extends CompatEvent {
  const CompatClosed();
}

String _s(Object? v, [int max = 120]) {
  final t = v is String ? v : '';
  final clean = t.replaceAll(RegExp(r'[\u0000-\u001f\u007f]'), ' ').trim();
  return clean.length > max ? clean.substring(0, max) : clean;
}

int _i(Object? v) => v is int ? v : 0;

/// Parses one line from Rust's `compatStart` stream.
CompatEvent? parseCompatEvent(String line) {
  final Object? raw;
  try {
    raw = jsonDecode(line);
  } on FormatException {
    return null;
  }
  if (raw is! Map<String, Object?>) return null;
  switch (raw['t']) {
    case 'seen':
      final to = _s(raw['to'], 64);
      if (to.isEmpty) return null;
      final via = _s(raw['via'], 16);
      return CompatSeen(
        CompatDevice(
          via: via.isEmpty ? 'localsend' : via,
          name: _s(raw['alias'], 64),
          to: to,
          https: raw['https'] == true,
          type: _s(raw['device_type'], 16),
          deviceId: raw['device_id'] is String ? _s(raw['device_id'], 64) : null,
        ),
      );
    case 'pair_request':
      return CompatPairAsk(
        sessionId: _s(raw['session_id']),
        alias: _s(raw['alias'], 64),
        code: _s(raw['code'], 16),
        deviceId: _s(raw['device_id'], 64),
      );
    case 'paired':
      return CompatPaired(_s(raw['device_id'], 64), on: true);
    case 'unpaired':
      return CompatPaired(_s(raw['device_id'], 64), on: false);
    case 'offer':
      final files = <CompatFile>[
        for (final f in (raw['files'] as List? ?? const []).whereType<Map<String, Object?>>())
          (id: _s(f['id']), name: _s(f['name'], 200), size: _i(f['size'])),
      ];
      final via = _s(raw['via'], 16);
      return CompatOffer(
        sessionId: _s(raw['session_id']),
        alias: _s(raw['alias'], 64),
        via: via.isEmpty ? 'localsend' : via,
        files: files,
      );
    case 'text':
      return CompatText(_s(raw['text'], 4000));
    case 'received':
      final path = raw['path'] is String ? raw['path'] as String : '';
      if (path.isEmpty) return null;
      return CompatReceived(sessionId: _s(raw['session_id']), path: path, name: _s(raw['name'], 200));
    case 'finished':
      return CompatDone(_s(raw['session_id']), cancelled: false);
    case 'cancelled':
      return CompatDone(_s(raw['session_id']), cancelled: true);
    case 'sent':
      return CompatAnswer(_i(raw['request_id']));
    case 'send_failed' || 'wormhole_failed':
      final reason = _s(raw['reason'], 200);
      return CompatAnswer(_i(raw['request_id']), reason: reason.isEmpty ? 'failed' : reason);
    case 'wormhole_code':
      return CompatAnswer(_i(raw['request_id']), code: _s(raw['code'], 80));
    case 'closed':
      return const CompatClosed();
  }
  return null;
}

/// magic-wormhole leaves this network (public mailbox and transit servers), so it is offered only when the route
/// choice allows the internet (Rust refuses it otherwise too).
bool wormholeAllowed(String route) => route == 'auto' || route == 'internet';

/// The adapters as the screens see them. Opened by a screen that needs them and closed when the last one leaves.
class Compat extends ChangeNotifier {
  Compat._();
  static final Compat instance = Compat._();

  bool running = false;

  /// The service could not start (old Android, or it crashed): sending is off, discovery still shows devices.
  bool unavailable = false;

  final Map<String, CompatDevice> devices = {};
  final Set<String> pairedKde = {};

  /// Offers, pair requests, texts and received files, for the screen that is open to ask the user.
  final _asks = StreamController<CompatEvent>.broadcast();
  Stream<CompatEvent> get asks => _asks.stream;

  final Map<int, Completer<String?>> _done = {};
  final Map<int, Completer<String>> _codes = {};
  int _nextId = 1;
  int _users = 0;
  StreamSubscription<String>? _sub;

  /// Starts the adapters if they are not running; true when they run.
  Future<bool> open() async {
    _users++;
    if (running) return true;
    if (!Platform.isAndroid) return false;
    final fd = await AndroidBridge.instance.compatOpen();
    if (fd == null) {
      unavailable = true;
      notifyListeners();
      return false;
    }
    try {
      final data = await getApplicationSupportDirectory();
      final cache = await getTemporaryDirectory();
      _sub = vutric
          .compatStart(fd: fd, alias: Session.instance.deviceName, dataDir: data.path, cacheDir: cache.path)
          .listen((l) {
            final e = parseCompatEvent(l);
            if (e != null) handle(e);
          }, onError: (Object _) => handle(const CompatClosed()));
      running = true;
      unavailable = false;
      notifyListeners();
      unawaited(vutric.compatAnnounce().catchError((Object _) {}));
      return true;
    } catch (_) {
      unavailable = true;
      await AndroidBridge.instance.compatClose();
      notifyListeners();
      return false;
    }
  }

  /// Stops the adapters once no screen needs them.
  Future<void> close() async {
    if (_users > 0) _users--;
    if (_users > 0) return;
    await vutric.compatStop().catchError((Object _) {});
    await AndroidBridge.instance.compatClose();
    handle(const CompatClosed());
  }

  /// Applies one event (public for tests).
  @visibleForTesting
  void handle(CompatEvent e) {
    switch (e) {
      case CompatSeen(:final device):
        devices[device.key] = device;
        notifyListeners();
      case CompatPaired(:final deviceId, :final on):
        on ? pairedKde.add(deviceId) : pairedKde.remove(deviceId);
        notifyListeners();
      case CompatAnswer(:final requestId, :final code, :final reason):
        final c = _codes.remove(requestId);
        if (code != null) {
          c?.complete(code);
          return;
        }
        if (reason != null) c?.completeError(reason);
        final d = _done.remove(requestId);
        if (d != null) {
          d.complete(reason);
        } else if (c == null && reason != null) {
          // A wormhole receive that failed: nobody waits on it, so the open screen says it.
          _asks.add(e);
        }
      case CompatClosed():
        _sub?.cancel();
        _sub = null;
        for (final c in _done.values) {
          c.complete('compat.off');
        }
        for (final c in _codes.values) {
          c.completeError('compat.off');
        }
        _done.clear();
        _codes.clear();
        devices.clear();
        if (running) {
          running = false;
          notifyListeners();
        }
      case CompatPairAsk() || CompatOffer() || CompatText() || CompatReceived() || CompatDone():
        _asks.add(e);
    }
  }

  int _request() => _nextId++;

  /// Sends [paths] to [d]; completes with null when the other app took them, else the reason.
  Future<String?> send(CompatDevice d, List<String> paths) async {
    final id = _request();
    final done = Completer<String?>();
    _done[id] = done;
    try {
      if (d.via == 'kdeconnect') {
        final dev = d.deviceId;
        if (dev == null || !pairedKde.contains(dev)) {
          _done.remove(id);
          return 'compat.kde_unpaired';
        }
        await vutric.compatKdeSend(requestId: BigInt.from(id), to: d.to, deviceId: dev, paths: paths);
      } else {
        await vutric.compatSend(requestId: BigInt.from(id), to: d.to, https: d.https, paths: paths);
      }
    } catch (e) {
      _done.remove(id);
      return '$e';
    }
    return done.future;
  }

  /// Answers an offer or pair request: everything ([accept]) or nothing.
  Future<void> decide(String sessionId, List<String> fileIds, {required bool accept}) =>
      vutric.compatDecide(sessionId: sessionId, accept: accept ? fileIds : const []);

  /// Sends by magic-wormhole: returns the code to read out at once, and [finished] with the outcome.
  Future<({String code, Future<String?> finished})> wormholeSend({List<String> paths = const [], String? text}) async {
    final id = _request();
    final code = Completer<String>();
    final done = Completer<String?>();
    _codes[id] = code;
    _done[id] = done;
    try {
      await vutric.compatWormholeSend(requestId: BigInt.from(id), paths: paths, text: text);
    } catch (e) {
      _codes.remove(id);
      _done.remove(id);
      rethrow;
    }
    return (code: await code.future, finished: done.future);
  }

  /// Receives by a magic-wormhole code: the offer (or text) comes through [asks] like any other, and a failure
  /// as a [CompatAnswer] with a reason. Returns the error when the request could not even start.
  Future<String?> wormholeReceive(String code) async {
    try {
      await vutric.compatWormholeReceive(requestId: BigInt.from(_request()), code: code);
    } catch (e) {
      return '$e';
    }
    return null;
  }
}
