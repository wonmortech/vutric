// Companion (backlog A12, ADR-0009, ADR-0065): clipboard text and phone notifications shared with a paired
// device over the open session. Both are opt-in and off by default (Sozlamalar → Kompyuter bilan bogʻlash), both
// go only to a paired (trusted) device, and nothing from the other side is acted on without a tap.

import 'dart:async';
import 'dart:convert';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'android.dart';
import 'design/tokens.dart';
import 'design/widgets.dart';
import 'i18n.dart';
import 'session.dart';
import 'src/rust/api.dart' as vutric;
import 'ui.dart';

/// Longest clipboard text sent, in UTF-8 bytes (`Message::Clipboard`, 64 KiB, ADR-0065).
const maxClipboardBytes = 64 * 1024;

/// Why [text] cannot go as clipboard text (an i18n key), or null when it can.
String? clipboardProblem(String text) {
  if (text.trim().isEmpty) return 'companion.clipboard.empty';
  if (utf8.encode(text).length > maxClipboardBytes) return 'companion.clipboard.too_long';
  return null;
}

/// Sends this phone's clipboard text to the connected, paired device: it gets a "Nusxalash" prompt.
Future<void> sendClipboard(BuildContext context) async {
  final text = (await Clipboard.getData(Clipboard.kTextPlain))?.text ?? '';
  if (!context.mounted) return;
  final messenger = ScaffoldMessenger.of(context);
  final problem = clipboardProblem(text);
  if (problem != null) {
    messenger.showSnackBar(SnackBar(content: Text(tr(problem))));
    return;
  }
  try {
    await Session.instance.exclusive(() => vutric.sendClipboard(text: text));
    messenger.showSnackBar(SnackBar(content: Text(tr('companion.clipboard.sent'))));
  } catch (e) {
    if (!isCancelled(e)) messenger.showSnackBar(SnackBar(content: Text(errorText(e))));
  }
}

/// Notifications collected while mirroring is on, newest first, at most 50, and the pump that mirrors each one
/// to the connected device while that device is paired (a guest gets none).
class NoticeQueue extends ChangeNotifier {
  NoticeQueue._();
  static final NoticeQueue instance = NoticeQueue._();

  static const max = 50;

  /// Most notices waiting to go out; older ones are dropped (a burst behind a long transfer is stale anyway).
  static const maxPending = 20;
  final List<PhoneNotice> items = [];
  final List<PhoneNotice> _pending = [];
  bool _pumping = false;

  /// How many went to the other device since the app started.
  int sent = 0;

  void add(PhoneNotice n) {
    items.insert(0, n);
    if (items.length > max) items.removeRange(max, items.length);
    final peer = Session.instance.peer;
    if (peer != null && peer.trusted) {
      _pending.add(n);
      if (_pending.length > maxPending) _pending.removeRange(0, _pending.length - maxPending);
      unawaited(_pump());
    }
    notifyListeners();
  }

  Future<void> _pump() async {
    if (_pumping) return;
    _pumping = true;
    try {
      while (_pending.isNotEmpty) {
        final peer = Session.instance.peer;
        if (peer == null || !peer.trusted) {
          _pending.clear();
          break;
        }
        final n = _pending.removeAt(0);
        try {
          final ok = await Session.instance.exclusive(
            () => vutric.sendNotice(
              app: n.app,
              title: n.title,
              text: n.text,
              atMs: BigInt.from(n.at.millisecondsSinceEpoch),
            ),
          );
          if (ok) sent += 1;
        } catch (e) {
          if (!isCancelled(e)) debugPrint('vutric: notice not sent: $e');
          _pending.clear();
        }
      }
    } finally {
      _pumping = false;
      notifyListeners();
    }
  }

  void clear() {
    items.clear();
    _pending.clear();
    notifyListeners();
  }
}

/// A notice from the other device: shown, never acted on (ADR-0065). In the app it is a snack bar; in the
/// background a heads-up notification.
Future<void> showNotice(BuildContext context, vutric.CompanionNotice n, {required bool front}) async {
  final head = [n.app, n.title].where((s) => s.isNotEmpty).join(' · ');
  if (!front) {
    await AndroidBridge.instance.notifyIncoming(head, n.text);
    return;
  }
  if (!context.mounted) return;
  ScaffoldMessenger.of(context).showSnackBar(
    SnackBar(
      behavior: SnackBarBehavior.floating,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            head,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: const TextStyle(fontWeight: FontWeight.w600),
          ),
          if (n.text.isNotEmpty) Text(n.text, maxLines: 4, overflow: TextOverflow.ellipsis),
        ],
      ),
    ),
  );
}

/// Clipboard text from the other device: a prompt with a preview; only "Nusxalash" writes the clipboard.
Future<void> offerClipboard(BuildContext context, String text, {required bool front, String? from}) async {
  final peer = from ?? Session.instance.peer?.peerName ?? 'Qurilma';
  if (!front) {
    await AndroidBridge.instance.notifyIncoming(tr('companion.clipboard.came', {'peer': peer}), text);
  }
  if (!context.mounted) return;
  final copy = await showModalBottomSheet<bool>(
    context: context,
    isScrollControlled: true,
    builder: (_) => ClipboardSheet(peer: peer, text: text),
  );
  if (copy == true) {
    await Clipboard.setData(ClipboardData(text: text));
    if (context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(tr('companion.clipboard.copied'))));
    }
  }
}

class ClipboardSheet extends StatelessWidget {
  const ClipboardSheet({super.key, required this.peer, required this.text});
  final String peer;
  final String text;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final preview = text.length > 2000 ? '${text.substring(0, 2000)}…' : text;
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(tr('companion.clipboard.came', {'peer': peer}), style: v.title),
            const SizedBox(height: 10),
            ConstrainedBox(
              constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * 0.4),
              child: Container(
                padding: const EdgeInsets.all(12),
                decoration: BoxDecoration(color: v.bgAlt, borderRadius: BorderRadius.circular(12)),
                child: SingleChildScrollView(child: Text(preview, style: v.text(15, FontWeight.w400, mono: true))),
              ),
            ),
            const SizedBox(height: 14),
            Row(
              children: [
                Expanded(
                  child: VButton(
                    label: tr('companion.clipboard.close'),
                    kind: VButtonKind.secondary,
                    onPressed: () => Navigator.pop(context, false),
                  ),
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: VButton(
                    label: tr('companion.clipboard.copy'),
                    icon: Icons.copy_rounded,
                    onPressed: () => Navigator.pop(context, true),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

/// LocalSend and KDE Connect devices heard while the connect sheet is open (A13, C10), one per app and id.
class ForeignDevices extends ChangeNotifier {
  ForeignDevices._();
  static final ForeignDevices instance = ForeignDevices._();

  final Map<String, ForeignDevice> byKey = {};

  void add(ForeignDevice d) {
    final old = byKey[d.key];
    if (old != null && old.name == d.name && old.host == d.host && old.port == d.port) return;
    byKey[d.key] = d;
    notifyListeners();
  }

  void clear() {
    if (byKey.isEmpty) return;
    byKey.clear();
    notifyListeners();
  }
}
