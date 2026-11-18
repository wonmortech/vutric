// The Tarix tab: every send, throw and receive, newest first, grouped by day. A row opens its files with "Qayta
// yuborish" (to the connected device) or "Ulashish" (to any app). Kept on the phone only (history.json, ADR-0011).

import 'dart:io';

import 'package:material_ui/material_ui.dart';

import '../design/tokens.dart';
import '../design/widgets.dart';
import '../session.dart';
import '../transfer_ui.dart';
import 'connect.dart';

class HistoryTab extends StatefulWidget {
  const HistoryTab({super.key, required this.header});
  final Widget header;

  @override
  State<HistoryTab> createState() => _HistoryTabState();
}

class _HistoryTabState extends State<HistoryTab> {
  final _s = Session.instance;

  @override
  void initState() {
    super.initState();
    _s.addListener(_changed);
  }

  @override
  void dispose() {
    _s.removeListener(_changed);
    super.dispose();
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  String _day(DateTime d) {
    final now = DateTime.now();
    final diff = DateTime(now.year, now.month, now.day).difference(DateTime(d.year, d.month, d.day)).inDays;
    if (diff == 0) return 'Bugun';
    if (diff == 1) return 'Kecha';
    const months = ['yan', 'fev', 'mar', 'apr', 'may', 'iyn', 'iyl', 'avg', 'sen', 'okt', 'noy', 'dek'];
    return '${d.day}-${months[d.month - 1]}${d.year == now.year ? '' : ' ${d.year}'}';
  }

  Future<void> _clear() async {
    final v = context.v;
    final yes = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Tarixni tozalaysizmi?', style: v.heading),
        content: Text('Fayllarning oʻzi oʻchmaydi, faqat roʻyxat.', style: v.body),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Bekor qilish')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Tozalash')),
        ],
      ),
    );
    if (yes == true) await _s.clearHistory();
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final items = _s.history;
    final rows = <Widget>[];
    String? lastDay;
    for (final h in items) {
      final day = _day(h.at);
      if (day != lastDay) {
        lastDay = day;
        rows.add(
          Padding(
            padding: const EdgeInsets.fromLTRB(4, 18, 4, 8),
            child: Text(day.toUpperCase(), style: v.text(12, FontWeight.w600, color: v.muted, tracking: 0.06)),
          ),
        );
      }
      rows.add(Padding(padding: const EdgeInsets.only(bottom: 8), child: _Row(item: h)));
    }
    return SafeArea(
      bottom: false,
      child: ListView(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 120),
        children: [
          widget.header,
          const SizedBox(height: 18),
          Row(
            crossAxisAlignment: CrossAxisAlignment.end,
            children: [
              Expanded(child: Text('Tarix', style: v.largeTitle)),
              if (items.isNotEmpty)
                TextButton(
                  onPressed: _clear,
                  child: Text('Tozalash', style: v.text(15, FontWeight.w500, color: v.accent)),
                ),
            ],
          ),
          if (items.isEmpty)
            const Padding(
              padding: EdgeInsets.only(top: 80),
              child: VEmpty(
                icon: Icons.schedule_rounded,
                title: 'Hali boʻsh',
                body: 'Yuborgan va qabul qilgan rasm hamda fayllaringiz shu yerda chiqadi.',
              ),
            ),
          ...rows,
        ],
      ),
    );
  }
}

class _Row extends StatelessWidget {
  const _Row({required this.item});
  final HistoryItem item;

  String get _time => '${item.at.hour.toString().padLeft(2, '0')}:${item.at.minute.toString().padLeft(2, '0')}';

  String _detail() {
    if (item.ms != null && item.files == 1) {
      return '${item.names.isEmpty ? 'Rasm' : item.names.first} · ${item.ms!.round()} ms';
    }
    final speed = item.seconds > 0 ? ' · ${sizeText(item.bytes / item.seconds)}/s' : '';
    return '${item.files} ta · ${sizeText(item.bytes.toDouble())}$speed';
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final tint = item.sent ? v.accent : v.ok;
    return VPressable(
      onTap: () => showModalBottomSheet<void>(
        context: context,
        isScrollControlled: true,
        builder: (_) => _Detail(item: item),
      ),
      semanticLabel: '${item.sent ? 'Yuborildi' : 'Keldi'}: ${item.peer}',
      child: VSurface(
        color: v.raised,
        padding: const EdgeInsets.all(12),
        child: Row(
          children: [
            VIconTile(
              icon: item.sent ? Icons.arrow_upward_rounded : Icons.arrow_downward_rounded,
              size: 40,
              tint: tint,
              background: Color.alphaBlend(tint.withValues(alpha: 0.12), v.raised),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    '${item.sent ? 'Yuborildi' : 'Keldi'} · ${item.peer.isEmpty ? 'qurilma' : item.peer}',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: v.text(15, FontWeight.w600),
                  ),
                  const SizedBox(height: 2),
                  Text(_detail(), maxLines: 1, overflow: TextOverflow.ellipsis, style: v.monoSmall),
                ],
              ),
            ),
            const SizedBox(width: 8),
            Text(_time, style: v.text(13, FontWeight.w500, color: v.muted)),
          ],
        ),
      ),
    );
  }
}

class _Detail extends StatelessWidget {
  const _Detail({required this.item});
  final HistoryItem item;

  Future<void> _resend(BuildContext context, List<String> paths) async {
    // The sheet closes first; everything after runs on the navigator's own (still mounted) context.
    final nav = Navigator.of(context)..pop();
    final s = Session.instance;
    if (!await s.stillConnected()) {
      final ctx = nav.context;
      if (!ctx.mounted) return;
      if (await connectFlow(ctx) == null) return;
    }
    final ctx = nav.context;
    if (!ctx.mounted) return;
    await sendPaths(ctx, paths, [for (final p in paths) baseName(p)]);
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final existing = [
      for (final p in item.paths)
        if (File(p).existsSync() || Directory(p).existsSync()) p,
    ];
    return SafeArea(
      child: ConstrainedBox(
        constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * 0.75),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(item.sent ? 'Yuborildi' : 'Keldi', style: v.title),
              Text(
                '${item.peer} · ${item.files} ta · ${sizeText(item.bytes.toDouble())}'
                '${item.seconds > 0 ? ' · ${item.seconds.toStringAsFixed(1)} s' : ''}',
                style: v.monoSmall,
              ),
              const SizedBox(height: 12),
              Flexible(
                child: ListView(
                  shrinkWrap: true,
                  children: [
                    for (final n in item.names)
                      Padding(
                        padding: const EdgeInsets.symmetric(vertical: 6),
                        child: Row(
                          children: [
                            Icon(fileIcon(n), size: 20, color: v.muted),
                            const SizedBox(width: 10),
                            Expanded(
                              child: Text(
                                n,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: v.text(14.5, FontWeight.w500),
                              ),
                            ),
                          ],
                        ),
                      ),
                    if (item.files > item.names.length)
                      Text('yana ${item.files - item.names.length} ta', style: v.caption),
                  ],
                ),
              ),
              const SizedBox(height: 14),
              if (existing.isEmpty)
                Text('Fayllar endi telefonda yoʻq (yoki galereyaga koʻchirilgan).', style: v.caption)
              else
                Row(
                  children: [
                    Expanded(
                      child: VButton(
                        label: 'Ulashish',
                        kind: VButtonKind.secondary,
                        onPressed: () => shareFiles(existing.where((p) => File(p).existsSync()).toList()),
                      ),
                    ),
                    const SizedBox(width: 10),
                    Expanded(
                      child: VButton(
                        label: 'Qayta yuborish',
                        icon: Icons.arrow_upward_rounded,
                        onPressed: () => _resend(context, existing),
                      ),
                    ),
                  ],
                ),
            ],
          ),
        ),
      ),
    );
  }
}
