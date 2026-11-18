// Files of any type, many at once, both ways: the pickers, the progress sheet, and what happens to files that
// arrived (photos and videos go to the gallery, everything else stays in Vutric's folder, which the iPhone
// Files app shows under "On My iPhone").

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:gal/gal.dart';
import 'package:image_picker/image_picker.dart';
import 'package:material_ui/material_ui.dart';
import 'package:path_provider/path_provider.dart';

import 'src/rust/api.dart' as vutric;
import 'ui.dart';

/// Files picked for sending: where they are now, and the names the receiver sees.
class Picked {
  Picked(this.paths, this.names);
  final List<String> paths;
  final List<String> names;
  bool get isEmpty => paths.isEmpty;
}

/// Photos and videos from the gallery, as many as the user selects.
Future<Picked> pickMedia() async {
  final files = await ImagePicker().pickMultipleMedia(requestFullMetadata: false);
  return Picked([for (final f in files) f.path], [for (final f in files) f.name]);
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
    backgroundColor: paper,
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

  @override
  void initState() {
    super.initState();
    _sub = widget.stream.listen(
      (p) {
        if (!mounted) return;
        setState(() => _p = p);
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
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context).textTheme;
    final p = _p;
    final total = p?.bytesTotal.toDouble() ?? 0;
    final done = p?.bytesDone.toDouble() ?? 0;
    final frac = total > 0 ? (done / total).clamp(0.0, 1.0) : null;
    final speed = p?.bytesPerSec ?? 0;
    final left = speed > 0 ? (total - done) / speed : null;
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(20, 20, 20, 12),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(widget.title, style: t.titleMedium?.copyWith(color: cobalt)),
            const SizedBox(height: 12),
            if (_error == null) ...[
              LinearProgressIndicator(value: frac, color: signal, minHeight: 8, borderRadius: BorderRadius.circular(4)),
              const SizedBox(height: 10),
              Text(
                p == null
                    ? 'Qabul qiluvchining javobi kutilmoqda…'
                    : '${p.filesDone} / ${p.filesTotal} fayl · ${sizeText(done)} / ${sizeText(total)}',
                style: t.bodyMedium,
              ),
              if (p != null && speed > 0)
                Text(
                  '${sizeText(speed)}/s${left != null ? ' · ~${_duration(left)} qoldi' : ''}',
                  style: t.bodySmall,
                ),
              if (p != null && p.current.isNotEmpty)
                Text(p.current, style: t.bodySmall, maxLines: 1, overflow: TextOverflow.ellipsis),
              if (p != null && p.failed > 0)
                Text('${p.failed} ta fayl oʻtmadi', style: t.bodySmall?.copyWith(color: err)),
              const SizedBox(height: 12),
              OutlinedButton(
                onPressed: () {
                  _cancelled = true;
                  disconnectNow();
                },
                child: const Text('Bekor qilish'),
              ),
            ] else ...[
              Text(_error!, style: t.bodyMedium?.copyWith(color: err)),
              const SizedBox(height: 12),
              FilledButton(onPressed: () => Navigator.of(context).pop(), child: const Text('Yopish')),
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

/// "12 ta fayl, 340 MB: IMG_1.jpg, IMG_2.jpg…" for the accept question.
String offerText(vutric.FilesOffer o) {
  final more = o.files > o.names.length ? '…' : '';
  return '${o.files} ta fayl, ${sizeText(o.bytes.toDouble())}\n${o.names.join(', ')}$more';
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
