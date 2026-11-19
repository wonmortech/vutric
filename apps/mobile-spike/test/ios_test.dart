// The Dart half of the iOS modules (ADR-0015) and the static parts of the Xcode project that no Mac checks on a
// Linux runner: the String Catalogs agree with lib/i18n.dart, every target names the App Group, and Bonjour is
// declared where discovery runs.

import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:vutric_spike/i18n.dart';
import 'package:vutric_spike/ios.dart';

Map<String, ({String uz, String en})> _catalog(String path) {
  final j = jsonDecode(File(path).readAsStringSync()) as Map<String, dynamic>;
  final strings = j['strings'] as Map<String, dynamic>;
  String line(Map<String, dynamic> v, String lang) =>
      ((v['localizations'] as Map)[lang] as Map)['stringUnit']['value'] as String;
  return {
    for (final e in strings.entries)
      e.key: (uz: line(e.value as Map<String, dynamic>, 'uz'), en: line(e.value as Map<String, dynamic>, 'en')),
  };
}

const _kit = 'ios/VutricKit/Sources/VutricKit/Resources/Localizable.xcstrings';
const _runner = 'ios/Runner/Localizable.xcstrings';

void main() {
  group('events', () {
    test('a send request keeps only a well-formed device id', () {
      final id = 'ab' * 32;
      expect(parseIosEvent({'kind': 'send', 'device': id})?.device, id);
      expect(parseIosEvent({'kind': 'send'})?.device, isNull);
      expect(parseIosEvent({'kind': 'send', 'device': '../etc'})?.device, isNull);
      expect(parseIosEvent({'kind': 'send', 'device': 'zz' * 32})?.device, isNull);
      expect(parseIosEvent({'kind': 'pair'}), isNull);
      expect(parseIosEvent('send'), isNull);
      expect(parseIosEvent(null), isNull);
    });
  });

  test('the snapshot has the shape Swift decodes', () {
    final a = iosSnapshotArgs(
      deviceName: 'Nosir iPhone',
      heicToJpeg: true,
      devices: [(id: 'a' * 64, name: 'Ofis', platform: 'windows', code: 'ABCD-EFGH', lastSeen: 7)],
    );
    expect(a.keys, unorderedEquals(['deviceName', 'heicToJpeg', 'devices']));
    final d = (a['devices']! as List).single as Map;
    expect(d.keys, unorderedEquals(['id', 'name', 'platform', 'code', 'lastSeen']));
    expect(d['lastSeen'], 7);
  });

  test('HEIC copies are named like Swift names them', () {
    expect(heicAsJpegName('IMG_0001.HEIC'), 'IMG_0001.jpg');
    expect(heicAsJpegName('a.b.heif'), 'a.b.jpg');
    expect(heicAsJpegName('.HEIC'), 'photo.jpg');
    expect(heicAsJpegName('noext'), 'noext.jpg');
  });

  test('a bookmark must be base64', () {
    expect(looksLikeBookmark(base64.encode([1, 2, 3])), isTrue);
    expect(looksLikeBookmark(''), isFalse);
    expect(looksLikeBookmark('not base64!'), isFalse);
  });

  test('settings and trust move into the App Group once, never over newer files', () async {
    final tmp = await Directory.systemTemp.createTemp('vutric-group');
    addTearDown(() => tmp.delete(recursive: true));
    final own = await Directory('${tmp.path}/own').create();
    final group = await Directory('${tmp.path}/group').create();
    await File('${own.path}/settings.json').writeAsString('old settings');
    await File('${own.path}/trusted.json').writeAsString('old trust');
    await File('${group.path}/trusted.json').writeAsString('group trust');
    await moveIntoGroup(own, group);
    expect(await File('${group.path}/settings.json').readAsString(), 'old settings');
    expect(await File('${own.path}/settings.json').exists(), isFalse);
    expect(await File('${group.path}/trusted.json').readAsString(), 'group trust');
    expect(await File('${own.path}/trusted.json').exists(), isTrue);
    await moveIntoGroup(own, group);
    expect(await File('${group.path}/settings.json').readAsString(), 'old settings');
  });

  group('String Catalogs', () {
    tearDown(() => uiLanguage = 'uz');

    for (final path in [_kit, _runner]) {
      test('$path agrees with lib/i18n.dart', () {
        final cat = _catalog(path);
        expect(cat, isNotEmpty);
        for (final e in cat.entries) {
          uiLanguage = 'uz';
          expect(tr(e.key), e.value.uz, reason: '${e.key} (uz)');
          uiLanguage = 'en';
          expect(tr(e.key), e.value.en, reason: '${e.key} (en)');
        }
      });
    }

    test('every key Swift uses is in the catalog', () {
      final src = File('ios/VutricKit/Sources/VutricKit/L10n.swift').readAsStringSync();
      final list = src.substring(src.indexOf('keys: [String] = [') + 'keys: [String] = ['.length);
      final used = RegExp(r'"([a-z_.]+)"').allMatches(list.substring(0, list.indexOf(']'))).map((m) => m[1]!);
      final cat = _catalog(_kit).keys.toSet();
      expect(used, isNotEmpty);
      for (final k in used) {
        expect(cat, contains(k), reason: k);
      }
      // Keys used directly in Swift sources of the targets must be there too.
      final swift = [
        'ios/VutricShare/ShareView.swift',
        'ios/VutricShare/ShareModel.swift',
        'ios/RustShare/RustShare.swift',
        'ios/VutricWidgets/VutricWidgets.swift',
        'ios/Runner/Intents.swift',
        'ios/VutricKit/Sources/VutricKit/TransferState.swift',
      ];
      final all = {...cat, ..._catalog(_runner).keys};
      for (final f in swift) {
        final text = File(f).readAsStringSync();
        for (final m in RegExp(
          r'(?:L10n\.text\(|LocalizedStringResource = |title: |shortTitle: |IntentDescription\()"([a-z_]+\.[a-z_.]+)"',
        ).allMatches(text)) {
          expect(all, contains(m[1]), reason: '${m[1]} in $f');
        }
      }
    });
  });

  group('Xcode project', () {
    final pbx = File('ios/Runner.xcodeproj/project.pbxproj').readAsStringSync();

    test('every target carries the App Group and its entitlements', () {
      expect('VUTRIC_APP_GROUP = group.io.wonmor.vutricSpike;'.allMatches(pbx).length, 3);
      for (final t in ['Runner', 'VutricShare', 'VutricWidgets']) {
        expect(pbx, contains('CODE_SIGN_ENTITLEMENTS = $t/$t.entitlements;'));
        final ent = File('ios/$t/$t.entitlements').readAsStringSync();
        expect(ent, contains('com.apple.security.application-groups'));
        expect(ent, contains(r'$(VUTRIC_APP_GROUP)'));
        final plist = File('ios/$t/Info.plist').readAsStringSync();
        expect(plist, contains('<key>VutricAppGroup</key>\n\t<string>\$(VUTRIC_APP_GROUP)</string>'));
      }
    });

    test('Bonjour and the local network prompt are declared where discovery runs', () {
      for (final t in ['Runner', 'VutricShare']) {
        final plist = File('ios/$t/Info.plist').readAsStringSync();
        expect(plist, contains('NSLocalNetworkUsageDescription'));
        expect(plist, contains('<string>_vutric._udp</string>'), reason: t);
      }
    });

    test('the extensions are embedded and the Share Extension links Rust without -force_load', () {
      expect(pbx, contains('VutricShare.appex in Embed Foundation Extensions'));
      expect(pbx, contains('VutricWidgets.appex in Embed Foundation Extensions'));
      expect(pbx.indexOf('/* Embed Foundation Extensions */,'), lessThan(pbx.indexOf('/* Thin Binary */,')));
      // Three Runner configs force-load the bridge for Dart's lookup; the extension must not.
      expect('"-force_load",'.allMatches(pbx).length, 3);
      expect('"\$(PROJECT_DIR)/Rust/libvutric_mobile.a",'.allMatches(pbx).length, 6);
    });

    test('the Live Activity and the URL scheme are declared', () {
      final plist = File('ios/Runner/Info.plist').readAsStringSync();
      expect(plist, contains('<key>NSSupportsLiveActivities</key>\n\t<true/>'));
      expect(plist, contains('<string>vutric</string>'));
      expect(plist, contains('<string>INSendMessageIntent</string>'));
    });
  });
}
