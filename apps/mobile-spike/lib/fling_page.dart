// The Phase 0 end-to-end check from the phone: pick a photo, throw it upwards, and it lands on the computer
// screen through the overlay (`vutric-overlay-spike --listen`). The number shown is release → the computer's
// first frame of the effect, there and back.

import 'dart:math' as math;

import 'package:flutter/services.dart';
import 'package:image_picker/image_picker.dart';
import 'package:material_ui/material_ui.dart';

import 'src/rust/api.dart' as vutric;

const _cobalt = Color(0xFF062B8A);
const _signal = Color(0xFF2F6BFF);
const _sand = Color(0xFFE6DBC7);
const _paper = Color(0xFFF7F2E9);
const _ok = Color(0xFF1D7A4E);
const _err = Color(0xFFA8321F);

/// Upward release speed (logical px/s) that counts as a throw; slower drags spring back.
const _throwSpeed = 700.0;

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
  Uint8List? _shown;
  String? _status;
  String? _error;
  bool _busy = false;
  final _throws = <_Throw>[];

  Offset _drag = Offset.zero;
  late final AnimationController _fly = AnimationController(vsync: this, duration: const Duration(milliseconds: 260));
  Animation<Offset>? _flight;

  @override
  void dispose() {
    _fly.dispose();
    vutric.disconnect();
    super.dispose();
  }

  Future<void> _connect(String label, Future<vutric.Connected> Function() job) async {
    setState(() {
      _busy = true;
      _status = label;
      _error = null;
    });
    try {
      final c = await job();
      setState(() {
        _peer = c;
        _status = null;
      });
    } catch (e) {
      setState(() {
        _error = '$e';
        _status = null;
      });
    } finally {
      setState(() => _busy = false);
    }
  }

  Future<void> _scanAndConnect() async {
    final link = await widget.scan();
    if (link == null || !mounted) return;
    await _connect('Ulanmoqda…', () => vutric.connectLink(link: link));
  }

  Future<void> _pick() async {
    final file = await ImagePicker().pickImage(
      source: ImageSource.gallery,
      maxWidth: 1024,
      maxHeight: 1024,
      imageQuality: 85,
    );
    if (file == null) return;
    try {
      final bytes = await file.readAsBytes();
      final photo = await vutric.preparePhoto(fileName: file.name, bytes: bytes);
      setState(() {
        _photo = photo;
        _shown = photo.jpeg;
        _error = null;
        _drag = Offset.zero;
        _flight = null;
      });
      _fly.reset();
    } catch (e) {
      setState(() => _error = 'Rasmni ochib boʻlmadi: $e');
    }
  }

  void _release(DragEndDetails d, Size area) {
    final v = d.velocity.pixelsPerSecond;
    final photo = _photo;
    if (photo == null || _peer == null || _busy || -v.dy < _throwSpeed) {
      setState(() => _drag = Offset.zero); // not a throw: back to the middle
      return;
    }
    final clock = Stopwatch()..start();
    HapticFeedback.mediumImpact();
    final speed = v.distance;
    final angle = math.atan2(v.dx, -v.dy);
    final exitX = ((area.width / 2 + _drag.dx) / area.width).clamp(0.0, 1.0);
    _flight = Tween(
      begin: _drag,
      end: Offset(_drag.dx + v.dx * 0.25, -area.height * 1.2),
    ).animate(CurvedAnimation(parent: _fly, curve: Curves.easeIn));
    _fly.forward(from: 0);
    setState(() => _busy = true);
    vutric
        .fling(photo: photo, speed: speed, angle: angle, exitX: exitX)
        .then((rust) {
          clock.stop();
          setState(() => _throws.insert(0, _Throw(photo.fileName, rust, clock.elapsedMicroseconds / 1000)));
        })
        .catchError((Object e) {
          setState(() => _error = '$e');
        })
        .whenComplete(() {
          // The photo comes back after a moment, ready for the next throw.
          Future.delayed(const Duration(milliseconds: 600), () {
            if (!mounted) return;
            _fly.reset();
            setState(() {
              _busy = false;
              _drag = Offset.zero;
              _flight = null;
            });
          });
        });
  }

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context).textTheme;
    final peer = _peer;
    return Scaffold(
      appBar: AppBar(backgroundColor: _sand, title: const Text('Rasm otish')),
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (peer == null) ...[
                Text('Kompyuterda `vutric-overlay-spike --listen` ishlab turgan boʻlsin.', style: t.bodyMedium),
                const SizedBox(height: 12),
                FilledButton(
                  onPressed: _busy ? null : _scanAndConnect,
                  child: const Text('QR kodni skanerlab ulanish'),
                ),
                const SizedBox(height: 8),
                OutlinedButton(
                  onPressed: _busy ? null : () => _connect('Qidirilmoqda (mDNS)…', vutric.connectDiscovered),
                  child: const Text('Tarmoqdan topish'),
                ),
                if (widget.link.trim().isNotEmpty) ...[
                  const SizedBox(height: 8),
                  OutlinedButton(
                    onPressed: _busy ? null : () => _connect('Ulanmoqda…', () => vutric.connectLink(link: widget.link)),
                    child: const Text('Kiritilgan havola orqali ulanish'),
                  ),
                ],
              ] else
                _PeerBar(peer: peer),
              if (_status != null)
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: Text(_status!, style: t.bodyMedium?.copyWith(color: _cobalt)),
                ),
              if (_error != null)
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: Text(_error!, style: t.bodyMedium?.copyWith(color: _err)),
                ),
              const SizedBox(height: 12),
              Expanded(child: LayoutBuilder(builder: (context, box) => _stage(context, box.biggest))),
              const SizedBox(height: 8),
              OutlinedButton(onPressed: _pick, child: Text(_photo == null ? 'Rasm tanlash' : 'Boshqa rasm')),
              if (_throws.isNotEmpty) ...[
                const SizedBox(height: 8),
                for (final th in _throws.take(4))
                  Text(
                    '${th.name}: qoʻyib yuborish → kompyuterda birinchi kadr ${th.appMs.toStringAsFixed(1)} ms '
                    '(tarmoq ${th.rustMs.toStringAsFixed(1)} ms)',
                    style: t.bodySmall?.copyWith(color: th.appMs < 100 ? _ok : _err),
                  ),
              ],
            ],
          ),
        ),
      ),
    );
  }

  Widget _stage(BuildContext context, Size area) {
    final shown = _shown;
    if (shown == null) {
      return Center(
        child: Text(
          'Rasm tanlang, keyin uni barmoq bilan yuqoriga oting',
          style: Theme.of(context).textTheme.bodyMedium,
          textAlign: TextAlign.center,
        ),
      );
    }
    final side = math.min(area.width, area.height) * 0.7;
    return AnimatedBuilder(
      animation: _fly,
      builder: (context, child) {
        final offset = _flight?.value ?? _drag;
        return Stack(
          children: [
            AnimatedPositioned(
              duration: _flight == null && _drag == Offset.zero ? const Duration(milliseconds: 220) : Duration.zero,
              curve: Curves.easeOutBack,
              left: (area.width - side) / 2 + offset.dx,
              top: (area.height - side) / 2 + offset.dy,
              width: side,
              height: side,
              child: child!,
            ),
          ],
        );
      },
      child: GestureDetector(
        onPanUpdate: _busy ? null : (d) => setState(() => _drag += d.delta),
        onPanEnd: _busy ? null : (d) => _release(d, area),
        child: Container(
          decoration: BoxDecoration(
            color: _paper,
            borderRadius: BorderRadius.circular(16),
            boxShadow: const [BoxShadow(color: Color(0x33062B8A), blurRadius: 24, offset: Offset(0, 8))],
          ),
          clipBehavior: Clip.antiAlias,
          child: Image.memory(shown, fit: BoxFit.cover, gaplessPlayback: true),
        ),
      ),
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
      color: _paper,
      padding: const EdgeInsets.all(12),
      child: Row(
        children: [
          const Icon(Icons.desktop_windows_outlined, color: _signal),
          const SizedBox(width: 12),
          Expanded(
            child: Text('${peer.peerName} (${peer.peerPlatform})', style: t.titleSmall?.copyWith(color: _cobalt)),
          ),
          Column(
            crossAxisAlignment: CrossAxisAlignment.end,
            children: [
              Text('tekshiruv kodi', style: t.labelSmall),
              Text(peer.checkCode, style: t.titleMedium?.copyWith(color: _cobalt, letterSpacing: 2)),
            ],
          ),
        ],
      ),
    );
  }
}
