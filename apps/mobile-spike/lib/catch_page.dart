// The other direction: this phone catches photos thrown by a computer or another phone. The thrower either scans
// this phone's QR code (we wait), or this phone scans the computer's QR code (`vutric-net-spike fling --photo
// PATH`). Each photo flies in from the top edge, the way it left the thrower, and the thrower's round trip
// closes once its first frame is on this screen.

import 'dart:math' as math;

import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';
import 'package:qr_flutter/qr_flutter.dart';

import 'src/rust/api.dart' as vutric;

const _cobalt = Color(0xFF062B8A);
const _signal = Color(0xFF2F6BFF);
const _sand = Color(0xFFE6DBC7);
const _paper = Color(0xFFF7F2E9);
const _err = Color(0xFFA8321F);

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
  String? _status;
  String? _error;
  bool _waiting = false;
  bool _accepting = false;
  bool _open = true;
  int _count = 0;
  double? _lastMs;

  late final AnimationController _land = AnimationController(vsync: this, duration: const Duration(milliseconds: 420));

  @override
  void dispose() {
    _open = false;
    _land.dispose();
    vutric.disconnect();
    super.dispose();
  }

  Future<void> _start(String label, Future<vutric.Connected> Function() job, {bool accepting = false}) async {
    setState(() {
      _waiting = true;
      _accepting = accepting;
      _status = label;
      _error = null;
    });
    try {
      final c = await job();
      if (!mounted) return;
      setState(() {
        _peer = c;
        _status = 'Ulandi. Rasm kutilmoqda…';
      });
      _catchLoop();
    } catch (e) {
      if (mounted) setState(() => _error = '$e');
    } finally {
      if (mounted) {
        setState(() {
          _waiting = false;
          _accepting = false;
        });
      }
    }
  }

  Future<void> _scanAndConnect() async {
    final link = await widget.scan();
    if (link == null || !mounted) return;
    await _start('Ulanmoqda…', () => vutric.connectLink(link: link));
  }

  /// One photo at a time: show it, wait for the frame that draws it, then tell the thrower.
  Future<void> _catchLoop() async {
    while (_open && mounted) {
      final vutric.Caught p;
      try {
        p = await vutric.nextFling();
      } catch (e) {
        if (mounted) {
          setState(() {
            _error = 'Aloqa uzildi: $e';
            _peer = null;
            _status = null;
          });
        }
        return;
      }
      if (!mounted) return;
      final clock = Stopwatch()..start();
      HapticFeedback.lightImpact();
      setState(() {
        _photo = p;
        _count++;
        _status = null;
      });
      _land.forward(from: 0);
      await SchedulerBinding.instance.endOfFrame;
      clock.stop();
      if (mounted) setState(() => _lastMs = clock.elapsedMicroseconds / 1000);
      try {
        await vutric.shown(id: p.id);
      } catch (e) {
        if (mounted) setState(() => _error = '$e');
        return;
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context).textTheme;
    final peer = _peer;
    return Scaffold(
      appBar: AppBar(backgroundColor: _sand, title: const Text('Rasm qabul qilish')),
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (peer == null) ...[
                FilledButton(
                  onPressed: _waiting ? null : _scanAndConnect,
                  child: const Text('Kompyuter QR kodini skanerlash'),
                ),
                const SizedBox(height: 8),
                OutlinedButton(
                  onPressed: _waiting
                      ? null
                      : () => _start('Ulanish kutilmoqda…', vutric.acceptSession, accepting: true),
                  child: const Text('Kutish: QR kodimni skanerlashsin'),
                ),
              ] else
                Container(
                  color: _paper,
                  padding: const EdgeInsets.all(12),
                  child: Row(
                    children: [
                      const Icon(Icons.south, color: _signal),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Text(
                          '${peer.peerName} (${peer.peerPlatform})',
                          style: t.titleSmall?.copyWith(color: _cobalt),
                        ),
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
                ),
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
              if (_count > 0)
                Text(
                  '$_count ta rasm keldi · oxirgisi ekranga ${_lastMs?.toStringAsFixed(1) ?? '–'} ms da chiqdi',
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
    if (p == null) {
      if (_accepting) {
        // Waiting to be found: the QR code the thrower scans.
        return Center(
          child: Container(
            color: _paper,
            padding: const EdgeInsets.all(12),
            child: QrImageView(data: widget.myLink, size: math.min(area.width, area.height) * 0.7),
          ),
        );
      }
      return Center(
        child: Text(
          'Kompyuterda: vutric-net-spike fling --photo RASM.jpg\nso‘ng uning QR kodini skanerlang',
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
        final dx = fromX * (1 - k);
        final dy = -(area.height / 2 + h) * (1 - k);
        final tilt = p.angle.clamp(-0.6, 0.6) * (1 - k);
        return Stack(
          children: [
            Positioned(
              left: (area.width - w) / 2 + dx,
              top: (area.height - h) / 2 + dy,
              width: w,
              height: h,
              child: Transform.rotate(angle: tilt, child: child),
            ),
          ],
        );
      },
      child: Container(
        decoration: BoxDecoration(
          color: _paper,
          borderRadius: BorderRadius.circular(16),
          boxShadow: const [BoxShadow(color: Color(0x33062B8A), blurRadius: 24, offset: Offset(0, 8))],
        ),
        clipBehavior: Clip.antiAlias,
        child: Image.memory(p.jpeg, fit: BoxFit.cover, gaplessPlayback: true),
      ),
    );
  }
}
