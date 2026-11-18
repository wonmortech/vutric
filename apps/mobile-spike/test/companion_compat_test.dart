// The screens added for W3 #11, #12 and the R1a/Rb/C7 rows (ADR-0014): clipboard sheet, Companion app picker,
// wormhole code dialogs, the big-batch dialog and the failed-connect explanation. Same bar as widgets_test.dart:
// every theme, 320 dp, text 2x, no overflow; tap targets 48 dp and labelled. No Rust library is loaded.

import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:vutric_spike/companion.dart';
import 'package:vutric_spike/design/themes.g.dart';
import 'package:vutric_spike/design/tokens.dart';
import 'package:vutric_spike/design/widgets.dart';
import 'package:vutric_spike/i18n.dart';
import 'package:vutric_spike/screens/app_picker_page.dart';
import 'package:vutric_spike/screens/connect.dart' show diagnosisText;
import 'package:vutric_spike/screens/foreign_page.dart';
import 'package:vutric_spike/src/rust/api.dart' as vutric;
import 'package:vutric_spike/transfer_ui.dart' show BulkDialog;

const _apps = <AppEntry>[
  (package: 'uz.bank.mobile', label: 'Bank'),
  (package: 'org.telegram.messenger', label: 'Telegram'),
  (package: 'com.example.with.a.very.long.package.name.that.wraps', label: 'Juda uzun nomli ilova, sinov uchun'),
];

Future<List<AppEntry>> _load() async => _apps;

Widget gallery() {
  final why = diagnosisText(
    const vutric.Diagnosis(
      key: 'lan.unreachable.captive',
      next: ['sign_in', 'hotspot', 'cable'],
      login: 'http://login.mehmonxona.example/portal?room=1204',
    ),
  );
  return ListView(
    padding: const EdgeInsets.all(16),
    children: [
      const ClipboardSheet(peer: 'Nosirbekning MacBook Pro', text: 'salom dunyo\nikkinchi qator'),
      VNotice(title: why.title, body: why.body),
      const BulkDialog(bytes: 3 << 30, refuse: false),
      const BulkDialog(bytes: 12 << 30, refuse: true),
      const WormholeCodeDialog(code: '7-guitarist-revenge'),
    ],
  );
}

Future<void> pumpIn(WidgetTester tester, VutricTokens t, Widget child, {double scale = 2}) async {
  tester.view
    ..physicalSize = const Size(320, 3600)
    ..devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    MaterialApp(
      theme: buildTheme(t),
      home: MediaQuery(
        data: MediaQueryData(size: const Size(320, 3600), textScaler: TextScaler.linear(scale)),
        child: child,
      ),
    ),
  );
  await tester.pump(const Duration(seconds: 1));
}

void main() {
  setUp(() => uiLanguage = 'uz');

  for (final t in vutricThemes) {
    testWidgets('${t.id}: companion and compat screens at 320 dp, text 2x, no overflow', (tester) async {
      await pumpIn(tester, t, Scaffold(body: gallery()));
      expect(tester.takeException(), isNull);
      await pumpIn(tester, t, const AppPickerPage(selected: ['uz.bank.mobile'], load: _load));
      expect(tester.takeException(), isNull);
      await pumpIn(tester, t, const Scaffold(body: WormholeCodeEntry()));
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('companion and compat screens: tap targets are 48 dp and labelled', (tester) async {
    final handle = tester.ensureSemantics();
    for (final w in [
      Scaffold(body: gallery()),
      const AppPickerPage(selected: [], load: _load),
    ]) {
      await pumpIn(tester, vutricThemes.first, w, scale: 1);
      await expectLater(tester, meetsGuideline(androidTapTargetGuideline));
      await expectLater(tester, meetsGuideline(labeledTapTargetGuideline));
    }
    handle.dispose();
  });

  testWidgets('app picker: nothing picked says nothing is mirrored; search and save', (tester) async {
    List<String>? saved;
    tester.view
      ..physicalSize = const Size(400, 900)
      ..devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      MaterialApp(
        theme: buildTheme(vutricThemes.first),
        home: Builder(
          builder: (context) => Scaffold(
            body: Center(
              child: TextButton(
                onPressed: () async => saved = await Navigator.of(context).push<List<String>>(
                  MaterialPageRoute(builder: (_) => const AppPickerPage(selected: [], load: _load)),
                ),
                child: const Text('ochish'),
              ),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('ochish'));
    await tester.pumpAndSettle();
    expect(find.text(tr('companion.apps.none')), findsOneWidget);
    await tester.enterText(find.byType(TextField), 'tele');
    await tester.pump();
    expect(find.text('Bank'), findsNothing);
    await tester.tap(find.text('Telegram'));
    await tester.pump();
    expect(find.text(tr('companion.apps.some', {'n': '1'})), findsOneWidget);
    await tester.tap(find.text(tr('companion.apps.some', {'n': '1'})));
    await tester.pumpAndSettle();
    expect(saved, ['org.telegram.messenger']);
  });

  testWidgets('big batch through the relay: asks, or refuses with no send button', (tester) async {
    await pumpIn(tester, vutricThemes.first, const Scaffold(body: BulkDialog(bytes: 3 << 30, refuse: false)), scale: 1);
    expect(find.text(tr('bulk.ask.go')), findsOneWidget);
    await pumpIn(tester, vutricThemes.first, const Scaffold(body: BulkDialog(bytes: 12 << 30, refuse: true)), scale: 1);
    expect(find.text(tr('bulk.ask.go')), findsNothing);
    expect(find.text(tr('bulk.too_big')), findsOneWidget);
  });

  test('every key these screens use has an Uzbek line', () {
    for (final k in [
      'companion.clipboard.came',
      'companion.clipboard.copy',
      'companion.clipboard.close',
      'companion.clipboard.copied',
      'companion.clipboard.sent',
      'companion.clipboard.too_long',
      'companion.untrusted',
      'companion.apps',
      'companion.apps.none',
      'companion.apps.some',
      'compat.offer.title',
      'compat.offer.body',
      'compat.pair.title',
      'compat.pair.body',
      'compat.unavailable',
      'compat.off',
      'compat.bad_path',
      'compat.kde_unpaired',
      'wormhole.internet',
      'wormhole.lan_only',
      'wormhole.code.body',
      'bulk.ask.title',
      'bulk.ask.body',
      'bulk.too_big',
      'lan.captive.body',
    ]) {
      expect(hasUzbek(k), isTrue, reason: k);
    }
  });
}
