// Receiving settings on the phone: where files go, gallery album, conflicts, when to accept without asking.
// The Rust side owns the file (settings.json); this screen edits its JSON and writes it back whole.

import 'dart:convert';

import 'package:material_ui/material_ui.dart';

import 'src/rust/api.dart' as vutric;
import 'ui.dart';

class SettingsPage extends StatefulWidget {
  const SettingsPage({super.key});

  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends State<SettingsPage> {
  Map<String, dynamic>? _s;
  String? _error;
  final _album = TextEditingController();

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _album.dispose();
    super.dispose();
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

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context).textTheme;
    final s = _s;
    return Scaffold(
      appBar: AppBar(backgroundColor: sand, title: const Text('Sozlamalar')),
      body: s == null
          ? Center(child: _error == null ? const CircularProgressIndicator() : Text(_error!))
          : ListView(
              padding: const EdgeInsets.symmetric(vertical: 8),
              children: [
                if (_error != null)
                  Padding(
                    padding: const EdgeInsets.all(16),
                    child: Text(_error!, style: t.bodyMedium?.copyWith(color: err)),
                  ),
                const _Header('Qabul qilish'),
                for (final (value, label) in const [
                  ('ask', 'Har safar soʻrash'),
                  ('trusted', 'Ishonchli qurilmadan soʻramasdan'),
                  ('up_to', 'Ishonchli qurilmadan, 100 MB gacha soʻramasdan'),
                ])
                  ListTile(
                    leading: Icon(
                      _acceptMode(s) == value ? Icons.radio_button_checked : Icons.radio_button_unchecked,
                      color: cobalt,
                    ),
                    title: Text(label),
                    onTap: () => _set((s) => s['accept'] = _acceptJson(value)),
                  ),
                const _Header('Galereya'),
                SwitchListTile(
                  title: const Text('Rasm va videolar galereyaga ham tushsin'),
                  value: _save(s)['gallery'] == true,
                  onChanged: (v) => _set((s) => _save(s)['gallery'] = v),
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: TextField(
                    controller: _album,
                    enabled: _save(s)['gallery'] == true,
                    decoration: const InputDecoration(labelText: 'Albom nomi', filled: true, fillColor: paper),
                    onSubmitted: (_) => _saveAlbum(),
                    onEditingComplete: _saveAlbum,
                  ),
                ),
                SwitchListTile(
                  title: const Text('Galereyaga tushgach faylni ham saqlash'),
                  subtitle: const Text('Oʻchirilsa, nusxa faqat galereyada qoladi'),
                  value: _save(s)['keep_copy'] != false,
                  onChanged: _save(s)['gallery'] == true ? (v) => _set((s) => _save(s)['keep_copy'] = v) : null,
                ),
                const _Header('Papkalar'),
                SwitchListTile(
                  title: const Text('Turi boʻyicha papkalarga ajratish'),
                  subtitle: const Text('Rasmlar, Videolar, Hujjatlar…'),
                  value: _save(s)['split_by_type'] == true,
                  onChanged: (v) => _set((s) => _save(s)['split_by_type'] = v),
                ),
                SwitchListTile(
                  title: const Text('Yuboruvchining papka tuzilmasini saqlash'),
                  value: _save(s)['keep_tree'] != false,
                  onChanged: (v) => _set((s) => _save(s)['keep_tree'] = v),
                ),
                ListTile(
                  title: const Text('Ichki papka'),
                  trailing: DropdownButton<String>(
                    value: (_save(s)['subfolder'] as String?) ?? 'none',
                    items: const [
                      DropdownMenuItem(value: 'none', child: Text('Yoʻq')),
                      DropdownMenuItem(value: 'device', child: Text('Qurilma nomi')),
                      DropdownMenuItem(value: 'date', child: Text('Sana')),
                      DropdownMenuItem(value: 'device_date', child: Text('Qurilma / sana')),
                    ],
                    onChanged: (v) => _set((s) => _save(s)['subfolder'] = v ?? 'none'),
                  ),
                ),
                ListTile(
                  title: const Text('Shu nomli fayl bor boʻlsa'),
                  trailing: DropdownButton<String>(
                    value: (_save(s)['conflict'] as String?) ?? 'keep_both',
                    items: const [
                      DropdownMenuItem(value: 'keep_both', child: Text('Ikkalasi qolsin')),
                      DropdownMenuItem(value: 'replace', child: Text('Almashtirish')),
                      DropdownMenuItem(value: 'skip', child: Text('Oʻtkazib yuborish')),
                    ],
                    onChanged: (v) => _set((s) => _save(s)['conflict'] = v ?? 'keep_both'),
                  ),
                ),
              ],
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

class _Header extends StatelessWidget {
  const _Header(this.text);
  final String text;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.fromLTRB(16, 16, 16, 4),
    child: Text(text, style: Theme.of(context).textTheme.titleSmall?.copyWith(color: cobalt)),
  );
}
