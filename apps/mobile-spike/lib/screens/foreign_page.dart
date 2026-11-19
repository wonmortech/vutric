// Other apps (C10, C7; ADR-0066, ADR-0067): send to LocalSend and KDE Connect devices on this Wi-Fi, take what
// they offer (always asked), and magic-wormhole codes over the internet. The adapters run while this page is open.

import 'dart:async';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../companion.dart';
import '../compat.dart';
import '../design/tokens.dart';
import '../design/widgets.dart';
import '../i18n.dart';
import '../session.dart';
import '../transfer_ui.dart';
import '../ui.dart';

class ForeignPage extends StatefulWidget {
  const ForeignPage({super.key});

  @override
  State<ForeignPage> createState() => _ForeignPageState();
}

class _ForeignPageState extends State<ForeignPage> {
  final _c = Compat.instance;
  StreamSubscription<CompatEvent>? _asks;
  final Map<String, List<String>> _arrived = {};
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    _c.addListener(_changed);
    _asks = _c.asks.listen(_onAsk);
    unawaited(_c.open());
  }

  @override
  void dispose() {
    _c.removeListener(_changed);
    unawaited(_asks?.cancel());
    unawaited(_c.close());
    super.dispose();
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  void _say(String text) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(text)));
  }

  Future<void> _onAsk(CompatEvent e) async {
    if (!mounted) return;
    switch (e) {
      case CompatOffer():
        final yes = await _confirm(
          title: tr('compat.offer.title', {'peer': e.alias.isEmpty ? '?' : e.alias}),
          body: tr('compat.offer.body', {
            'n': '${e.files.length}',
            'size': sizeText(e.bytes.toDouble()),
            'app': tr('foreign.${e.via}'),
          }),
          yes: tr('compat.accept'),
        );
        await _c.decide(e.sessionId, [for (final f in e.files) f.id], accept: yes).catchError((Object _) {});
      case CompatPairAsk():
        final yes = await _confirm(
          title: tr('compat.pair.title', {'peer': e.alias.isEmpty ? '?' : e.alias}),
          body: tr('compat.pair.body', {'code': e.code}),
          yes: tr('compat.pair.yes'),
        );
        await _c.decide(e.sessionId, const ['pair'], accept: yes).catchError((Object _) {});
      case CompatReceived():
        (_arrived[e.sessionId] ??= []).add(e.path);
      case CompatDone(:final sessionId, :final cancelled):
        final paths = _arrived.remove(sessionId) ?? const [];
        if (cancelled || paths.isEmpty) return;
        final n = await _keep(paths);
        _say(tr('compat.received', {'n': '$n'}));
      case CompatText(:final text):
        if (text.isNotEmpty) await offerClipboard(context, text, front: true, from: tr('foreign.title'));
      case CompatAnswer(:final reason):
        if (reason != null) _say(errorText(reason));
      case CompatSeen() || CompatPaired() || CompatClosed():
        break;
    }
  }

  /// Moves arrived files from the adapters' staging folder into Vutric's own folder (and the gallery, by the
  /// settings); returns how many were kept.
  Future<int> _keep(List<String> staged) async {
    final dir = Directory('${await receiveDir()}/${localDate()}');
    await dir.create(recursive: true);
    final kept = <String>[];
    for (final p in staged) {
      final src = File(p);
      var dst = File('${dir.path}/${baseName(p)}');
      for (var i = 1; await dst.exists(); i++) {
        dst = File('${dir.path}/$i-${baseName(p)}');
      }
      try {
        kept.add((await src.rename(dst.path)).path);
      } on FileSystemException {
        try {
          kept.add((await src.copy(dst.path)).path);
          await src.delete();
        } on FileSystemException {
          // Left in staging; the OS clears the cache.
        }
      }
    }
    await saveMediaToGallery(kept);
    return kept.length;
  }

  Future<bool> _confirm({required String title, required String body, required String yes}) async {
    final r = await showDialog<bool>(
      context: context,
      barrierDismissible: false,
      builder: (ctx) => AlertDialog(
        title: Text(title),
        content: Text(body),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: Text(tr('compat.refuse'))),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: Text(yes)),
        ],
      ),
    );
    return r == true;
  }

  Future<void> _sendTo(CompatDevice d) async {
    if (_busy) return;
    final picked = await pickAnyFiles();
    if (picked.isEmpty || !mounted) return;
    setState(() => _busy = true);
    final reason = await _c.send(d, picked.paths);
    if (mounted) setState(() => _busy = false);
    _say(reason == null ? tr('compat.sent', {'peer': d.name.isEmpty ? d.host : d.name}) : errorText(reason));
  }

  Future<void> _wormholeSend() async {
    if (_busy) return;
    final picked = await pickAnyFiles();
    if (picked.isEmpty || !mounted) return;
    setState(() => _busy = true);
    try {
      final w = await _c.wormholeSend(paths: picked.paths.take(1).toList());
      if (!mounted) return;
      unawaited(w.finished.then((reason) => _say(reason == null ? tr('wormhole.sent') : errorText(reason))));
      await showDialog<void>(
        context: context,
        builder: (_) => WormholeCodeDialog(code: w.code),
      );
    } catch (e) {
      _say(errorText(e));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _wormholeReceive() async {
    final code = await showDialog<String>(context: context, builder: (_) => const WormholeCodeEntry());
    if (code == null || code.trim().isEmpty) return;
    final err = await _c.wormholeReceive(code);
    if (err != null) _say(errorText(err));
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final devices = _c.devices.values.toList()..sort((a, b) => a.name.compareTo(b.name));
    final internet = wormholeAllowed(Session.instance.route);
    return Scaffold(
      body: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            VNavHeader(title: tr('foreign.page'), onBack: () => Navigator.of(context).maybePop()),
            Expanded(
              child: ListView(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
                children: [
                  if (_c.unavailable) ...[VNotice(title: tr('compat.unavailable')), const SizedBox(height: 12)],
                  Text(tr('foreign.title'), style: v.label),
                  const SizedBox(height: 8),
                  if (devices.isEmpty)
                    Padding(
                      padding: const EdgeInsets.symmetric(vertical: 12),
                      child: Row(
                        children: [
                          if (_c.running)
                            const SizedBox(width: 18, height: 18, child: CircularProgressIndicator(strokeWidth: 2)),
                          if (_c.running) const SizedBox(width: 12),
                          Expanded(child: Text(tr('compat.looking'), style: v.caption)),
                        ],
                      ),
                    )
                  else
                    for (final d in devices)
                      Padding(
                        padding: const EdgeInsets.only(bottom: 8),
                        child: VRowCard(
                          icon: d.type == 'mobile' ? Icons.smartphone_rounded : Icons.computer_rounded,
                          title: d.name.isEmpty ? d.host : d.name,
                          subtitle: _subtitle(d),
                          onTap: _busy || !_c.running ? null : () => _sendTo(d),
                        ),
                      ),
                  Text(tr('compat.hint'), style: v.caption),
                  if (internet) ...[
                    const SizedBox(height: 24),
                    Text(tr('wormhole.title'), style: v.label),
                    const SizedBox(height: 4),
                    Text(tr('wormhole.internet'), style: v.caption),
                    const SizedBox(height: 8),
                    VRowCard(
                      icon: Icons.north_east_rounded,
                      title: tr('wormhole.send'),
                      subtitle: tr('wormhole.send.sub'),
                      onTap: _busy || !_c.running ? null : _wormholeSend,
                    ),
                    const SizedBox(height: 8),
                    VRowCard(
                      icon: Icons.south_west_rounded,
                      title: tr('wormhole.receive'),
                      subtitle: tr('wormhole.receive.sub'),
                      onTap: !_c.running ? null : _wormholeReceive,
                    ),
                  ],
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  String _subtitle(CompatDevice d) {
    final via = tr('foreign.via', {'app': tr('foreign.${d.via}')});
    if (d.via == 'kdeconnect' && !_c.pairedKde.contains(d.deviceId)) return '$via · ${tr('compat.kde_pair_first')}';
    return '$via · ${d.host}';
  }
}

/// The wormhole code to read out to the other person; it is the password, so it is shown, never logged.
class WormholeCodeDialog extends StatelessWidget {
  const WormholeCodeDialog({super.key, required this.code});
  final String code;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return AlertDialog(
      title: Text(tr('wormhole.code.title')),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SelectableText(code, style: v.title),
          const SizedBox(height: 8),
          Text(tr('wormhole.code.body'), style: v.caption),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Clipboard.setData(ClipboardData(text: code)),
          child: Text(tr('companion.clipboard.copy')),
        ),
        FilledButton(onPressed: () => Navigator.pop(context), child: Text(tr('companion.clipboard.close'))),
      ],
    );
  }
}

/// Typing the code the other person read out.
class WormholeCodeEntry extends StatefulWidget {
  const WormholeCodeEntry({super.key});

  @override
  State<WormholeCodeEntry> createState() => _WormholeCodeEntryState();
}

class _WormholeCodeEntryState extends State<WormholeCodeEntry> {
  final _text = TextEditingController();

  @override
  void dispose() {
    _text.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(tr('wormhole.receive')),
      content: TextField(
        controller: _text,
        autofocus: true,
        autocorrect: false,
        enableSuggestions: false,
        maxLength: 80,
        decoration: InputDecoration(hintText: tr('wormhole.code.hint')),
        onSubmitted: (t) => Navigator.pop(context, t),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(tr('compat.refuse'))),
        FilledButton(onPressed: () => Navigator.pop(context, _text.text), child: Text(tr('compat.accept'))),
      ],
    );
  }
}
