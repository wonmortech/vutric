// Vutric spike 0.2 on Android and iOS. The app opens on the phone's own gallery and files (select, then send);
// receiving, the photo throw, settings and the latency measurement are one tap away in the menu. Same network
// first, the internet (direct or encrypted relay) when the devices are apart (ADR-0006).

import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart' show ExternalLibrary;
import 'package:material_ui/material_ui.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:path_provider/path_provider.dart';
import 'package:qr_flutter/qr_flutter.dart';

import 'catch_page.dart';
import 'fling_page.dart';
import 'gallery_page.dart';
import 'settings_page.dart';
import 'src/rust/api.dart' as vutric;
import 'src/rust/frb_generated.dart';
import 'ui.dart';

const _runs = 100;

const _multicast = MethodChannel('vutric/multicast');

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  try {
    // iOS: the Rust library is linked into the app itself (-force_load in the Xcode project), not a framework.
    await RustLib.init(externalLibrary: Platform.isIOS ? ExternalLibrary.process(iKnowHowToUseIt: true) : null);
  } catch (e) {
    // A broken build (e.g. the Rust bridge stripped by the linker) says so instead of showing a blank screen.
    runApp(
      MaterialApp(
        home: Scaffold(
          body: SafeArea(child: Center(child: Text('Rust kutubxonasi yuklanmadi: $e'))),
        ),
      ),
    );
    return;
  }
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
        colorScheme: ColorScheme.fromSeed(seedColor: cobalt, primary: cobalt, surface: sand),
        scaffoldBackgroundColor: sand,
        useMaterial3: true,
      ),
      home: const VutricHome(),
    );
  }
}

/// The device identity lives in app-private storage and is reused, so the pairing link stays stable.
Future<Uint8List> loadIdentity() async {
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

Future<String?> scanLink(BuildContext context) =>
    Navigator.of(context).push<String>(MaterialPageRoute(builder: (_) => const _ScanPage()));

/// Starts this device on the network once, then shows the gallery. Everything else hangs off its menu.
class VutricHome extends StatefulWidget {
  const VutricHome({super.key});

  @override
  State<VutricHome> createState() => _VutricHomeState();
}

class _VutricHomeState extends State<VutricHome> with WidgetsBindingObserver {
  final _gallery = GlobalKey<GalleryPageState>();
  String? _myLink;
  String? _bootError;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _boot();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (!Platform.isAndroid) return;
    if (state == AppLifecycleState.resumed) {
      _multicast.invokeMethod('acquire');
    } else if (state == AppLifecycleState.paused) {
      _multicast.invokeMethod('release');
    }
  }

  Future<void> _boot() async {
    try {
      await vutric.setSettingsDir(dir: (await getApplicationSupportDirectory()).path);
      if (Platform.isAndroid) await _multicast.invokeMethod('acquire');
      final link = await vutric.start(
        name: Platform.isIOS ? 'iPhone' : 'Android telefon',
        identity: await loadIdentity(),
        lanOnly: false,
      );
      if (!mounted) return;
      setState(() => _myLink = link);
      await vutric.waitOnline(maxMs: 10000);
      final full = await vutric.currentLink();
      if (mounted) setState(() => _myLink = full);
    } catch (e) {
      if (mounted) setState(() => _bootError = errorText(e));
    }
  }

  /// Shows a modal "connecting…" with a cancel button while [job] runs; the peer's name, or null.
  Future<String?> _wait(String label, Future<vutric.Connected> Function() job, {String? qr}) async {
    final nav = Navigator.of(context);
    var open = true;
    showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (ctx) => AlertDialog(
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (qr != null) ...[
              Container(color: Colors.white, padding: const EdgeInsets.all(8), child: QrImageView(data: qr, size: 220)),
              const SizedBox(height: 12),
            ] else
              const Padding(padding: EdgeInsets.all(16), child: CircularProgressIndicator()),
            Text(label, textAlign: TextAlign.center),
          ],
        ),
        actions: [TextButton(onPressed: disconnectNow, child: const Text('Bekor qilish'))],
      ),
    ).whenComplete(() => open = false);
    try {
      final c = await afterDisconnect(job);
      return c.peerName;
    } catch (e) {
      if (!isCancelled(e) && mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(errorText(e))));
      }
      return null;
    } finally {
      if (open) nav.pop();
    }
  }

  Future<String?> _connect() async {
    if (_myLink == null) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(_bootError ?? 'Tayyorlanmoqda, bir soniya…')),
      );
      return null;
    }
    final how = await showModalBottomSheet<String>(
      context: context,
      backgroundColor: paper,
      builder: (ctx) => SafeArea(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const ListTile(title: Text('Qaysi qurilmaga?', style: TextStyle(fontWeight: FontWeight.w600))),
            ListTile(
              leading: const Icon(Icons.qr_code_scanner, color: cobalt),
              title: const Text('Kompyuter QR kodini skanerlash'),
              subtitle: const Text('Boshqa tarmoqda boʻlsa ham ulanadi'),
              onTap: () => Navigator.pop(ctx, 'scan'),
            ),
            if (hasMdns)
              ListTile(
                leading: const Icon(Icons.wifi_find, color: cobalt),
                title: const Text('Shu Wi-Fi’dan topish'),
                onTap: () => Navigator.pop(ctx, 'find'),
              ),
            ListTile(
              leading: const Icon(Icons.qr_code_2, color: cobalt),
              title: const Text('Mening QR kodimni koʻrsatish'),
              subtitle: const Text('Boshqa qurilma skanerlaydi'),
              onTap: () => Navigator.pop(ctx, 'wait'),
            ),
          ],
        ),
      ),
    );
    if (!mounted || how == null) return null;
    final String? peer;
    switch (how) {
      case 'scan':
        final link = await scanLink(context);
        if (link == null || !mounted) return null;
        peer = await _wait('Ulanmoqda…', () => vutric.connectLink(link: link));
      case 'find':
        peer = await _wait('Shu tarmoqdan qidirilmoqda…', vutric.connectDiscovered);
      default:
        peer = await _wait('Boshqa qurilmada skanerlang', vutric.acceptSession, qr: _myLink);
    }
    return peer;
  }

  Future<void> _push(Widget page) async {
    await Navigator.of(context).push(MaterialPageRoute(builder: (_) => page));
    // Those pages close their session when they leave.
    _gallery.currentState?.peerChanged(null);
  }

  void _menu() {
    final link = _myLink;
    showModalBottomSheet<void>(
      context: context,
      backgroundColor: paper,
      builder: (ctx) {
        void go(Widget page) {
          Navigator.pop(ctx);
          _push(page);
        }

        return SafeArea(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              ListTile(
                leading: const Icon(Icons.south, color: cobalt),
                title: const Text('Qabul qilish'),
                subtitle: const Text('Kompyuterdan rasm va fayllar'),
                enabled: link != null,
                onTap: () => go(CatchPage(scan: () => scanLink(context), myLink: link!)),
              ),
              ListTile(
                leading: const Icon(Icons.north, color: cobalt),
                title: const Text('Rasm otish'),
                enabled: link != null,
                onTap: () => go(FlingPage(scan: () => scanLink(context), link: '')),
              ),
              ListTile(
                leading: const Icon(Icons.tune, color: cobalt),
                title: const Text('Sozlamalar'),
                subtitle: const Text('Qayerga saqlash, qabul qilish'),
                onTap: () => go(const SettingsPage()),
              ),
              ListTile(
                leading: const Icon(Icons.speed, color: cobalt),
                title: const Text('Texnik oʻlchov'),
                enabled: link != null,
                onTap: () => go(SpikeHome(link: link)),
              ),
            ],
          ),
        );
      },
    );
  }

  @override
  Widget build(BuildContext context) => GalleryPage(key: _gallery, connect: _connect, openMenu: _menu);
}

class SpikeHome extends StatefulWidget {
  const SpikeHome({super.key, this.link});

  /// The pairing link when this device is already started (opened from the menu): no restart then.
  final String? link;

  @override
  State<SpikeHome> createState() => _SpikeHomeState();
}

class _SpikeHomeState extends State<SpikeHome> with WidgetsBindingObserver {
  final _name = TextEditingController(text: Platform.isIOS ? 'iPhone' : 'Android telefon');
  final _link = TextEditingController();
  String? _myLink;
  String _status = 'Tayyorlanmoqda…';
  bool _busy = false;

  /// A measurement run is in flight (the only thing Bekor qilish can cancel on this screen).
  bool _running = false;
  vutric.RunReport? _report;
  String? _error;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    final link = widget.link;
    if (link != null) {
      _myLink = link;
      _status = 'Tayyor.';
    } else {
      _start();
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _name.dispose();
    _link.dispose();
    super.dispose();
  }

  /// Android: hold the multicast lock (battery) only while the app is in front.
  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (!Platform.isAndroid) return;
    if (state == AppLifecycleState.resumed) {
      _multicast.invokeMethod('acquire');
    } else if (state == AppLifecycleState.paused) {
      _multicast.invokeMethod('release');
    }
  }

  /// Starts (or, after a rename, restarts) this device on the network.
  Future<void> _start() async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
      _status = 'Tayyorlanmoqda…';
    });
    try {
      // Android drops mDNS replies without a multicast lock; iOS has no such switch.
      if (Platform.isAndroid) await _multicast.invokeMethod('acquire');
      final name = _name.text.trim().isEmpty ? 'Telefon' : _name.text.trim();
      final link = await vutric.start(name: name, identity: await loadIdentity(), lanOnly: false);
      if (!mounted) return;
      setState(() {
        _myLink = link;
        _status = hasMdns ? 'Tarmoqda koʻrinadi. Internet tekshirilmoqda…' : 'Tayyor: QR kod orqali ulanadi. Internet tekshirilmoqda…';
      });
      // The relay takes ~3 s; until then the QR works on this network only. Swap in the fuller link when ready.
      final online = await vutric.waitOnline(maxMs: 10000);
      final full = await vutric.currentLink();
      if (!mounted) return;
      setState(() {
        _myLink = full;
        _status = online
            ? 'Tayyor: shu tarmoqda va internet orqali (shifrlangan) ulanadi.'
            : 'Tayyor: faqat shu tarmoqda (internet yoʻq).';
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = errorText(e);
        _status = 'Ishga tushmadi. Nomni oʻzgartirib Enter bosing yoki ilovani qayta oching.';
      });
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _run(String label, Future<vutric.RunReport> Function() job) async {
    setState(() {
      _busy = true;
      _running = true;
      _status = label;
      _error = null;
      _report = null;
    });
    try {
      final r = await afterDisconnect(job);
      if (!mounted) return;
      setState(() {
        _report = r;
        _status = 'Tayyor';
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = isCancelled(e) ? null : errorText(e);
        _status = isCancelled(e) ? 'Bekor qilindi' : 'Xato';
      });
    } finally {
      if (mounted) {
        setState(() {
          _busy = false;
          _running = false;
        });
      }
    }
  }

  void _connectLink(String link) => _run('Havola orqali ulanmoqda…', () => vutric.runLink(link: link, runs: _runs));

  Future<String?> _scanLink() => scanLink(context);

  void _openFling() => Navigator.of(context).push(
    MaterialPageRoute(
      builder: (_) => FlingPage(scan: _scanLink, link: _link.text),
    ),
  );

  void _openCatch() => Navigator.of(context).push(
    MaterialPageRoute(
      builder: (_) => CatchPage(scan: _scanLink, myLink: _myLink!),
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
      appBar: AppBar(backgroundColor: sand, title: const Text('Vutric · spike 0.2')),
      body: SafeArea(
        child: ListView(
          padding: const EdgeInsets.all(16),
          children: [
            Text(_status, style: t.bodyMedium?.copyWith(color: cobalt)),
            const SizedBox(height: 12),
            TextField(
              controller: _name,
              decoration: const InputDecoration(labelText: 'Qurilma nomi', filled: true, fillColor: paper),
              onSubmitted: (_) => _start(),
            ),
            const SizedBox(height: 16),
            FilledButton.icon(
              onPressed: ready ? _openFling : null,
              icon: const Icon(Icons.north),
              label: const Text('Yuborish: rasm otish yoki fayllar'),
            ),
            const SizedBox(height: 8),
            FilledButton.tonalIcon(
              onPressed: ready ? _openCatch : null,
              icon: const Icon(Icons.south),
              label: const Text('Qabul qilish'),
            ),
            const SizedBox(height: 8),
            if (hasMdns) ...[
              OutlinedButton(
                onPressed: ready
                    ? () => _run('Kompyuter qidirilmoqda (mDNS)…', () => vutric.runDiscovered(runs: _runs))
                    : null,
                child: const Text('Kompyuterni topish va oʻlchash'),
              ),
              const SizedBox(height: 8),
            ],
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
                fillColor: paper,
              ),
            ),
            const SizedBox(height: 8),
            OutlinedButton(onPressed: ready ? _scan : null, child: const Text('QR kodni skanerlab oʻlchash')),
            const SizedBox(height: 8),
            OutlinedButton(
              onPressed: ready ? () => _connectLink(_link.text) : null,
              child: const Text('Havola orqali ulanib oʻlchash'),
            ),
            if (_busy) ...[
              const Padding(
                padding: EdgeInsets.all(16),
                child: LinearProgressIndicator(color: signal),
              ),
              if (_running) OutlinedButton(onPressed: disconnectNow, child: const Text('Bekor qilish')),
            ],
            if (_error != null)
              Padding(
                padding: const EdgeInsets.only(top: 16),
                child: Text(_error!, style: t.bodyMedium?.copyWith(color: err)),
              ),
            if (_report != null) _ReportCard(report: _report!),
            if (_myLink != null) ...[
              const SizedBox(height: 24),
              Text('Bu qurilmaning QR kodi', style: t.titleSmall?.copyWith(color: cobalt)),
              const SizedBox(height: 8),
              Center(
                child: Container(
                  color: paper,
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
      color: paper,
      margin: const EdgeInsets.only(top: 16),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('${report.peerName} (${report.peerPlatform})', style: t.titleMedium?.copyWith(color: cobalt)),
            const SizedBox(height: 8),
            Text('Tekshiruv kodi', style: t.labelMedium),
            Text(report.checkCode, style: t.displaySmall?.copyWith(color: cobalt, letterSpacing: 4)),
            const SizedBox(height: 8),
            if (report.foundMs > 0) Text('Topildi: ${ms(report.foundMs)}'),
            if (report.connectMs > 0) Text('Ulandi: ${ms(report.connectMs)}'),
            const SizedBox(height: 8),
            Text('Telefon → ${report.peerName}, bir tomonga: p50 ${ms(report.sentP50Ms)}, p95 ${ms(report.sentP95Ms)}'),
            Text(
              pass ? 'Mezon bajarildi (p50 < 40, p95 < 80 ms)' : 'Mezondan oshdi (p50 < 40, p95 < 80 ms)',
              style: t.bodyMedium?.copyWith(color: pass ? ok : err),
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
