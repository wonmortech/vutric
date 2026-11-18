// The route picker (backlog Rx, ADR-0060): Avtomatik, Faqat shu tarmoq, Internet orqali, Tarmoqsiz, Bluetooth.
// The same list sits in Sozlamalar and, as a chip, in the connect sheet. The choice lives in Rust's settings
// (`route`); a choice that reaches differently restarts the node, so an open session is asked about first.

import 'package:material_ui/material_ui.dart';

import '../design/tokens.dart';
import '../design/widgets.dart';
import '../i18n.dart';
import '../session.dart';

/// The choices in the order ADR-0060 lists them; `bluetooth` is shown but not selectable until R4.
const routeChoices = ['auto', 'lan_only', 'internet', 'offline', 'bluetooth'];
const _built = {'auto', 'lan_only', 'internet', 'offline'};

IconData routeIcon(String r) => switch (r) {
  'lan_only' => Icons.router_outlined,
  'internet' => Icons.public_rounded,
  'offline' => Icons.wifi_tethering_rounded,
  'bluetooth' => Icons.bluetooth_rounded,
  _ => Icons.auto_mode_rounded,
};

/// "Yoʻl: Avtomatik ▾" in the connect sheet.
class RouteChip extends StatelessWidget {
  const RouteChip({super.key, required this.route, required this.onTap});
  final String route;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return VPressable(
      onTap: onTap,
      semanticLabel: '${tr('route.title')}: ${tr('route.picker.$route')}',
      child: ExcludeSemantics(
        child: Container(
          constraints: const BoxConstraints(maxWidth: 170),
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
          decoration: BoxDecoration(color: v.bgAlt, borderRadius: BorderRadius.circular(v.rPill.clamp(0, 16))),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(routeIcon(route), size: 15, color: v.accent),
              const SizedBox(width: 6),
              Flexible(
                child: Text(
                  tr('route.picker.$route'),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: v.text(13, FontWeight.w600),
                ),
              ),
              Icon(Icons.expand_more_rounded, size: 16, color: v.muted),
            ],
          ),
        ),
      ),
    );
  }
}

/// The list itself, for Sozlamalar and the sheet.
class RouteList extends StatelessWidget {
  const RouteList({super.key, required this.current, required this.onPick});
  final String current;
  final ValueChanged<String> onPick;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return Column(
      children: [
        for (final r in routeChoices)
          Semantics(
            selected: r == current,
            enabled: _built.contains(r),
            child: InkWell(
              onTap: _built.contains(r) ? () => onPick(r) : null,
              child: ConstrainedBox(
                constraints: const BoxConstraints(minHeight: 56),
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 8),
                  child: Row(
                    children: [
                      Icon(routeIcon(r), size: 20, color: _built.contains(r) ? v.ink : v.muted),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              tr('route.picker.$r'),
                              style: v.text(15, FontWeight.w500, color: _built.contains(r) ? v.ink : v.muted),
                            ),
                            Text(tr('route.picker.$r.hint'), style: v.caption),
                          ],
                        ),
                      ),
                      AnimatedOpacity(
                        duration: VMotion.fast,
                        opacity: r == current ? 1 : 0,
                        child: Icon(Icons.check_rounded, color: v.accent),
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ),
      ],
    );
  }
}

/// Opens the picker as a sheet and applies the choice (asking first when it would close the open session).
Future<void> pickRoute(BuildContext context) async {
  final s = Session.instance;
  final choice = await showModalBottomSheet<String>(
    context: context,
    isScrollControlled: true,
    builder: (ctx) => SafeArea(
      child: SingleChildScrollView(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(tr('route.title'), style: ctx.v.title),
            const SizedBox(height: 10),
            VSurface(
              color: ctx.v.raised,
              padding: const EdgeInsets.symmetric(vertical: 4),
              child: RouteList(current: s.route, onPick: (r) => Navigator.pop(ctx, r)),
            ),
          ],
        ),
      ),
    ),
  );
  if (choice == null || !context.mounted) return;
  await applyRoute(context, choice);
}

/// Saves [choice]; when it restarts the node while a session is open, asks first.
Future<void> applyRoute(BuildContext context, String choice) async {
  final s = Session.instance;
  if (choice == s.route) return;
  if (s.peer != null && routeReachChanges(s.route, choice)) {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(tr('route.picker.$choice')),
        content: const Text('Ochiq ulanish yopiladi va qayta ulanish kerak boʻladi.'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Bekor qilish')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Oʻzgartirish')),
        ],
      ),
    );
    if (ok != true) return;
  }
  await s.setRoute(choice);
}
