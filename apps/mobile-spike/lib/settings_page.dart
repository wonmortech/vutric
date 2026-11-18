// Sozlamalar: the look (9 themes, S6 of the handoff), this phone's name, and receiving (when to accept, the
// gallery album, folders, name conflicts). Receiving settings belong to Rust (settings.json, written back whole);
// the theme and the name are UI-only and live in Session (ADR-0011).

import 'dart:convert';

import 'package:material_ui/material_ui.dart';

import 'design/themes.g.dart';
import 'design/tokens.dart';
import 'design/widgets.dart';
import 'screens/measure_page.dart';
import 'session.dart';
import 'src/rust/api.dart' as vutric;
import 'ui.dart';

class SettingsPage extends StatefulWidget {
  const SettingsPage({super.key});

  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends State<SettingsPage> {
  final _session = Session.instance;
  Map<String, dynamic>? _s;
  String? _error;
  final _album = TextEditingController();

  @override
  void initState() {
    super.initState();
    _session.addListener(_changed);
    _load();
  }

  @override
  void dispose() {
    _session.removeListener(_changed);
    _album.dispose();
    super.dispose();
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  Future<void> _load() async {
    try {
      final s = jsonDecode(await vutric.getSettings()) as Map<String, dynamic>;
      if (!mounted) return;
      _album.text = (_save(s)['album'] as String?) ?? 'Vutric';
      setState(() => _s = s);
    } catch (e) {
      if (mounted) setState(() => _error = errorText(e));
    }
  }

  static Map<String, dynamic> _save(Map<String, dynamic> s) => s['save'] as Map<String, dynamic>;

  Future<void> _set(void Function(Map<String, dynamic> s) change) async {
    final s = _s;
    if (s == null) return;
    setState(() => change(s));
    try {
      await vutric.putSettings(json: jsonEncode(s));
    } catch (e) {
      if (mounted) setState(() => _error = 'Saqlab boʻlmadi: ${errorText(e)}');
    }
  }

  void _saveAlbum() {
    final s = _s;
    if (s == null) return;
    final v = _album.text.trim();
    final name = v.isEmpty ? 'Vutric' : v;
    if (name != _save(s)['album']) _set((s) => _save(s)['album'] = name);
  }

  Future<void> _rename() async {
    final v = context.v;
    final c = TextEditingController(text: _session.deviceName);
    final name = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Qurilma nomi', style: v.heading),
        content: TextField(
          controller: c,
          autofocus: true,
          maxLength: 32,
          onSubmitted: (t) => Navigator.pop(ctx, t),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Bekor qilish')),
          FilledButton(onPressed: () => Navigator.pop(ctx, c.text), child: const Text('Saqlash')),
        ],
      ),
    );
    c.dispose();
    if (name != null && await _session.setDeviceName(name)) await _session.restart();
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final s = _s;
    return Scaffold(
      backgroundColor: v.bg,
      body: SafeArea(
        child: ListView(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 32),
          children: [
            VNavHeader(title: 'Sozlamalar', onBack: () => Navigator.pop(context)),
            const SizedBox(height: 12),
            const _Header('Koʻrinish'),
            _ThemeGrid(current: _session.themeId, onPick: _session.setTheme),
            const _Header('Qurilma'),
            VRowCard(
              icon: Icons.smartphone_rounded,
              title: _session.deviceName,
              subtitle: 'Boshqa qurilmalarda shu nom chiqadi',
              onTap: _session.peer == null ? _rename : null,
              trailing: Icon(Icons.edit_outlined, size: 18, color: v.muted),
            ),
            if (_error != null) ...[const SizedBox(height: 12), VNotice(title: 'Xato', body: _error)],
            if (s == null)
              const Padding(padding: EdgeInsets.all(32), child: Center(child: CircularProgressIndicator()))
            else ...[
              const _Header('Qabul qilish'),
              _Group(
                children: [
                  for (final (value, label) in const [
                    ('ask', 'Har safar soʻrash'),
                    ('trusted', 'Ishonchli qurilmadan soʻramasdan'),
                    ('up_to', 'Ishonchli qurilmadan, 100 MB gacha soʻramasdan'),
                  ])
                    _Choice(
                      label: label,
                      on: _acceptMode(s) == value,
                      onTap: () => _set((s) => s['accept'] = _acceptJson(value)),
                    ),
                ],
              ),
              const _Header('Galereya'),
              _Group(
                children: [
                  _Switch(
                    label: 'Rasm va videolar galereyaga ham tushsin',
                    value: _save(s)['gallery'] == true,
                    onChanged: (x) => _set((s) => _save(s)['gallery'] = x),
                  ),
                  Padding(
                    padding: const EdgeInsets.fromLTRB(14, 4, 14, 10),
                    child: TextField(
                      controller: _album,
                      enabled: _save(s)['gallery'] == true,
                      style: v.body,
                      decoration: const InputDecoration(labelText: 'Albom nomi'),
                      onSubmitted: (_) => _saveAlbum(),
                      onEditingComplete: _saveAlbum,
                    ),
                  ),
                  _Switch(
                    label: 'Galereyaga tushgach faylni ham saqlash',
                    hint: 'Oʻchirilsa, nusxa faqat galereyada qoladi',
                    value: _save(s)['keep_copy'] != false,
                    onChanged: _save(s)['gallery'] == true ? (x) => _set((s) => _save(s)['keep_copy'] = x) : null,
                  ),
                ],
              ),
              const _Header('Papkalar'),
              _Group(
                children: [
                  _Switch(
                    label: 'Turi boʻyicha papkalarga ajratish',
                    hint: 'Rasmlar, Videolar, Hujjatlar…',
                    value: _save(s)['split_by_type'] == true,
                    onChanged: (x) => _set((s) => _save(s)['split_by_type'] = x),
                  ),
                  _Switch(
                    label: 'Yuboruvchining papka tuzilmasini saqlash',
                    value: _save(s)['keep_tree'] != false,
                    onChanged: (x) => _set((s) => _save(s)['keep_tree'] = x),
                  ),
                  _Drop(
                    label: 'Ichki papka',
                    value: (_save(s)['subfolder'] as String?) ?? 'none',
                    items: const {'none': 'Yoʻq', 'device': 'Qurilma nomi', 'date': 'Sana', 'device_date': 'Qurilma / sana'},
                    onChanged: (x) => _set((s) => _save(s)['subfolder'] = x),
                  ),
                  _Drop(
                    label: 'Shu nomli fayl bor boʻlsa',
                    value: (_save(s)['conflict'] as String?) ?? 'keep_both',
                    items: const {'keep_both': 'Ikkalasi qolsin', 'replace': 'Almashtirish', 'skip': 'Oʻtkazib yuborish'},
                    onChanged: (x) => _set((s) => _save(s)['conflict'] = x),
                  ),
                ],
              ),
            ],
            const _Header('Texnik'),
            VRowCard(
              icon: Icons.speed_rounded,
              title: 'Kechikishni oʻlchash',
              subtitle: '100 ta preview, p50 / p95',
              onTap: () => Navigator.of(context).push(MaterialPageRoute(builder: (_) => const MeasurePage())),
            ),
          ],
        ),
      ),
    );
  }
}

const _upTo = 100 << 20;

/// `accept` is `{"mode":"ask"}`, `{"mode":"trusted"}` or `{"mode":"up_to","max_bytes":N}` (serde adjacent tag).
String _acceptMode(Map<String, dynamic> s) {
  final a = s['accept'];
  final mode = a is Map ? a['mode'] : a;
  return mode is String ? mode : 'ask';
}

Object _acceptJson(String mode) => mode == 'up_to' ? {'mode': 'up_to', 'max_bytes': _upTo} : {'mode': mode};

/// S6: each theme as a tiny phone in its own colours; "Avto" follows the phone's light/dark setting.
class _ThemeGrid extends StatelessWidget {
  const _ThemeGrid({required this.current, required this.onPick});
  final String current;
  final ValueChanged<String> onPick;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final cells = <(String, String, VutricTokens?)>[
      (autoTheme, 'Avto', null),
      for (final t in vutricThemes) (t.id, t.name, t),
    ];
    return GridView.count(
      crossAxisCount: 5,
      shrinkWrap: true,
      physics: const NeverScrollableScrollPhysics(),
      mainAxisSpacing: 10,
      crossAxisSpacing: 8,
      childAspectRatio: 0.62,
      children: [
        for (final (id, name, t) in cells)
          VPressable(
            onTap: () => onPick(id),
            semanticLabel: 'Mavzu: $name',
            haptic: id != current,
            child: Column(
              children: [
                Expanded(
                  child: AnimatedContainer(
                    duration: VMotion.fast,
                    padding: const EdgeInsets.all(2),
                    decoration: BoxDecoration(
                      borderRadius: BorderRadius.circular(14),
                      border: Border.all(color: id == current ? v.accent : v.line, width: id == current ? 2.5 : 1),
                    ),
                    child: ClipRRect(borderRadius: BorderRadius.circular(11), child: _Mini(t: t)),
                  ),
                ),
                const SizedBox(height: 4),
                Text(
                  name,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: v.text(11, id == current ? FontWeight.w700 : FontWeight.w500, color: id == current ? v.accent : v.ink),
                ),
              ],
            ),
          ),
      ],
    );
  }
}

class _Mini extends StatelessWidget {
  const _Mini({required this.t});
  final VutricTokens? t;

  @override
  Widget build(BuildContext context) {
    final t = this.t;
    if (t == null) {
      final light = vutricThemes[0], dark = vutricThemes[1];
      return Row(children: [Expanded(child: _Mini(t: light)), Expanded(child: _Mini(t: dark))]);
    }
    return ColoredBox(
      color: t.bg,
      child: Padding(
        padding: const EdgeInsets.all(5),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Container(height: 5, width: 18, color: t.ink),
            const SizedBox(height: 4),
            Expanded(
              child: Container(
                decoration: BoxDecoration(color: t.hero, borderRadius: BorderRadius.circular(4)),
                alignment: Alignment.bottomRight,
                padding: const EdgeInsets.all(3),
                child: Container(width: 8, height: 8, decoration: BoxDecoration(color: t.accent, shape: BoxShape.circle)),
              ),
            ),
            const SizedBox(height: 4),
            Container(height: 7, decoration: BoxDecoration(color: t.raised, borderRadius: BorderRadius.circular(3))),
          ],
        ),
      ),
    );
  }
}

class _Header extends StatelessWidget {
  const _Header(this.text);
  final String text;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.fromLTRB(4, 22, 4, 8),
    child: Text(text.toUpperCase(), style: context.v.text(12, FontWeight.w600, color: context.v.muted, tracking: 0.06)),
  );
}

class _Group extends StatelessWidget {
  const _Group({required this.children});
  final List<Widget> children;

  @override
  Widget build(BuildContext context) => VSurface(
    color: context.v.raised,
    padding: const EdgeInsets.symmetric(vertical: 4),
    child: Column(children: children),
  );
}

class _Choice extends StatelessWidget {
  const _Choice({required this.label, required this.on, required this.onTap});
  final String label;
  final bool on;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return Semantics(
      selected: on,
      child: InkWell(
        onTap: onTap,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
          child: Row(
            children: [
              Expanded(child: Text(label, style: v.text(15, FontWeight.w500))),
              AnimatedOpacity(
                duration: VMotion.fast,
                opacity: on ? 1 : 0,
                child: Icon(Icons.check_rounded, color: v.accent),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _Switch extends StatelessWidget {
  const _Switch({required this.label, required this.value, required this.onChanged, this.hint});
  final String label;
  final String? hint;
  final bool value;
  final ValueChanged<bool>? onChanged;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return SwitchListTile.adaptive(
      value: value,
      onChanged: onChanged,
      activeTrackColor: v.accent,
      title: Text(label, style: v.text(15, FontWeight.w500)),
      subtitle: hint == null ? null : Text(hint!, style: v.caption),
    );
  }
}

class _Drop extends StatelessWidget {
  const _Drop({required this.label, required this.value, required this.items, required this.onChanged});
  final String label;
  final String value;
  final Map<String, String> items;
  final ValueChanged<String> onChanged;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return Padding(
      padding: const EdgeInsets.fromLTRB(14, 2, 8, 2),
      child: Row(
        children: [
          Expanded(child: Text(label, style: v.text(15, FontWeight.w500))),
          DropdownButton<String>(
            value: items.containsKey(value) ? value : items.keys.first,
            underline: const SizedBox.shrink(),
            style: v.text(14.5, FontWeight.w500, color: v.accent),
            items: [for (final e in items.entries) DropdownMenuItem(value: e.key, child: Text(e.value))],
            onChanged: (x) => onChanged(x ?? items.keys.first),
          ),
        ],
      ),
    );
  }
}
