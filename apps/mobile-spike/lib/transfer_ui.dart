// Files of any type, many at once, both ways: the pickers, the progress sheet, sending with the session to itself,
// and what happens to files that arrived (photos and videos go to the gallery, everything else stays in Vutric's
// folder, which the iPhone Files app shows under "On My iPhone").

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:gal/gal.dart';
import 'package:material_ui/material_ui.dart';
import 'package:path_provider/path_provider.dart';
import 'package:share_plus/share_plus.dart';

import 'android.dart';
import 'design/tokens.dart';
import 'design/widgets.dart';
import 'i18n.dart';
import 'session.dart';
import 'src/rust/api.dart' as vutric;
import 'ui.dart';

/// Files picked for sending: where they are now, and the names the receiver sees.
class Picked {
  Picked(this.paths, this.names);
  final List<String> paths;
  final List<String> names;
  bool get isEmpty => paths.isEmpty;
}

/// Any files (documents, archives, audio…) from the Files app or the Android document picker.
Future<Picked> pickAnyFiles() async {
  final files = await FilePicker.pickFiles(type: FileType.any);
  final tmp = await getTemporaryDirectory();
  final paths = <String>[];
  final names = <String>[];
  for (final (i, f) in files.indexed) {
    var path = f.path;
    if (path == null) {
      // A content:// file with no path of its own: copy it out once, streaming.
      path = '${tmp.path}/vutric-pick-$i-${f.name}';
      await f.xFile.saveTo(path);
    }
    paths.add(path);
    names.add(f.name);
  }
  return Picked(paths, names);
}

/// Where received files are saved: inside the app's documents (visible in the iPhone Files app).
Future<String> receiveDir() async {
  final base = Platform.isAndroid ? (await getExternalStorageDirectory()) : null;
  final docs = base ?? await getApplicationDocumentsDirectory();
  return '${docs.path}/Vutric';
}

String sizeText(double bytes) {
  if (bytes < 1e3) return '${bytes.toStringAsFixed(0)} B';
  if (bytes < 1e6) return '${(bytes / 1e3).toStringAsFixed(0)} KB';
  if (bytes < 1e9) return '${(bytes / 1e6).toStringAsFixed(1)} MB';
  return '${(bytes / 1e9).toStringAsFixed(2)} GB';
}

const _media = {'jpg', 'jpeg', 'png', 'heic', 'heif', 'gif', 'webp', 'mp4', 'mov', 'm4v', 'webm', '3gp'};
const _video = {'mp4', 'mov', 'm4v', 'webm', '3gp'};

String _ext(String path) {
  final dot = path.lastIndexOf('.');
  return dot < 0 ? '' : path.substring(dot + 1).toLowerCase();
}

String baseName(String path) => path.split(RegExp(r'[/\\]')).lastWhere((s) => s.isNotEmpty, orElse: () => 'fayl');

IconData fileIcon(String name) => switch (_ext(name)) {
  'jpg' || 'jpeg' || 'png' || 'heic' || 'heif' || 'webp' || 'gif' => Icons.image_outlined,
  'mp4' || 'mov' || 'm4v' || 'webm' || 'mkv' || '3gp' => Icons.movie_outlined,
  'mp3' || 'm4a' || 'wav' || 'flac' || 'ogg' => Icons.audiotrack_outlined,
  'zip' || 'rar' || '7z' || 'gz' || 'tar' => Icons.folder_zip_outlined,
  'pdf' => Icons.picture_as_pdf_outlined,
  'doc' || 'docx' || 'txt' || 'md' || 'rtf' => Icons.description_outlined,
  'xls' || 'xlsx' || 'csv' => Icons.table_chart_outlined,
  '' => Icons.folder_outlined,
  _ => Icons.insert_drive_file_outlined,
};

/// Hands files to the system share sheet (any app on the phone).
Future<void> shareFiles(List<String> paths) =>
    SharePlus.instance.share(ShareParams(files: [for (final p in paths) XFile(p)]));

/// Today in the phone's calendar, `YYYY-MM-DD`, for a per-day save folder.
String localDate() {
  final d = DateTime.now();
  return '${d.year.toString().padLeft(4, '0')}-${d.month.toString().padLeft(2, '0')}-${d.day.toString().padLeft(2, '0')}';
}

/// Puts received photos and videos into the gallery when the settings say so (album, keep a copy or not);
/// returns how many went there.
Future<int> saveMediaToGallery(List<String> saved) async {
  final media = saved.where((p) => _media.contains(_ext(p))).toList();
  if (media.isEmpty) return 0;
  final save = (jsonDecode(await vutric.getSettings()) as Map<String, dynamic>)['save'] as Map<String, dynamic>;
  if (save['gallery'] != true) return 0;
  final album = (save['album'] as String?)?.trim();
  final keepCopy = save['keep_copy'] != false;
  if (!await Gal.hasPermission() && !await Gal.requestPermission()) return 0;
  var n = 0;
  for (final p in media) {
    try {
      final name = album == null || album.isEmpty ? 'Vutric' : album;
      if (_video.contains(_ext(p))) {
        await Gal.putVideo(p, album: name);
      } else {
        await Gal.putImage(p, album: name);
      }
      n++;
      if (!keepCopy) await File(p).delete();
    } catch (_) {
      // Not a format the gallery takes (or no space): it stays in Vutric's folder.
    }
  }
  return n;
}

/// Sends [paths] to the connected device with the progress sheet, the session to itself, and a history entry.
/// Returns the final progress, or null when it failed or was cancelled.
Future<vutric.TransferProgress?> sendPaths(BuildContext context, List<String> paths, List<String> names) async {
  final s = Session.instance;
  final peer = s.peer?.peerName ?? 'qurilma';
  final bytes = await totalBytes(paths);
  if (!context.mounted || !await bulkAllowed(context, bytes) || !context.mounted) return null;
  final r = await s.exclusive(
    () => runTransfer(
      context,
      title: '$peer’ga yuborilmoqda',
      stream: vutric.sendFiles(paths: paths, names: names),
    ),
  );
  if (r == null) {
    await s.stillConnected();
    return null;
  }
  unawaited(
    s.addHistory(
      HistoryItem(
        sent: true,
        peer: peer,
        files: r.filesTotal - r.failedNames.length,
        bytes: r.bytesDone.toInt(),
        seconds: r.seconds,
        at: DateTime.now(),
        names: names.take(20).toList(),
        paths: paths.take(200).toList(),
      ),
    ),
  );
  return r;
}

/// The size of [paths] on disk (folders counted through), for the relay rule below.
Future<int> totalBytes(List<String> paths) async {
  var n = 0;
  for (final p in paths) {
    try {
      final t = await FileSystemEntity.type(p);
      if (t == FileSystemEntityType.directory) {
        await for (final f in Directory(p).list(recursive: true, followLinks: false)) {
          if (f is File) n += await f.length();
        }
      } else if (t == FileSystemEntityType.file) {
        n += await File(p).length();
      }
    } on FileSystemException {
      // Unreadable: the send itself reports it.
    }
  }
  return n;
}

/// Big batches through the relay (Rb, ADR-0068): over 2 GiB on a relayed path asks first (a hotspot or cable is
/// faster), over 10 GiB is refused with the same advice. A direct path, or no answer from Rust, sends as before.
Future<bool> bulkAllowed(BuildContext context, int bytes) async {
  String verdict;
  try {
    verdict = await Session.instance.exclusive(() => vutric.bulkVerdict(total: BigInt.from(bytes)));
  } catch (_) {
    return true;
  }
  if (verdict == 'go' || !context.mounted) return verdict == 'go';
  if (verdict == 'too_big') {
    await showDialog<void>(
      context: context,
      builder: (_) => BulkDialog(bytes: bytes, refuse: true),
    );
    return false;
  }
  return await showDialog<bool>(
        context: context,
        builder: (_) => BulkDialog(bytes: bytes, refuse: false),
      ) ==
      true;
}

class BulkDialog extends StatelessWidget {
  const BulkDialog({super.key, required this.bytes, required this.refuse});
  final int bytes;
  final bool refuse;

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(tr('bulk.ask.title', {'size': sizeText(bytes.toDouble())})),
      content: Text(refuse ? tr('bulk.too_big') : tr('bulk.ask.body')),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context, false), child: Text(tr('companion.clipboard.close'))),
        if (!refuse) FilledButton(onPressed: () => Navigator.pop(context, true), child: Text(tr('bulk.ask.go'))),
      ],
    );
  }
}

/// Runs a transfer with a progress sheet the user can cancel. Returns the last event (done), or null when it
/// failed or was cancelled (the error is shown in the sheet).
Future<vutric.TransferProgress?> runTransfer(
  BuildContext context, {
  required String title,
  required Stream<vutric.TransferProgress> stream,
}) {
  return showModalBottomSheet<vutric.TransferProgress?>(
    context: context,
    isDismissible: false,
    enableDrag: false,
    builder: (_) => _TransferSheet(title: title, stream: stream),
  );
}

class _TransferSheet extends StatefulWidget {
  const _TransferSheet({required this.title, required this.stream});
  final String title;
  final Stream<vutric.TransferProgress> stream;

  @override
  State<_TransferSheet> createState() => _TransferSheetState();
}

class _TransferSheetState extends State<_TransferSheet> {
  StreamSubscription<vutric.TransferProgress>? _sub;
  vutric.TransferProgress? _p;
  String? _error;
  bool _cancelled = false;

  final _bridge = AndroidBridge.instance;

  @override
  void initState() {
    super.initState();
    // Android: a foreground service keeps the process alive if the user leaves the app mid-transfer (A4).
    _bridge.transferUpdate(widget.title, 'Javob kutilmoqda…', force: true);
    _sub = widget.stream.listen(
      (p) {
        if (!mounted) return;
        setState(() => _p = p);
        final total = p.bytesTotal.toDouble();
        _bridge.transferUpdate(
          widget.title,
          '${p.filesDone} / ${p.filesTotal} fayl · ${sizeText(p.bytesDone.toDouble())} / ${sizeText(total)}',
          fraction: total > 0 ? p.bytesDone.toDouble() / total : null,
        );
        if (p.done) Navigator.of(context).pop(p);
      },
      onError: (Object e) {
        if (!mounted) return;
        if (_cancelled || isCancelled(e)) {
          Navigator.of(context).pop();
          return;
        }
        setState(() => _error = '$e'.contains('declined') ? 'Qabul qiluvchi rad etdi' : errorText(e));
      },
    );
  }

  @override
  void dispose() {
    _sub?.cancel();
    _bridge.transferStop();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final p = _p;
    final total = p?.bytesTotal.toDouble() ?? 0;
    final done = p?.bytesDone.toDouble() ?? 0;
    final frac = total > 0 ? (done / total).clamp(0.0, 1.0) : null;
    final speed = p?.bytesPerSec ?? 0;
    final left = speed > 0 ? (total - done) / speed : null;
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(20, 0, 20, 16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Expanded(child: Text(widget.title, style: v.heading)),
                if (frac != null && _error == null)
                  Text('${(frac * 100).floor()}%', style: v.text(17, FontWeight.w600, mono: true, color: v.accent)),
              ],
            ),
            const SizedBox(height: 14),
            if (_error == null) ...[
              ClipRRect(
                borderRadius: BorderRadius.circular(v.rPill.clamp(0, 5)),
                child: TweenAnimationBuilder<double>(
                  tween: Tween(end: frac ?? 0),
                  duration: VMotion.fast,
                  builder: (_, value, _) => LinearProgressIndicator(
                    value: frac == null ? null : value,
                    minHeight: 10,
                    color: v.accent,
                    backgroundColor: v.bgAlt,
                  ),
                ),
              ),
              const SizedBox(height: 12),
              Text(
                p == null
                    ? 'Qabul qiluvchining javobi kutilmoqda…'
                    : '${p.filesDone} / ${p.filesTotal} fayl · ${sizeText(done)} / ${sizeText(total)}',
                style: v.text(15, FontWeight.w500),
              ),
              if (p != null && speed > 0)
                Padding(
                  padding: const EdgeInsets.only(top: 2),
                  child: Text(
                    '${sizeText(speed)}/s${left != null ? ' · ~${_duration(left)} qoldi' : ''}',
                    style: v.monoSmall,
                  ),
                ),
              if (p != null && p.current.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(top: 2),
                  child: Text(p.current, style: v.caption, maxLines: 1, overflow: TextOverflow.ellipsis),
                ),
              if (p != null && p.failed > 0)
                Text('${p.failed} ta fayl oʻtmadi', style: v.text(13, FontWeight.w500, color: v.err)),
              const SizedBox(height: 16),
              VButton(
                label: 'Bekor qilish',
                kind: VButtonKind.secondary,
                onPressed: () {
                  _cancelled = true;
                  disconnectNow();
                },
              ),
            ] else ...[
              VNotice(title: 'Uzatish toʻxtadi', body: _error),
              const SizedBox(height: 16),
              VButton(label: 'Yopish', onPressed: () => Navigator.of(context).pop()),
            ],
          ],
        ),
      ),
    );
  }
}

String _duration(double seconds) {
  if (seconds < 60) return '${seconds.ceil()} s';
  final m = seconds ~/ 60;
  if (m < 60) return '$m daq';
  return '${m ~/ 60} soat ${m % 60} daq';
}

/// The line after a transfer ended.
String doneText(vutric.TransferProgress p, {required bool sending, int inGallery = 0}) {
  final ok = p.filesTotal - p.failedNames.length;
  final verb = sending ? 'yuborildi' : 'qabul qilindi';
  final rate = p.seconds > 0 ? ' · ${sizeText(p.bytesDone.toDouble() / p.seconds)}/s' : '';
  final gallery = inGallery > 0 ? ' · $inGallery tasi galereyada' : '';
  final failed = p.failedNames.isEmpty ? '' : '\nOʻtmadi: ${p.failedNames.join(', ')}';
  return '$ok ta fayl $verb (${sizeText(p.bytesDone.toDouble())}, ${p.seconds.toStringAsFixed(1)} s$rate)$gallery$failed';
}
