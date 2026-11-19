// S2 — QR scanner: black screen, four accent corner brackets, a scan line sweeping every 2.6 s, the command to run
// on the computer, and "Havolani qoʻlda kiritish" for when the camera cannot be used.

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';
import 'package:mobile_scanner/mobile_scanner.dart';

import '../design/tokens.dart';
import '../design/widgets.dart';

const _prefix = 'vutric://pair?';

/// Opens the scanner; the first `vutric://pair?` link seen (or typed), or null.
Future<String?> scanLink(BuildContext context) =>
    Navigator.of(context).push<String>(MaterialPageRoute(fullscreenDialog: true, builder: (_) => const ScanPage()));

class ScanPage extends StatefulWidget {
  const ScanPage({super.key});

  @override
  State<ScanPage> createState() => _ScanPageState();
}

class _ScanPageState extends State<ScanPage> with SingleTickerProviderStateMixin {
  final _camera = MobileScannerController(formats: const [BarcodeFormat.qrCode]);
  late final AnimationController _sweep = AnimationController(vsync: this, duration: const Duration(milliseconds: 2600));
  bool _done = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (context.reducedMotion) {
      _sweep.stop();
    } else if (!_sweep.isAnimating) {
      _sweep.repeat(reverse: true);
    }
  }

  @override
  void dispose() {
    _sweep.dispose();
    _camera.dispose();
    super.dispose();
  }

  void _finish(String link) {
    if (_done) return;
    _done = true;
    HapticFeedback.mediumImpact();
    Navigator.of(context).pop(link);
  }

  Future<void> _manual() async {
    final v = context.v;
    final input = TextEditingController();
    final clip = await Clipboard.getData(Clipboard.kTextPlain);
    if (clip?.text?.trim().startsWith(_prefix) == true) input.text = clip!.text!.trim();
    if (!mounted) return;
    final link = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Ulash havolasi', style: v.heading),
        content: TextField(
          controller: input,
          autofocus: true,
          minLines: 1,
          maxLines: 4,
          style: v.text(14, FontWeight.w500, mono: true),
          decoration: InputDecoration(
            hintText: '${_prefix}v=1&…',
            filled: true,
            fillColor: v.bgAlt,
            border: OutlineInputBorder(borderRadius: BorderRadius.circular(12), borderSide: BorderSide.none),
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Bekor qilish')),
          FilledButton(onPressed: () => Navigator.pop(ctx, input.text.trim()), child: const Text('Ulanish')),
        ],
      ),
    );
    input.dispose();
    if (link == null || !mounted) return;
    if (!link.startsWith(_prefix)) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Bu Vutric havolasi emas: u “vutric://pair?” bilan boshlanadi')),
      );
      return;
    }
    _finish(link);
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final size = MediaQuery.sizeOf(context);
    final box = (size.width * 0.7).clamp(220.0, 320.0);
    return AnnotatedRegion<SystemUiOverlayStyle>(
      value: SystemUiOverlayStyle.light,
      child: Scaffold(
        backgroundColor: Colors.black,
        body: Stack(
          children: [
            Positioned.fill(
              child: MobileScanner(
                controller: _camera,
                onDetect: (capture) {
                  for (final code in capture.barcodes) {
                    final link = code.rawValue;
                    if (link != null && link.startsWith(_prefix)) return _finish(link);
                  }
                },
                errorBuilder: (context, error) => _CameraBlocked(onManual: _manual, error: error),
              ),
            ),
            // Darken everything except the window, so the brackets read as the target.
            Positioned.fill(
              child: IgnorePointer(
                child: CustomPaint(painter: _Scrim(window: box, top: size.height * 0.30)),
              ),
            ),
            SafeArea(
              child: Column(
                children: [
                  Align(
                    alignment: Alignment.centerLeft,
                    child: Padding(
                      padding: const EdgeInsets.all(16),
                      child: VCircleButton(
                        icon: Icons.close_rounded,
                        dark: true,
                        label: 'Yopish',
                        onTap: () => Navigator.of(context).pop(),
                      ),
                    ),
                  ),
                  Text(
                    'Kompyuterdagi QR kodni\nskanerlang',
                    textAlign: TextAlign.center,
                    style: v.text(24, FontWeight.w600, color: Colors.white, tracking: -0.03, height: 1.15),
                  ),
                  const Spacer(),
                ],
              ),
            ),
            Positioned(
              top: size.height * 0.30,
              left: (size.width - box) / 2,
              width: box,
              height: box,
              child: IgnorePointer(
                child: AnimatedBuilder(
                  animation: _sweep,
                  builder: (context, _) => CustomPaint(
                    painter: _Brackets(color: v.accent, sweep: context.reducedMotion ? null : _sweep.value),
                  ),
                ),
              ),
            ),
            Positioned(
              top: size.height * 0.30 + box + 28,
              left: 0,
              right: 0,
              child: Column(
                children: [
                  Text('Kompyuterda ishga tushiring:', style: v.text(14, FontWeight.w500, color: Colors.white70)),
                  const SizedBox(height: 8),
                  Container(
                    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                    decoration: BoxDecoration(
                      color: const Color(0x1FFFFFFF),
                      borderRadius: BorderRadius.circular(8),
                      border: Border.all(color: const Color(0x33FFFFFF), width: 0.5),
                    ),
                    child: Text('vutric --listen', style: v.text(14, FontWeight.w500, mono: true, color: Colors.white)),
                  ),
                ],
              ),
            ),
            Positioned(
              left: 0,
              right: 0,
              bottom: 0,
              child: SafeArea(
                child: Padding(
                  padding: const EdgeInsets.only(bottom: 16),
                  child: Center(
                    child: VPressable(
                      onTap: _manual,
                      child: Container(
                        padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 12),
                        decoration: BoxDecoration(
                          color: const Color(0x29FFFFFF),
                          borderRadius: BorderRadius.circular(999),
                          border: Border.all(color: const Color(0x33FFFFFF), width: 0.5),
                        ),
                        child: Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            const Icon(Icons.link_rounded, size: 18, color: Colors.white),
                            const SizedBox(width: 8),
                            Text(
                              'Havolani qoʻlda kiritish',
                              style: v.text(15, FontWeight.w600, color: Colors.white),
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _CameraBlocked extends StatelessWidget {
  const _CameraBlocked({required this.onManual, required this.error});
  final VoidCallback onManual;
  final MobileScannerException error;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final denied = error.errorCode == MobileScannerErrorCode.permissionDenied;
    return ColoredBox(
      color: Colors.black,
      child: Center(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 40),
          child: Text(
            denied
                ? 'Kamera ruxsati yoʻq. Sozlamalarda ruxsat bering yoki havolani qoʻlda kiriting.'
                : 'Kamera ochilmadi. Havolani qoʻlda kiriting.',
            textAlign: TextAlign.center,
            style: v.text(15, FontWeight.w500, color: Colors.white70, height: 1.4),
          ),
        ),
      ),
    );
  }
}

class _Scrim extends CustomPainter {
  _Scrim({required this.window, required this.top});
  final double window;
  final double top;

  @override
  void paint(Canvas canvas, Size size) {
    final hole = RRect.fromRectAndRadius(
      Rect.fromLTWH((size.width - window) / 2, top, window, window),
      const Radius.circular(28),
    );
    final path = Path()
      ..fillType = PathFillType.evenOdd
      ..addRect(Offset.zero & size)
      ..addRRect(hole);
    canvas.drawPath(path, Paint()..color = const Color(0x99000000));
  }

  @override
  bool shouldRepaint(_Scrim old) => old.window != window || old.top != top;
}

/// Four 56 px corner brackets (4 px, radius 28) and the sweeping scan line.
class _Brackets extends CustomPainter {
  _Brackets({required this.color, required this.sweep});
  final Color color;
  final double? sweep;

  @override
  void paint(Canvas canvas, Size size) {
    const len = 56.0;
    const r = 28.0;
    final p = Paint()
      ..color = color
      ..style = PaintingStyle.stroke
      ..strokeWidth = 4
      ..strokeCap = StrokeCap.round;
    final w = size.width;
    final h = size.height;

    // Each corner: a straight run into a quarter arc and out along the other edge.
    Path bracket(Offset start, Offset arcFrom, Offset arcTo, Offset end) => Path()
      ..moveTo(start.dx, start.dy)
      ..lineTo(arcFrom.dx, arcFrom.dy)
      ..arcToPoint(arcTo, radius: const Radius.circular(r))
      ..lineTo(end.dx, end.dy);
    canvas
      ..drawPath(bracket(const Offset(0, len), const Offset(0, r), const Offset(r, 0), const Offset(len, 0)), p)
      ..drawPath(bracket(Offset(w - len, 0), Offset(w - r, 0), Offset(w, r), Offset(w, len)), p)
      ..drawPath(bracket(Offset(w, h - len), Offset(w, h - r), Offset(w - r, h), Offset(w - len, h)), p)
      ..drawPath(bracket(Offset(len, h), Offset(r, h), Offset(0, h - r), Offset(0, h - len)), p);
    final s = sweep;
    if (s != null) {
      final y = 16 + (h - 32) * Curves.easeInOut.transform(s);
      final line = Paint()
        ..shader = LinearGradient(
          colors: [color.withValues(alpha: 0), color, color.withValues(alpha: 0)],
        ).createShader(Rect.fromLTWH(16, y - 1, w - 32, 2));
      canvas.drawRect(Rect.fromLTWH(16, y - 1, w - 32, 2), line);
    }
  }

  @override
  bool shouldRepaint(_Brackets old) => old.sweep != sweep || old.color != color;
}
