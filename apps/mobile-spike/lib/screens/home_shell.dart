// The app's frame: three tabs (Otish · Qabul · Tarix) under the floating glass tab bar, the round QR button beside
// it, and the shared top row (wordmark, this phone's name, settings). It also routes everything the other side
// sends (photos, file offers) to the Qabul tab, whichever tab is open.

import 'dart:async';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../android.dart';
import '../companion.dart';
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
  final _send = GlobalKey<SendTabState>();
  StreamSubscription<AndroidEvent>? _android;
  int _tab = 0;

  @override
  void initState() {
    super.initState();
    _s.addListener(_changed);
    _s.onIncoming = (next) => handleIncoming(context, next, () {
      if (mounted && _tab != 1) setState(() => _tab = 1);
    });
    final bridge = AndroidBridge.instance;
    _android = bridge.events.listen(_onAndroid);
    bridge.takeStartEvent().then((e) {
      if (e != null) _onAndroid(e);
    });
  }

  @override
  void dispose() {
    _android?.cancel();
    _s.removeListener(_changed);
    _s.onIncoming = null;
    super.dispose();
  }

  /// Files from another app's Share menu land in the Otish selection; the tile opens Qabul with this phone's QR.
  Future<void> _onAndroid(AndroidEvent e) async {
    if (!mounted) return;
    switch (e) {
      case SharedFiles(:final paths, :final names):
        setState(() => _tab = 0);
        await WidgetsBinding.instance.endOfFrame;
        _send.currentState?.addFiles(paths, names);
      case ReceiveTile():
        setState(() => _tab = 1);
        if (_s.peer != null) return;
        // The node may still be starting after a cold start from the tile.
        for (var i = 0; i < 40 && _s.myLink == null && _s.bootError == null; i++) {
          await Future<void>.delayed(const Duration(milliseconds: 250));
        }
        if (mounted && _s.myLink != null) await connectFlow(context, way: ConnectWay.wait);
      case PhoneNotice():
        NoticeQueue.instance.add(e);
      case ForeignDevice():
        ForeignDevices.instance.add(e);
    }
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
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text(_s.bootError ?? 'Tayyorlanmoqda, bir soniya…')));
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
                SendTab(key: _send, header: header),
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
