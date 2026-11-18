// Logic that runs without Rust or a device: themes (contrast included), the Wi-Fi QR, sizes, names, history JSON.

import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:vutric_spike/design/themes.g.dart';
import 'package:vutric_spike/main.dart' show themeFor;
import 'package:vutric_spike/companion.dart' show clipboardFileName;
import 'package:vutric_spike/i18n.dart';
import 'package:vutric_spike/screens/connect.dart' show ConnectWay, waysFor;
import 'package:vutric_spike/screens/device_page.dart' show agoText;
import 'package:vutric_spike/screens/hotspot_page.dart' show hotspotError;
import 'package:vutric_spike/session.dart';
import 'package:vutric_spike/transfer_ui.dart';
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
    test('clipboard file name has no character an OS refuses', () {
      final n = clipboardFileName(DateTime(2026, 11, 8, 4, 5, 6));
      expect(n, 'Bufer 2026-11-08 04.05.06.txt');
      expect(n.contains(RegExp(r'[\\/:*?"<>|]')), isFalse);
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
