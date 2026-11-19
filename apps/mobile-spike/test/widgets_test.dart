// The design widgets in every theme, on the narrowest phone (320 dp) with the largest common text size (2x):
// nothing may overflow, every tap target is at least 48 dp and labelled.

import 'dart:ui' show SemanticsAction;

import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:vutric_spike/design/themes.g.dart';
import 'package:vutric_spike/design/tokens.dart';
import 'package:vutric_spike/design/widgets.dart';

Widget gallery() => ListView(
  padding: const EdgeInsets.all(16),
  children: [
    const VNavHeader(title: 'Sozlamalar'),
    VButton(label: 'Yuborish · 12', icon: Icons.arrow_upward_rounded, onPressed: () {}),
    VButton(label: 'Bekor qilish', kind: VButtonKind.secondary, onPressed: () {}),
    VButton(label: 'Keyinroq', kind: VButtonKind.ghost, onPressed: () {}),
    const VButton(label: 'Ulanmoqda', busy: true),
    Row(
      children: [VCircleButton(icon: Icons.close_rounded, label: 'Yopish', onTap: () {})],
    ),
    VDevicePill(name: 'Nosirbekning MacBook Pro', platform: 'macos', code: 'K7Q2', onTap: () {}),
    const VDevicePill(name: 'Telefon', lost: true),
    const VResultPill(text: 'Ekranda', mono: '38 ms'),
    VNotice(title: 'Aloqa uzildi', body: 'Qurilmalar bir tarmoqdami, tekshiring.', onClose: () {}),
    VRowCard(icon: Icons.folder_outlined, title: 'Saqlash papkasi', subtitle: 'Download/Vutric', onTap: () {}),
    const VEmpty(
      icon: Icons.history_rounded,
      title: 'Hali hech narsa yoʻq',
      body: 'Yuborilgan va kelgan fayllar shu yerda.',
    ),
    VSegmented(labels: const ['Rasmlar', 'Fayllar'], index: 0, onChanged: (_) {}),
    VGlassTabBar(
      tabs: const [
        VTab(Icons.north_east_rounded, 'Otish'),
        VTab(Icons.south_west_rounded, 'Qabul'),
        VTab(Icons.history_rounded, 'Tarix'),
      ],
      index: 0,
      onTap: (_) {},
      sideIcon: Icons.qr_code_rounded,
      sideLabel: 'Mening QR kodim',
      onSide: () {},
    ),
  ],
);

Future<void> pumpIn(WidgetTester tester, VutricTokens t, {double scale = 2}) async {
  tester.view
    ..physicalSize = const Size(320, 2400)
    ..devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    MaterialApp(
      theme: buildTheme(t),
      home: MediaQuery(
        data: MediaQueryData(size: const Size(320, 2400), textScaler: TextScaler.linear(scale)),
        child: Scaffold(body: gallery()),
      ),
    ),
  );
  await tester.pump(const Duration(seconds: 1));
}

void main() {
  for (final t in vutricThemes) {
    testWidgets('${t.id}: 320 dp, text 2x, no overflow', (tester) async {
      await pumpIn(tester, t);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('device pill: one label, still tappable by a screen reader', (tester) async {
    final handle = tester.ensureSemantics();
    await pumpIn(tester, vutricThemes.first, scale: 1);
    final node = tester.getSemantics(find.bySemanticsLabel(RegExp('ga ulangan')));
    expect(node.label, 'Nosirbekning MacBook Pro ga ulangan, tekshiruv kodi K7Q2');
    expect(node.getSemanticsData().hasAction(SemanticsAction.tap), isTrue);
    handle.dispose();
  });

  testWidgets('tap targets are 48 dp and labelled', (tester) async {
    final handle = tester.ensureSemantics();
    await pumpIn(tester, vutricThemes.first, scale: 1);
    await expectLater(tester, meetsGuideline(androidTapTargetGuideline));
    await expectLater(tester, meetsGuideline(labeledTapTargetGuideline));
    handle.dispose();
  });
}
