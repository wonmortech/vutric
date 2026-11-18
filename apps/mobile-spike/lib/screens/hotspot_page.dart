// Routersiz (backlog A6, route R3): this phone opens a local-only hotspot. Step 1 shows how to join it (a Wi-Fi QR
// the other phone's camera understands, and the name and password for a computer); step 2 is the normal pairing
// QR, which by then carries the hotspot address. The hotspot closes when this page closes or the session ends.

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';
import 'package:qr_flutter/qr_flutter.dart';

import '../android.dart';
import '../design/tokens.dart';
import '../design/widgets.dart';
import '../session.dart';
import '../src/rust/api.dart' as vutric;
import 'connect.dart';

class HotspotPage extends StatefulWidget {
  const HotspotPage({super.key});

  @override
  State<HotspotPage> createState() => _HotspotPageState();
}

class _HotspotPageState extends State<HotspotPage> {
  final _bridge = AndroidBridge.instance;
  ({String ssid, String password})? _net;
  String? _error;
  bool _paired = false;

  @override
  void initState() {
    super.initState();
    _start();
  }

  @override
  void dispose() {
    // Paired: the hotspot is the session's road, so it stays up until the session ends (Session.onDisconnect).
    if (!_paired) _bridge.hotspotStop();
    super.dispose();
  }

  Future<void> _start() async {
    setState(() => _error = null);
    try {
      final n = await _bridge.hotspotStart();
      if (mounted) setState(() => _net = n);
    } on PlatformException catch (e) {
      if (!mounted) return;
      setState(
        () => _error = switch (e.code) {
          'permission' => 'Yaqin-atrofdagi qurilmalar ruxsati kerak. Sozlamalar → Ilovalar → Vutric.',
          'disallowed' => 'Bu telefonda hotspot yoqish taqiqlangan (administrator yoki operator).',
          'incompatible' => 'Hotspot hozir yoqilmaydi: oddiy hotspot yoki Wi-Fi Direct band. Ularni oʻchirib koʻring.',
          'no_channel' => 'Boʻsh Wi-Fi kanali topilmadi. Birozdan keyin qayta urinib koʻring.',
          'unsupported' => 'Android 8 yoki yangisi kerak.',
          _ => 'Hotspot ochilmadi (${e.code}).',
        },
      );
    }
  }

  Future<void> _pair() async {
    // The node notices the new interface on its own; ask for the link again so the QR carries the hotspot address.
    await Session.instance.refreshLink();
    if (!mounted) return;
    final c = await Navigator.of(context).push<vutric.Connected>(MaterialPageRoute(builder: (_) => const MyQrPage()));
    if (c == null || !mounted) return;
    _paired = true;
    Session.instance.onDisconnect = _bridge.hotspotStop;
    Navigator.of(context).pop(c);
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final n = _net;
    final w = MediaQuery.sizeOf(context).width;
    return Scaffold(
      backgroundColor: v.bg,
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              VNavHeader(title: 'Routersiz', onBack: () => Navigator.pop(context)),
              const SizedBox(height: 12),
              Text('1. Boshqa qurilma\nshu Wi-Fi’ga ulansin', style: v.title),
              const SizedBox(height: 6),
              Text(
                'Internet kerak emas. Telefon kamerasi bilan skanerlang yoki kompyuterda nom va parolni kiriting.',
                style: v.caption,
              ),
              const Spacer(),
              if (_error != null)
                VNotice(title: 'Hotspot ochilmadi', body: _error)
              else if (n == null)
                const Center(child: CircularProgressIndicator())
              else ...[
                Center(
                  child: Container(
                    padding: const EdgeInsets.all(16),
                    decoration: BoxDecoration(color: Colors.white, borderRadius: BorderRadius.circular(24)),
                    child: QrImageView(
                      data: wifiQr(n.ssid, n.password),
                      size: (w * 0.5).clamp(180.0, 260.0),
                      padding: EdgeInsets.zero,
                      semanticsLabel: 'Wi-Fi ulanish QR kodi',
                    ),
                  ),
                ),
                const SizedBox(height: 18),
                _Field(label: 'Tarmoq', value: n.ssid),
                const SizedBox(height: 8),
                _Field(label: 'Parol', value: n.password),
              ],
              const Spacer(),
              if (_error != null)
                VButton(label: 'Qayta urinish', icon: Icons.refresh_rounded, onPressed: _start)
              else
                VButton(
                  label: 'Ulandi: 2. QR kodim',
                  icon: Icons.arrow_forward_rounded,
                  onPressed: n == null ? null : _pair,
                ),
            ],
          ),
        ),
      ),
    );
  }
}

/// The `WIFI:` QR format phone cameras join directly (special characters escaped as the format requires).
String wifiQr(String ssid, String password) {
  String esc(String s) => s.replaceAllMapped(RegExp(r'([\\;,:"])'), (m) => '\\${m[1]}');
  return 'WIFI:T:WPA;S:${esc(ssid)};P:${esc(password)};;';
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
