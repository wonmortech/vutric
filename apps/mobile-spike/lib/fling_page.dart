// Sending from the phone. Throw one photo upwards and it lands on the other device (a computer running
// `vutric-overlay-spike --listen`, or a phone on its catch screen); the number shown is release → the receiver's
// first frame, there and back. Or send whole files, as many as you like: photos and videos from the gallery, or
// any file from the Files app, with progress, resume and a hash check on every file.

import 'dart:math' as math;

import 'package:flutter/services.dart';
import 'package:image_picker/image_picker.dart';
import 'package:material_ui/material_ui.dart';

import 'src/rust/api.dart' as vutric;
import 'transfer_ui.dart';
import 'ui.dart';

/// Upward release speed (logical px/s) that counts as a throw; slower drags spring back.
const _throwSpeed = 700.0;

/// Phase 0 end-to-end criterion: release → first frame on the receiver.
const _passMs = 100.0;

class FlingPage extends StatefulWidget {
  const FlingPage({super.key, required this.scan, required this.link});

  /// Opens the QR scanner and returns a `vutric://pair?` link, or null.
  final Future<String?> Function() scan;

  /// A link typed on the first screen, if any.
  final String link;

  @override
  State<FlingPage> createState() => _FlingPageState();
}

class _Throw {
  _Throw(this.name, this.rustMs, this.appMs);
  final String name;
  final double rustMs;
  final double appMs;
}

class _FlingPageState extends State<FlingPage> with SingleTickerProviderStateMixin {
  vutric.Connected? _peer;
  vutric.Photo? _photo;
  String? _status;
  String? _error;
  bool _connecting = false;
  bool _throwing = false;
  bool _preparing = false;
  bool _sending = false;
  String? _sent;
  final _throws = <_Throw>[];

  /// Where the card is, relative to the middle of the stage; driven by the finger or by [_move].
  Offset _pos = Offset.zero;
  late final AnimationController _move = AnimationController(vsync: this)..addListener(_onMove);
  Animation<Offset>? _path;
  CurvedAnimation? _curve;

  /// The stage's size at the last layout (the Yuborish button throws across the real stage).
  Size _stage = const Size(360, 600);

  @override
  void dispose() {
    _curve?.dispose();
    _move.dispose();
    disconnectNow(); // also cancels a connect or a throw still waiting
    super.dispose();
  }

  void _onMove() {
    final p = _path;
    if (p != null) setState(() => _pos = p.value);
  }

  void _animateTo(Offset target, Duration duration, Curve curve) {
    _curve?.dispose();
    _curve = CurvedAnimation(parent: _move, curve: curve);
    _path = Tween(begin: _pos, end: target).animate(_curve!);
    _move.duration = duration;
    _move.forward(from: 0);
  }

  void _springBack() => _animateTo(Offset.zero, const Duration(milliseconds: 320), Curves.easeOutBack);

  Future<void> _connect(String label, Future<vutric.Connected> Function() job) async {
    setState(() {
      _connecting = true;
      _status = label;
      _error = null;
    });
    try {
      final c = await afterDisconnect(job);
      if (!mounted) return;
      setState(() => _peer = c);
    } catch (e) {
      if (!mounted || isCancelled(e)) return;
      setState(() => _error = errorText(e));
    } finally {
      if (mounted) {
        setState(() {
          _connecting = false;
          _status = null;
        });
      }
    }
  }

  Future<void> _scanAndConnect() async {
    final link = await widget.scan();
    if (link == null || !mounted) return;
    await _connect('Ulanmoqda…', () => vutric.connectLink(link: link));
  }

  Future<void> _pick() async {
    setState(() {
      _preparing = true;
      _error = null;
    });
    try {
      final file = await ImagePicker().pickImage(source: ImageSource.gallery, maxWidth: 2048, maxHeight: 2048);
      if (file == null || !mounted) return;
      final photo = await vutric.preparePhoto(fileName: file.name, bytes: await file.readAsBytes());
      if (!mounted) return;
      _move.stop();
      setState(() {
        _photo = photo;
        _pos = Offset.zero;
      });
    } catch (e) {
      if (mounted) setState(() => _error = 'Rasmni ochib boʻlmadi: ${errorText(e)}');
    } finally {
      if (mounted) setState(() => _preparing = false);
    }
  }

  /// Picks files and sends them whole, with a progress sheet.
  Future<void> _sendPicked(Future<Picked> Function() pick) async {
    final peer = _peer;
    if (peer == null) {
      setState(() => _error = 'Avval qurilmaga ulaning');
      return;
    }
    setState(() {
      _sending = true;
      _error = null;
    });
    try {
      final Picked picked;
      try {
        picked = await pick();
      } catch (e) {
        if (mounted) setState(() => _error = 'Tanlab boʻlmadi: ${errorText(e)}');
        return;
      }
      if (picked.isEmpty || !mounted) return;
      final r = await runTransfer(
        context,
        title: '${peer.peerName} ga yuborilmoqda',
        stream: vutric.sendFiles(paths: picked.paths, names: picked.names),
      );
      if (!mounted) return;
      if (r != null) {
        setState(() => _sent = doneText(r, sending: true));
      } else if ((await vutric.currentRoute()).isEmpty && mounted) {
        setState(() => _peer = null); // the session is gone; connect again
      }
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  bool get _canThrow => _photo != null && _peer != null && !_throwing;

  void _release(DragEndDetails d, Size area) {
    final v = d.velocity.pixelsPerSecond;
    // A throw goes mostly upwards and fast; anything else springs back.
    if (!_canThrow || -v.dy < _throwSpeed || -v.dy < v.dx.abs()) {
      if (_canThrow || _photo != null) _springBack();
      if (_peer == null && -v.dy >= _throwSpeed) {
        setState(() => _error = 'Avval qurilmaga ulaning');
      }
      return;
    }
    _throw(v, area);
  }

  Future<void> _throw(Offset v, Size area) async {
    final photo = _photo!;
    final clock = Stopwatch()..start();
    HapticFeedback.mediumImpact();
    final exitX = ((area.width / 2 + _pos.dx) / area.width).clamp(0.0, 1.0);
    setState(() {
      _throwing = true;
      _error = null;
    });
    _animateTo(Offset(_pos.dx + v.dx * 0.25, -area.height * 1.2), const Duration(milliseconds: 260), Curves.easeIn);
    try {
      final rust = await vutric.fling(photo: photo, speed: v.distance, angle: math.atan2(v.dx, -v.dy), exitX: exitX);
      clock.stop();
      if (!mounted) return;
      setState(() => _throws.insert(0, _Throw(photo.fileName, rust, clock.elapsedMicroseconds / 1000)));
    } catch (e) {
      if (!mounted || isCancelled(e)) return;
      // The receiver said no to this photo; the session itself is still fine.
      final refused = '$e'.contains('the other device refused the photo');
      setState(() {
        _error = errorText(e);
        if (!refused) _peer = null; // the session is gone; connect again
      });
    } finally {
      if (mounted) {
        // The photo comes back from below, ready for the next throw.
        await Future<void>.delayed(const Duration(milliseconds: 350));
        if (mounted) {
          setState(() {
            _throwing = false;
            _pos = Offset(0, area.height);
          });
          _animateTo(Offset.zero, const Duration(milliseconds: 380), Curves.easeOutCubic);
        }
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context).textTheme;
    final peer = _peer;
    return Scaffold(
      appBar: AppBar(backgroundColor: sand, title: const Text('Yuborish')),
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (peer != null)
                _PeerBar(peer: peer)
              else if (_connecting) ...[
                Text(_status ?? '', style: t.bodyMedium?.copyWith(color: cobalt)),
                const SizedBox(height: 8),
                const LinearProgressIndicator(color: signal),
                const SizedBox(height: 8),
                OutlinedButton(onPressed: disconnectNow, child: const Text('Bekor qilish')),
              ] else ...[
                Text(
                  'Qabul qiluvchi: kompyuterda `vutric-overlay-spike --listen` yoki boshqa telefonda '
                  '“Rasm qabul qilish”.',
                  style: t.bodyMedium,
                ),
                const SizedBox(height: 12),
                FilledButton(onPressed: _scanAndConnect, child: const Text('QR kodni skanerlab ulanish')),
                if (hasMdns) ...[
                  const SizedBox(height: 8),
                  OutlinedButton(
                    onPressed: () => _connect('Tarmoqdan qidirilmoqda (mDNS)…', vutric.connectDiscovered),
                    child: const Text('Tarmoqdan topish'),
                  ),
                ],
                if (widget.link.trim().isNotEmpty) ...[
                  const SizedBox(height: 8),
                  OutlinedButton(
                    onPressed: () => _connect('Ulanmoqda…', () => vutric.connectLink(link: widget.link)),
                    child: const Text('Kiritilgan havola orqali ulanish'),
                  ),
                ],
              ],
              if (_error != null)
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: Text(_error!, style: t.bodyMedium?.copyWith(color: err)),
                ),
              const SizedBox(height: 12),
              Expanded(child: LayoutBuilder(builder: (context, box) => _buildStage(context, _stage = box.biggest))),
              const SizedBox(height: 8),
              Row(
                children: [
                  Expanded(
                    child: OutlinedButton(
                      onPressed: _preparing || _throwing ? null : _pick,
                      child: Text(_photo == null ? 'Rasm tanlash' : 'Boshqa rasm'),
                    ),
                  ),
                  const SizedBox(width: 8),
                  // The same throw without the gesture (accessibility, and for a steady measurement).
                  Expanded(
                    child: FilledButton.icon(
                      onPressed: _canThrow ? () => _throw(const Offset(0, -1800), _stage) : null,
                      icon: const Icon(Icons.north),
                      label: const Text('Yuborish'),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 8),
              Row(
                children: [
                  Expanded(
                    child: OutlinedButton.icon(
                      onPressed: _peer == null || _sending || _throwing ? null : () => _sendPicked(pickMedia),
                      icon: const Icon(Icons.photo_library_outlined),
                      label: const Text('Galereyadan'),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: OutlinedButton.icon(
                      onPressed: _peer == null || _sending || _throwing ? null : () => _sendPicked(pickAnyFiles),
                      icon: const Icon(Icons.folder_open_outlined),
                      label: const Text('Fayllar'),
                    ),
                  ),
                ],
              ),
              if (_sent != null)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: Text(_sent!, style: Theme.of(context).textTheme.bodySmall?.copyWith(color: ok)),
                ),
              if (_throws.isNotEmpty) ...[
                const SizedBox(height: 8),
                for (final th in _throws.take(4))
                  Text(
                    '${th.name}: qoʻyib yuborish → u yerda birinchi kadr ${th.appMs.toStringAsFixed(1)} ms, '
                    'borib-qaytib (tarmoq ${th.rustMs.toStringAsFixed(1)} ms)',
                    style: t.bodySmall?.copyWith(color: th.appMs < _passMs ? ok : err),
                  ),
              ],
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildStage(BuildContext context, Size area) {
    final photo = _photo;
    if (photo == null) {
      return Center(
        child: _preparing
            ? const CircularProgressIndicator(color: signal)
            : Text(
                'Rasm tanlang, keyin uni barmoq bilan yuqoriga oting',
                style: Theme.of(context).textTheme.bodyMedium,
                textAlign: TextAlign.center,
              ),
      );
    }
    final aspect = photo.height == 0 ? 1.0 : photo.width / photo.height;
    final maxSide = math.min(area.width, area.height) * 0.75;
    final w = aspect >= 1 ? maxSide : maxSide * aspect;
    final h = aspect >= 1 ? maxSide / aspect : maxSide;
    final dpr = MediaQuery.devicePixelRatioOf(context);
    return Stack(
      children: [
        Positioned(
          left: (area.width - w) / 2 + _pos.dx,
          top: (area.height - h) / 2 + _pos.dy,
          width: w,
          height: h,
          child: Semantics(
            label: 'Tanlangan rasm ${photo.fileName}. Yuborish uchun yuqoriga suring',
            child: GestureDetector(
              onPanStart: _throwing
                  ? null
                  : (_) {
                      _move.stop();
                      _path = null;
                    },
              onPanUpdate: _throwing ? null : (d) => setState(() => _pos += d.delta),
              onPanEnd: _throwing ? null : (d) => _release(d, area),
              onPanCancel: _throwing ? null : _springBack,
              child: Container(
                decoration: BoxDecoration(
                  color: paper,
                  borderRadius: BorderRadius.circular(16),
                  boxShadow: const [BoxShadow(color: Color(0x33062B8A), blurRadius: 24, offset: Offset(0, 8))],
                ),
                clipBehavior: Clip.antiAlias,
                child: Image.memory(
                  photo.jpeg,
                  fit: BoxFit.cover,
                  gaplessPlayback: true,
                  cacheWidth: (w * dpr).round(),
                ),
              ),
            ),
          ),
        ),
      ],
    );
  }
}

class _PeerBar extends StatelessWidget {
  const _PeerBar({required this.peer});

  final vutric.Connected peer;

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context).textTheme;
    return Container(
      color: paper,
      padding: const EdgeInsets.all(12),
      child: Row(
        children: [
          const Icon(Icons.devices_outlined, color: signal),
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
    );
  }
}
