// How two devices meet: the "Qanday ulanamiz?" sheet, the connecting screen (S3-2: the V inside expanding rings,
// the peer's name and address from its QR) and the waiting screen with this phone's own QR (S4-2).
// Every path ends in Session.connect, so whichever tab started it, the whole app sees the same peer.

import 'dart:math' as math;

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';
import 'package:qr_flutter/qr_flutter.dart';

import '../design/tokens.dart';
import '../design/widgets.dart';
import '../session.dart';
import '../src/rust/api.dart' as vutric;
import '../ui.dart';
import 'scan_page.dart';

enum ConnectWay { scan, find, wait }

/// Asks how to connect (or uses [way]), runs it with the right screen, and returns the peer, or null.
Future<vutric.Connected?> connectFlow(BuildContext context, {ConnectWay? way}) async {
  final s = Session.instance;
  if (s.myLink == null) {
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(s.bootError ?? 'Tayyorlanmoqda, bir soniya…')),
    );
    return null;
  }
  way ??= await showModalBottomSheet<ConnectWay>(context: context, builder: (_) => const _WaySheet());
  if (way == null || !context.mounted) return null;
  switch (way) {
    case ConnectWay.scan:
      final link = await scanLink(context);
      if (link == null || !context.mounted) return null;
      return _run(context, () => vutric.connectLink(link: link), link: link);
    case ConnectWay.find:
      return _run(context, vutric.connectDiscovered);
    case ConnectWay.wait:
      return Navigator.of(context).push<vutric.Connected>(MaterialPageRoute(builder: (_) => const MyQrPage()));
  }
}

Future<vutric.Connected?> _run(BuildContext context, Future<vutric.Connected> Function() job, {String? link}) =>
    Navigator.of(context).push<vutric.Connected>(
      PageRouteBuilder(
        transitionDuration: VMotion.open,
        reverseTransitionDuration: VMotion.fast,
        pageBuilder: (_, _, _) => ConnectingPage(job: job, link: link),
        transitionsBuilder: (_, a, _, child) => FadeTransition(opacity: a, child: child),
      ),
    );

class _WaySheet extends StatelessWidget {
  const _WaySheet();

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text('Qanday ulanamiz?', style: v.title),
            const SizedBox(height: 4),
            Text('Bitta Wi-Fi’da eng tez. Boshqa tarmoqda ham ishlaydi, shifrlangan.', style: v.caption),
            const SizedBox(height: 16),
            VRowCard(
              icon: Icons.qr_code_scanner_rounded,
              title: 'Kompyuter QR kodini skanerlash',
              subtitle: 'Kompyuterdagi Vutric oynasidan',
              onTap: () => Navigator.pop(context, ConnectWay.scan),
            ),
            if (hasMdns) ...[
              const SizedBox(height: 10),
              VRowCard(
                icon: Icons.wifi_find_rounded,
                title: 'Tarmoqdan topish',
                subtitle: 'Shu Wi-Fi’dagi Vutric qurilmasi',
                onTap: () => Navigator.pop(context, ConnectWay.find),
              ),
            ],
            const SizedBox(height: 10),
            VRowCard(
              icon: Icons.qr_code_2_rounded,
              title: 'Kutish: QR kodimni skanerlashsin',
              subtitle: 'Boshqa telefon yoki kompyuter sizga ulanadi',
              onTap: () => Navigator.pop(context, ConnectWay.wait),
            ),
          ],
        ),
      ),
    );
  }
}

/// The peer's name and first address from its pairing link (`n=` and `a=`), for the connecting screen.
({String? name, String? addr}) peekLink(String? link) {
  if (link == null) return (name: null, addr: null);
  try {
    final q = Uri.parse(link).queryParameters;
    final a = q['a']?.split(',').first;
    final name = q['n']?.trim();
    return (name: name == null || name.isEmpty ? null : name, addr: a?.split(':').first);
  } catch (_) {
    return (name: null, addr: null);
  }
}

/// S3-2: the V inside expanding rings while the session opens; cancel goes back.
class ConnectingPage extends StatefulWidget {
  const ConnectingPage({super.key, required this.job, this.link});
  final Future<vutric.Connected> Function() job;
  final String? link;

  @override
  State<ConnectingPage> createState() => _ConnectingPageState();
}

class _ConnectingPageState extends State<ConnectingPage> with SingleTickerProviderStateMixin {
  late final AnimationController _pulse = AnimationController(vsync: this, duration: const Duration(milliseconds: 1800));
  String? _error;
  bool _closing = false;

  @override
  void initState() {
    super.initState();
    _go();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (!context.reducedMotion && !_pulse.isAnimating) _pulse.repeat();
  }

  @override
  void dispose() {
    _pulse.dispose();
    super.dispose();
  }

  Future<void> _go() async {
    setState(() => _error = null);
    try {
      final c = await Session.instance.connect(widget.job);
      if (!mounted) return;
      HapticFeedback.mediumImpact();
      _closing = true;
      Navigator.of(context).pop(c);
    } catch (e) {
      if (!mounted || _closing) return;
      if (isCancelled(e)) {
        Navigator.of(context).pop();
        return;
      }
      setState(() => _error = errorText(e));
    }
  }

  Future<void> _cancel() async {
    if (_closing) return;
    _closing = true;
    await disconnectNow();
    if (mounted) Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final peek = peekLink(widget.link);
    final name = peek.name;
    return PopScope(
      canPop: false,
      onPopInvokedWithResult: (did, _) {
        if (!did) _cancel();
      },
      child: Scaffold(
        body: SafeArea(
          child: Column(
            children: [
              VNavHeader(title: 'Ulanish', onBack: _cancel),
              const Spacer(),
              SizedBox.square(
                dimension: 180,
                child: AnimatedBuilder(
                  animation: _pulse,
                  builder: (context, child) => CustomPaint(
                    painter: _Rings(color: v.accent, t: _error == null && !context.reducedMotion ? _pulse.value : null),
                    child: child,
                  ),
                  child: const Center(child: VAppIcon(size: 64)),
                ),
              ),
              const SizedBox(height: 28),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 24),
                child: Text(
                  _error != null ? 'Ulanib boʻlmadi' : (name == null ? 'Ulanmoqda…' : '$name’ga ulanmoqda'),
                  textAlign: TextAlign.center,
                  style: v.title,
                ),
              ),
              const SizedBox(height: 8),
              Text([if (peek.addr != null) peek.addr!, 'kalit almashinuvi'].join(' · '), style: v.monoSmall),
              const SizedBox(height: 18),
              if (_error == null)
                SizedBox(
                  width: 160,
                  child: ClipRRect(
                    borderRadius: BorderRadius.circular(2),
                    child: LinearProgressIndicator(minHeight: 3, color: v.accent, backgroundColor: v.line),
                  ),
                )
              else
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: VNotice(
                    title: 'Aloqa oʻrnatilmadi',
                    body: '$_error\nIkkala qurilma yoqilgan va Wi-Fi’da ekanini tekshiring.',
                  ),
                ),
              const Spacer(),
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
                child: _error == null
                    ? VButton(label: 'Bekor qilish', kind: VButtonKind.secondary, onPressed: _cancel)
                    : Row(
                        children: [
                          Expanded(
                            flex: 10,
                            child: VButton(label: 'Orqaga', kind: VButtonKind.secondary, onPressed: _cancel),
                          ),
                          const SizedBox(width: 10),
                          Expanded(
                            flex: 14,
                            child: VButton(label: 'Qayta urinish', icon: Icons.refresh_rounded, onPressed: _go),
                          ),
                        ],
                      ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _Rings extends CustomPainter {
  _Rings({required this.color, required this.t});
  final Color color;

  /// 0..1 phase of the pulse, or null for still rings (reduced motion, or after an error).
  final double? t;

  @override
  void paint(Canvas canvas, Size size) {
    final c = size.center(Offset.zero);
    final maxR = size.width / 2;
    final p = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.5;
    final phase = t;
    if (phase == null) {
      for (final f in [0.62, 0.86]) {
        canvas.drawCircle(c, maxR * f, p..color = color.withValues(alpha: 0.25));
      }
      return;
    }
    for (var i = 0; i < 3; i++) {
      final k = (phase + i / 3) % 1;
      canvas.drawCircle(
        c,
        maxR * (0.45 + 0.55 * k),
        p..color = color.withValues(alpha: 0.45 * math.pow(1 - k, 1.4).toDouble()),
      );
    }
  }

  @override
  bool shouldRepaint(_Rings old) => old.t != t || old.color != color;
}

/// The pairing QR in a white card with the V in its middle (error correction H keeps it readable).
class PairingQr extends StatelessWidget {
  const PairingQr({super.key, required this.data, required this.size});
  final String data;
  final double size;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return Semantics(
      label: 'Ulash uchun QR kod',
      child: Container(
        padding: const EdgeInsets.all(18),
        decoration: BoxDecoration(
          color: Colors.white,
          borderRadius: BorderRadius.circular(28),
          boxShadow: [
            BoxShadow(
              color: v.accent.withValues(alpha: 0.28),
              blurRadius: 48,
              offset: const Offset(0, 18),
              spreadRadius: -12,
            ),
          ],
        ),
        child: Stack(
          alignment: Alignment.center,
          children: [
            QrImageView(
              data: data,
              size: size,
              padding: EdgeInsets.zero,
              errorCorrectionLevel: QrErrorCorrectLevel.H,
              eyeStyle: const QrEyeStyle(eyeShape: QrEyeShape.square, color: Colors.black),
              dataModuleStyle: const QrDataModuleStyle(
                dataModuleShape: QrDataModuleShape.circle,
                color: Colors.black,
              ),
            ),
            Container(
              padding: const EdgeInsets.all(4),
              decoration: BoxDecoration(color: Colors.white, borderRadius: BorderRadius.circular(14)),
              child: VAppIcon(size: size * 0.16),
            ),
          ],
        ),
      ),
    );
  }
}

/// S4-2: this phone's QR while it waits to be scanned.
class MyQrPage extends StatefulWidget {
  const MyQrPage({super.key});

  @override
  State<MyQrPage> createState() => _MyQrPageState();
}

class _MyQrPageState extends State<MyQrPage> {
  bool _closing = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _wait();
  }

  Future<void> _wait() async {
    setState(() => _error = null);
    try {
      final c = await Session.instance.connect(vutric.acceptSession);
      if (!mounted) return;
      HapticFeedback.mediumImpact();
      _closing = true;
      Navigator.of(context).pop(c);
    } catch (e) {
      if (!mounted || _closing) return;
      if (isCancelled(e)) {
        Navigator.of(context).pop();
        return;
      }
      setState(() => _error = errorText(e));
    }
  }

  Future<void> _cancel() async {
    if (_closing) return;
    _closing = true;
    await disconnectNow();
    if (mounted) Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final s = Session.instance;
    final w = MediaQuery.sizeOf(context).width;
    return PopScope(
      canPop: false,
      onPopInvokedWithResult: (did, _) {
        if (!did) _cancel();
      },
      child: Scaffold(
        body: SafeArea(
          child: Column(
            children: [
              const SizedBox(height: 28),
              Text('Shu kodni\nskanerlashsin', textAlign: TextAlign.center, style: v.largeTitle),
              const SizedBox(height: 8),
              Text('Boshqa qurilmada: Vutric → Ulanish → QR skanerlash', style: v.caption),
              const Spacer(),
              PairingQr(data: s.myLink ?? '', size: (w * 0.56).clamp(200.0, 300.0)),
              const Spacer(),
              Row(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Text(s.deviceName, style: v.text(14, FontWeight.w500)),
                  const SizedBox(width: 8),
                  Text(s.online ? 'Wi-Fi + internet' : 'shu Wi-Fi', style: v.monoSmall),
                ],
              ),
              if (_error != null)
                Padding(
                  padding: const EdgeInsets.fromLTRB(16, 12, 16, 0),
                  child: VNotice(title: 'Kutish toʻxtadi', body: _error),
                ),
              Padding(
                padding: const EdgeInsets.all(16),
                child: _error == null
                    ? VButton(label: 'Bekor qilish', kind: VButtonKind.secondary, onPressed: _cancel)
                    : VButton(label: 'Yana kutish', icon: Icons.refresh_rounded, onPressed: _wait),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
