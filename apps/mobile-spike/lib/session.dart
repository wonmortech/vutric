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
        // Before W3 #8 the SAF folder lived here; it moves into the settings file once (loadTransferSettings).
        final folder = j['folder'];
        if (folder is Map && folder['uri'] is String) {
          _legacyFolder = (uri: folder['uri'] as String, label: '${folder['label'] ?? ''}');
        }
        final name = (j['name'] as String?)?.trim();
        if (name != null && name.isNotEmpty) deviceName = name;
        final c = j['companion'];
        if (c is Map) {
          mirrorNotifications = c['notifications'] == true;
          clipboardButton = c['clipboard'] == true;
          mirrorApps = [for (final a in (c['apps'] as List? ?? const [])) '$a'];
        }
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
        'companion': {'notifications': mirrorNotifications, 'clipboard': clipboardButton, 'apps': mirrorApps},
      }),
      flush: true,
    );
  }

  /// Android: a folder picked through the system (SafStore) where received files go after they are checked. Kept
  /// in the settings file as `save.android_tree` (W3 #8, ADR-0057 addendum), so it lives with the other
  /// receiving rules and Rust checks it.
  ({String uri, String label})? saveFolder;
  ({String uri, String label})? _legacyFolder;

  /// The route choice (ADR-0060): `auto`, `lan_only`, `internet`, `offline` or `bluetooth`.
  String route = 'auto';

  /// The privacy lines for [route] (ADR-0061), as i18n keys.
  List<String> privacy = const ['privacy.encrypted'];

  /// Companion (A12): both off by default; the user turns them on in Sozlamalar.
  bool mirrorNotifications = false;
  bool clipboardButton = false;
  List<String> mirrorApps = const [];

  /// Reads the transfer settings this UI shows outside Sozlamalar (route, SAF folder), and moves an old
  /// ui.json folder into the settings file once.
  Future<void> loadTransferSettings() async {
    try {
      final j = jsonDecode(await vutric.getSettings()) as Map<String, dynamic>;
      final save = j['save'] as Map<String, dynamic>? ?? {};
      final tree = save['android_tree'];
      if (tree is Map && tree['uri'] is String) {
        saveFolder = (uri: tree['uri'] as String, label: '${tree['label'] ?? ''}');
      } else if (_legacyFolder != null) {
        save['android_tree'] = {'uri': _legacyFolder!.uri, 'label': _legacyFolder!.label};
        j['save'] = save;
        await vutric.putSettings(json: jsonEncode(j));
        saveFolder = _legacyFolder;
      }
      final hadLegacy = _legacyFolder != null;
      _legacyFolder = null;
      route = j['route'] as String? ?? 'auto';
      privacy = await vutric.privacyLines(choice: route);
      notifyListeners();
      if (hadLegacy) await _saveUi();
    } catch (e) {
      debugPrint('vutric: settings not read: $e');
    }
  }

  Future<void> setSaveFolder(({String uri, String label})? folder) async {
    final j = jsonDecode(await vutric.getSettings()) as Map<String, dynamic>;
    final save = j['save'] as Map<String, dynamic>? ?? {};
    save['android_tree'] = folder == null ? null : {'uri': folder.uri, 'label': folder.label};
    j['save'] = save;
    await vutric.putSettings(json: jsonEncode(j));
    saveFolder = folder;
    notifyListeners();
  }

  /// Changes the route choice; the node restarts when the new choice reaches differently (internet or not).
  /// An open session closes then, so the caller asks first.
  Future<void> setRoute(String choice) async {
    if (choice == route) return;
    final j = jsonDecode(await vutric.getSettings()) as Map<String, dynamic>;
    j['route'] = choice;
    await vutric.putSettings(json: jsonEncode(j));
    final changed = routeReachChanges(route, choice);
    route = choice;
    privacy = await vutric.privacyLines(choice: choice);
    notifyListeners();
    if (changed) await restart();
  }

  Future<void> setCompanion({bool? notifications, bool? clipboard, List<String>? apps}) async {
    if (notifications != null) mirrorNotifications = notifications;
    if (clipboard != null) clipboardButton = clipboard;
    if (apps != null) mirrorApps = apps;
    notifyListeners();
    await _saveUi();
  }

  /// The open session's route badge key (`route.badge.*`), refreshed every 2 s while connected.
  String badge = '';
  Timer? _badgeTimer;

  void _watchBadge() {
    _badgeTimer?.cancel();
    _badgeTimer = Timer.periodic(const Duration(seconds: 2), (_) => _refreshBadge());
    _refreshBadge();
  }

  Future<void> _refreshBadge() async {
    if (peer == null) {
      _badgeTimer?.cancel();
      _badgeTimer = null;
      return;
    }
    try {
      final b = await vutric.currentBadge();
      if (b != badge) {
        badge = b;
        notifyListeners();
      }
    } catch (_) {}
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
  ///
  /// A peer that must compare the six digits (found nearby, by address or by code, or one that dialed this phone
  /// that way: ADR-0050, ADR-0064) goes through [verify] first; nothing else touches the session until it says
  /// yes. A "no", or no [verify] to ask with, closes the session and ends in [PairRefused]. A guest (connected
  /// without proving anything) is kept untrusted: its offers are always asked about.
  Future<vutric.Connected> connect(
    Future<vutric.Connected> Function() job, {
    Future<bool> Function(vutric.Connected peer)? verify,
  }) async {
    final c = await afterDisconnect(job);
    if (c.needsCheck && (verify == null || !await verify(c))) {
      await disconnectNow();
      throw const PairRefused();
    }
    peer = c;
    lostPeer = null;
    badge = '';
    notifyListeners();
    _watchBadge();
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
        if (isPaused(next)) break; // paused for a send
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

  int _exclusive = 0;

  /// Runs [job] (a throw, a send, a mirrored notice) with the session to itself, then listens again. Jobs may
  /// overlap (a notice during a send): the listener comes back only when the last one ends, so it never holds
  /// the session while a job waits for it.
  Future<T> exclusive<T>(Future<T> Function() job) async {
    _exclusive++;
    _pauseWanted = true;
    try {
      if (_listening) {
        await vutric.pauseIncoming();
        await _listenDone?.future;
      }
      return await job();
    } finally {
      _exclusive--;
      if (_exclusive == 0) {
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
      myLink = await vutric.start(name: deviceName, identity: await loadIdentity());
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

/// The six digits were refused on one of the two screens (or could not be asked): nothing was trusted. Reads as
/// a cancelled call, so pages that only know [isCancelled] still go back quietly.
class PairRefused implements Exception {
  const PairRefused();

  @override
  String toString() => 'vutric: cancelled (pairing refused)';
}

/// An [vutric.Incoming] with nothing in it: [vutric.pauseIncoming] asked for the session.
bool isPaused(vutric.Incoming next) =>
    next.photo == null && next.files == null && next.notice == null && next.clipboard == null;

/// Whether switching the route from [from] to [to] changes how far the node reaches (internet or not), which
/// needs a restart (ADR-0060 `Reach::for_picker`).
bool routeReachChanges(String from, String to) {
  const internet = {'auto', 'internet'};
  return internet.contains(from) != internet.contains(to);
}
