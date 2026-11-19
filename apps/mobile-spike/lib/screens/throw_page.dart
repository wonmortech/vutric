// S3 — Rasm otish, the key screen, in all 9 states of the handoff:
//   1 ulanmagan · 2 ulanmoqda (ConnectingPage) · 3 ulangan, rasm yoʻq · 4 rasm tayyor · 5 sudrash paytida ·
//   6 uchib ketdi · 7 yetib bordi · 8 rad etildi · 9 javob yoʻq / aloqa uzildi.
// The card follows the finger 1:1, tilts dx × 0.05°, shrinks up to 14% as it rises and opens the portal glow at the
// top; it is thrown past −150 px or 0.5 px/ms, leaves in 280 ms and comes back from below in 560 ms (easeOutBack).
// Under reduced motion every movement becomes a 200 ms fade. The card never sits under the device pill; results and
// errors appear at the bottom, next to the thumb.

import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/services.dart';
import 'package:image_picker/image_picker.dart';
import 'package:material_ui/material_ui.dart';
import 'package:photo_manager/photo_manager.dart';

import '../companion.dart';
import '../design/tokens.dart';
import '../design/widgets.dart';
import '../i18n.dart';
import '../session.dart';
import '../src/rust/api.dart' as vutric;
import '../ui.dart';
import 'connect.dart';
import 'device_page.dart';

enum _Phase { idle, flying, arrived, rejected, timeout }

class _Sent {
  _Sent(this.jpeg, this.ms);
  final Uint8List jpeg;
  final double ms;
}

class ThrowPage extends StatefulWidget {
  const ThrowPage({super.key, this.asset});

  /// A photo already chosen in the gallery; otherwise the user picks one here.
  final AssetEntity? asset;

  @override
  State<ThrowPage> createState() => _ThrowPageState();
}

/// Where the card is drawn: offset from the stage centre, tilt in degrees, scale and opacity.
class _Pose {
  const _Pose(this.pos, {this.rot = 0, this.scale = 1, this.opacity = 1});
  final Offset pos;
  final double rot;
  final double scale;
  final double opacity;
  static const rest = _Pose(Offset.zero);

  static _Pose lerp(_Pose a, _Pose b, double t) => _Pose(
    Offset.lerp(a.pos, b.pos, t)!,
    rot: a.rot + (b.rot - a.rot) * t,
    scale: a.scale + (b.scale - a.scale) * t,
    opacity: (a.opacity + (b.opacity - a.opacity) * t).clamp(0.0, 1.0),
  );
}

class _ThrowPageState extends State<ThrowPage> with SingleTickerProviderStateMixin {
  final _s = Session.instance;
  vutric.Photo? _photo;
  bool _preparing = false;
  _Phase _phase = _Phase.idle;
  String? _error;
  double? _lastMs;
  final _sent = <_Sent>[];

  bool _dragging = false;
  _Pose _pose = _Pose.rest;
  late final AnimationController _anim = AnimationController(vsync: this)..addListener(_tick);
  _Pose _from = _Pose.rest;
  _Pose _to = _Pose.rest;
  Curve _curve = Curves.linear;
  Size _stage = const Size(390, 520);

  @override
  void initState() {
    super.initState();
    _s.addListener(_onSession);
    final a = widget.asset;
    if (a != null) _loadAsset(a);
  }

  @override
  void dispose() {
    _s.removeListener(_onSession);
    _anim.dispose();
    super.dispose();
  }

  void _onSession() {
    if (mounted) setState(() {});
  }

  void _tick() => setState(() => _pose = _Pose.lerp(_from, _to, _curve.transform(_anim.value)));

  Future<void> _animate(_Pose to, Duration d, Curve curve, {_Pose? from}) {
    _from = from ?? _pose;
    _to = to;
    _curve = curve;
    _anim.duration = d;
    return _anim.forward(from: 0).orCancel.catchError((Object _) {});
  }

  bool get _reduced => context.reducedMotion;

  // ── Photo ──────────────────────────────────────────────────────────────────────────────────────────────────────

  Future<void> _use(String name, Uint8List bytes) async {
    final photo = await vutric.preparePhoto(fileName: name, bytes: bytes);
    if (!mounted) return;
    _anim.stop();
    setState(() {
      _photo = photo;
      _phase = _Phase.idle;
      _error = null;
    });
    // A new photo rises into place.
    await _animate(
      _Pose.rest,
      _reduced ? VMotion.reduced : VMotion.open,
      VMotion.out,
      from: _reduced ? const _Pose(Offset.zero, opacity: 0) : const _Pose(Offset(0, 40), scale: 0.96, opacity: 0),
    );
  }

  Future<void> _loadAsset(AssetEntity a) async {
    setState(() => _preparing = true);
    try {
      final bytes = await a.thumbnailDataWithSize(const ThumbnailSize(2048, 2048), quality: 90);
      if (bytes == null) throw StateError('rasm oʻqilmadi');
      await _use(await a.titleAsync, bytes);
    } catch (e) {
      if (mounted) setState(() => _error = 'Rasmni ochib boʻlmadi: ${errorText(e)}');
    } finally {
      if (mounted) setState(() => _preparing = false);
    }
  }

  Future<void> _pick() async {
    setState(() => _preparing = true);
    try {
      final f = await ImagePicker().pickImage(source: ImageSource.gallery, maxWidth: 2048, maxHeight: 2048);
      if (f == null || !mounted) return;
      await _use(f.name, await f.readAsBytes());
    } catch (e) {
      if (mounted) setState(() => _error = 'Rasmni ochib boʻlmadi: ${errorText(e)}');
    } finally {
      if (mounted) setState(() => _preparing = false);
    }
  }

  // ── Gesture ────────────────────────────────────────────────────────────────────────────────────────────────────

  bool get _canThrow => _photo != null && _s.peer != null && _phase != _Phase.flying;

  /// The live pose for a finger offset: tilt dx × 0.05° − up × 0.02°, shrink up to 14% while rising.
  _Pose _poseFor(Offset d) {
    final up = math.max(0.0, -d.dy);
    return _Pose(d, rot: d.dx * 0.05 - up * 0.02, scale: 1 - 0.14 * math.min(up / 400, 1));
  }

  void _panStart(DragStartDetails _) {
    if (_phase == _Phase.flying || _photo == null) return;
    _anim.stop();
    setState(() {
      _dragging = true;
      if (_phase != _Phase.idle && _s.peer != null) _phase = _Phase.idle;
      _error = null;
    });
  }

  void _panUpdate(DragUpdateDetails d) {
    if (!_dragging) return;
    setState(() => _pose = _poseFor(_pose.pos + d.delta));
  }

  void _panEnd(DragEndDetails d) {
    if (!_dragging) return;
    setState(() => _dragging = false);
    final v = d.velocity.pixelsPerSecond;
    final thrown = _pose.pos.dy < -VMotion.throwDistance || v.dy < -VMotion.throwSpeed;
    if (thrown && _canThrow) {
      _throw(v);
      return;
    }
    if (thrown && _s.peer == null) setState(() => _error = 'Avval kompyuterga ulaning');
    _animate(_Pose.rest, _reduced ? VMotion.reduced : VMotion.returnDuration, VMotion.returnBack);
  }

  /// The portal at the top edge opens as the card rises: min(up / 170, 1).
  double get _portal {
    if (_phase == _Phase.flying) return 1 - _anim.value * 0.6;
    return (math.max(0.0, -_pose.pos.dy) / 170).clamp(0.0, 1.0);
  }

  // ── Throw ──────────────────────────────────────────────────────────────────────────────────────────────────────

  Future<void> _throw(Offset velocity) async {
    final photo = _photo!;
    final peerName = _s.peer!.peerName;
    final clock = Stopwatch()..start();
    HapticFeedback.lightImpact();
    final exitX = ((_stage.width / 2 + _pose.pos.dx) / _stage.width).clamp(0.0, 1.0);
    setState(() {
      _phase = _Phase.flying;
      _error = null;
    });
    // Out over everything: up past the top edge, carrying the sideways speed.
    final out = _reduced
        ? _Pose(_pose.pos, rot: _pose.rot, scale: _pose.scale, opacity: 0)
        : _Pose(
            Offset(_pose.pos.dx + velocity.dx * 0.12, -_stage.height),
            rot: _pose.rot + velocity.dx * 0.004,
            scale: 0.7,
          );
    unawaited(_animate(out, _reduced ? VMotion.reduced : VMotion.throwDuration, VMotion.throwOut));
    _Phase result;
    String? error;
    try {
      final speed = velocity.distance == 0 ? 1800.0 : velocity.distance;
      await _s.exclusive(
        () => vutric.fling(photo: photo, speed: speed, angle: math.atan2(velocity.dx, -velocity.dy), exitX: exitX),
      );
      clock.stop();
      final ms = clock.elapsedMicroseconds / 1000;
      _lastMs = ms;
      result = _Phase.arrived;
      _sent.insert(0, _Sent(photo.jpeg, ms));
      if (_sent.length > 5) _sent.removeLast();
      unawaited(
        _s.addHistory(
          HistoryItem(
            sent: true,
            peer: peerName,
            files: 1,
            bytes: photo.jpeg.length,
            seconds: ms / 1000,
            at: DateTime.now(),
            names: [photo.fileName],
            ms: ms,
          ),
        ),
      );
    } catch (e) {
      if (!mounted || isCancelled(e)) return;
      final refused = '$e'.contains('refused');
      result = refused ? _Phase.rejected : _Phase.timeout;
      error = errorText(e);
      if (!refused) _s.lost();
    }
    if (!mounted) return;
    HapticFeedback.mediumImpact();
    setState(() {
      _phase = result;
      _error = error;
    });
    // Back from below, ready for the next throw (a fade under reduced motion).
    await _animate(
      _Pose.rest,
      _reduced ? VMotion.reduced : VMotion.returnDuration,
      VMotion.returnBack,
      from: _reduced
          ? const _Pose(Offset.zero, opacity: 0)
          : _Pose(Offset(0, _stage.height * 0.9), rot: 4, scale: 0.92),
    );
  }

  Future<void> _connect() async {
    setState(() => _error = null);
    final c = await connectFlow(context);
    if (c != null && mounted) setState(() => _phase = _Phase.idle);
  }

  // ── Build ──────────────────────────────────────────────────────────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final peer = _s.peer;
    final lost = peer == null && _s.lostPeer != null;
    return Scaffold(
      body: Stack(
        children: [
          // The portal: a glow from the top edge that opens as the card rises.
          Positioned(
            top: 0,
            left: 0,
            right: 0,
            height: 220,
            child: IgnorePointer(
              child: Opacity(
                opacity: _portal.clamp(0.0, 1.0),
                child: DecoratedBox(
                  decoration: BoxDecoration(
                    gradient: RadialGradient(
                      center: const Alignment(0, -1.4),
                      radius: 1.1,
                      colors: [v.accent.withValues(alpha: 0.55), v.accent.withValues(alpha: 0)],
                    ),
                  ),
                ),
              ),
            ),
          ),
          SafeArea(
            child: peer == null && !lost
                ? _notConnected(v)
                : Column(
                    children: [
                      const SizedBox(height: 8),
                      _topBar(v, peer, lost),
                      SizedBox(
                        height: 26,
                        child: AnimatedOpacity(
                          opacity: _dragging && _pose.pos.dy < -VMotion.throwDistance ? 1 : 0,
                          duration: VMotion.fast,
                          child: Center(
                            child: Text('Qoʻyib yuboring', style: v.text(13.5, FontWeight.w600, color: v.accent)),
                          ),
                        ),
                      ),
                      Expanded(
                        child: LayoutBuilder(
                          builder: (context, box) {
                            _stage = box.biggest;
                            return _photo == null ? _empty(v) : _card(v, box.biggest, lost);
                          },
                        ),
                      ),
                      _bottom(v, lost),
                    ],
                  ),
          ),
        ],
      ),
    );
  }

  Widget _topBar(VutricTokens v, vutric.Connected? peer, bool lost) {
    return Row(
      children: [
        const SizedBox(width: 16),
        VCircleButton(icon: Icons.chevron_left_rounded, onTap: () => Navigator.of(context).maybePop(), label: 'Orqaga'),
        Expanded(
          child: Center(
            child: lost
                ? VDevicePill(name: _s.lostPeer!, lost: true)
                : VDevicePill(
                    name: peer!.peerName,
                    platform: peer.peerPlatform,
                    code: peer.checkCode,
                    onTap: () => showPeerSheet(context, peer),
                  ),
          ),
        ),
        const SizedBox(width: 52),
      ],
    );
  }

  /// S3-1: not connected.
  Widget _notConnected(VutricTokens v) {
    return Column(
      children: [
        const VNavHeader(title: 'Rasm otish'),
        const Expanded(
          child: VEmpty(
            icon: Icons.devices_rounded,
            title: 'Kompyuterga ulaning',
            body: 'Ikkala qurilma bitta Wi-Fi tarmogʻida boʻlsin. Internet kerak emas.',
          ),
        ),
        if (_error != null)
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
            child: VNotice(title: _error!),
          ),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
          child: Column(
            children: [
              VButton(
                label: 'QR kodni skanerlash',
                icon: Icons.qr_code_scanner_rounded,
                onPressed: () => connectFlow(context, way: ConnectWay.scan),
              ),
              const SizedBox(height: 10),
              VButton(
                label: hasMdns ? 'Tarmoqdan topish' : 'Boshqa usul',
                kind: VButtonKind.secondary,
                tag: hasMdns ? 'Android' : null,
                onPressed: () => connectFlow(context, way: hasMdns ? ConnectWay.find : null),
              ),
            ],
          ),
        ),
      ],
    );
  }

  /// S3-3: connected, no photo yet.
  Widget _empty(VutricTokens v) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(28, 8, 28, 12),
      child: CustomPaint(
        painter: DashedBorder(color: v.lineStrong, radius: v.rCard),
        child: Center(
          child: _preparing
              ? CircularProgressIndicator(color: v.accent)
              : Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    const VIconTile(icon: Icons.image_outlined, size: 52),
                    const SizedBox(height: 16),
                    Text('Rasm tanlang', style: v.title),
                    const SizedBox(height: 6),
                    Text('Galereyadan yoki kameradan', style: v.caption),
                  ],
                ),
        ),
      ),
    );
  }

  Widget _card(VutricTokens v, Size area, bool lost) {
    final p = _photo!;
    final aspect = p.height == 0 ? 0.75 : p.width / p.height;
    // 250 × 334 on a 393-wide phone, kept inside the stage and true to the photo's shape.
    final maxW = math.min(area.width * 0.64, 300.0);
    final maxH = area.height * 0.82;
    var w = maxW;
    var h = w / aspect;
    if (h > maxH) {
      h = maxH;
      w = h * aspect;
    }
    final dpr = MediaQuery.devicePixelRatioOf(context);
    return Stack(
      clipBehavior: Clip.none,
      children: [
        Positioned(
          left: (area.width - w) / 2 + _pose.pos.dx,
          top: (area.height - h) / 2 + _pose.pos.dy,
          width: w,
          height: h,
          child: Opacity(
            opacity: _pose.opacity * (lost ? 0.55 : 1),
            child: Transform.rotate(
              angle: _pose.rot * math.pi / 180,
              child: Transform.scale(
                scale: _pose.scale,
                child: Semantics(
                  label: 'Tanlangan rasm ${p.fileName}. Yuborish uchun yuqoriga suring',
                  child: GestureDetector(
                    onPanStart: _panStart,
                    onPanUpdate: _panUpdate,
                    onPanEnd: _panEnd,
                    onPanCancel: () {
                      if (_dragging) _panEnd(DragEndDetails());
                    },
                    child: VPhotoCard(
                      lifted: _dragging ? 1 : 0,
                      child: Image.memory(
                        p.jpeg,
                        fit: BoxFit.cover,
                        gaplessPlayback: true,
                        cacheWidth: (w * dpr).round(),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ),
        ),
      ],
    );
  }

  Widget _bottom(VutricTokens v, bool lost) {
    final flying = _phase == _Phase.flying;
    final Widget status;
    if (_dragging) {
      status = Text(
        'dy ${_pose.pos.dy.toStringAsFixed(0)} px · ogʻish ${_pose.rot.toStringAsFixed(0)}° · ${(_pose.scale * 100).round()}%',
        style: v.monoSmall,
      );
    } else if (flying) {
      status = const VResultPill(text: 'Yuborilmoqda…', busy: true);
    } else if (_phase == _Phase.arrived) {
      status = Column(
        children: [
          VResultPill(text: 'Yetib bordi', mono: '${_lastMs!.round()} ms'),
          const SizedBox(height: 10),
          _SentStrip(sent: _sent),
        ],
      );
    } else if (_phase == _Phase.rejected) {
      status = VNotice(
        title: 'Kompyuter rasmni qabul qilmadi',
        body: '${_s.peer?.peerName ?? 'Kompyuter'}’da «Rad etish» bosildi. Sessiya saqlandi — yana otishingiz mumkin.',
      );
    } else if (lost) {
      status = VNotice(
        title: 'Javob yoʻq',
        body: 'Kompyuter uxlab qolgan yoki boshqa Wi-Fi’ga oʻtgan boʻlishi mumkin.${_error == null ? '' : '\n$_error'}',
      );
    } else if (_error != null) {
      status = VNotice(title: _error!, onClose: () => setState(() => _error = null));
    } else if (_photo != null) {
      status = const _RiseHint();
    } else {
      status = const SizedBox.shrink();
    }

    final Widget buttons;
    if (lost) {
      buttons = VButton(label: 'Qayta ulanish', icon: Icons.refresh_rounded, onPressed: _connect);
    } else if (_photo == null) {
      buttons = VButton(
        label: 'Galereyani ochish',
        icon: Icons.photo_library_outlined,
        busy: _preparing,
        onPressed: _pick,
      );
    } else {
      buttons = Row(
        children: [
          Expanded(
            flex: 10,
            child: VButton(
              label: 'Boshqa rasm',
              kind: VButtonKind.secondary,
              onPressed: flying || _preparing ? null : _pick,
            ),
          ),
          const SizedBox(width: 10),
          Expanded(
            flex: 14,
            child: VButton(
              label: _phase == _Phase.rejected ? 'Yana otish' : 'Yuborish',
              icon: _phase == _Phase.rejected ? null : Icons.arrow_upward_rounded,
              onPressed: _canThrow && !_dragging ? () => _throw(const Offset(0, -1800)) : null,
            ),
          ),
        ],
      );
    }
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          AnimatedSwitcher(
            duration: VMotion.fast,
            child: KeyedSubtree(
              key: ValueKey('${_phase.name}$lost$_dragging${_error != null}${_photo != null}'),
              child: status,
            ),
          ),
          const SizedBox(height: 12),
          buttons,
        ],
      ),
    );
  }
}

/// The connected device's details: the route badge (ADR-0060, live), its check code in large type, its own
/// settings (T3), the clipboard action when turned on (A12), and "Uzish".
Future<void> showPeerSheet(BuildContext context, vutric.Connected peer) async {
  final action = await showModalBottomSheet<String>(
    context: context,
    builder: (ctx) => PeerSheet(peer: peer),
  );
  if (!context.mounted) return;
  switch (action) {
    case 'close':
      await Session.instance.disconnect();
    case 'device':
      final d = (await vutric.devices()).where((d) => d.id == peer.peerId).firstOrNull;
      if (d != null && context.mounted) {
        await Navigator.of(context).push(MaterialPageRoute<bool>(builder: (_) => DevicePage(device: d)));
      }
    case 'clipboard':
      await sendClipboard(context);
  }
}

class PeerSheet extends StatelessWidget {
  const PeerSheet({super.key, required this.peer});
  final vutric.Connected peer;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final s = Session.instance;
    return ListenableBuilder(
      listenable: s,
      builder: (context, _) => SafeArea(
        child: SingleChildScrollView(
          padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(peer.peerName, style: v.title),
              const SizedBox(height: 6),
              Wrap(
                spacing: 8,
                runSpacing: 6,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  Text(platformName(peer.peerPlatform), style: v.caption),
                  RouteBadge(badge: s.badge.isEmpty ? peer.route : s.badge),
                ],
              ),
              const SizedBox(height: 16),
              Text('Tekshiruv kodi', style: v.label),
              Text(peer.checkCode, style: v.text(34, FontWeight.w600, mono: true, tracking: 0.08)),
              Text('Ikkala ekranda bir xil boʻlishi kerak.', style: v.caption),
              const SizedBox(height: 16),
              if (peer.trusted) ...[
                VRowCard(
                  icon: Icons.tune_rounded,
                  title: tr('devices.rules'),
                  subtitle: tr('devices.rules.hint'),
                  onTap: () => Navigator.pop(context, 'device'),
                ),
                const SizedBox(height: 10),
              ],
              if (s.clipboardButton && peer.trusted) ...[
                VButton(
                  label: tr('companion.clipboard.send'),
                  icon: Icons.content_paste_go_rounded,
                  kind: VButtonKind.secondary,
                  onPressed: () => Navigator.pop(context, 'clipboard'),
                ),
                const SizedBox(height: 10),
              ],
              VButton(label: 'Uzish', kind: VButtonKind.secondary, onPressed: () => Navigator.pop(context, 'close')),
            ],
          ),
        ),
      ),
    );
  }
}

/// The route badge (ADR-0060): what the packets travel over now, never a promise.
class RouteBadge extends StatelessWidget {
  const RouteBadge({super.key, required this.badge});
  final String badge;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final icon = switch (badge) {
      'route.badge.direct_lan' || 'direct' => Icons.router_outlined,
      'route.badge.direct_internet' => Icons.public_rounded,
      'route.badge.relay' || 'relay' => Icons.cloud_outlined,
      'route.badge.hotspot' => Icons.wifi_tethering_rounded,
      'route.badge.cable' => Icons.cable_rounded,
      _ => Icons.more_horiz_rounded,
    };
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
      decoration: BoxDecoration(color: v.bgAlt, borderRadius: BorderRadius.circular(v.rPill.clamp(0, 10))),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, size: 13, color: v.accent),
          const SizedBox(width: 5),
          Flexible(child: Text(routeLabel(badge), style: v.text(12, FontWeight.w600))),
        ],
      ),
    );
  }
}

/// "⌃ Yuborish uchun yuqoriga suring" with two chevrons rising (1.6 s, staggered 0.3 s).
class _RiseHint extends StatefulWidget {
  const _RiseHint();

  @override
  State<_RiseHint> createState() => _RiseHintState();
}

class _RiseHintState extends State<_RiseHint> with SingleTickerProviderStateMixin {
  late final AnimationController _c = AnimationController(vsync: this, duration: const Duration(milliseconds: 1600));

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (context.reducedMotion) {
      _c.stop();
    } else if (!_c.isAnimating) {
      _c.repeat();
    }
  }

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final reduced = context.reducedMotion;
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        SizedBox(
          width: 14,
          height: 20,
          child: AnimatedBuilder(
            animation: _c,
            builder: (context, _) => Stack(
              children: [
                for (final delay in const [0.0, 0.3 / 1.6])
                  () {
                    final t = ((_c.value - delay) % 1 + 1) % 1;
                    return Positioned(
                      top: reduced ? 3 : 8 - 8 * Curves.easeOut.transform(t),
                      left: 0,
                      child: Opacity(
                        opacity: reduced ? 1 : math.sin(t * math.pi),
                        child: Icon(Icons.keyboard_arrow_up_rounded, size: 14, color: v.accent),
                      ),
                    );
                  }(),
              ],
            ),
          ),
        ),
        const SizedBox(width: 6),
        Text('Yuborish uchun yuqoriga suring', style: v.text(13.5, FontWeight.w400, color: v.muted)),
      ],
    );
  }
}

/// The last thrown photos as small thumbnails, then "4 ta · 41 ms oʻrt.".
class _SentStrip extends StatelessWidget {
  const _SentStrip({required this.sent});
  final List<_Sent> sent;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    if (sent.length < 2) return const SizedBox.shrink();
    final avg = sent.map((s) => s.ms).reduce((a, b) => a + b) / sent.length;
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        for (final s in sent.take(4))
          Padding(
            padding: const EdgeInsets.only(right: 6),
            child: ClipRRect(
              borderRadius: BorderRadius.circular(8),
              child: Image.memory(
                s.jpeg,
                width: 30,
                height: 30,
                fit: BoxFit.cover,
                cacheWidth: 90,
                gaplessPlayback: true,
              ),
            ),
          ),
        const SizedBox(width: 4),
        Text('${sent.length} ta · ${avg.round()} ms oʻrt.', style: v.monoSmall),
      ],
    );
  }
}

/// A dashed rounded outline (empty drop areas, S3-3 and S4-3).
class DashedBorder extends CustomPainter {
  DashedBorder({required this.color, required this.radius});
  final Color color;
  final double radius;

  @override
  void paint(Canvas canvas, Size size) {
    final path = Path()..addRRect(RRect.fromRectAndRadius(Offset.zero & size, Radius.circular(radius)));
    final paint = Paint()
      ..color = color
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.5;
    for (final m in path.computeMetrics()) {
      for (var d = 0.0; d < m.length; d += 10) {
        canvas.drawPath(m.extractPath(d, math.min(d + 5, m.length)), paint);
      }
    }
  }

  @override
  bool shouldRepaint(DashedBorder old) => old.color != color || old.radius != radius;
}
