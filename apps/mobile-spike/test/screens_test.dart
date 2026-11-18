// The W1 screens built on the W3 API (ADR-0013): route picker and badge, privacy line, pairing digits, foreign
// devices, the device list and the device page. Same bar as widgets_test.dart: every theme, 320 dp, text 2x, no
// overflow; tap targets 48 dp and labelled. Nothing here loads the Rust library: the device page takes its
// reads and writes as functions.

import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:vutric_spike/android.dart';
import 'package:vutric_spike/design/themes.g.dart';
import 'package:vutric_spike/design/tokens.dart';
import 'package:vutric_spike/i18n.dart';
import 'package:vutric_spike/screens/connect.dart';
import 'package:vutric_spike/screens/device_page.dart';
import 'package:vutric_spike/screens/route_picker.dart';
import 'package:vutric_spike/screens/throw_page.dart';
import 'package:vutric_spike/src/rust/api.dart' as vutric;

final _now = BigInt.from(DateTime.now().millisecondsSinceEpoch ~/ 1000);

vutric.DeviceInfo device({String name = 'Nosirbekning MacBook Pro', bool rules = true}) => vutric.DeviceInfo(
  id: 'a' * 64,
  name: name,
  platform: 'macos',
  method: 'code',
  pairedAt: _now - BigInt.from(86400 * 3),
  lastSeen: _now - BigInt.from(3600),
  code: 'K7Q2-M4XD',
  hasRules: rules,
);

const _peer = vutric.Connected(
  peerName: 'Nosirbekning MacBook Pro',
  peerPlatform: 'macos',
  checkCode: 'K7Q2',
  connectMs: 38,
  route: 'route.badge.direct_lan',
  peerId: '',
  peerCode: 'K7Q2-M4XD',
  needsCheck: false,
  trusted: true,
);

Widget gallery() => ListView(
  padding: const EdgeInsets.all(16),
  children: [
    Row(
      children: [RouteChip(route: 'lan_only', onTap: () {})],
    ),
    RouteList(current: 'auto', onPick: (_) {}),
    const PrivacyLine(keys: ['privacy.encrypted', 'privacy.nothing_leaves']),
    const PairDigits(digits: '483920', waiting: false),
    const PairDigits(digits: '483920', waiting: true),
    const Wrap(
      children: [
        RouteBadge(badge: 'route.badge.direct_internet'),
        RouteBadge(badge: 'route.badge.relay'),
        RouteBadge(badge: 'route.badge.hotspot'),
        RouteBadge(badge: ''),
      ],
    ),
    const ForeignList(
      devices: [
        ForeignDevice(app: 'localsend', id: 'f1', name: 'Pixel 8', type: 'mobile', host: '192.168.1.20', port: 53317),
        ForeignDevice(app: 'kdeconnect', id: 'k1', name: '', type: 'desktop', host: '192.168.1.31', port: 1716),
      ],
    ),
    DeviceList(
      devices: [
        device(),
        device(name: '', rules: false),
      ],
      onOpen: (_) {},
    ),
    const DeviceList(devices: [], onOpen: _ignore),
    OverrideRow(
      label: 'Turi boʻyicha papkalar',
      value: 'general',
      items: {'general': tr('devices.rules.general'), 'yes': 'Ha', 'no': 'Yoʻq'},
      onChanged: (_) {},
    ),
    const PeerSheet(peer: _peer),
  ],
);

void _ignore(vutric.DeviceInfo _) {}

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

/// The device page with an in-memory rules store.
class _Store {
  String json = jsonEncode({
    'accept': {'mode': 'ask'},
    'gallery': false,
  });
  final saved = <String>[];
  bool forgot = false;

  DevicePage page() => DevicePage(
    device: device(),
    load: (_) async => json,
    save: (_, j) async {
      json = j;
      saved.add(j);
    },
    forget: (_) async => forgot = true,
  );
}

void main() {
  setUp(() => uiLanguage = 'uz');

  for (final t in vutricThemes) {
    testWidgets('${t.id}: W1 screens at 320 dp, text 2x, no overflow', (tester) async {
      await pumpIn(tester, t, Scaffold(body: gallery()));
      expect(tester.takeException(), isNull);
      await pumpIn(tester, t, _Store().page());
      expect(tester.takeException(), isNull);
      expect(find.text('Har safar soʻrash'), findsOneWidget);
    });
  }

  testWidgets('W1 screens: tap targets are 48 dp and labelled', (tester) async {
    final handle = tester.ensureSemantics();
    await pumpIn(tester, vutricThemes.first, Scaffold(body: gallery()), scale: 1);
    await expectLater(tester, meetsGuideline(androidTapTargetGuideline));
    await expectLater(tester, meetsGuideline(labeledTapTargetGuideline));
    await pumpIn(tester, vutricThemes.first, _Store().page(), scale: 1);
    await expectLater(tester, meetsGuideline(androidTapTargetGuideline));
    await expectLater(tester, meetsGuideline(labeledTapTargetGuideline));
    handle.dispose();
  });

  testWidgets('pairing digits read one by one to a screen reader', (tester) async {
    final handle = tester.ensureSemantics();
    await pumpIn(
      tester,
      vutricThemes.first,
      const Scaffold(body: PairDigits(digits: '483920', waiting: false)),
      scale: 1,
    );
    expect(find.text('483 920'), findsOneWidget);
    expect(find.bySemanticsLabel('4 8 3 9 2 0'), findsOneWidget);
    handle.dispose();
  });

  testWidgets('Bluetooth is listed but cannot be picked', (tester) async {
    String? picked;
    await pumpIn(
      tester,
      vutricThemes.first,
      Scaffold(
        body: RouteList(current: 'auto', onPick: (r) => picked = r),
      ),
      scale: 1,
    );
    await tester.tap(find.text(tr('route.picker.bluetooth')));
    expect(picked, isNull);
    await tester.tap(find.text(tr('route.picker.lan_only')));
    expect(picked, 'lan_only');
  });

  testWidgets('device page: a change is written whole; Umumiy removes the override', (tester) async {
    final store = _Store();
    await pumpIn(tester, vutricThemes.first, store.page(), scale: 1);
    await tester.tap(find.text('Har safar soʻrash'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Soʻramasdan').last);
    await tester.pumpAndSettle();
    expect(jsonDecode(store.saved.last), {
      'accept': {'mode': 'trusted'},
      'gallery': false,
    });

    await tester.tap(find.text('Yoʻq').first);
    await tester.pumpAndSettle();
    await tester.tap(find.text(tr('devices.rules.general')).last);
    await tester.pumpAndSettle();
    expect((jsonDecode(store.saved.last) as Map)['gallery'], isNull);
  });

  testWidgets('device page: Unutish asks first', (tester) async {
    final store = _Store();
    await pumpIn(tester, vutricThemes.first, store.page(), scale: 1);
    await tester.ensureVisible(find.text(tr('devices.forget')));
    await tester.tap(find.text(tr('devices.forget')));
    await tester.pumpAndSettle();
    expect(find.text(tr('devices.forget.confirm')), findsOneWidget);
    await tester.tap(find.text('Bekor qilish'));
    await tester.pumpAndSettle();
    expect(store.forgot, isFalse);
  });
}
