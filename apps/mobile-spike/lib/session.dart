// One place for the app's live state: this device on the network, the open session, the chosen theme and the
// transfer history. Screens listen to it instead of passing peers around, so Home, Send and Receive always agree
// on who is connected.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';
import 'package:path_provider/path_provider.dart';

import 'src/rust/api.dart' as vutric;
import 'ui.dart';

const _multicast = MethodChannel('vutric/multicast');

/// The device identity lives in app-private storage and is reused, so the pairing link stays stable.
Future<Uint8List> loadIdentity() async {
  final dir = await getApplicationSupportDirectory();
  final file = File('${dir.path}/identity.key');
  if (await file.exists()) {
    final bytes = await file.readAsBytes();
    if (bytes.length == 32) return bytes;
  }
  final fresh = await vutric.newIdentity();
  await file.writeAsBytes(fresh, flush: true);
  return fresh;
}

/// One line of history: what went where, when and how fast.
class HistoryItem {
  HistoryItem({
    required this.sent,
    required this.peer,
    required this.files,
    required this.bytes,
    required this.seconds,
    required this.at,
    this.names = const [],
    this.paths = const [],
    this.ms,
  });

  factory HistoryItem.fromJson(Map<String, dynamic> j) => HistoryItem(
    sent: j['sent'] == true,
    peer: j['peer'] as String? ?? '',
    files: j['files'] as int? ?? 0,
    bytes: j['bytes'] as int? ?? 0,
    seconds: (j['seconds'] as num?)?.toDouble() ?? 0,
    at: DateTime.tryParse(j['at'] as String? ?? '') ?? DateTime.now(),
    names: [for (final n in (j['names'] as List? ?? const [])) '$n'],
    paths: [for (final n in (j['paths'] as List? ?? const [])) '$n'],
    ms: (j['ms'] as num?)?.toDouble(),
  );

  final bool sent;
  final String peer;
  final int files;
  final int bytes;
  final double seconds;
  final DateTime at;
  final List<String> names;

  /// Where the files are on this phone (received ones, or sent files that still exist): for resend and open.
  final List<String> paths;

  /// Throw latency (release → first frame on the receiver), for thrown photos.
  final double? ms;

  Map<String, dynamic> toJson() => {
    'sent': sent,
    'peer': peer,
    'files': files,
    'bytes': bytes,
    'seconds': seconds,
    'at': at.toIso8601String(),
    'names': names,
    'paths': paths,
    if (ms != null) 'ms': ms,
  };
}

/// `auto` follows the phone's light/dark setting with the iOS Native pair; any other id pins one theme.
const autoTheme = 'auto';

class Session extends ChangeNotifier {
  Session._();
  static final Session instance = Session._();

  String? myLink;
  String? bootError;
  bool online = false;
  String deviceName = Platform.isIOS ? 'iPhone' : 'Android';

  vutric.Connected? peer;

  /// The last peer's name after a session dropped, until the next connect: screens show "aloqa uzildi".
  String? lostPeer;

  String themeId = autoTheme;
  final List<HistoryItem> history = [];

  Directory? _dir;

  Future<File> _file(String name) async {
    _dir ??= await getApplicationSupportDirectory();
    return File('${_dir!.path}/$name');
  }

  /// UI-only preferences (theme, device name) and the history. Transfer settings stay in Rust (ADR-0008 decision
  /// 12); these never leave the phone UI, so ADR-0011 keeps them here.
  Future<void> loadLocal() async {
    try {
      final f = await _file('ui.json');
      if (await f.exists()) {
        final j = jsonDecode(await f.readAsString()) as Map<String, dynamic>;
        themeId = j['theme'] as String? ?? autoTheme;
        onboarded = j['onboarded'] == true;
        final folder = j['folder'];
        if (folder is Map && folder['uri'] is String) {
          saveFolder = (uri: folder['uri'] as String, label: '${folder['label'] ?? ''}');
        }
        final name = (j['name'] as String?)?.trim();
        if (name != null && name.isNotEmpty) deviceName = name;
      }
    } catch (_) {
      // A broken file is replaced on the next save.
    }
    try {
      final f = await _file('history.json');
      if (await f.exists()) {
        final list = jsonDecode(await f.readAsString()) as List;
        history
          ..clear()
          ..addAll(list.map((e) => HistoryItem.fromJson(e as Map<String, dynamic>)));
      }
    } catch (_) {}
  }

  Future<void> _saveUi() async {
    final f = await _file('ui.json');
    await f.writeAsString(
      jsonEncode({
        'theme': themeId,
        'name': deviceName,
        'onboarded': onboarded,
        if (saveFolder != null) 'folder': {'uri': saveFolder!.uri, 'label': saveFolder!.label},
      }),
      flush: true,
    );
  }

  /// Android: a folder picked through the system (SafStore) where received files go after they are checked.
  /// TEMP until W3 #8 adds a SAF tree field to vutric-core's settings (its serde drops unknown keys); ADR-0012.
  ({String uri, String label})? saveFolder;

  Future<void> setSaveFolder(({String uri, String label})? folder) async {
    saveFolder = folder;
    notifyListeners();
    await _saveUi();
  }

  /// Fetches this device's pairing link again (a new network, such as the phone's own hotspot, adds addresses).
  Future<void> refreshLink() async {
    try {
      myLink = await vutric.currentLink();
      notifyListeners();
    } catch (_) {
      // Not started yet: restart() fills it.
    }
  }

  /// Runs once when the session ends (the hotspot it rode on closes, for example).
  VoidCallback? onDisconnect;

  void _ended() {
    final f = onDisconnect;
    onDisconnect = null;
    f?.call();
  }

  /// The first-start screens were shown (ADR-0012); they never come back on their own.
  bool onboarded = false;

  Future<void> finishOnboarding() async {
    onboarded = true;
    notifyListeners();
    await _saveUi();
  }

  Future<void> setTheme(String id) async {
    themeId = id;
    notifyListeners();
    await _saveUi();
  }

  /// Saves a new device name; the caller restarts the node so the pairing link carries it.
  Future<bool> setDeviceName(String name) async {
    final n = name.trim();
    if (n.isEmpty || n == deviceName) return false;
    deviceName = n;
    notifyListeners();
    await _saveUi();
    return true;
  }

  /// Keeps the last 200 entries (retention in days is the W3 engine's job, backlog T6).
  Future<void> addHistory(HistoryItem item) async {
    history.insert(0, item);
    if (history.length > 200) history.removeRange(200, history.length);
    notifyListeners();
    final f = await _file('history.json');
    await f.writeAsString(jsonEncode([for (final h in history) h.toJson()]), flush: true);
  }

  Future<void> clearHistory() async {
    history.clear();
    notifyListeners();
    final f = await _file('history.json');
    if (await f.exists()) await f.delete();
  }

  void setBoot({String? link, String? error, bool? isOnline}) {
    if (link != null) myLink = link;
    if (error != null) bootError = error;
    if (isOnline != null) online = isOnline;
    notifyListeners();
  }

  /// Runs a connect job (scan link, find on this network, wait for a scan) and keeps the peer it returns. From
  /// then on the session listens for photos and files from the other side, whichever tab is open.
  Future<vutric.Connected> connect(Future<vutric.Connected> Function() job) async {
    final c = await afterDisconnect(job);
    peer = c;
    lostPeer = null;
    notifyListeners();
    unawaited(_listen());
    return c;
  }

  /// The session dropped: remember the name for the "aloqa uzildi" state.
  void lost() {
    if (peer == null) return;
    lostPeer = peer!.peerName;
    peer = null;
    _ended();
    notifyListeners();
  }

  // ── Incoming ───────────────────────────────────────────────────────────────────────────────────────────────────
  //
  // One loop waits for whatever the other side sends (a thrown photo, a file offer) and hands it to [onIncoming],
  // which the home shell sets. Waiting holds the session, so sending goes through [exclusive]: it pauses the wait
  // (vutric.pauseIncoming), sends, and listens again. Nothing the other side sends meanwhile is lost.

  /// Handles one photo or offer; the loop waits for it before listening again.
  Future<void> Function(vutric.Incoming next)? onIncoming;

  bool _listening = false;
  bool _pauseWanted = false;
  Completer<void>? _listenDone;

  Future<void> _listen() async {
    if (_listening || _pauseWanted) return;
    _listening = true;
    final done = _listenDone = Completer<void>();
    try {
      while (peer != null && !_pauseWanted) {
        final next = await vutric.nextIncoming(); // only an error here means the session is gone
        if (next.photo == null && next.files == null) break; // paused for a send
        try {
          await onIncoming?.call(next);
        } catch (e) {
          // A failing handler (gallery, folder, notification) loses that one item, not the session.
          if (isCancelled(e)) rethrow;
          debugPrint('vutric: incoming handler failed: $e');
        }
      }
    } catch (e) {
      if (!isCancelled(e)) lost();
    } finally {
      _listening = false;
      done.complete();
    }
  }

  /// Runs [job] (a throw or a send) with the session to itself, then listens again.
  Future<T> exclusive<T>(Future<T> Function() job) async {
    _pauseWanted = true;
    try {
      if (_listening) {
        await vutric.pauseIncoming();
        await _listenDone?.future;
      }
      return await job();
    } finally {
      // Whatever failed above, the listener comes back; a resume error must not hide the job's own error.
      _pauseWanted = false;
      try {
        await vutric.resumeIncoming();
      } catch (e) {
        debugPrint('vutric: resumeIncoming failed: $e');
      }
      if (peer != null) unawaited(_listen());
    }
  }

  /// Asks Rust whether the session is still open; marks it lost when not.
  Future<bool> stillConnected() async {
    final open = peer != null && (await vutric.currentRoute()).isNotEmpty;
    if (!open) lost();
    return open;
  }

  bool _starting = false;
  bool _restartAgain = false;

  /// Puts this device on the network under [deviceName] (again after a rename: the pairing link carries the
  /// name). The relay takes ~3 s; until then the link works on this network only, so the fuller one replaces it.
  /// A call during a start is not dropped: one more start follows, with the name as it is by then.
  Future<void> restart() async {
    if (_starting) {
      _restartAgain = true;
      return;
    }
    _starting = true;
    bootError = null;
    myLink = null;
    online = false;
    notifyListeners();
    try {
      await disconnect();
      if (Platform.isAndroid) await _multicast.invokeMethod<void>('acquire');
      myLink = await vutric.start(name: deviceName, identity: await loadIdentity(), lanOnly: false);
      notifyListeners();
      online = await vutric.waitOnline(maxMs: 10000);
      myLink = await vutric.currentLink();
    } catch (e) {
      bootError = errorText(e);
    } finally {
      _starting = false;
      notifyListeners();
    }
    if (_restartAgain) {
      _restartAgain = false;
      await restart();
    }
  }

  /// Android drops mDNS replies without a multicast lock; hold it (battery) only while the app is in front.
  void foreground(bool front) {
    if (Platform.isAndroid) _multicast.invokeMethod<void>(front ? 'acquire' : 'release');
  }

  /// The user closed the session on purpose: no "aloqa uzildi" afterwards.
  Future<void> disconnect() async {
    await disconnectNow();
    peer = null;
    lostPeer = null;
    _ended();
    notifyListeners();
  }
}
