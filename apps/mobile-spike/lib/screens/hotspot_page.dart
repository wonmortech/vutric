// Routersiz (backlog A6, route R3, ADR-0058): this phone opens a local-only hotspot and shows ONE QR that carries
// both the network (name, password, security) and the pairing link. Another Vutric phone scans it, joins the
// network and pairs in one go (joinHotspot below); a phone without Vutric gets the plain `WIFI:` QR from the
// "Kamera bilan" tab, and a computer types the name and password. The hotspot closes when this page closes
// unpaired, or when the session it carries ends.

import 'dart:async';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';
import 'package:qr_flutter/qr_flutter.dart';

import '../android.dart';
import '../design/tokens.dart';
import '../design/widgets.dart';
import '../i18n.dart';
import '../ios.dart';
import '../session.dart';
import '../src/rust/api.dart' as vutric;
import '../ui.dart';
import 'connect.dart';

class HotspotPage extends StatefulWidget {
  const HotspotPage({super.key});

  @override
  State<HotspotPage> createState() => _HotspotPageState();
}

class _HotspotPageState extends State<HotspotPage> {
  final _bridge = AndroidBridge.instance;
  HotspotCredentials? _net;
  String? _link;
  String? _wifiQr;
  String? _error;
  bool _paired = false;
  bool _closing = false;
  int _qr = 0;

  /// The phone notices the hotspot interface a moment after it is up: refresh the link so it carries its address.
  Timer? _refresh;

  @override
  void initState() {
    super.initState();
    _start();
  }

  @override
  void dispose() {
    _refresh?.cancel();
    // Paired: the hotspot is the session's road, so it stays up until the session ends (Session.onDisconnect).
    if (!_paired) {
      _bridge.hotspotStop();
      vutric.setVisibility(mode: 'trusted_only').catchError((Object _) => vutric.visibility());
    }
    super.dispose();
  }

  Future<void> _start() async {
    setState(() => _error = null);
    try {
      final n = await _bridge.hotspotStart();
      if (!mounted) {
        // Left during the start: dispose's stop ran too early, so close the hotspot that came up after it.
        unawaited(_bridge.hotspotStop());
        return;
      }
      final wifi = await vutric.wifiQr(ssid: n.ssid, password: n.password, security: n.security, hidden: false);
      setState(() {
        _net = n;
        _wifiQr = wifi;
      });
      // The window first: its one-time secret then rides in the first QR shown (ADR-0064).
      await vutric.setVisibility(mode: 'pairing');
      await _relink();
      _refresh = Timer.periodic(const Duration(seconds: 4), (_) => _relink());
      unawaited(_wait());
    } on PlatformException catch (e) {
      if (!mounted) return;
      setState(() => _error = hotspotError(e.code));
    } catch (e) {
      if (mounted) setState(() => _error = errorText(e));
    }
  }

  Future<void> _relink() async {
    final n = _net;
    if (n == null) return;
    try {
      final link = await vutric.hotspotLink(ssid: n.ssid, password: n.password, security: n.security);
      if (mounted && link != _link) setState(() => _link = link);
    } catch (e) {
      if (mounted) setState(() => _error = errorText(e));
    }
  }

  /// Waits for the other device on this phone's own session listener (same as "Kutish: QR kodimni").
  Future<void> _wait() async {
    try {
      final c = await Session.instance.connect(vutric.acceptSession, verify: (_) => checkDigits(context));
      if (!mounted) return;
      HapticFeedback.mediumImpact();
      _paired = true;
      _closing = true;
      Session.instance.onDisconnect = _bridge.hotspotStop;
      unawaited(vutric.setVisibility(mode: 'trusted_only'));
      Navigator.of(context).pop(c);
    } catch (e) {
      if (!mounted || _closing) return;
      if (e is PairRefused) {
        // "Farq qiladi" on either screen: nothing trusted, the QR stays up for the next try.
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(tr('pair.refused'))));
        unawaited(_wait());
        return;
      }
      if (isCancelled(e)) return;
      setState(() => _error = errorText(e));
    }
  }

  Future<void> _back() async {
    _closing = true;
    await disconnectNow();
    if (mounted) Navigator.pop(context);
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final n = _net;
    final w = MediaQuery.sizeOf(context).width;
    final qr = _qr == 0 ? _link : _wifiQr;
    return PopScope(
      canPop: false,
      onPopInvokedWithResult: (did, _) {
        if (!did) _back();
      },
      child: Scaffold(
        backgroundColor: v.bg,
        body: SafeArea(
          child: ListView(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 16),
            children: [
              VNavHeader(title: 'Routersiz', onBack: _back),
              const SizedBox(height: 12),
              Text('Boshqa qurilma shu\nQR kodni skanerlasin', style: v.title),
              const SizedBox(height: 6),
              Text(
                'Internet kerak emas. Vutric oʻzi Wi-Fi’ga ulanadi va juftlaydi. Kompyuterda nom va parolni kiriting.',
                style: v.caption,
              ),
              const SizedBox(height: 18),
              if (_error != null) ...[
                VNotice(title: 'Hotspot ochilmadi', body: _error),
                const SizedBox(height: 18),
                VButton(label: 'Qayta urinish', icon: Icons.refresh_rounded, onPressed: _start),
              ] else if (n == null || qr == null)
                const Padding(
                  padding: EdgeInsets.all(48),
                  child: Center(child: CircularProgressIndicator()),
                )
              else ...[
                VSegmented(
                  labels: [tr('hotspot.qr.vutric'), tr('hotspot.qr.camera')],
                  index: _qr,
                  onChanged: (i) => setState(() => _qr = i),
                ),
                const SizedBox(height: 18),
                Center(
                  child: _qr == 0
                      ? PairingQr(data: qr, size: (w * 0.56).clamp(180.0, 280.0))
                      : Container(
                          padding: const EdgeInsets.all(16),
                          decoration: BoxDecoration(color: Colors.white, borderRadius: BorderRadius.circular(24)),
                          child: QrImageView(
                            data: qr,
                            size: (w * 0.5).clamp(180.0, 260.0),
                            padding: EdgeInsets.zero,
                            semanticsLabel: 'Wi-Fi ulanish QR kodi',
                          ),
                        ),
                ),
                const SizedBox(height: 18),
                _Field(label: 'Tarmoq', value: n.ssid),
                const SizedBox(height: 8),
                if (n.password.isNotEmpty) _Field(label: 'Parol', value: n.password),
                const SizedBox(height: 14),
                Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    SizedBox.square(dimension: 14, child: CircularProgressIndicator(strokeWidth: 2, color: v.muted)),
                    const SizedBox(width: 8),
                    Flexible(child: Text('Ikkinchi qurilma kutilmoqda…', style: v.caption)),
                  ],
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}

/// The hotspot start error, said plainly.
String hotspotError(String code) => switch (code) {
  'permission' => 'Yaqin-atrofdagi qurilmalar ruxsati kerak. Sozlamalar → Ilovalar → Vutric.',
  'disallowed' => 'Bu telefonda hotspot yoqish taqiqlangan (administrator yoki operator).',
  'incompatible' => 'Hotspot hozir yoqilmaydi: oddiy hotspot yoki Wi-Fi Direct band. Ularni oʻchirib koʻring.',
  'no_channel' => 'Boʻsh Wi-Fi kanali topilmadi. Birozdan keyin qayta urinib koʻring.',
  'unsupported' => 'Android 8 yoki yangisi kerak.',
  _ => 'Hotspot ochilmadi ($code).',
};

/// The joining side of R3: asks once, joins the hotspot a scanned link carries, and restarts the node so its
/// sockets ride the new network. Leaves the network again when the session ends. False: not joined (the user
/// said no, the network was not found, or this Android is too old, in which case it says how to join by hand).
Future<bool> joinHotspot(BuildContext context, vutric.HotspotNet net) async {
  final go = await showDialog<bool>(
    context: context,
    builder: (ctx) => AlertDialog(
      title: Text(tr('hotspot.join.title', {'ssid': net.ssid})),
      content: Text(tr('hotspot.join.body')),
      actions: [
        TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Bekor qilish')),
        FilledButton(onPressed: () => Navigator.pop(ctx, true), child: Text(tr('hotspot.join.go'))),
      ],
    ),
  );
  if (go != true || !context.mounted) return false;
  final messenger = ScaffoldMessenger.of(context);
  messenger.showSnackBar(SnackBar(content: Text(tr('hotspot.join.wait')), duration: const Duration(seconds: 30)));
  final bridge = AndroidBridge.instance;
  try {
    // iOS: NEHotspotConfiguration (I5); the same answer codes as Android's LinkManager.
    if (Platform.isIOS) {
      await IosBridge.instance.hotspotJoin(
        ssid: net.ssid,
        password: net.password,
        security: net.security,
        hidden: net.hidden,
      );
    } else {
      await bridge.hotspotJoin(ssid: net.ssid, password: net.password, security: net.security, hidden: net.hidden);
    }
  } on PlatformException catch (e) {
    messenger
      ..hideCurrentSnackBar()
      ..showSnackBar(
        SnackBar(
          content: Text(
            e.code == 'unsupported'
                ? tr('hotspot.join.manual', {'ssid': net.ssid})
                : '${tr('hotspot.join.failed')} (${e.code})',
          ),
        ),
      );
    return false;
  }
  messenger.hideCurrentSnackBar();
  // Sockets opened before the join stay on the old network: start the node again on the hotspot.
  await Session.instance.restart();
  Session.instance.onDisconnect = () {
    Platform.isIOS ? IosBridge.instance.hotspotLeave(net.ssid) : bridge.hotspotLeave();
  };
  return true;
}

class _Field extends StatelessWidget {
  const _Field({required this.label, required this.value});
  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return VSurface(
      color: v.raised,
      padding: const EdgeInsets.fromLTRB(14, 10, 6, 10),
      child: Row(
        children: [
          SizedBox(width: 64, child: Text(label, style: v.caption)),
          Expanded(child: SelectableText(value, style: v.text(15, FontWeight.w600, mono: true))),
          IconButton(
            tooltip: 'Nusxalash',
            icon: Icon(Icons.copy_rounded, size: 18, color: v.muted),
            onPressed: () {
              Clipboard.setData(ClipboardData(text: value));
              HapticFeedback.selectionClick();
            },
          ),
        ],
      ),
    );
  }
}
