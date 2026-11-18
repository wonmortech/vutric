// How two devices meet: the "Qanday ulanamiz?" sheet (ordered by the route choice, ADR-0060), the connecting
// screen (S3-2: the V inside expanding rings, the peer's name and address from its QR, and the six-digit check for
// a device found nearby, ADR-0050), the typed code or address page (ADR-0059) and the waiting screen with this
// phone's own QR and short code (S4-2). Every path ends in Session.connect, so whichever tab started it, the whole
// app sees the same peer.

import 'dart:async';
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';
import 'package:qr_flutter/qr_flutter.dart';

import '../android.dart';
import '../companion.dart';
import '../design/tokens.dart';
import '../design/widgets.dart';
import '../i18n.dart';
import '../session.dart';
import '../src/rust/api.dart' as vutric;
import '../ui.dart';
import 'hotspot_page.dart';
import 'route_picker.dart';
import 'scan_page.dart';

enum ConnectWay { scan, find, typed, wait, hotspot }

/// Asks how to connect (or uses [way]), runs it with the right screen, and returns the peer, or null.
Future<vutric.Connected?> connectFlow(BuildContext context, {ConnectWay? way}) async {
  final s = Session.instance;
  if (s.myLink == null) {
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(s.bootError ?? 'Tayyorlanmoqda, bir soniya…')));
    return null;
  }
  way ??= await showModalBottomSheet<ConnectWay>(
    context: context,
    isScrollControlled: true,
    builder: (_) => const WaySheet(),
  );
  if (way == null || !context.mounted) return null;
  switch (way) {
    case ConnectWay.scan:
      final link = await scanLink(context);
      if (link == null || !context.mounted) return null;
      return connectWithLink(context, link);
    case ConnectWay.find:
      return _run(context, vutric.connectDiscovered);
    case ConnectWay.typed:
      return Navigator.of(context).push<vutric.Connected>(MaterialPageRoute(builder: (_) => const TypedPage()));
    case ConnectWay.wait:
      return Navigator.of(context).push<vutric.Connected>(MaterialPageRoute(builder: (_) => const MyQrPage()));
    case ConnectWay.hotspot:
      return Navigator.of(context).push<vutric.Connected>(MaterialPageRoute(builder: (_) => const HotspotPage()));
  }
}

/// A scanned or pasted link. When it carries a hotspot (ADR-0058), this phone joins that Wi-Fi first (Android),
/// then pairs over it; the hotspot is left again when the session ends.
Future<vutric.Connected?> connectWithLink(BuildContext context, String link) async {
  vutric.HotspotNet? net;
  try {
    net = await vutric.hotspotOfLink(link: link);
  } catch (e) {
    if (context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(errorText(e))));
    }
    return null;
  }
  final joined = net != null && Platform.isAndroid;
  if (joined) {
    if (!context.mounted || !await joinHotspot(context, net)) return null;
  }
  if (!context.mounted) return null;
  final c = await _run(context, () => vutric.connectLink(link: link), link: link);
  if (c == null && joined) {
    // Not paired after all: back to the phone's own Wi-Fi.
    Session.instance.onDisconnect = null;
    await AndroidBridge.instance.hotspotLeave();
    await Session.instance.restart();
  }
  return c;
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

/// The ways to connect that the route plan allows (ADR-0060), best first. Pure, so it is tested without Rust.
List<ConnectWay> waysFor(List<String> steps, {required bool mdns, required bool android}) {
  final lan = steps.contains('same_network');
  final internet = steps.contains('internet_direct') || steps.contains('relay');
  final out = <ConnectWay>[
    if (lan || internet) ConnectWay.scan,
    if (lan && mdns) ConnectWay.find,
    if (lan) ConnectWay.typed,
    if (lan || internet) ConnectWay.wait,
  ];
  if (android && steps.contains('offer_hotspot')) {
    // "Tarmoqsiz" leads with the hotspot; otherwise it is the last resort.
    if (!lan && !internet) {
      out.insert(0, ConnectWay.hotspot);
    } else {
      out.add(ConnectWay.hotspot);
    }
  }
  return out;
}

class WaySheet extends StatefulWidget {
  const WaySheet({super.key});

  @override
  State<WaySheet> createState() => _WaySheetState();
}

class _WaySheetState extends State<WaySheet> {
  final _s = Session.instance;
  vutric.RoutePlan? _plan;

  final _foreign = ForeignDevices.instance;

  @override
  void initState() {
    super.initState();
    _s.addListener(_replan);
    _foreign.addListener(_heard);
    _replan();
    // LocalSend and KDE Connect devices are looked for only while this sheet is open (A13).
    if (Platform.isAndroid) AndroidBridge.instance.foreignStart();
  }

  @override
  void dispose() {
    _s.removeListener(_replan);
    _foreign.removeListener(_heard);
    if (Platform.isAndroid) AndroidBridge.instance.foreignStop();
    super.dispose();
  }

  void _heard() {
    if (mounted) setState(() {});
  }

  Future<void> _replan() async {
    try {
      final lan = await vutric.hasLan();
      final p = await vutric.routePlan(choice: _s.route, lan: lan, internet: _s.online, hotspot: Platform.isAndroid);
      if (mounted) setState(() => _plan = p);
    } catch (_) {
      // Rust not ready: every way stays offered.
      if (mounted) {
        setState(() => _plan = const vutric.RoutePlan(steps: ['same_network', 'internet_direct', 'relay'], none: ''));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final plan = _plan;
    final ways = plan == null ? const <ConnectWay>[] : waysFor(plan.steps, mdns: hasMdns, android: Platform.isAndroid);
    return SafeArea(
      child: SingleChildScrollView(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Expanded(child: Text('Qanday ulanamiz?', style: v.title)),
                RouteChip(route: _s.route, onTap: () => pickRoute(context)),
              ],
            ),
            const SizedBox(height: 4),
            Text('Bitta Wi-Fi’da eng tez. Boshqa tarmoqda ham ishlaydi, shifrlangan.', style: v.caption),
            const SizedBox(height: 16),
            if (plan == null)
              const Padding(
                padding: EdgeInsets.all(24),
                child: Center(child: CircularProgressIndicator()),
              )
            else if (ways.isEmpty)
              VNotice(title: tr(_s.route == 'bluetooth' ? 'route.none.not_built' : 'route.none.unavailable'))
            else
              for (final (i, w) in ways.indexed) ...[if (i > 0) const SizedBox(height: 10), _row(context, w)],
            if (_foreign.byKey.isNotEmpty) ...[
              const SizedBox(height: 18),
              ForeignList(devices: _foreign.byKey.values.toList()),
            ],
            const SizedBox(height: 14),
            PrivacyLine(keys: _s.privacy),
          ],
        ),
      ),
    );
  }

  Widget _row(BuildContext context, ConnectWay w) {
    void go() => Navigator.pop(context, w);
    return switch (w) {
      ConnectWay.scan => VRowCard(
        icon: Icons.qr_code_scanner_rounded,
        title: 'Kompyuter QR kodini skanerlash',
        subtitle: 'Kompyuterdagi Vutric oynasidan',
        onTap: go,
      ),
      ConnectWay.find => VRowCard(
        icon: Icons.wifi_find_rounded,
        title: 'Tarmoqdan topish',
        subtitle: 'Shu Wi-Fi’dagi Vutric qurilmasi',
        onTap: go,
      ),
      ConnectWay.typed => VRowCard(
        icon: Icons.keyboard_rounded,
        title: tr('find.title'),
        subtitle: tr('find.subtitle'),
        onTap: go,
      ),
      ConnectWay.wait => VRowCard(
        icon: Icons.qr_code_2_rounded,
        title: 'Kutish: QR kodimni skanerlashsin',
        subtitle: 'Boshqa telefon yoki kompyuter sizga ulanadi',
        onTap: go,
      ),
      ConnectWay.hotspot => VRowCard(
        icon: Icons.wifi_tethering_rounded,
        title: 'Routersiz: telefon hotspoti',
        subtitle: 'Wi-Fi ham, internet ham kerak emas',
        onTap: go,
      ),
    };
  }
}

/// LocalSend and KDE Connect devices heard on this Wi-Fi (A13). Shown for what they are; sending to them opens
/// when the phone host of the compat adapters exists (W3 #11).
class ForeignList extends StatelessWidget {
  const ForeignList({super.key, required this.devices});
  final List<ForeignDevice> devices;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(tr('foreign.title'), style: v.label),
        const SizedBox(height: 8),
        for (final d in devices)
          Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: VRowCard(
              icon: d.type == 'mobile' || d.type == 'phone' ? Icons.smartphone_rounded : Icons.computer_rounded,
              title: d.name.isEmpty ? d.host : d.name,
              subtitle: '${tr('foreign.${d.app}')} · ${d.host}',
            ),
          ),
        Text(tr('foreign.hint'), style: v.caption),
      ],
    );
  }
}

/// The honest privacy line (S6, ADR-0061): only what the route choice makes true.
class PrivacyLine extends StatelessWidget {
  const PrivacyLine({super.key, required this.keys, this.center = false});
  final List<String> keys;
  final bool center;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return Column(
      crossAxisAlignment: center ? CrossAxisAlignment.center : CrossAxisAlignment.start,
      children: [
        for (final (i, k) in keys.indexed)
          Padding(
            padding: EdgeInsets.only(top: i == 0 ? 0 : 4),
            child: Row(
              mainAxisSize: center ? MainAxisSize.min : MainAxisSize.max,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Padding(
                  padding: const EdgeInsets.only(top: 1),
                  child: Icon(
                    k == 'privacy.encrypted' ? Icons.lock_outline_rounded : Icons.wifi_off_rounded,
                    size: 15,
                    color: v.muted,
                  ),
                ),
                const SizedBox(width: 6),
                Flexible(child: Text(tr(k), style: v.caption)),
              ],
            ),
          ),
      ],
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
  late final AnimationController _pulse = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 1800),
  );
  String? _error;
  bool _closing = false;

  /// The six digits while a device found nearby is being paired (ADR-0050), and the user's answer.
  String? _digits;
  Completer<bool>? _answer;
  bool _sentAnswer = false;

  @override
  void initState() {
    super.initState();
    _go();
  }

  /// Runs the commit-then-reveal comparison on the fresh session and waits for both users' answers.
  Future<bool> _verify(vutric.Connected c) async {
    final digits = await vutric.pairStart();
    if (!mounted) return false;
    final answer = _answer = Completer<bool>();
    setState(() {
      _digits = digits;
      _sentAnswer = false;
    });
    HapticFeedback.selectionClick();
    final mine = await answer.future;
    if (mounted) setState(() => _sentAnswer = true);
    final both = await vutric.pairConfirm(matches: mine);
    if (!both && mounted) setState(() => _error = tr('pair.refused'));
    return both;
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
      final c = await Session.instance.connect(widget.job, verify: _verify);
      if (!mounted) return;
      HapticFeedback.mediumImpact();
      _closing = true;
      Navigator.of(context).pop(c);
    } catch (e) {
      if (!mounted || _closing) return;
      if (isCancelled(e) && _digits == null) {
        Navigator.of(context).pop();
        return;
      }
      setState(() {
        _error ??= isCancelled(e) ? tr('pair.refused') : errorText(e);
        _digits = null;
      });
    }
  }

  Future<void> _cancel() async {
    if (_closing) return;
    _closing = true;
    if (_answer?.isCompleted == false) _answer!.complete(false);
    await disconnectNow();
    if (mounted) Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final digits = _digits;
    if (digits != null && _error == null) {
      return PopScope(
        canPop: false,
        onPopInvokedWithResult: (did, _) {
          if (!did) _cancel();
        },
        child: Scaffold(
          body: SafeArea(
            child: Column(
              children: [
                VNavHeader(title: 'Juftlash', onBack: _cancel),
                const Spacer(),
                PairDigits(digits: digits, waiting: _sentAnswer),
                const Spacer(),
                Padding(
                  padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
                  child: Row(
                    children: [
                      Expanded(
                        child: VButton(
                          label: tr('pair.no'),
                          kind: VButtonKind.secondary,
                          onPressed: _sentAnswer ? null : () => _answer?.complete(false),
                        ),
                      ),
                      const SizedBox(width: 10),
                      Expanded(
                        child: VButton(
                          label: tr('pair.yes'),
                          icon: Icons.check_rounded,
                          busy: _sentAnswer,
                          onPressed: _sentAnswer ? null : () => _answer?.complete(true),
                        ),
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
/// The six-digit pairing check (ADR-0050): the same digits must be on both screens.
class PairDigits extends StatelessWidget {
  const PairDigits({super.key, required this.digits, required this.waiting});
  final String digits;
  final bool waiting;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 24),
      child: Column(
        children: [
          Text(tr('pair.title'), textAlign: TextAlign.center, style: v.title),
          const SizedBox(height: 14),
          Semantics(
            label: digits.split('').join(' '),
            excludeSemantics: true,
            child: FittedBox(
              child: Text(
                digits.length == 6 ? '${digits.substring(0, 3)} ${digits.substring(3)}' : digits,
                style: v.text(44, FontWeight.w600, mono: true, tracking: 0.08),
              ),
            ),
          ),
          const SizedBox(height: 10),
          Text(waiting ? tr('pair.waiting') : tr('pair.body'), textAlign: TextAlign.center, style: v.caption),
        ],
      ),
    );
  }
}

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
              dataModuleStyle: const QrDataModuleStyle(dataModuleShape: QrDataModuleShape.circle, color: Colors.black),
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
  String? _code;

  /// The link gains the relay and new addresses (Wi-Fi switch, hotspot) while the page is open: keep the QR fresh.
  Timer? _refresh;

  @override
  void initState() {
    super.initState();
    Session.instance.addListener(_changed);
    Session.instance.refreshLink();
    _refresh = Timer.periodic(const Duration(seconds: 4), (_) => Session.instance.refreshLink());
    _open();
  }

  /// Unknown devices may connect only while this QR is on screen (ADR-0050 pairing window, 10 minutes at most).
  Future<void> _open() async {
    try {
      await vutric.setVisibility(mode: 'pairing');
      final code = await vutric.myCode();
      if (mounted) setState(() => _code = code);
    } catch (_) {}
    _wait();
  }

  @override
  void dispose() {
    _refresh?.cancel();
    Session.instance.removeListener(_changed);
    vutric.setVisibility(mode: 'trusted_only').catchError((Object _) => vutric.visibility());
    super.dispose();
  }

  void _changed() {
    if (mounted) setState(() {});
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
              if (_code != null) ...[
                const SizedBox(height: 14),
                Semantics(
                  label: '${tr('find.my_code')}: ${_code!.split('').join(' ')}',
                  excludeSemantics: true,
                  child: Column(
                    children: [
                      Text(tr('find.my_code'), style: v.caption),
                      Text(_code!, style: v.text(22, FontWeight.w600, mono: true, tracking: 0.1)),
                    ],
                  ),
                ),
              ],
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

/// Connect without the camera (issue #9, ADR-0059): one field takes the other device's short code, its IPv4
/// address or a whole pairing link, and says which it read while the user types.
class TypedPage extends StatefulWidget {
  const TypedPage({super.key});

  @override
  State<TypedPage> createState() => _TypedPageState();
}

class _TypedPageState extends State<TypedPage> {
  final _text = TextEditingController();
  String _kind = '';
  bool _scan = false;

  @override
  void initState() {
    super.initState();
    _text.addListener(_typed);
    _fromClipboard();
  }

  @override
  void dispose() {
    _text.dispose();
    super.dispose();
  }

  /// A link or code copied on this phone is offered at once.
  Future<void> _fromClipboard() async {
    final clip = (await Clipboard.getData(Clipboard.kTextPlain))?.text?.trim() ?? '';
    if (clip.isEmpty || clip.length > 2048 || !mounted) return;
    final kind = await _kindOf(clip);
    if (kind.isNotEmpty && mounted && _text.text.isEmpty) _text.text = clip;
  }

  Future<String> _kindOf(String t) async {
    try {
      return await vutric.typedKind(text: t);
    } catch (_) {
      return '';
    }
  }

  Future<void> _typed() async {
    final t = _text.text;
    final k = await _kindOf(t);
    if (mounted && t == _text.text && k != _kind) setState(() => _kind = k);
  }

  Future<void> _go() async {
    final t = _text.text.trim();
    if (_kind.isEmpty) return;
    FocusScope.of(context).unfocus();
    final c = _kind == 'link'
        ? await connectWithLink(context, t)
        : await _run(context, () => vutric.connectTyped(text: t, scan: _scan && _kind == 'code'));
    if (c != null && mounted) Navigator.of(context).pop(c);
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return Scaffold(
      backgroundColor: v.bg,
      body: SafeArea(
        child: ListView(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 16),
          children: [
            VNavHeader(title: tr('find.title'), onBack: () => Navigator.pop(context)),
            const SizedBox(height: 12),
            Text('Boshqa qurilmadagi kodni\nyoki manzilni yozing', style: v.title),
            const SizedBox(height: 6),
            Text(
              'Kod QR ostida turadi. Ulangach ikkala ekranda olti raqam chiqadi: ular bir xil boʻlsin.',
              style: v.caption,
            ),
            const SizedBox(height: 18),
            TextField(
              controller: _text,
              autofocus: true,
              autocorrect: false,
              enableSuggestions: false,
              textCapitalization: TextCapitalization.characters,
              style: v.text(18, FontWeight.w600, mono: true),
              onSubmitted: (_) => _go(),
              decoration: InputDecoration(
                hintText: tr('find.hint'),
                helperText: _kind.isEmpty ? ' ' : tr('find.kind.$_kind'),
                filled: true,
                fillColor: v.bgAlt,
                border: OutlineInputBorder(borderRadius: BorderRadius.circular(12), borderSide: BorderSide.none),
              ),
            ),
            if (_kind == 'code')
              SwitchListTile.adaptive(
                contentPadding: EdgeInsets.zero,
                value: _scan,
                onChanged: (x) => setState(() => _scan = x),
                activeTrackColor: v.accent,
                title: Text(tr('find.scan'), style: v.text(15, FontWeight.w500)),
              ),
            const SizedBox(height: 18),
            VButton(label: tr('find.go'), icon: Icons.search_rounded, onPressed: _kind.isEmpty ? null : _go),
          ],
        ),
      ),
    );
  }
}
