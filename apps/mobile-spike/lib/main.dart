// Vutric on Android and iOS. Three tabs: Otish (the phone's gallery and files, plus the photo throw), Qabul and
// Tarix; the UI follows the Claude Design handoff (design/claude-design, ADR-0011) in 9 themes. Same network first,
// the internet (direct or encrypted relay) when the devices are apart (ADR-0006).

import 'dart:io';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart' show ExternalLibrary;
import 'package:material_ui/material_ui.dart';
import 'package:path_provider/path_provider.dart';

import 'design/themes.g.dart';
import 'design/tokens.dart';
import 'ios.dart';
import 'screens/home_shell.dart';
import 'screens/onboarding.dart';
import 'session.dart';
import 'src/rust/api.dart' as vutric;
import 'src/rust/frb_generated.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  try {
    // iOS: the Rust library is linked into the app itself (-force_load in the Xcode project), not a framework.
    await RustLib.init(externalLibrary: Platform.isIOS ? ExternalLibrary.process(iKnowHowToUseIt: true) : null);
    await vutric.setSettingsDir(dir: await settingsDir());
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
  await Session.instance.loadLocal();
  await Session.instance.loadTransferSettings();
  runApp(const VutricApp());
  await Session.instance.restart();
}

/// Rust's settings folder. iOS: inside the App Group, so the Share Extension and App Intents see the same paired
/// devices (ADR-0015); files an older build left in the app's own folder move there once.
Future<String> settingsDir() async {
  final own = await getApplicationSupportDirectory();
  final group = await IosBridge.instance.groupDir();
  if (group == null) return own.path;
  await moveIntoGroup(own, Directory(group));
  return group;
}

/// The theme for [id]; `auto` is the iOS Native pair following the phone's light/dark setting.
VutricTokens themeFor(String id, Brightness platform) {
  final want = id == autoTheme ? (platform == Brightness.dark ? 'ios-dark' : 'ios') : id;
  return vutricThemes.firstWhere((t) => t.id == want, orElse: () => vutricThemes.first);
}

class VutricApp extends StatefulWidget {
  const VutricApp({super.key});

  @override
  State<VutricApp> createState() => _VutricAppState();
}

class _VutricAppState extends State<VutricApp> with WidgetsBindingObserver {
  final _s = Session.instance;
  String _theme = Session.instance.themeId;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _s.addListener(_changed);
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _s.removeListener(_changed);
    super.dispose();
  }

  void _changed() {
    if (_s.themeId != _theme) setState(() => _theme = _s.themeId);
  }

  @override
  void didChangePlatformBrightness() {
    if (_theme == autoTheme) setState(() {});
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) _s.foreground(true);
    if (state == AppLifecycleState.paused) _s.foreground(false);
  }

  @override
  Widget build(BuildContext context) {
    final platform = WidgetsBinding.instance.platformDispatcher.platformBrightness;
    return MaterialApp(
      title: 'Vutric',
      debugShowCheckedModeBanner: false,
      theme: buildTheme(themeFor(_theme, platform)),
      themeAnimationDuration: const Duration(milliseconds: 260),
      home: _s.onboarded ? const HomeShell() : Onboarding(onDone: () => setState(() {})),
    );
  }
}
