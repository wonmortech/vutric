// Vutric spike 0.2 on Android and iOS: one LAN-only peer (Rust, vutric-net) and three ways to measure a 30 KB preview
// in both directions. No internet is used (ADR-0005).

import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart' show ExternalLibrary;
import 'package:material_ui/material_ui.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:path_provider/path_provider.dart';
import 'package:qr_flutter/qr_flutter.dart';

import 'fling_page.dart';
import 'src/rust/api.dart' as vutric;
import 'src/rust/frb_generated.dart';

const _cobalt = Color(0xFF062B8A);
const _signal = Color(0xFF2F6BFF);
const _sand = Color(0xFFE6DBC7);
const _paper = Color(0xFFF7F2E9);
const _ok = Color(0xFF1D7A4E);
const _err = Color(0xFFA8321F);
const _runs = 100;

const _multicast = MethodChannel('vutric/multicast');

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  // iOS: the Rust library is linked into the app itself (-force_load in the Xcode project), not shipped as a framework.
  await RustLib.init(externalLibrary: Platform.isIOS ? ExternalLibrary.process(iKnowHowToUseIt: true) : null);
  runApp(const SpikeApp());
}

class SpikeApp extends StatelessWidget {
  const SpikeApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Vutric spike',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: _cobalt, primary: _cobalt, surface: _sand),
        scaffoldBackgroundColor: _sand,
        useMaterial3: true,
      ),
      home: const SpikeHome(),
    );
  }
}

class SpikeHome extends StatefulWidget {
  const SpikeHome({super.key});

  @override
  State<SpikeHome> createState() => _SpikeHomeState();
}

class _SpikeHomeState extends State<SpikeHome> {
  final _name = TextEditingController(text: Platform.isIOS ? 'iPhone' : 'Android telefon');
  final _link = TextEditingController();
  String? _myLink;
  String _status = 'Tayyorlanmoqda…';
  bool _busy = false;
  vutric.RunReport? _report;
  String? _error;

  @override
  void initState() {
    super.initState();
    _start();
  }

  /// The device identity lives in app-private storage and is reused, so the pairing link stays stable.
  Future<Uint8List> _identity() async {
    final dir = await getApplicationSupportDirectory();
    final file = File('${dir.path}/identity.key');
    if (await file.exists()) {
      final bytes = await file.readAsBytes();
      if (bytes.length == 32) return bytes;
    }
    final fresh = await vutric.newIdentity();
    await file.writeAsBytes(fresh, flush: true);
    return fresh;
  }

  Future<void> _start() async {
    try {
      // Android drops mDNS replies without a multicast lock; iOS has no such switch.
      if (Platform.isAndroid) await _multicast.invokeMethod('acquire');
      final link = await vutric.start(name: _name.text.trim(), identity: await _identity());
      setState(() {
        _myLink = link;
        _status = 'Tarmoqda koʻrinadi. Internet ishlatilmaydi.';
      });
    } catch (e) {
      setState(() => _error = '$e');
    }
  }

  Future<void> _run(String label, Future<vutric.RunReport> Function() job) async {
    setState(() {
      _busy = true;
      _status = label;
      _error = null;
      _report = null;
    });
    try {
      final r = await job();
      setState(() {
        _report = r;
        _status = 'Tayyor';
      });
    } catch (e) {
      setState(() {
        _error = '$e';
        _status = 'Xato';
      });
    } finally {
      setState(() => _busy = false);
    }
  }

  void _connectLink(String link) => _run('Havola orqali ulanmoqda…', () => vutric.runLink(link: link, runs: _runs));

  Future<String?> _scanLink() =>
      Navigator.of(context).push<String>(MaterialPageRoute(builder: (_) => const _ScanPage()));

  void _openFling() => Navigator.of(context).push(
    MaterialPageRoute(
      builder: (_) => FlingPage(scan: _scanLink, link: _link.text),
    ),
  );

  Future<void> _scan() async {
    final link = await _scanLink();
    if (link == null || !mounted) return;
    _link.text = link;
    _connectLink(link);
  }

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context).textTheme;
    final ready = !_busy && _myLink != null;
    return Scaffold(
      appBar: AppBar(backgroundColor: _sand, title: const Text('Vutric · spike 0.2')),
      body: SafeArea(
        child: ListView(
          padding: const EdgeInsets.all(16),
          children: [
            Text(_status, style: t.bodyMedium?.copyWith(color: _cobalt)),
            const SizedBox(height: 12),
            TextField(
              controller: _name,
              decoration: const InputDecoration(labelText: 'Qurilma nomi', filled: true, fillColor: _paper),
              onSubmitted: (_) => _start(),
            ),
            const SizedBox(height: 16),
            FilledButton.icon(
              onPressed: ready ? _openFling : null,
              icon: const Icon(Icons.north),
              label: const Text('Rasmni kompyuterga otish'),
            ),
            const SizedBox(height: 8),
            OutlinedButton(
              onPressed: ready
                  ? () => _run('Kompyuter qidirilmoqda (mDNS)…', () => vutric.runDiscovered(runs: _runs))
                  : null,
              child: const Text('Kompyuterni topish va oʻlchash'),
            ),
            const SizedBox(height: 8),
            OutlinedButton(
              onPressed: ready ? () => _run('Ulanish kutilmoqda…', () => vutric.listen(runs: _runs)) : null,
              child: const Text('Kutish: boshqa qurilma ulansin'),
            ),
            const SizedBox(height: 8),
            TextField(
              controller: _link,
              decoration: const InputDecoration(
                labelText: 'Ulash havolasi (vutric://pair?…)',
                filled: true,
                fillColor: _paper,
              ),
            ),
            const SizedBox(height: 8),
            OutlinedButton(onPressed: ready ? _scan : null, child: const Text('QR kodni skanerlab oʻlchash')),
            const SizedBox(height: 8),
            OutlinedButton(
              onPressed: ready ? () => _connectLink(_link.text) : null,
              child: const Text('Havola orqali ulanib oʻlchash'),
            ),
            if (_busy)
              const Padding(
                padding: EdgeInsets.all(16),
                child: LinearProgressIndicator(color: _signal),
              ),
            if (_error != null)
              Padding(
                padding: const EdgeInsets.only(top: 16),
                child: Text(_error!, style: t.bodyMedium?.copyWith(color: _err)),
              ),
            if (_report != null) _ReportCard(report: _report!),
            if (_myLink != null) ...[
              const SizedBox(height: 24),
              Text('Bu qurilmaning QR kodi', style: t.titleSmall?.copyWith(color: _cobalt)),
              const SizedBox(height: 8),
              Center(
                child: Container(
                  color: _paper,
                  padding: const EdgeInsets.all(12),
                  child: QrImageView(data: _myLink!, size: 220),
                ),
              ),
              const SizedBox(height: 8),
              SelectableText(_myLink!, style: t.bodySmall?.copyWith(fontFamily: 'monospace')),
            ],
          ],
        ),
      ),
    );
  }
}

class _ReportCard extends StatelessWidget {
  const _ReportCard({required this.report});

  final vutric.RunReport report;

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context).textTheme;
    final pass = report.sentP50Ms < 40 && report.sentP95Ms < 80;
    String ms(double v) => '${v.toStringAsFixed(1)} ms';
    return Card(
      color: _paper,
      margin: const EdgeInsets.only(top: 16),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('${report.peerName} (${report.peerPlatform})', style: t.titleMedium?.copyWith(color: _cobalt)),
            const SizedBox(height: 8),
            Text('Tekshiruv kodi', style: t.labelMedium),
            Text(report.checkCode, style: t.displaySmall?.copyWith(color: _cobalt, letterSpacing: 4)),
            const SizedBox(height: 8),
            if (report.foundMs > 0) Text('Topildi: ${ms(report.foundMs)}'),
            if (report.connectMs > 0) Text('Ulandi: ${ms(report.connectMs)}'),
            const SizedBox(height: 8),
            Text('Telefon → ${report.peerName}, bir tomonga: p50 ${ms(report.sentP50Ms)}, p95 ${ms(report.sentP95Ms)}'),
            Text(
              pass ? 'Mezon bajarildi (p50 < 40, p95 < 80 ms)' : 'Mezondan oshdi (p50 < 40, p95 < 80 ms)',
              style: t.bodyMedium?.copyWith(color: pass ? _ok : _err),
            ),
            const SizedBox(height: 8),
            Text(
              '${report.peerName} → telefon: ${report.receivedProbes} ta preview qabul qilindi; '
              'raqamlari u tomonda chiqadi.',
            ),
            const SizedBox(height: 8),
            SelectableText(report.sent, style: t.bodySmall?.copyWith(fontFamily: 'monospace')),
          ],
        ),
      ),
    );
  }
}

/// Full-screen camera that returns the first `vutric://pair?` link it sees.
class _ScanPage extends StatefulWidget {
  const _ScanPage();

  @override
  State<_ScanPage> createState() => _ScanPageState();
}

class _ScanPageState extends State<_ScanPage> {
  bool _done = false;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('QR kodni skanerlang')),
      body: MobileScanner(
        onDetect: (capture) {
          if (_done) return;
          for (final code in capture.barcodes) {
            final v = code.rawValue;
            if (v != null && v.startsWith('vutric://pair?')) {
              _done = true;
              Navigator.of(context).pop(v);
              return;
            }
          }
        },
      ),
    );
  }
}
