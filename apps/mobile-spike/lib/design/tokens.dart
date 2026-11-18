// The Vutric design system on the phone: colour, type, radius, material and motion tokens from the Claude Design
// handoff (design/claude-design/README.md). Every screen reads them through `context.v`, never hard-coded colours,
// so all 9 themes and reduced motion work everywhere at once.

import 'dart:io';

import 'package:material_ui/material_ui.dart';

/// Vutric Blue, the brand colour (logo, brand surfaces). The iOS theme uses the system blue for actions on purpose.
const vutricBlue = Color(0xFF2969FF);

enum VFont { system, geist, mono, serif }

/// How raised controls look: frosted glass, a thin outline (Paper, Terminal) or a hard black outline (Signal).
enum VMaterial { glass, outline, brutal }

@immutable
class VutricTokens extends ThemeExtension<VutricTokens> {
  const VutricTokens({
    required this.id,
    required this.name,
    required this.tag,
    required this.desc,
    required this.dark,
    required this.bg,
    required this.bgAlt,
    required this.raised,
    required this.ink,
    required this.muted,
    required this.line,
    required this.lineStrong,
    required this.action,
    required this.actionInk,
    required this.accent,
    required this.accentHi,
    required this.accent2,
    required this.accentInk,
    required this.hero,
    required this.heroInk,
    required this.heroMuted,
    required this.glass,
    required this.canvas,
    required this.ok,
    required this.err,
    required this.glassShadow,
    required this.glassBorder,
    required this.glassBorderWidth,
    required this.glassHighlight,
    required this.font,
    required this.material,
    required this.rPill,
    required this.rCard,
    required this.rPanel,
  });

  final String id, name, tag, desc;
  final bool dark;
  final Color bg, bgAlt, raised, ink, muted, line, lineStrong;

  /// Neutral strong action; [accent] is the one primary action per screen.
  final Color action, actionInk, accent, accentHi, accent2, accentInk;

  /// The large dark panel on Home ("Rasm otish").
  final Color hero, heroInk, heroMuted;
  final Color glass, canvas, ok, err;
  final List<BoxShadow> glassShadow;
  final Color glassBorder;
  final double glassBorderWidth;
  final Color? glassHighlight;
  final VFont font;
  final VMaterial material;
  final double rPill, rCard, rPanel;

  static bool get _apple => Platform.isIOS || Platform.isMacOS;

  /// Body font family. "system" means SF Pro on Apple; elsewhere the handoff asks for Geist.
  String? get family => switch (font) {
    VFont.system => _apple ? null : 'Geist',
    VFont.geist => 'Geist',
    VFont.mono => 'GeistMono',
    VFont.serif => _apple ? 'Georgia' : 'serif',
  };

  String get monoFamily => _apple ? 'Menlo' : 'GeistMono';

  /// One text style in the theme's font. Geist is a single variable file, so the weight axis is set as well.
  TextStyle text(
    double size,
    FontWeight weight, {
    Color? color,
    double tracking = 0,
    double? height,
    bool mono = false,
  }) {
    final fam = mono ? monoFamily : family;
    return TextStyle(
      fontFamily: fam,
      fontSize: size,
      fontWeight: weight,
      fontVariations: fam == 'Geist' || fam == 'GeistMono' ? [FontVariation.weight(weight.value.toDouble())] : null,
      letterSpacing: size * tracking,
      height: height,
      color: color ?? ink,
    );
  }

  // Type scale (README "Type"). Tracking is a fraction of the size, as in the CSS (−5% = -0.05).
  TextStyle get display => text(48, FontWeight.w600, tracking: -0.05, height: 1.0);
  TextStyle get largeTitle => text(34, FontWeight.w600, tracking: -0.05, height: 1.05);
  TextStyle get title => text(28, FontWeight.w600, tracking: -0.03, height: 1.1);
  TextStyle get heading => text(20, FontWeight.w600, tracking: -0.02);
  TextStyle get body => text(17, FontWeight.w400, height: 1.3);
  TextStyle get callout => text(15, FontWeight.w500);
  TextStyle get label => text(13, FontWeight.w500, color: muted);
  TextStyle get caption => text(12.5, FontWeight.w400, color: muted, height: 1.4);
  TextStyle get monoSmall => text(12, FontWeight.w500, mono: true, color: muted);

  @override
  VutricTokens copyWith() => this;

  @override
  VutricTokens lerp(VutricTokens? other, double t) => t < 0.5 ? this : (other ?? this);
}

extension VutricContext on BuildContext {
  VutricTokens get v => Theme.of(this).extension<VutricTokens>()!;

  /// The OS "reduce motion" switch: every effect becomes a 200 ms fade (README "Reduced motion").
  bool get reducedMotion => MediaQuery.of(this).disableAnimations;
}

/// Motion tokens (README "Easing" and the S3 motion notes). Interface motion stays under 250 ms; only the throw runs
/// longer.
abstract final class VMotion {
  static const out = Cubic(.2, .8, .2, 1);
  static const openBack = Cubic(.34, 1.6, .64, 1);
  static const throwOut = Cubic(.3, .7, .4, 1);
  static const returnBack = Cubic(.34, 1.56, .64, 1);
  static const fast = Duration(milliseconds: 180);
  static const open = Duration(milliseconds: 250);
  static const throwDuration = Duration(milliseconds: 280);
  static const returnDuration = Duration(milliseconds: 560);
  static const resultIn = Duration(milliseconds: 220);
  static const reduced = Duration(milliseconds: 200);

  /// The throw threshold: 150 px up, or 0.5 px/ms (= 500 px/s) upward speed.
  static const throwDistance = 150.0;
  static const throwSpeed = 500.0;
}

ThemeData buildTheme(VutricTokens v) {
  final scheme = ColorScheme.fromSeed(
    seedColor: v.accent,
    brightness: v.dark ? Brightness.dark : Brightness.light,
  ).copyWith(primary: v.accent, onPrimary: v.accentInk, surface: v.bg, onSurface: v.ink, error: v.err);
  return ThemeData(
    useMaterial3: true,
    colorScheme: scheme,
    scaffoldBackgroundColor: v.bg,
    fontFamily: v.family,
    extensions: [v],
    textTheme: TextTheme(
      bodyLarge: v.body,
      bodyMedium: v.text(15, FontWeight.w400),
      bodySmall: v.caption,
      titleLarge: v.heading,
      titleMedium: v.text(17, FontWeight.w600),
      labelLarge: v.text(17, FontWeight.w600),
    ),
    snackBarTheme: SnackBarThemeData(
      behavior: SnackBarBehavior.floating,
      backgroundColor: v.hero,
      contentTextStyle: v.text(15, FontWeight.w500, color: v.heroInk),
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(v.rCard.clamp(0, 18))),
    ),
    bottomSheetTheme: BottomSheetThemeData(
      backgroundColor: v.raised,
      showDragHandle: true,
      dragHandleColor: v.lineStrong,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.vertical(top: Radius.circular(v.rPanel))),
    ),
    dialogTheme: DialogThemeData(
      backgroundColor: v.raised,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(v.rPanel)),
    ),
    switchTheme: SwitchThemeData(
      thumbColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.selected) ? v.accentInk : null),
      trackColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.selected) ? v.accent : null),
    ),
    dividerColor: v.line,
  );
}
