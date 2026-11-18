// The other direction: this phone receives from a computer or another phone. The sender either scans this phone's
// QR code (we wait), or this phone scans the computer's QR code (`vutric-net-spike fling --photo PATH`, or
// `vutric-net-spike files PATH...`). A thrown photo flies in from the top edge, the way it left the thrower, and
// the thrower's round trip closes only once it is drawn here. Offered files are accepted or declined by the user;
// photos and videos then go to the gallery, everything else stays in Vutric's folder (Files app).

import 'dart:math' as math;

import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';
import 'package:qr_flutter/qr_flutter.dart';

import 'src/rust/api.dart' as vutric;
import 'transfer_ui.dart';
import 'ui.dart';

class CatchPage extends StatefulWidget {
  const CatchPage({super.key, required this.scan, required this.myLink});

  /// Opens the QR scanner and returns a `vutric://pair?` link, or null.
  final Future<String?> Function() scan;

  /// This phone's pairing link, shown as a QR code for the thrower.
  final String myLink;

  @override
  State<CatchPage> createState() => _CatchPageState();
}

class _CatchPageState extends State<CatchPage> with SingleTickerProviderStateMixin {
  vutric.Connected? _peer;
  vutric.Caught? _photo;
  MemoryImage? _image;
  String? _status;
  String? _error;
  bool _waiting = false;
  bool _showQr = false;
  int _count = 0;
  double? _lastMs;
  String? _received;

  late final AnimationController _land = AnimationController(vsync: this, duration: const Duration(milliseconds: 420));

  @override
  void dispose() {
    _land.dispose();
    disconnectNow(); // cancels a wait for a device or for the next photo
    super.dispose();
  }

  Future<void> _start(String label, Future<vutric.Connected> Function() job, {bool showQr = false}) async {
    setState(() {
      _waiting = true;
      _showQr = showQr;
      _status = label;
      _error = null;
    });
    try {
      final c = await afterDisconnect(job);
      if (!mounted) return;
      setState(() {
        _peer = c;
        _status = 'Ulandi. Rasm yoki fayllar kutilmoqda…';
      });
      _catchLoop();
    } catch (e) {
      if (!mounted || isCancelled(e)) return;
      setState(() {
        _error = errorText(e);
        _status = null;
      });
    } finally {
      if (mounted) {
        setState(() {
          _waiting = false;
          _showQr = false;
        });
      }
    }
  }

  Future<void> _cancel() async {
    await disconnectNow();
    if (mounted) setState(() => _status = null);
  }

  Future<void> _scanAndConnect() async {
    final link = await widget.scan();
    if (link == null || !mounted) return;
    await _start('Ulanmoqda…', () => vutric.connectLink(link: link));
  }

  void _lost(Object e) {
    if (!mounted || isCancelled(e)) return;
    setState(() {
      _error = 'Aloqa uzildi: ${errorText(e)}';
      _peer = null;
      _status = null;
    });
  }

  /// Asks the user, then saves the files with a progress sheet. False when the session is gone.
  Future<bool> _offered(vutric.FilesOffer offer) async {
    HapticFeedback.mediumImpact();
    // Settings may say yes already (trusted device, or under the size limit): then no question.
    final yes =
        offer.autoAccept ||
        await showDialog<bool>(
              context: context,
              barrierDismissible: false,
              builder: (ctx) => AlertDialog(
                title: Text('${_peer?.peerName ?? 'Qurilma'} fayl yubormoqda'),
                content: Text(offerText(offer)),
                actions: [
                  TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Rad etish')),
                  FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Qabul qilish')),
                ],
              ),
            ) ==
            true;
    if (!mounted) return false;
    try {
      if (!yes) {
        await vutric.declineFiles();
        return true;
      }
      final dir = await receiveDir();
      if (!mounted) return false;
      final r = await runTransfer(context, title: 'Qabul qilinmoqda', stream: vutric.receiveFiles(dir: dir, date: localDate()));
      if (!mounted) return false;
      if (r == null) return (await vutric.currentRoute()).isNotEmpty;
      final inGallery = await saveMediaToGallery(r.saved);
      if (mounted) setState(() => _received = doneText(r, sending: false, inGallery: inGallery));
      return true;
    } catch (e) {
      _lost(e);
      return false;
    }
  }

  /// One thing at a time: a photo is decoded, put on screen, and the thrower told once it is drawn; files are
  /// offered to the user.
  Future<void> _catchLoop() async {
    while (mounted && _peer != null) {
      final vutric.Incoming next;
      try {
        next = await vutric.nextIncoming();
      } catch (e) {
        _lost(e);
        return;
      }
      if (!mounted) return;
      final files = next.files;
      if (files != null) {
        if (!await _offered(files)) {
          if (mounted) setState(() => _peer = null);
          return;
        }
        continue;
      }
      final p = next.photo;
      if (p == null) continue;
      final clock = Stopwatch()..start();
      final image = MemoryImage(p.jpeg);
      // Decode before showing, so the frame we report really contains the photo. precacheImage reports a decode
      // failure through onError (it does not throw).
      var decoded = true;
      await precacheImage(image, context, onError: (_, _) => decoded = false);
      if (!mounted) return;
      if (!decoded) {
        // Rust accepted the JPEG but Flutter cannot draw it: tell the thrower now instead of letting it time out.
        try {
          await vutric.refuse(id: p.id);
        } catch (e) {
          _lost(e);
          return;
        }
        continue;
      }
      HapticFeedback.lightImpact();
      setState(() {
        _photo = p;
        _image = image;
        _count++;
        _status = null;
      });
      _land.forward(from: 0);
      await SchedulerBinding.instance.endOfFrame;
      clock.stop();
      if (!mounted) return;
      setState(() => _lastMs = clock.elapsedMicroseconds / 1000);
      try {
        await vutric.shown(id: p.id);
      } catch (e) {
        _lost(e);
        return;
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context).textTheme;
    final peer = _peer;
    return Scaffold(
      appBar: AppBar(backgroundColor: sand, title: const Text('Qabul qilish')),
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (peer != null)
                Container(
                  color: paper,
                  padding: const EdgeInsets.all(12),
                  child: Row(
                    children: [
                      const Icon(Icons.south, color: signal),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text('${peer.peerName} (${peer.peerPlatform})', style: t.titleSmall?.copyWith(color: cobalt)),
                            Text(routeLabel(peer.route), style: t.labelSmall),
                          ],
                        ),
                      ),
                      Column(
                        crossAxisAlignment: CrossAxisAlignment.end,
                        children: [
                          Text('tekshiruv kodi', style: t.labelSmall),
                          Text(peer.checkCode, style: t.titleMedium?.copyWith(color: cobalt, letterSpacing: 2)),
                        ],
                      ),
                    ],
                  ),
                )
              else if (_waiting)
                OutlinedButton(onPressed: _cancel, child: const Text('Bekor qilish'))
              else ...[
                FilledButton(onPressed: _scanAndConnect, child: const Text('Kompyuter QR kodini skanerlash')),
                const SizedBox(height: 8),
                OutlinedButton(
                  onPressed: () => _start('Ulanish kutilmoqda…', vutric.acceptSession, showQr: true),
                  child: const Text('Kutish: QR kodimni skanerlashsin'),
                ),
              ],
              if (_status != null)
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: Text(_status!, style: t.bodyMedium?.copyWith(color: cobalt)),
                ),
              if (_error != null)
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: Text(_error!, style: t.bodyMedium?.copyWith(color: err)),
                ),
              const SizedBox(height: 12),
              Expanded(child: LayoutBuilder(builder: (context, box) => _stage(context, box.biggest))),
              if (_received != null)
                Padding(
                  padding: const EdgeInsets.only(bottom: 8),
                  child: Text(_received!, style: t.bodySmall?.copyWith(color: ok)),
                ),
              if (_count > 0)
                Text(
                  '$_count ta rasm keldi · oxirgisi kelgandan ekranda koʻringuncha '
                  '${_lastMs?.toStringAsFixed(1) ?? '–'} ms',
                  style: t.bodySmall,
                ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _stage(BuildContext context, Size area) {
    final p = _photo;
    final image = _image;
    if (p == null || image == null) {
      if (_showQr) {
        // Waiting to be found: the QR code the thrower scans.
        return Center(
          child: Container(
            color: paper,
            padding: const EdgeInsets.all(12),
            child: QrImageView(data: widget.myLink, size: math.min(area.width, area.height) * 0.7),
          ),
        );
      }
      return Center(
        child: Text(
          _peer == null
              ? 'Kompyuterda: vutric-net-spike files FAYL_YOKI_PAPKA\nsoʻng uning QR kodini skanerlang'
              : 'Rasm yoki fayllar kutilmoqda…',
          style: Theme.of(context).textTheme.bodyMedium,
          textAlign: TextAlign.center,
        ),
      );
    }
    final aspect = p.height == 0 ? 1.0 : p.width / p.height;
    final maxSide = math.min(area.width, area.height) * 0.85;
    final w = aspect >= 1 ? maxSide : maxSide * aspect;
    final h = aspect >= 1 ? maxSide / aspect : maxSide;
    // Fly in from the top edge where the thrower let go, carrying its tilt, and settle in the middle.
    final fromX = (p.exitX - 0.5) * area.width;
    return AnimatedBuilder(
      animation: _land,
      builder: (context, child) {
        final k = Curves.easeOutBack.transform(_land.value);
        final tilt = p.angle.clamp(-0.6, 0.6) * (1 - k);
        return Stack(
          children: [
            Positioned(
              left: (area.width - w) / 2 + fromX * (1 - k),
              top: (area.height - h) / 2 - (area.height / 2 + h) * (1 - k),
              width: w,
              height: h,
              child: Transform.rotate(angle: tilt, child: child),
            ),
          ],
        );
      },
      child: Container(
        decoration: BoxDecoration(
          color: paper,
          borderRadius: BorderRadius.circular(16),
          boxShadow: const [BoxShadow(color: Color(0x33062B8A), blurRadius: 24, offset: Offset(0, 8))],
        ),
        clipBehavior: Clip.antiAlias,
        child: Image(
          image: image,
          fit: BoxFit.cover,
          gaplessPlayback: true,
          semanticLabel: 'Qabul qilingan rasm ${p.fileName}',
        ),
      ),
    );
  }
}
