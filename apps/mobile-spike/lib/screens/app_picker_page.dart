// The Companion allowlist (A12, ADR-0065): which apps' notifications go to the paired computer. Nothing is
// picked at first, and nothing picked means nothing is mirrored.

import 'package:material_ui/material_ui.dart';

import '../android.dart';
import '../design/tokens.dart';
import '../design/widgets.dart';
import '../i18n.dart';

typedef AppEntry = ({String package, String label});

/// Opens the picker; returns the new list, or null when the user went back without saving.
Future<List<String>?> pickMirrorApps(BuildContext context, List<String> current) => Navigator.of(context)
    .push<List<String>>(
      MaterialPageRoute(
        builder: (_) => AppPickerPage(selected: current, load: AndroidBridge.instance.installedApps),
      ),
    );

class AppPickerPage extends StatefulWidget {
  const AppPickerPage({super.key, required this.selected, required this.load});
  final List<String> selected;
  final Future<List<AppEntry>> Function() load;

  @override
  State<AppPickerPage> createState() => _AppPickerPageState();
}

class _AppPickerPageState extends State<AppPickerPage> {
  late final Set<String> _on = {...widget.selected};
  List<AppEntry>? _apps;
  String _query = '';

  @override
  void initState() {
    super.initState();
    widget.load().then((a) {
      if (mounted) setState(() => _apps = a);
    });
  }

  void _save() => Navigator.of(context).pop(_on.toList()..sort());

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final apps = _apps;
    final q = _query.trim().toLowerCase();
    final shown = apps == null
        ? const <AppEntry>[]
        : [
            for (final a in apps)
              if (q.isEmpty || a.label.toLowerCase().contains(q) || a.package.contains(q)) a,
          ];
    return Scaffold(
      body: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            VNavHeader(title: tr('companion.apps'), onBack: () => Navigator.of(context).pop()),
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
              child: Text(tr('companion.apps.hint'), style: v.caption),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
              child: TextField(
                decoration: InputDecoration(
                  prefixIcon: const Icon(Icons.search_rounded),
                  hintText: tr('companion.apps.search'),
                ),
                onChanged: (t) => setState(() => _query = t),
              ),
            ),
            Expanded(
              child: apps == null
                  ? const Center(child: CircularProgressIndicator())
                  : shown.isEmpty
                  ? Center(child: Text(tr('companion.apps.empty'), style: v.caption))
                  : ListView.builder(
                      itemCount: shown.length,
                      itemBuilder: (_, i) {
                        final a = shown[i];
                        return CheckboxListTile(
                          minTileHeight: 56,
                          value: _on.contains(a.package),
                          title: Text(a.label, maxLines: 1, overflow: TextOverflow.ellipsis),
                          subtitle: Text(a.package, maxLines: 1, overflow: TextOverflow.ellipsis, style: v.caption),
                          onChanged: (x) => setState(() => x == true ? _on.add(a.package) : _on.remove(a.package)),
                        );
                      },
                    ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 8, 16, 16),
              child: VButton(
                label: _on.isEmpty ? tr('companion.apps.none') : tr('companion.apps.some', {'n': '${_on.length}'}),
                onPressed: _save,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
