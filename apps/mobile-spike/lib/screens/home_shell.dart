// The app's frame: three tabs (Otish · Qabul · Tarix) under the floating glass tab bar, the round QR button beside
// it, and the shared top row (wordmark, this phone's name, settings). It also routes everything the other side
// sends (photos, file offers) to the Qabul tab, whichever tab is open.

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../design/tokens.dart';
import '../design/widgets.dart';
import '../session.dart';
import '../settings_page.dart';
import 'connect.dart';
import 'history_tab.dart';
import 'receive_tab.dart';
import 'send_tab.dart';

class HomeShell extends StatefulWidget {
  const HomeShell({super.key});

  @override
  State<HomeShell> createState() => _HomeShellState();
}

class _HomeShellState extends State<HomeShell> {
  final _s = Session.instance;
  int _tab = 0;

  @override
  void initState() {
    super.initState();
    _s.addListener(_changed);
    _s.onIncoming = (next) => handleIncoming(context, next, () {
      if (mounted && _tab != 1) setState(() => _tab = 1);
    });
  }

  @override
  void dispose() {
    _s.removeListener(_changed);
    _s.onIncoming = null;
    super.dispose();
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  Future<void> _rename() async {
    final v = context.v;
    final c = TextEditingController(text: _s.deviceName);
    final name = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Qurilma nomi', style: v.heading),
        content: TextField(
          controller: c,
          autofocus: true,
          maxLength: 32,
          textCapitalization: TextCapitalization.sentences,
          decoration: const InputDecoration(hintText: 'Masalan: Azizning telefoni'),
          onSubmitted: (t) => Navigator.pop(ctx, t),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Bekor qilish')),
          FilledButton(onPressed: () => Navigator.pop(ctx, c.text), child: const Text('Saqlash')),
        ],
      ),
    );
    c.dispose();
    if (name == null) return;
    if (await _s.setDeviceName(name)) await _s.restart();
  }

  void _settings() => Navigator.of(context).push(MaterialPageRoute(builder: (_) => const SettingsPage()));

  Widget _header(VutricTokens v) {
    final peer = _s.peer;
    return Row(
      children: [
        const VWordmark(),
        const Spacer(),
        VPressable(
          onTap: peer == null ? _rename : null,
          semanticLabel: 'Qurilma nomi: ${_s.deviceName}',
          child: Container(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
            decoration: BoxDecoration(color: v.bgAlt, borderRadius: BorderRadius.circular(v.rPill.clamp(0, 16))),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Container(
                  width: 7,
                  height: 7,
                  decoration: BoxDecoration(
                    shape: BoxShape.circle,
                    color: _s.bootError != null ? v.err : (_s.myLink == null ? v.muted : v.ok),
                  ),
                ),
                const SizedBox(width: 6),
                ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 140),
                  child: Text(
                    _s.deviceName,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: v.text(13, FontWeight.w600, mono: true),
                  ),
                ),
              ],
            ),
          ),
        ),
        const SizedBox(width: 8),
        VCircleButton(icon: Icons.tune_rounded, onTap: _settings, label: 'Sozlamalar'),
      ],
    );
  }

  Future<void> _side() async {
    if (_s.peer != null) {
      setState(() => _tab = 1);
      return;
    }
    if (_s.myLink == null) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(_s.bootError ?? 'Tayyorlanmoqda, bir soniya…')),
      );
      return;
    }
    await connectFlow(context);
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final header = _header(v);
    final bootError = _s.bootError;
    return AnnotatedRegion<SystemUiOverlayStyle>(
      value: v.dark ? SystemUiOverlayStyle.light : SystemUiOverlayStyle.dark,
      child: Scaffold(
        backgroundColor: v.bg,
        body: Stack(
          children: [
            IndexedStack(
              index: _tab,
              children: [
                SendTab(header: header),
                ReceiveTab(header: header),
                HistoryTab(header: header),
              ],
            ),
            if (bootError != null)
              Positioned(
                left: 16,
                right: 16,
                top: MediaQuery.paddingOf(context).top + 56,
                child: VNotice(title: 'Tarmoqqa chiqib boʻlmadi', body: bootError, onClose: _s.restart),
              ),
            Positioned(
              left: 16,
              right: 16,
              bottom: 12 + MediaQuery.paddingOf(context).bottom,
              child: VGlassTabBar(
                tabs: const [
                  VTab(Icons.arrow_upward_rounded, 'Otish'),
                  VTab(Icons.arrow_downward_rounded, 'Qabul'),
                  VTab(Icons.schedule_rounded, 'Tarix'),
                ],
                index: _tab,
                onTap: (i) => setState(() => _tab = i),
                sideIcon: _s.peer == null ? Icons.qr_code_2_rounded : Icons.link_rounded,
                sideLabel: _s.peer == null ? 'Ulanish' : 'Ulangan qurilma',
                onSide: _side,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
