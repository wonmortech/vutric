// Logic that runs without Rust or a device: themes (contrast included), the Wi-Fi QR, sizes, names, history JSON.

import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:vutric_spike/design/themes.g.dart';
import 'package:vutric_spike/main.dart' show themeFor;
import 'package:vutric_spike/screens/hotspot_page.dart' show wifiQr;
import 'package:vutric_spike/session.dart';
import 'package:vutric_spike/transfer_ui.dart';

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

  group('wifiQr', () {
    test('plain network', () {
      expect(wifiQr('AndroidShare_1234', 'abcd1234'), 'WIFI:T:WPA;S:AndroidShare_1234;P:abcd1234;;');
    });

    test('special characters are escaped so the camera reads the right password', () {
      expect(wifiQr(r'a;b,c', r'p:"\'), r'WIFI:T:WPA;S:a\;b\,c;P:p\:\"\\;;');
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
