// Companion (backlog A12, ADR-0009): the clipboard sent to the connected computer, and phone notifications kept
// for mirroring. Both are opt-in and off by default (Sozlamalar → Kompyuter bilan bogʻlash).

import 'dart:io';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';
import 'package:path_provider/path_provider.dart';

import 'android.dart';
import 'i18n.dart';
import 'transfer_ui.dart';

/// The file name the clipboard travels under: "Bufer 2026-11-18 14.05.33.txt" (no characters any OS refuses).
String clipboardFileName(DateTime t) {
  String two(int n) => n.toString().padLeft(2, '0');
  return 'Bufer ${t.year}-${two(t.month)}-${two(t.day)} ${two(t.hour)}.${two(t.minute)}.${two(t.second)}.txt';
}

/// Longest clipboard text sent (64 KiB, the cap proposed for `Message::Clipboard` in W3 #12).
const maxClipboardChars = 64 * 1024;

// TEMP until W3 #12: there is no clipboard message yet, so the text goes as a small .txt file over the file
// transfer every receiver already understands.
/// Sends the clipboard text to the connected device.
Future<void> sendClipboard(BuildContext context) async {
  final text = (await Clipboard.getData(Clipboard.kTextPlain))?.text ?? '';
  if (!context.mounted) return;
  if (text.trim().isEmpty) {
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(tr('companion.clipboard.empty'))));
    return;
  }
  final dir = await getTemporaryDirectory();
  final name = clipboardFileName(DateTime.now());
  final f = File('${dir.path}/clipboard/$name');
  await f.parent.create(recursive: true);
  final body = text.length > maxClipboardChars ? text.substring(0, maxClipboardChars) : text;
  await f.writeAsString(body, flush: true);
  if (!context.mounted) return;
  await sendPaths(context, [f.path], [name]);
  if (await f.exists()) await f.delete();
}

// TEMP until W3 #12: nothing leaves the phone until `Message::Notice` exists; the queue shows the feature works.
/// Notifications collected while mirroring is on, newest first, at most 50.
class NoticeQueue extends ChangeNotifier {
  NoticeQueue._();
  static final NoticeQueue instance = NoticeQueue._();

  static const max = 50;
  final List<PhoneNotice> items = [];

  void add(PhoneNotice n) {
    items.insert(0, n);
    if (items.length > max) items.removeRange(max, items.length);
    notifyListeners();
  }

  void clear() {
    items.clear();
    notifyListeners();
  }
}

/// LocalSend and KDE Connect devices heard while the connect sheet is open (A13), one per app and id.
class ForeignDevices extends ChangeNotifier {
  ForeignDevices._();
  static final ForeignDevices instance = ForeignDevices._();

  final Map<String, ForeignDevice> byKey = {};

  void add(ForeignDevice d) {
    final old = byKey[d.key];
    if (old != null && old.name == d.name && old.host == d.host) return;
    byKey[d.key] = d;
    notifyListeners();
  }

  void clear() {
    if (byKey.isEmpty) return;
    byKey.clear();
    notifyListeners();
  }
}
