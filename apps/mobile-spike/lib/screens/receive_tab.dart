// S4 — Rasm qabul qilish, in the handoff's 5 states: tanlov · kutish (my QR, MyQrPage) · ulangan, kutilmoqda ·
// rasm keldi · aloqa uzildi. Incoming photos and file offers are handled here for the whole app (the home shell
// routes Session.onIncoming to [handleIncoming]), so a photo thrown at this phone lands even if another tab was open.

import 'dart:async';
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:gal/gal.dart';
import 'package:material_ui/material_ui.dart';

import '../android.dart';
import '../design/tokens.dart';
import '../design/widgets.dart';
import '../session.dart';
import '../src/rust/api.dart' as vutric;
import '../transfer_ui.dart';
import '../ui.dart';
import 'connect.dart';
import 'throw_page.dart' show DashedBorder, showPeerSheet;

/// What arrived on this phone during the current session: the photo on screen and the counters under it.
class Catches extends ChangeNotifier {
  Catches._();
  static final Catches instance = Catches._();

  vutric.Caught? photo;
  MemoryImage? image;

  /// Bumped on every new photo so the landing animation replays.
  int landing = 0;
  int photos = 0;
  int files = 0;
  double? lastMs;
  bool savedLast = false;

  void reset() {
    photo = null;
    image = null;
    photos = 0;
    files = 0;
    lastMs = null;
    notifyListeners();
  }

  void update() => notifyListeners();
}

/// Handles one photo or offer from the other side. [showReceive] brings the Qabul tab to the front for a photo.
Future<void> handleIncoming(BuildContext context, vutric.Incoming next, VoidCallback showReceive) async {
  final files = next.files;
  if (files != null) return _offer(context, files);
  final p = next.photo;
  if (p == null) return;
  final c = Catches.instance;
  final clock = Stopwatch()..start();
  final image = MemoryImage(p.jpeg);
  // Decode before showing, so the frame reported back really contains the photo.
  var decoded = true;
  await precacheImage(image, context, onError: (_, _) => decoded = false);
  if (!decoded) {
    await vutric.refuse(id: p.id);
    return;
  }
  HapticFeedback.lightImpact();
  showReceive();
  c
    ..photo = p
    ..image = image
    ..photos += 1
    ..landing += 1
    ..savedLast = false
    ..update();
  // In the background no frame is drawn, so waiting for one would hang the thrower. There the photo reaches the
  // screen as a heads-up notification with its thumbnail (A11), and that is what "shown" reports.
  if (_inFront) {
    await SchedulerBinding.instance.endOfFrame;
  } else {
    await AndroidBridge.instance.notifyIncoming(
      '${Session.instance.peer?.peerName ?? 'Qurilma'} rasm otdi',
      p.fileName,
      jpeg: p.jpeg,
    );
  }
  clock.stop();
  c
    ..lastMs = clock.elapsedMicroseconds / 1000
    ..update();
  await vutric.shown(id: p.id);
  unawaited(
    Session.instance.addHistory(
      HistoryItem(
        sent: false,
        peer: Session.instance.peer?.peerName ?? '',
        files: 1,
        bytes: p.jpeg.length,
        seconds: 0,
        at: DateTime.now(),
        names: [p.fileName],
        ms: c.lastMs,
      ),
    ),
  );
}

bool get _inFront => WidgetsBinding.instance.lifecycleState == AppLifecycleState.resumed;

/// The other side offers files: ask (unless the settings already say yes), then save with progress.
Future<void> _offer(BuildContext context, vutric.FilesOffer offer) async {
  HapticFeedback.mediumImpact();
  final s = Session.instance;
  final peer = s.peer?.peerName ?? 'Qurilma';
  if (!_inFront) {
    // The question waits in the app; the notification brings the user to it.
    unawaited(
      AndroidBridge.instance.notifyIncoming(
        '$peer fayl yubormoqda',
        '${offer.files} ta · ${sizeText(offer.bytes.toDouble())}${offer.autoAccept ? '' : ' · qabul qilasizmi?'}',
      ),
    );
  }
  final yes =
      offer.autoAccept ||
      await showModalBottomSheet<bool>(
            context: context,
            isDismissible: false,
            enableDrag: false,
            builder: (_) => _OfferSheet(offer: offer, peer: peer),
          ) ==
          true;
  if (!context.mounted) return;
  if (!yes) {
    await vutric.declineFiles();
    return;
  }
  final dir = await receiveDir();
  if (!context.mounted) return;
  final r = await runTransfer(
    context,
    title: '$peer’dan qabul qilinmoqda',
    stream: vutric.receiveFiles(dir: dir, date: localDate()),
  );
  if (r == null) {
    await s.stillConnected();
    return;
  }
  final ok = r.filesTotal - r.failedNames.length;
  final inGallery = await saveMediaToGallery(r.saved);
  // Android: the files that are still here (not moved into the gallery) go on to the folder the user picked.
  var saved = r.saved.where((p) => File(p).existsSync()).toList();
  var inFolder = 0;
  final folder = s.saveFolder;
  if (folder != null && saved.isNotEmpty && await AndroidBridge.instance.folderUsable(folder.uri)) {
    inFolder = await AndroidBridge.instance.moveToFolder(folder.uri, saved);
    saved = saved.where((p) => File(p).existsSync()).toList();
  }
  Catches.instance
    ..files += ok
    ..update();
  unawaited(
    s.addHistory(
      HistoryItem(
        sent: false,
        peer: peer,
        files: ok,
        bytes: r.bytesDone.toInt(),
        seconds: r.seconds,
        at: DateTime.now(),
        names: [for (final p in r.saved.take(20)) baseName(p)],
        paths: saved.take(200).toList(),
      ),
    ),
  );
  if (context.mounted) {
    final where = inFolder > 0 ? ' · $inFolder tasi “${folder!.label}” papkasida' : '';
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(doneText(r, sending: false, inGallery: inGallery) + where)),
    );
  }
}

class _OfferSheet extends StatelessWidget {
  const _OfferSheet({required this.offer, required this.peer});
  final vutric.FilesOffer offer;
  final String peer;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final more = offer.files - offer.names.length;
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                const VIconTile(icon: Icons.south_rounded, size: 44),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text('$peer fayl yubormoqda', style: v.heading),
                      Text('${offer.files} ta · ${sizeText(offer.bytes.toDouble())}', style: v.monoSmall),
                    ],
                  ),
                ),
              ],
            ),
            const SizedBox(height: 14),
            VSurface(
              color: v.bgAlt,
              shadow: false,
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
              child: Column(
                children: [
                  for (final n in offer.names)
                    Padding(
                      padding: const EdgeInsets.symmetric(vertical: 6),
                      child: Row(
                        children: [
                          Icon(fileIcon(n), size: 20, color: v.muted),
                          const SizedBox(width: 10),
                          Expanded(
                            child: Text(
                              n,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: v.text(14.5, FontWeight.w500),
                            ),
                          ),
                        ],
                      ),
                    ),
                  if (more > 0)
                    Padding(
                      padding: const EdgeInsets.symmetric(vertical: 6),
                      child: Text('yana $more ta', style: v.caption),
                    ),
                ],
              ),
            ),
            const SizedBox(height: 16),
            Row(
              children: [
                Expanded(
                  flex: 10,
                  child: VButton(
                    label: 'Rad etish',
                    kind: VButtonKind.secondary,
                    onPressed: () => Navigator.pop(context, false),
                  ),
                ),
                const SizedBox(width: 10),
                Expanded(
                  flex: 14,
                  child: VButton(
                    label: 'Qabul qilish',
                    icon: Icons.check_rounded,
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

class ReceiveTab extends StatefulWidget {
  const ReceiveTab({super.key, required this.header});

  /// The shared top row (wordmark, name, settings) from the home shell.
  final Widget header;

  @override
  State<ReceiveTab> createState() => _ReceiveTabState();
}

class _ReceiveTabState extends State<ReceiveTab> with SingleTickerProviderStateMixin {
  final _s = Session.instance;
  final _c = Catches.instance;
  late final AnimationController _land = AnimationController(vsync: this, duration: const Duration(milliseconds: 620));
  int _seen = 0;

  @override
  void initState() {
    super.initState();
    _s.addListener(_changed);
    _c.addListener(_changed);
  }

  @override
  void dispose() {
    _s.removeListener(_changed);
    _c.removeListener(_changed);
    _land.dispose();
    super.dispose();
  }

  void _changed() {
    if (!mounted) return;
    if (_c.landing != _seen) {
      _seen = _c.landing;
      _land
        ..duration = context.reducedMotion ? VMotion.reduced : const Duration(milliseconds: 620)
        ..forward(from: 0);
    }
    setState(() {});
  }

  Future<void> _save() async {
    final p = _c.photo;
    if (p == null) return;
    try {
      if (!await Gal.hasPermission() && !await Gal.requestPermission()) throw StateError('galereya ruxsati yoʻq');
      await Gal.putImageBytes(p.jpeg, album: 'Vutric', name: p.fileName.replaceAll(RegExp(r'\.\w+$'), ''));
      HapticFeedback.mediumImpact();
      setState(() => _c.savedLast = true);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Saqlab boʻlmadi: ${errorText(e)}')));
      }
    }
  }

  Future<void> _share() async {
    final p = _c.photo;
    if (p == null) return;
    // The photo as a file the system share sheet can hand to any app.
    final dir = await receiveDir();
    final f = File('$dir/${p.fileName}');
    await f.parent.create(recursive: true);
    await f.writeAsBytes(p.jpeg, flush: true);
    await shareFiles([f.path]);
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final peer = _s.peer;
    final lost = peer == null && _s.lostPeer != null;
    if (peer == null && !lost) return _choice(v);
    return SafeArea(
      bottom: false,
      child: Column(
        children: [
          const SizedBox(height: 8),
          Center(
            child: lost
                ? VDevicePill(name: _s.lostPeer!, lost: true)
                : VDevicePill(
                    name: peer!.peerName,
                    platform: peer.peerPlatform,
                    code: peer.checkCode,
                    onTap: () => showPeerSheet(context, peer),
                  ),
          ),
          Expanded(child: lost ? _lost(v) : (_c.photo == null ? _waiting(v) : _arrived(v))),
          Padding(padding: const EdgeInsets.fromLTRB(16, 0, 16, 104), child: _buttons(v, lost)),
        ],
      ),
    );
  }

  /// S4-1: how to connect.
  Widget _choice(VutricTokens v) {
    return SafeArea(
      bottom: false,
      child: ListView(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 120),
        children: [
          widget.header,
          const SizedBox(height: 18),
          Text('Qanday\nulanamiz?', style: v.largeTitle),
          const SizedBox(height: 8),
          Text(
            'Kompyuter yoki boshqa telefon sizga rasm va fayl yuboradi.',
            style: v.text(15, FontWeight.w400, color: v.muted),
          ),
          const SizedBox(height: 22),
          _ChoiceCard(
            icon: Icons.qr_code_scanner_rounded,
            title: 'Kompyuter QR kodini skanerlash',
            subtitle: 'Kompyuterdagi Vutric oynasidan',
            onTap: () => connectFlow(context, way: ConnectWay.scan),
          ),
          const SizedBox(height: 12),
          _ChoiceCard(
            icon: Icons.qr_code_2_rounded,
            title: 'Kutish: QR kodimni skanerlashsin',
            subtitle: 'Boshqa telefon sizga otadi',
            onTap: () => connectFlow(context, way: ConnectWay.wait),
          ),
          if (hasMdns) ...[
            const SizedBox(height: 12),
            _ChoiceCard(
              icon: Icons.wifi_find_rounded,
              title: 'Tarmoqdan topish',
              subtitle: 'Shu Wi-Fi’dagi kompyuter',
              onTap: () => connectFlow(context, way: ConnectWay.find),
            ),
          ],
        ],
      ),
    );
  }

  /// S4-3: connected, waiting.
  Widget _waiting(VutricTokens v) {
    return Column(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        SizedBox(
          width: 190,
          height: 250,
          child: CustomPaint(
            painter: DashedBorder(color: v.lineStrong, radius: v.rCard),
            child: Padding(
              padding: const EdgeInsets.all(18),
              child: DecoratedBox(
                decoration: BoxDecoration(
                  color: v.raised,
                  borderRadius: BorderRadius.circular(math.max(0, v.rCard - 8)),
                ),
                child: Center(child: _Bob(child: Icon(Icons.arrow_downward_rounded, color: v.muted))),
              ),
            ),
          ),
        ),
        const SizedBox(height: 24),
        Text('Rasm kutilmoqda…', style: v.heading),
        const SizedBox(height: 6),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 32),
          child: Text(
            'Kompyuterda rasmni oʻng chetga oting yoki fayl yuboring',
            style: v.caption,
            textAlign: TextAlign.center,
          ),
        ),
      ],
    );
  }

  /// S4-4: a photo landed. It flies in from the top edge where it left the thrower, carrying its tilt.
  Widget _arrived(VutricTokens v) {
    final p = _c.photo!;
    final image = _c.image!;
    return LayoutBuilder(
      builder: (context, box) {
        final aspect = p.height == 0 ? 0.75 : p.width / p.height;
        var w = math.min(box.maxWidth * 0.62, 300.0);
        var h = w / aspect;
        final maxH = box.maxHeight * 0.62;
        if (h > maxH) {
          h = maxH;
          w = h * aspect;
        }
        final fromX = (p.exitX - 0.5) * box.maxWidth;
        final reduced = context.reducedMotion;
        final stageH = box.maxHeight - 64;
        return Column(
          children: [
            SizedBox(
              height: stageH,
              child: AnimatedBuilder(
                animation: _land,
                builder: (context, child) {
                  final t = _land.value;
                  final k = reduced ? 1.0 : VMotion.returnBack.transform(t);
                  final tilt = reduced ? 0.0 : p.angle.clamp(-0.6, 0.6) * (1 - k);
                  return Stack(
                    clipBehavior: Clip.none,
                    children: [
                      Positioned(
                        left: (box.maxWidth - w) / 2 + fromX * (1 - k),
                        top: (stageH - h) / 2 - (stageH / 2 + h) * (1 - k),
                        width: w,
                        height: h,
                        child: Opacity(
                          opacity: reduced ? t : 1,
                          child: Transform.rotate(angle: tilt, child: child),
                        ),
                      ),
                    ],
                  );
                },
                child: VPhotoCard(
                  child: Image(
                    image: image,
                    fit: BoxFit.cover,
                    gaplessPlayback: true,
                    semanticLabel: 'Qabul qilingan rasm ${p.fileName}',
                  ),
                ),
              ),
            ),
            Text(_c.photos == 1 ? 'Rasm keldi' : '${_c.photos} ta rasm keldi', style: v.text(16, FontWeight.w600)),
            const SizedBox(height: 4),
            Text(
              [if (_c.lastMs != null) 'ekranda ${_c.lastMs!.round()} ms', p.fileName].join(' · '),
              style: v.monoSmall,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
          ],
        );
      },
    );
  }

  /// S4-5: the connection dropped.
  Widget _lost(VutricTokens v) {
    final n = _c.photos + _c.files;
    return VEmpty(
      icon: Icons.link_off_rounded,
      tint: v.err,
      tileColor: Color.alphaBlend(v.err.withValues(alpha: 0.12), v.raised),
      title: 'Aloqa uzildi',
      body: n == 0
          ? 'Kompyuter uxlab qolgan yoki boshqa Wi-Fi’ga oʻtgan boʻlishi mumkin.'
          : '$n ta saqlandi. Qayta ulanib, qolganini olishingiz mumkin.',
    );
  }

  Widget _buttons(VutricTokens v, bool lost) {
    if (lost) {
      return VButton(
        label: 'Qayta ulanish',
        icon: Icons.refresh_rounded,
        onPressed: () async {
          _c.reset();
          await connectFlow(context);
        },
      );
    }
    if (_c.photo == null) {
      return VButton(
        label: 'Uzish',
        kind: VButtonKind.secondary,
        onPressed: () async {
          _c.reset();
          await _s.disconnect();
        },
      );
    }
    return Row(
      children: [
        Expanded(child: VButton(label: 'Ulashish', kind: VButtonKind.secondary, onPressed: _share)),
        const SizedBox(width: 10),
        Expanded(
          child: VButton(
            label: _c.savedLast ? 'Saqlandi' : 'Saqlash',
            icon: _c.savedLast ? Icons.check_rounded : null,
            onPressed: _c.savedLast ? null : _save,
          ),
        ),
      ],
    );
  }
}

class _ChoiceCard extends StatelessWidget {
  const _ChoiceCard({required this.icon, required this.title, required this.subtitle, required this.onTap});
  final IconData icon;
  final String title;
  final String subtitle;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return VPressable(
      onTap: onTap,
      semanticLabel: title,
      child: VSurface(
        color: v.raised,
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            VIconTile(icon: icon, size: 40, background: v.bgAlt),
            const SizedBox(height: 22),
            Text(title, style: v.text(16, FontWeight.w600, tracking: -0.01)),
            const SizedBox(height: 2),
            Text(subtitle, style: v.caption),
          ],
        ),
      ),
    );
  }
}

/// A gentle up-down float for the waiting arrow (still under reduced motion).
class _Bob extends StatefulWidget {
  const _Bob({required this.child});
  final Widget child;

  @override
  State<_Bob> createState() => _BobState();
}

class _BobState extends State<_Bob> with SingleTickerProviderStateMixin {
  late final AnimationController _c = AnimationController(vsync: this, duration: const Duration(milliseconds: 1400));

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (context.reducedMotion) {
      _c.stop();
    } else if (!_c.isAnimating) {
      _c.repeat(reverse: true);
    }
  }

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: _c,
    builder: (_, child) => Transform.translate(
      offset: Offset(0, 6 * Curves.easeInOut.transform(_c.value)),
      child: child,
    ),
    child: widget.child,
  );
}
