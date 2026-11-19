// T3: the paired devices (ADR-0050 trust store) and one device's own receiving rules (ADR-0063 `devices`
// overrides). Every override can be left on "Umumiy", which follows the general setting; all "Umumiy" removes
// the entry. Rust checks the id and the values and keeps at most 1024 entries.

import 'dart:convert';

import 'package:material_ui/material_ui.dart';

import '../design/tokens.dart';
import '../design/widgets.dart';
import '../i18n.dart';
import '../src/rust/api.dart' as vutric;
import '../ui.dart';

/// "3 kun oldin" from Unix seconds, for "oxirgi marta …".
String agoText(int unixSeconds, DateTime now) {
  if (unixSeconds <= 0) return '—';
  final d = now.difference(DateTime.fromMillisecondsSinceEpoch(unixSeconds * 1000));
  if (d.inMinutes < 1) return 'hozirgina';
  if (d.inHours < 1) return '${d.inMinutes} daqiqa oldin';
  if (d.inDays < 1) return '${d.inHours} soat oldin';
  if (d.inDays < 30) return '${d.inDays} kun oldin';
  return '${(d.inDays / 30).floor()} oy oldin';
}

IconData platformIcon(String p) => switch (p) {
  'android' || 'ios' => Icons.smartphone_rounded,
  _ => Icons.computer_rounded,
};

/// The list of paired devices, newest first; a tap opens the device.
class DeviceList extends StatelessWidget {
  const DeviceList({super.key, required this.devices, required this.onOpen});
  final List<vutric.DeviceInfo> devices;
  final ValueChanged<vutric.DeviceInfo> onOpen;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    if (devices.isEmpty) {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: 8),
        child: Text(tr('devices.empty'), style: v.caption),
      );
    }
    final now = DateTime.now();
    return Column(
      children: [
        for (final (i, d) in devices.indexed) ...[
          if (i > 0) const SizedBox(height: 8),
          VRowCard(
            icon: platformIcon(d.platform),
            title: d.name.isEmpty ? d.code : d.name,
            subtitle: [
              tr('devices.method.${d.method}'),
              tr('devices.seen', {'d': agoText(d.lastSeen.toInt(), now)}),
              if (d.hasRules) tr('devices.rules'),
            ].join(' · '),
            onTap: () => onOpen(d),
            trailing: Icon(Icons.chevron_right_rounded, color: v.muted),
          ),
        ],
      ],
    );
  }
}

/// One device: who it is, how it was paired, and its own receiving rules.
class DevicePage extends StatefulWidget {
  const DevicePage({super.key, required this.device, this.load, this.save, this.forget});
  final vutric.DeviceInfo device;

  /// Injected in tests; default to the Rust bridge.
  final Future<String> Function(String id)? load;
  final Future<void> Function(String id, String json)? save;
  final Future<bool> Function(String id)? forget;

  @override
  State<DevicePage> createState() => _DevicePageState();
}

class _DevicePageState extends State<DevicePage> {
  Map<String, dynamic>? _rules;
  String? _error;

  Future<String> _load(String id) => (widget.load ?? (id) => vutric.deviceRules(id: id))(id);
  Future<void> _save(String id, String json) =>
      (widget.save ?? (id, json) => vutric.putDeviceRules(id: id, json: json))(id, json);
  Future<bool> _forget(String id) => (widget.forget ?? (id) => vutric.forgetDevice(id: id))(id);

  @override
  void initState() {
    super.initState();
    _read();
  }

  Future<void> _read() async {
    try {
      final r = jsonDecode(await _load(widget.device.id)) as Map<String, dynamic>;
      if (mounted) setState(() => _rules = r);
    } catch (e) {
      if (mounted) setState(() => _error = errorText(e));
    }
  }

  Future<void> _set(String key, Object? value) async {
    final r = _rules;
    if (r == null) return;
    setState(() => r[key] = value);
    try {
      await _save(widget.device.id, jsonEncode(r));
      if (mounted) setState(() => _error = null);
    } catch (e) {
      if (mounted) setState(() => _error = 'Saqlab boʻlmadi: ${errorText(e)}');
    }
  }

  Future<void> _confirmForget() async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(tr('devices.forget')),
        content: Text(tr('devices.forget.confirm')),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Bekor qilish')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: Text(tr('devices.forget'))),
        ],
      ),
    );
    if (ok != true) return;
    try {
      await _forget(widget.device.id);
      if (mounted) Navigator.pop(context, true);
    } catch (e) {
      if (mounted) setState(() => _error = errorText(e));
    }
  }

  static String _acceptOf(Object? a) => a is Map ? '${a['mode']}' : 'general';
  static Object? _acceptJson(String mode) => switch (mode) {
    'general' => null,
    'up_to' => {'mode': 'up_to', 'max_bytes': 100 << 20},
    _ => {'mode': mode},
  };
  static String _boolOf(Object? b) => b == null ? 'general' : (b == true ? 'yes' : 'no');
  static bool? _boolJson(String x) => x == 'general' ? null : x == 'yes';

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final d = widget.device;
    final r = _rules;
    final general = tr('devices.rules.general');
    return Scaffold(
      backgroundColor: v.bg,
      body: SafeArea(
        child: ListView(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 32),
          children: [
            VNavHeader(title: tr('devices.title'), onBack: () => Navigator.pop(context)),
            const SizedBox(height: 12),
            Row(
              children: [
                Icon(platformIcon(d.platform), size: 28, color: v.accent),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        d.name.isEmpty ? d.code : d.name,
                        style: v.title,
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                      ),
                      Text(
                        [
                          if (d.platform.isNotEmpty) platformName(d.platform),
                          tr('devices.method.${d.method}'),
                          tr('devices.seen', {'d': agoText(d.lastSeen.toInt(), DateTime.now())}),
                        ].join(' · '),
                        style: v.caption,
                      ),
                    ],
                  ),
                ),
              ],
            ),
            const SizedBox(height: 10),
            Semantics(
              label: 'Kod ${d.code.split('').join(' ')}',
              excludeSemantics: true,
              child: Text(d.code, style: v.text(15, FontWeight.w600, mono: true, tracking: 0.08, color: v.muted)),
            ),
            if (_error != null) ...[const SizedBox(height: 12), VNotice(title: 'Xato', body: _error)],
            Padding(
              padding: const EdgeInsets.fromLTRB(4, 22, 4, 4),
              child: Text(
                tr('devices.rules').toUpperCase(),
                style: v.text(12, FontWeight.w600, color: v.muted, tracking: 0.06),
              ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(4, 0, 4, 8),
              child: Text(tr('devices.rules.hint'), style: v.caption),
            ),
            if (r == null)
              const Padding(
                padding: EdgeInsets.all(32),
                child: Center(child: CircularProgressIndicator()),
              )
            else
              VSurface(
                color: v.raised,
                padding: const EdgeInsets.symmetric(vertical: 4),
                child: Column(
                  children: [
                    OverrideRow(
                      label: 'Qabul qilish',
                      value: _acceptOf(r['accept']),
                      items: {
                        'general': general,
                        'ask': 'Har safar soʻrash',
                        'trusted': 'Soʻramasdan',
                        'up_to': '100 MB gacha soʻramasdan',
                      },
                      onChanged: (x) => _set('accept', _acceptJson(x)),
                    ),
                    OverrideRow(
                      label: 'Galereyaga',
                      value: _boolOf(r['gallery']),
                      items: {'general': general, 'yes': 'Ha', 'no': 'Yoʻq'},
                      onChanged: (x) => _set('gallery', _boolJson(x)),
                    ),
                    OverrideRow(
                      label: 'Turi boʻyicha papkalar',
                      value: _boolOf(r['split_by_type']),
                      items: {'general': general, 'yes': 'Ha', 'no': 'Yoʻq'},
                      onChanged: (x) => _set('split_by_type', _boolJson(x)),
                    ),
                    OverrideRow(
                      label: 'Ichki papka',
                      value: (r['subfolder'] as String?) ?? 'general',
                      items: {
                        'general': general,
                        'none': 'Yoʻq',
                        'device': 'Qurilma nomi',
                        'date': 'Sana',
                        'device_date': 'Qurilma / sana',
                      },
                      onChanged: (x) => _set('subfolder', x == 'general' ? null : x),
                    ),
                  ],
                ),
              ),
            const SizedBox(height: 24),
            VButton(
              label: tr('devices.forget'),
              icon: Icons.link_off_rounded,
              kind: VButtonKind.secondary,
              onPressed: _confirmForget,
            ),
          ],
        ),
      ),
    );
  }
}

/// A label with a full-width dropdown under it, so nothing overflows at 2x.
class OverrideRow extends StatelessWidget {
  const OverrideRow({
    super.key,
    required this.label,
    required this.value,
    required this.items,
    required this.onChanged,
  });
  final String label;
  final String value;
  final Map<String, String> items;
  final ValueChanged<String> onChanged;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return Padding(
      padding: const EdgeInsets.fromLTRB(14, 4, 8, 4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.only(top: 6),
            child: Text(label, style: v.text(15, FontWeight.w500)),
          ),
          DropdownButton<String>(
            value: items.containsKey(value) ? value : items.keys.first,
            underline: const SizedBox.shrink(),
            isExpanded: true,
            itemHeight: null,
            style: v.text(14.5, FontWeight.w500, color: v.accent),
            items: [
              for (final e in items.entries)
                DropdownMenuItem(
                  value: e.key,
                  child: Text(e.value, maxLines: 2, overflow: TextOverflow.ellipsis),
                ),
            ],
            onChanged: (x) => onChanged(x ?? items.keys.first),
          ),
        ],
      ),
    );
  }
}
