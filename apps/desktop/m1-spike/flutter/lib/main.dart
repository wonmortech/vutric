// M1 spike: the desktop settings screen and a looping Halqa preview in Flutter desktop. Same screen and same
// timeline as the Slint and Tauri spikes (../README.md).

import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/scheduler.dart';
import 'package:material_ui/material_ui.dart';

const night = Color(0xFF040C29);
const surface = Color(0xFF0B1640);
const line = Color(0xFF1C2A63);
const signal = Color(0xFF2F6BFF);
const glow = Color(0xFFF4D9A6);
const textColor = Color(0xFFEEF1FB);
const muted = Color(0xFF9AA6CF);

class Probe {
  Probe() : _log = Platform.environment['VUTRIC_M1_LOG'] {
    _want = int.tryParse(Platform.environment['VUTRIC_M1_FRAMES'] ?? '') ?? 600;
  }

  final String? _log;
  late final int _want;
  final _iv = <double>[];
  bool _done = false;
  Duration? _first;
  Duration? _last;

  void _write(String s) {
    final p = _log;
    if (p != null) File(p).writeAsStringSync('$s\n', mode: FileMode.append, flush: true);
  }

  void firstFrame() =>
      _write('{"toolkit":"flutter","event":"first_frame","unix_ms":${DateTime.now().millisecondsSinceEpoch}}');

  void timings(List<FrameTiming> ts) {
    for (final t in ts) {
      final end = Duration(microseconds: t.timestampInMicroseconds(ui.FramePhase.rasterFinish));
      _first ??= end;
      if (end - _first! > const Duration(seconds: 1) && _last != null) {
        _iv.add((end - _last!).inMicroseconds / 1000.0);
      }
      _last = end;
    }
    if (!_done && _iv.length >= _want) {
      _done = true;
      final v = [..._iv]..sort();
      final mean = v.reduce((a, b) => a + b) / v.length;
      double p(double q) => v[((v.length - 1) * q).round()];
      _write('{"toolkit":"flutter","event":"frames","count":${v.length},'
          '"fps":${(1000 / mean).toStringAsFixed(1)},"p50_ms":${p(0.5).toStringAsFixed(2)},'
          '"p99_ms":${p(0.99).toStringAsFixed(2)},"max_ms":${v.last.toStringAsFixed(2)}}');
    }
  }
}

void main() {
  final probe = Probe();
  WidgetsFlutterBinding.ensureInitialized();
  SchedulerBinding.instance.addTimingsCallback(probe.timings);
  WidgetsBinding.instance.addPostFrameCallback((_) => probe.firstFrame());
  runApp(const SettingsApp());
}

class SettingsApp extends StatelessWidget {
  const SettingsApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      title: 'Vutric — Sozlamalar',
      theme: ThemeData(
        brightness: Brightness.dark,
        scaffoldBackgroundColor: night,
        fontFamily: 'Segoe UI',
        colorScheme: const ColorScheme.dark(primary: signal, surface: surface),
      ),
      home: const SettingsScreen(),
    );
  }
}

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  int effect = 0, zone = 1, accept = 0, after = 0;
  double duration = 1.05;
  bool autostart = true;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Padding(
        padding: const EdgeInsets.all(20),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            SizedBox(
              width: 480,
              child: ListView(
                padding: const EdgeInsets.only(right: 12),
                children: [
                  const Text('Sozlamalar',
                      style: TextStyle(color: textColor, fontSize: 26, fontWeight: FontWeight.w700)),
                  const SizedBox(height: 4),
                  const Text('Bu kompyuter · Ofis kompyuteri', style: TextStyle(color: muted, fontSize: 13)),
                  _Group(
                    title: 'Effekt',
                    hint: "Tizimda harakatni kamaytirish yoqilgan bo'lsa, doim Oddiy ishlatiladi.",
                    child: _Segmented(const ['Halqa', 'Kometa', 'Pufak', 'Oddiy'], effect,
                        (i) => setState(() => effect = i)),
                  ),
                  _Group(
                    title: 'Yuborish zonasi',
                    child: _Segmented(const ['Chap', "O'ng", 'Tepa'], zone, (i) => setState(() => zone = i)),
                  ),
                  _Group(
                    title: 'Davomiyligi',
                    child: Row(children: [
                      Expanded(
                        child: Slider(
                            min: 0.6, max: 2.0, value: duration, onChanged: (v) => setState(() => duration = v)),
                      ),
                      SizedBox(
                          width: 52,
                          child: Text('${duration.toStringAsFixed(2)} s', style: const TextStyle(color: textColor))),
                    ]),
                  ),
                  _Group(
                    title: 'Saqlash papkasi',
                    child: Row(children: [
                      const Expanded(
                        child: Text(r'C:\Users\Siz\Downloads\Vutric',
                            overflow: TextOverflow.ellipsis, style: TextStyle(color: textColor)),
                      ),
                      SizedBox(
                        width: 120,
                        height: 34,
                        child: FilledButton(
                          style: FilledButton.styleFrom(
                              backgroundColor: signal,
                              shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10))),
                          onPressed: () {},
                          child: const Text("O'zgartirish", style: TextStyle(fontSize: 13, color: Colors.white)),
                        ),
                      ),
                    ]),
                  ),
                  _Group(
                    title: 'Qabul qilish',
                    child: _Segmented(const ["So'rash", 'Tanishlardan', 'Hammadan'], accept,
                        (i) => setState(() => accept = i)),
                  ),
                  _Group(
                    title: 'Qabul qilingandan keyin',
                    child: _Segmented(const ['Bildirish', 'Faylni ochish', 'Papka', 'Hech narsa'], after,
                        (i) => setState(() => after = i)),
                  ),
                  _Group(
                    title: 'Tizim bilan birga ishga tushirish',
                    child: Row(children: [
                      const Expanded(
                          child: Text('Kompyuter yoqilganda qabul qilishga tayyor', style: TextStyle(color: muted))),
                      Switch(value: autostart, onChanged: (v) => setState(() => autostart = v)),
                    ]),
                  ),
                ],
              ),
            ),
            const SizedBox(width: 20),
            Expanded(
              child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                const Text("Ko'rinish", style: TextStyle(color: muted, fontSize: 13)),
                const SizedBox(height: 10),
                Expanded(child: HalqaPreview(playing: effect != 3)),
              ]),
            ),
          ],
        ),
      ),
    );
  }
}

class _Group extends StatelessWidget {
  const _Group({required this.title, this.hint, required this.child});
  final String title;
  final String? hint;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.only(top: 12),
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
          color: surface, borderRadius: BorderRadius.circular(14), border: Border.all(color: line)),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Text(title, style: const TextStyle(color: textColor, fontSize: 15, fontWeight: FontWeight.w600)),
        if (hint != null) ...[
          const SizedBox(height: 10),
          Text(hint!, style: const TextStyle(color: muted, fontSize: 12)),
        ],
        const SizedBox(height: 10),
        child,
      ]),
    );
  }
}

class _Segmented extends StatelessWidget {
  const _Segmented(this.options, this.selected, this.onSelect);
  final List<String> options;
  final int selected;
  final ValueChanged<int> onSelect;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 36,
      padding: const EdgeInsets.all(3),
      decoration: BoxDecoration(
          color: night, borderRadius: BorderRadius.circular(10), border: Border.all(color: line)),
      child: Row(children: [
        for (var i = 0; i < options.length; i++) ...[
          if (i > 0) const SizedBox(width: 3),
          Expanded(
            child: GestureDetector(
              onTap: () => onSelect(i),
              child: AnimatedContainer(
                duration: const Duration(milliseconds: 140),
                alignment: Alignment.center,
                decoration: BoxDecoration(
                    color: i == selected ? signal : Colors.transparent, borderRadius: BorderRadius.circular(8)),
                child: Text(options[i],
                    style: TextStyle(color: i == selected ? Colors.white : muted, fontSize: 13)),
              ),
            ),
          ),
        ],
      ]),
    );
  }
}

class HalqaPreview extends StatefulWidget {
  const HalqaPreview({super.key, required this.playing});
  final bool playing;

  @override
  State<HalqaPreview> createState() => _HalqaPreviewState();
}

class _HalqaPreviewState extends State<HalqaPreview> with SingleTickerProviderStateMixin {
  late final Ticker _ticker;
  final _t = ValueNotifier<double>(0);

  @override
  void initState() {
    super.initState();
    _ticker = createTicker((e) => _t.value = widget.playing ? (e.inMicroseconds / 1e6) % 2.9 : 1.3)..start();
  }

  @override
  void dispose() {
    _ticker.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return ClipRRect(
      borderRadius: BorderRadius.circular(14),
      child: DecoratedBox(
        decoration: const BoxDecoration(
            gradient: RadialGradient(colors: [Color(0xFF0D1A4D), night])),
        child: CustomPaint(painter: _HalqaPainter(_t), size: Size.infinite),
      ),
    );
  }
}

double _c01(double x) => x.clamp(0.0, 1.0);
double _outCubic(double x) => 1 - math.pow(1 - x, 3).toDouble();
double _inCubic(double x) => x * x * x;
double _outBack(double x) => 1 + 2.4 * math.pow(x - 1, 3) + 1.4 * math.pow(x - 1, 2);
double _spring(double s) => math.exp(-0.42 * 20.1 * s) * math.cos(20.1 * 0.908 * s);
Offset _bez(Offset a, Offset b, Offset c, Offset d, double e) {
  final m = 1 - e;
  return a * (m * m * m) + b * (3 * m * m * e) + c * (3 * m * e * e) + d * (e * e * e);
}

class _HalqaPainter extends CustomPainter {
  _HalqaPainter(this.t) : super(repaint: t);
  final ValueNotifier<double> t;

  @override
  void paint(Canvas canvas, Size size) {
    final w = size.width, h = size.height, time = t.value;
    final ry = math.min(h * 0.34, 320.0), rx = ry * 0.42;
    final ch = math.min(math.min(h * 0.36, w * 0.3), 300.0), cw = ch * 4 / 3;
    final p0 = Offset(w - rx * 0.55, h * 0.42), p3 = Offset(w * 0.42, h * 0.55), dx = p0.dx - p3.dx;
    final p1 = Offset(p0.dx - dx * 0.25, p0.dy - ry * 0.55), p2 = Offset(p3.dx + dx * 0.2, p3.dy - ry * 0.12);
    final u = time / 1.05, open = _c01(u / 0.24), close = _c01((u - 0.6) / 0.3);
    final rs = _outBack(open) * (1 - 0.35 * _inCubic(close)), ra = _c01(open * 3) * (1 - close);

    if (ra > 0.002) {
      final r = ry * rs;
      canvas.save();
      canvas.translate(p0.dx, p0.dy);
      canvas.scale(0.42, 1);
      final rect = Rect.fromCircle(center: Offset.zero, radius: r);
      canvas.drawCircle(
          Offset.zero,
          r,
          Paint()
            ..shader = ui.Gradient.radial(Offset.zero, math.max(r, 0.01), [
              const Color(0xFF020616).withValues(alpha: ra),
              const Color(0xFF081440).withValues(alpha: ra),
              const Color(0x592F6BFF).withValues(alpha: 0.35 * ra),
            ], [0, 0.75, 1]));
      canvas.drawOval(
          rect,
          Paint()
            ..style = PaintingStyle.stroke
            ..strokeWidth = 8
            ..color = signal.withValues(alpha: ra)
            ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 14));
      canvas.drawOval(
          rect,
          Paint()
            ..style = PaintingStyle.stroke
            ..strokeWidth = 3
            ..color = glow.withValues(alpha: ra));
      canvas.restore();
    }

    final f = _c01((u - 0.10) / 0.52), e = _outCubic(f);
    if (f > 0 && f < 1) {
      for (var k = 1; k <= 4; k++) {
        final ek = _outCubic(_c01(f - k * 0.075));
        if (ek <= 0) continue;
        final pk = _bez(p0, p1, p2, p3, ek), sk = 0.16 + 0.84 * ek;
        canvas.save();
        canvas.translate(pk.dx, pk.dy);
        canvas.rotate(-0.42 * (1 - ek));
        canvas.scale(sk);
        canvas.drawRRect(
            RRect.fromRectAndRadius(Rect.fromCenter(center: Offset.zero, width: cw, height: ch),
                const Radius.circular(16)),
            Paint()
              ..style = PaintingStyle.stroke
              ..strokeWidth = 2 / sk
              ..color = (k < 3 ? const Color(0xFF6F8FF0) : glow).withValues(alpha: (1 - f) * 0.55 / k));
        canvas.restore();
      }
    }

    var pos = _bez(p0, p1, p2, p3, e);
    var sc = 0.16 + 0.84 * e, rot = -0.42 * (1 - e), a = _c01(f * 5);
    if (time > 0.651) {
      final s = _spring(time - 0.651);
      sc = 1 + 0.07 * s;
      rot = -0.05 * s;
    }
    final d = _inCubic(_c01((time - 2.45) / 0.45));
    pos = Offset(pos.dx + (w - 36 - pos.dx) * d, pos.dy + (h - 36 - pos.dy) * d);
    sc *= 1 - 0.78 * d;
    a *= 1 - d;
    if (a > 0.002) {
      canvas.save();
      canvas.translate(pos.dx, pos.dy);
      canvas.rotate(rot);
      canvas.scale(sc);
      final rr = RRect.fromRectAndRadius(
          Rect.fromCenter(center: Offset.zero, width: cw, height: ch), const Radius.circular(16));
      canvas.drawRRect(
          rr.shift(const Offset(0, 22)),
          Paint()
            ..color = Colors.black.withValues(alpha: 0.55 * a)
            ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 21));
      canvas.drawRRect(
          rr,
          Paint()
            ..shader = ui.Gradient.linear(Offset(-cw / 2, -ch / 2), Offset(cw / 4, ch / 2), [
              const Color(0xFFFFD696).withValues(alpha: a),
              const Color(0xFF5A3F68).withValues(alpha: a),
              const Color(0xFF241F45).withValues(alpha: a),
              const Color(0xFF151634).withValues(alpha: a),
            ], [0, 0.45, 0.75, 1]));
      canvas.drawRRect(
          rr,
          Paint()
            ..style = PaintingStyle.stroke
            ..strokeWidth = 1.5 / sc
            ..color = glow.withValues(alpha: 0.55 * a));
      canvas.restore();
    }
  }

  @override
  bool shouldRepaint(_HalqaPainter old) => false;
}
