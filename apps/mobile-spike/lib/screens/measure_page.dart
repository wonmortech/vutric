// The engineering screen behind Settings → "Texnik oʻlchov": 100 preview round trips to a computer, with the p50 /
// p95 numbers of the Phase 0.2 criterion. Kept for measuring on real Wi-Fi (backlog N2), not for everyday use.

import 'package:material_ui/material_ui.dart';

import '../design/tokens.dart';
import '../design/widgets.dart';
import '../session.dart';
import '../src/rust/api.dart' as vutric;
import '../ui.dart';
import 'scan_page.dart';

const _runs = 100;

class MeasurePage extends StatefulWidget {
  const MeasurePage({super.key});

  @override
  State<MeasurePage> createState() => _MeasurePageState();
}

class _MeasurePageState extends State<MeasurePage> {
  bool _running = false;
  vutric.RunReport? _report;
  String? _error;
  String _status = '';

  Future<void> _run(String label, Future<vutric.RunReport> Function() job) async {
    // A measurement opens its own session; the app's session (if any) closes first.
    await Session.instance.disconnect();
    setState(() {
      _running = true;
      _status = label;
      _error = null;
      _report = null;
    });
    try {
      final r = await afterDisconnect(job);
      if (mounted) setState(() => _report = r);
    } catch (e) {
      if (mounted && !isCancelled(e)) setState(() => _error = errorText(e));
    } finally {
      if (mounted) setState(() => _running = false);
    }
  }

  Future<void> _scan() async {
    final link = await scanLink(context);
    if (link == null || !mounted) return;
    await _run('Havola orqali ulanmoqda…', () => vutric.runLink(link: link, runs: _runs));
  }

  @override
  void dispose() {
    if (_running) disconnectNow();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final r = _report;
    return Scaffold(
      body: SafeArea(
        child: Column(
          children: [
            const VNavHeader(title: 'Texnik oʻlchov'),
            Expanded(
              child: ListView(
                padding: const EdgeInsets.fromLTRB(16, 8, 16, 24),
                children: [
                  Text(
                    '$_runs ta preview borib-qaytadi. Mezon: bir tomonga p50 < 40 ms, p95 < 80 ms '
                    '(oddiy Wi-Fi, router internetdan uzilgan holda ham).',
                    style: v.text(14, FontWeight.w400, color: v.muted, height: 1.4),
                  ),
                  const SizedBox(height: 16),
                  VButton(
                    label: 'QR kodni skanerlab oʻlchash',
                    icon: Icons.qr_code_scanner_rounded,
                    onPressed: _running ? null : _scan,
                  ),
                  const SizedBox(height: 10),
                  if (hasMdns) ...[
                    VButton(
                      label: 'Tarmoqdan topib oʻlchash',
                      kind: VButtonKind.secondary,
                      onPressed: _running
                          ? null
                          : () => _run('Kompyuter qidirilmoqda…', () => vutric.runDiscovered(runs: _runs)),
                    ),
                    const SizedBox(height: 10),
                  ],
                  VButton(
                    label: 'Kutish: boshqa qurilma ulansin',
                    kind: VButtonKind.secondary,
                    onPressed: _running ? null : () => _run('Ulanish kutilmoqda…', () => vutric.listen(runs: _runs)),
                  ),
                  if (_running) ...[
                    const SizedBox(height: 20),
                    Center(child: VResultPill(text: _status, busy: true)),
                    const SizedBox(height: 10),
                    VButton(label: 'Bekor qilish', kind: VButtonKind.ghost, onPressed: disconnectNow),
                  ],
                  if (_error != null) ...[
                    const SizedBox(height: 16),
                    VNotice(title: 'Oʻlchab boʻlmadi', body: _error),
                  ],
                  if (r != null) ...[const SizedBox(height: 16), _Report(report: r)],
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _Report extends StatelessWidget {
  const _Report({required this.report});
  final vutric.RunReport report;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final pass = report.sentP50Ms < 40 && report.sentP95Ms < 80;
    String ms(double x) => '${x.toStringAsFixed(1)} ms';
    Widget row(String k, String val) => Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        children: [
          Expanded(child: Text(k, style: v.text(14, FontWeight.w400, color: v.muted))),
          Text(val, style: v.text(14, FontWeight.w600, mono: true)),
        ],
      ),
    );
    return VSurface(
      color: v.raised,
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          VDevicePill(name: report.peerName, platform: report.peerPlatform, code: report.checkCode),
          const SizedBox(height: 12),
          if (report.foundMs > 0) row('Topildi', ms(report.foundMs)),
          if (report.connectMs > 0) row('Ulandi', ms(report.connectMs)),
          row('p50 (bir tomonga)', ms(report.sentP50Ms)),
          row('p95 (bir tomonga)', ms(report.sentP95Ms)),
          row('Qarshi tomondan kelgan', '${report.receivedProbes} ta'),
          const SizedBox(height: 8),
          Text(
            pass ? 'Mezon bajarildi' : 'Mezondan oshdi',
            style: v.text(15, FontWeight.w600, color: pass ? v.ok : v.err),
          ),
          const SizedBox(height: 8),
          SelectableText(report.sent, style: v.monoSmall),
        ],
      ),
    );
  }
}
