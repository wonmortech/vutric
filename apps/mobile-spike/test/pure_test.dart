// Logic that runs without Rust or a device: themes (contrast included), the Wi-Fi QR, sizes, names, history JSON.

import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:vutric_spike/design/themes.g.dart';
import 'package:vutric_spike/main.dart' show themeFor;
import 'package:vutric_spike/companion.dart' show clipboardProblem;
import 'package:vutric_spike/compat.dart';
import 'package:vutric_spike/i18n.dart';
import 'package:vutric_spike/screens/connect.dart' show ConnectWay, diagnosisText, waysFor;
import 'package:vutric_spike/screens/device_page.dart' show agoText;
import 'package:vutric_spike/screens/hotspot_page.dart' show hotspotError;
import 'package:vutric_spike/session.dart';
import 'package:vutric_spike/transfer_ui.dart';
import 'package:vutric_spike/src/rust/api.dart' as vutric;
import 'package:vutric_spike/ui.dart' show errorText, routeLabel;

/// WCAG 2.2 contrast ratio of [fg] over [bg] (alpha composited onto [bg] first).
double contrast(Color fg, Color bg) {
  final f = Color.alphaBlend(fg, bg);
  final a = f.computeLuminance(), b = bg.computeLuminance();
  final hi = a > b ? a : b, lo = a > b ? b : a;
  return (hi + 0.05) / (lo + 0.05);
}

void main() {
  group('themes', () {
    test('ids are unique and the auto pair exists', () {
      final ids = [for (final t in vutricThemes) t.id];
      expect(ids.toSet().length, ids.length);
      expect(ids, containsAll(['ios', 'ios-dark']));
    });

    test('auto follows the platform brightness; unknown ids fall back', () {
      expect(themeFor(autoTheme, Brightness.light).id, 'ios');
      expect(themeFor(autoTheme, Brightness.dark).id, 'ios-dark');
      expect(themeFor('no-such-theme', Brightness.light).id, vutricThemes.first.id);
    });

    // WCAG AA for body text (4.5:1). Every theme, every text pair the screens use.
    for (final t in vutricThemes) {
      test('${t.id}: text pairs meet WCAG AA', () {
        // Translucent surfaces are measured where they sit: on the page background.
        Color on(Color c) => Color.alphaBlend(c, t.bg);
        final pairs = {
          'ink/bg': (t.ink, t.bg),
          'ink/bgAlt': (t.ink, on(t.bgAlt)),
          'ink/raised': (t.ink, on(t.raised)),
          'muted/bg': (t.muted, t.bg),
          'muted/raised': (t.muted, on(t.raised)),
          'actionInk/action': (t.actionInk, on(t.action)),
          'accentInk/accent': (t.accentInk, on(t.accent)),
          'heroInk/hero': (t.heroInk, on(t.hero)),
        };
        final failing = {
          for (final e in pairs.entries)
            if (contrast(e.value.$1, e.value.$2) < 4.5) e.key: contrast(e.value.$1, e.value.$2).toStringAsFixed(2),
        };
        expect(failing, isEmpty, reason: '${t.id} below 4.5:1');
      });
    }
  });

  group('route plan to connect ways (ADR-0060)', () {
    const auto = ['same_network', 'internet_direct', 'relay', 'offer_hotspot'];
    test('Avtomatik: every way, the hotspot last', () {
      expect(waysFor(auto, mdns: true, android: true), [
        ConnectWay.scan,
        ConnectWay.find,
        ConnectWay.typed,
        ConnectWay.wait,
        ConnectWay.hotspot,
      ]);
    });
    test('iPhone: no mDNS row, no hotspot row', () {
      expect(waysFor(auto, mdns: false, android: false), [ConnectWay.scan, ConnectWay.typed, ConnectWay.wait]);
    });
    test('Internet orqali: no LAN search, no typed address', () {
      expect(waysFor(['internet_direct', 'relay'], mdns: true, android: true), [ConnectWay.scan, ConnectWay.wait]);
    });
    test('Tarmoqsiz leads with the hotspot; nothing when nothing works', () {
      expect(waysFor(['offer_hotspot'], mdns: true, android: true), [ConnectWay.hotspot]);
      expect(waysFor(const [], mdns: true, android: true), isEmpty);
    });
    test('a restart only when the reach changes', () {
      expect(routeReachChanges('auto', 'internet'), isFalse);
      expect(routeReachChanges('lan_only', 'offline'), isFalse);
      expect(routeReachChanges('auto', 'lan_only'), isTrue);
      expect(routeReachChanges('offline', 'internet'), isTrue);
    });
  });

  group('i18n', () {
    test('every key the engine hands over has an Uzbek line', () {
      const fromRust = [
        'route.picker.auto',
        'route.picker.lan_only',
        'route.picker.internet',
        'route.picker.offline',
        'route.picker.bluetooth',
        'route.badge.direct_lan',
        'route.badge.direct_internet',
        'route.badge.relay',
        'route.badge.hotspot',
        'route.badge.cable',
        'route.badge.connecting',
        'route.none.not_built',
        'route.none.unavailable',
        'privacy.encrypted',
        'privacy.nothing_leaves',
        'find.silent',
        'find.denied',
        'find.unreadable',
        'find.no_beacon',
      ];
      expect([
        for (final k in fromRust)
          if (!hasUzbek(k)) k,
      ], isEmpty);
      expect([
        for (final k in allKeys)
          if (!hasUzbek(k)) k,
      ], isEmpty);
    });
    test('the S6 lines say exactly what ADR-0061 says', () {
      uiLanguage = 'uz';
      expect(tr('privacy.encrypted'), 'Fayllar shifrlangan, faqat siz va qabul qiluvchi koʻradi');
      expect(tr('privacy.nothing_leaves'), 'Hech narsa internetga chiqmaydi');
    });
    test('placeholders, the English fallback and unknown keys', () {
      uiLanguage = 'en';
      expect(tr('route.picker.lan_only'), 'This network only');
      expect(tr('pair.yes'), 'Bir xil'); // no English line yet: Uzbek, never empty
      uiLanguage = 'uz';
      expect(tr('visibility.everyone.left', {'m': '7'}), 'yana 7 daqiqa');
      expect(tr('no.such.key'), 'no.such.key');
    });
    test('Rust errors that carry a key are shown through it; badges read', () {
      uiLanguage = 'uz';
      expect(errorText('AnyhowException(find.silent)'), tr('find.silent'));
      expect(
        errorText('AnyhowException(the other device accepts only its trusted devices right now)'),
        contains('juftlang'),
      );
      expect(routeLabel('route.badge.hotspot'), 'Hotspot');
      expect(routeLabel(''), tr('route.badge.connecting'));
    });
  });

  group('small helpers', () {
    test('clipboard text is checked before it goes (ADR-0065: empty or over 64 KiB is refused)', () {
      expect(clipboardProblem(''), 'companion.clipboard.empty');
      expect(clipboardProblem('   \n'), 'companion.clipboard.empty');
      expect(clipboardProblem('salom'), isNull);
      expect(clipboardProblem('a' * (64 * 1024)), isNull);
      expect(clipboardProblem('a' * (64 * 1024 + 1)), 'companion.clipboard.too_long');
      // Bytes, not characters: 2-byte letters reach the cap at half the count.
      expect(clipboardProblem('ʻ' * (32 * 1024 + 1)), 'companion.clipboard.too_long');
    });
    test('an empty incoming means paused, any content does not', () {
      expect(isPaused(const vutric.Incoming()), isTrue);
      expect(isPaused(const vutric.Incoming(clipboard: 'x')), isFalse);
      expect(
        isPaused(
          vutric.Incoming(
            notice: vutric.CompanionNotice(app: 'a', title: 't', text: '', atMs: BigInt.zero),
          ),
        ),
        isFalse,
      );
    });
    test('failed connects are explained, the captive portal with its login page (R1a)', () {
      final c = diagnosisText(
        const vutric.Diagnosis(
          key: 'lan.unreachable.captive',
          next: ['sign_in', 'hotspot', 'cable'],
          login: 'http://login.hotel.example/',
        ),
      );
      expect(c.title, tr('lan.unreachable.captive'));
      expect(c.body, contains(tr('lan.captive.body')));
      expect(c.body, contains('http://login.hotel.example/'));
      expect(c.body, contains(tr('lan.next.sign_in')));
      final iso = diagnosisText(const vutric.Diagnosis(key: 'lan.unreachable.isolated', next: ['relay'], login: ''));
      expect(iso.body, tr('lan.next.relay'));
      for (final k in ['local_network_denied', 'isolated', 'other_network', 'no_network', 'captive']) {
        expect(hasUzbek('lan.unreachable.$k'), isTrue, reason: k);
      }
      for (final n in ['fix_permission', 'relay', 'hotspot', 'cable', 'sign_in']) {
        expect(hasUzbek('lan.next.$n'), isTrue, reason: n);
      }
    });
    test('compat events from the :compat process are parsed and capped (C10, ADR-0066)', () {
      final seen = parseCompatEvent(
        '{"t":"seen","alias":"Ali\\u0007 PC","device_type":"desktop","to":"192.168.1.5:53317","https":true,'
        '"via":"localsend"}',
      );
      expect(seen, isA<CompatSeen>());
      final d = (seen! as CompatSeen).device;
      expect(d.name, 'Ali  PC');
      expect(d.host, '192.168.1.5');
      expect(d.https, isTrue);
      expect(d.key, 'localsend:192.168.1.5:53317');
      final kde =
          parseCompatEvent(
                '{"t":"seen","alias":"${'x' * 300}","to":"10.0.0.2:1716","https":false,"via":"kdeconnect","device_id":"abc"}',
              )!
              as CompatSeen;
      expect(kde.device.key, 'kdeconnect:abc');
      expect(kde.device.name.length, 64);
      final offer =
          parseCompatEvent(
                '{"t":"offer","session_id":"s1","peer":"10.0.0.2","peer_cert":null,"alias":"Telefon",'
                '"files":[{"id":"a","name":"x.jpg","size":10,"file_type":"image/jpeg"},'
                '{"id":"b","name":"y.pdf","size":5,"file_type":"application/pdf"}],"via":"localsend"}',
              )!
              as CompatOffer;
      expect(offer.files.length, 2);
      expect(offer.bytes, 15);
      expect(parseCompatEvent('{"t":"received","session_id":"s1","file_id":"a","name":"x","size":1}'), isNull);
      expect(parseCompatEvent('{"t":"wormhole_code","request_id":3,"code":"7-a-b"}'), isA<CompatAnswer>());
      expect((parseCompatEvent('{"t":"send_failed","request_id":4,"reason":""}')! as CompatAnswer).reason, 'failed');
      expect(parseCompatEvent('{"t":"ready","fingerprint":"f","port":1}'), isNull);
      expect(parseCompatEvent('not json'), isNull);
      expect(parseCompatEvent('[1,2]'), isNull);
    });
    test('the adapters keep devices and answers, and drop them when the process ends', () {
      final c = Compat.instance;
      c.handle(
        const CompatSeen(CompatDevice(via: 'localsend', name: 'PC', to: '1.2.3.4:53317', https: true, type: '')),
      );
      expect(c.devices.length, 1);
      c.handle(const CompatPaired('kde1', on: true));
      expect(c.pairedKde, contains('kde1'));
      c.handle(const CompatPaired('kde1', on: false));
      expect(c.pairedKde, isEmpty);
      c.handle(const CompatClosed());
      expect(c.devices, isEmpty);
      expect(c.running, isFalse);
    });
    test('magic-wormhole is offered only when the route may use the internet (ADR-0067)', () {
      expect(wormholeAllowed('auto'), isTrue);
      expect(wormholeAllowed('internet'), isTrue);
      expect(wormholeAllowed('lan_only'), isFalse);
      expect(wormholeAllowed('offline'), isFalse);
      expect(wormholeAllowed('bluetooth'), isFalse);
    });
    test('last seen in words', () {
      final now = DateTime(2026, 11, 18, 12);
      int ago(Duration d) => now.subtract(d).millisecondsSinceEpoch ~/ 1000;
      expect(agoText(0, now), '—');
      expect(agoText(ago(const Duration(seconds: 20)), now), 'hozirgina');
      expect(agoText(ago(const Duration(minutes: 5)), now), '5 daqiqa oldin');
      expect(agoText(ago(const Duration(hours: 3)), now), '3 soat oldin');
      expect(agoText(ago(const Duration(days: 3)), now), '3 kun oldin');
      expect(agoText(ago(const Duration(days: 65)), now), '2 oy oldin');
    });
    test('hotspot errors are said plainly, unknown codes still show', () {
      expect(hotspotError('permission'), contains('ruxsati'));
      expect(hotspotError('weird'), contains('weird'));
    });
  });

  group('sizeText', () {
    test('steps through units', () {
      expect(sizeText(999), '999 B');
      expect(sizeText(1500), '2 KB');
      expect(sizeText(12.34e6), '12.3 MB');
      expect(sizeText(1.5e9), '1.50 GB');
    });
  });

  group('names', () {
    test('baseName handles both separators and a trailing slash', () {
      expect(baseName('/storage/emulated/0/DCIM/IMG_1.jpg'), 'IMG_1.jpg');
      expect(baseName(r'C:\Users\a\doc.pdf'), 'doc.pdf');
      expect(baseName('/a/folder/'), 'folder');
      expect(baseName(''), 'fayl');
    });

    test('fileIcon by extension, case-insensitive', () {
      expect(fileIcon('A.JPG'), Icons.image_outlined);
      expect(fileIcon('clip.mov'), Icons.movie_outlined);
      expect(fileIcon('x.pdf'), Icons.picture_as_pdf_outlined);
      expect(fileIcon('archive.tar'), Icons.folder_zip_outlined);
      expect(fileIcon('noext'), Icons.folder_outlined);
      expect(fileIcon('a.xyz'), Icons.insert_drive_file_outlined);
    });
  });

  group('HistoryItem', () {
    test('round-trips through JSON', () {
      final h = HistoryItem(
        sent: true,
        peer: 'Mac',
        files: 2,
        bytes: 1234,
        seconds: 1.5,
        at: DateTime.utc(2026, 11, 18, 9, 30),
        names: ['a.jpg', 'b.mp4'],
        paths: ['/x/a.jpg'],
        ms: 42.5,
      );
      final back = HistoryItem.fromJson(h.toJson());
      expect(back.toJson(), h.toJson());
    });

    test('a damaged or older entry still loads', () {
      final h = HistoryItem.fromJson({
        'sent': 'yes',
        'files': 3,
        'seconds': 2,
        'at': 'garbage',
        'names': [1, 'b'],
      });
      expect(h.sent, isFalse);
      expect(h.peer, '');
      expect(h.files, 3);
      expect(h.seconds, 2.0);
      expect(h.names, ['1', 'b']);
      expect(h.paths, isEmpty);
      expect(h.ms, isNull);
      expect(h.toJson().containsKey('ms'), isFalse);
    });
  });
}
