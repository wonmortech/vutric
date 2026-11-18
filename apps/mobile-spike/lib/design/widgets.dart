// The handoff's building blocks as Flutter widgets: the V mark, raised surfaces in the theme's material (glass,
// outline or brutal), pill buttons, the connected-device pill, result and error notices, the glass tab bar.
// Screens compose these; they never draw their own buttons or cards.

import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'tokens.dart';

// ── Brand ────────────────────────────────────────────────────────────────────────────────────────────────────────

/// The Vutric "V": a separated top bar over a funnel pointing down (logo-mark.svg, viewBox 2.75 1 26.5 30).
class VLogoMark extends StatelessWidget {
  const VLogoMark({super.key, this.size = 24, this.color = vutricBlue});
  final double size;
  final Color color;

  @override
  Widget build(BuildContext context) => CustomPaint(size: Size(size * 26.5 / 30, size), painter: _MarkPainter(color));
}

class _MarkPainter extends CustomPainter {
  _MarkPainter(this.color);
  final Color color;

  static final Path _mark = () {
    final bar = Path()
      ..moveTo(9.3704, 3)
      ..lineTo(22.6265, 3)
      ..cubicTo(23.8941, 3, 25.0353, 3.7685, 25.5119, 4.94317)
      ..lineTo(27.25, 9.22751)
      ..lineTo(4.75, 9.22751)
      ..lineTo(6.48432, 4.94496)
      ..cubicTo(6.93972, 3.82046, 8.00401, 3.06766, 9.20572, 3.00434)
      ..close();
    final funnel = Path()
      ..moveTo(4.75, 12.3411)
      ..lineTo(10.6056, 12.3411)
      ..cubicTo(11.6407, 12.3411, 12.6081, 12.8555, 13.187, 13.7136)
      ..lineTo(16, 17.8836)
      ..lineTo(18.8115, 13.7141)
      ..cubicTo(19.3904, 12.8557, 20.3579, 12.3411, 21.3932, 12.3411)
      ..lineTo(27.25, 12.3411)
      ..lineTo(16, 29.0206)
      ..close();
    return Path()
      ..addPath(bar, Offset.zero)
      ..addPath(funnel, Offset.zero);
  }();

  @override
  void paint(Canvas canvas, Size size) {
    canvas
      ..scale(size.width / 26.5, size.height / 30)
      ..translate(-2.75, -1)
      ..drawPath(_mark, Paint()..color = color);
  }

  @override
  bool shouldRepaint(_MarkPainter old) => old.color != color;
}

/// The app icon: Vutric Blue rounded square (radius 22.4%) with the white V.
class VAppIcon extends StatelessWidget {
  const VAppIcon({super.key, this.size = 28});
  final double size;

  @override
  Widget build(BuildContext context) => Container(
    width: size,
    height: size,
    alignment: Alignment.center,
    decoration: BoxDecoration(color: vutricBlue, borderRadius: BorderRadius.circular(size * 0.224)),
    child: VLogoMark(size: size * 0.62, color: Colors.white),
  );
}

/// Logo plus the lowercase wordmark.
class VWordmark extends StatelessWidget {
  const VWordmark({super.key, this.dim = false});
  final bool dim;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return Opacity(
      opacity: dim ? 0.4 : 1,
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          const VAppIcon(size: 28),
          const SizedBox(width: 8),
          Text('vutric', style: v.text(21, FontWeight.w600, tracking: -0.04)),
        ],
      ),
    );
  }
}

// ── Surfaces ─────────────────────────────────────────────────────────────────────────────────────────────────────

/// A raised surface in the theme's material: frosted glass, a hairline outline, or Signal's black outline with a
/// hard offset shadow.
class VSurface extends StatelessWidget {
  const VSurface({
    super.key,
    required this.child,
    this.radius,
    this.padding,
    this.color,
    this.blur = true,
    this.shadow = true,
  });

  final Widget child;
  final double? radius;
  final EdgeInsetsGeometry? padding;

  /// Overrides the fill (e.g. `raised` for solid cards); glass fill otherwise.
  final Color? color;
  final bool blur;
  final bool shadow;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final r = BorderRadius.circular(radius ?? v.rCard);
    final fill = color ?? (v.material == VMaterial.glass ? v.glass : v.raised);
    final borderW = v.material == VMaterial.brutal ? 2.0 : (v.glassBorderWidth == 0 ? 0.5 : v.glassBorderWidth);
    final border = v.material == VMaterial.brutal ? v.ink : (v.glassBorderWidth == 0 ? v.line : v.glassBorder);
    Widget box = DecoratedBox(
      decoration: BoxDecoration(
        color: fill,
        borderRadius: r,
        border: Border.all(color: border, width: borderW),
        gradient: v.glassHighlight != null && v.material == VMaterial.glass && color == null
            ? LinearGradient(
                begin: Alignment.topCenter,
                end: const Alignment(0, -0.6),
                colors: [Color.alphaBlend(v.glassHighlight!.withValues(alpha: 0.18), fill), fill],
              )
            : null,
      ),
      child: Padding(padding: padding ?? EdgeInsets.zero, child: child),
    );
    if (blur && v.material == VMaterial.glass && color == null) {
      box = ClipRRect(
        borderRadius: r,
        child: BackdropFilter(filter: ui.ImageFilter.blur(sigmaX: 24, sigmaY: 24), child: box),
      );
    }
    if (!shadow || v.glassShadow.isEmpty) return box;
    return DecoratedBox(
      decoration: BoxDecoration(borderRadius: r, boxShadow: v.glassShadow),
      child: box,
    );
  }
}

/// Shrinks a little while pressed and gives a light haptic: the shared press feel for every tappable surface.
class VPressable extends StatefulWidget {
  const VPressable({super.key, required this.child, this.onTap, this.haptic = true, this.semanticLabel});
  final Widget child;
  final VoidCallback? onTap;
  final bool haptic;
  final String? semanticLabel;

  @override
  State<VPressable> createState() => _VPressableState();
}

class _VPressableState extends State<VPressable> {
  bool _down = false;

  @override
  Widget build(BuildContext context) {
    final on = widget.onTap != null;
    return Semantics(
      button: true,
      enabled: on,
      label: widget.semanticLabel,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTapDown: on ? (_) => setState(() => _down = true) : null,
        onTapCancel: on ? () => setState(() => _down = false) : null,
        onTapUp: on ? (_) => setState(() => _down = false) : null,
        onTap: on
            ? () {
                if (widget.haptic) HapticFeedback.lightImpact();
                widget.onTap!();
              }
            : null,
        // Every tappable surface is at least 48 x 48 to the finger (Android) and screen reader, whatever it looks
        // like: a 36 px circle keeps its look, with the touch area centred around it.
        child: ConstrainedBox(
          constraints: const BoxConstraints(minWidth: 48, minHeight: 48),
          child: Center(
            widthFactor: 1,
            heightFactor: 1,
            child: AnimatedScale(
              scale: _down && !context.reducedMotion ? 0.97 : 1,
              duration: VMotion.fast,
              curve: VMotion.out,
              child: widget.child,
            ),
          ),
        ),
      ),
    );
  }
}

// ── Buttons ──────────────────────────────────────────────────────────────────────────────────────────────────────

enum VButtonKind { primary, secondary, ghost }

/// The pill button: 56 tall on the phone. One primary per screen.
class VButton extends StatelessWidget {
  const VButton({
    super.key,
    required this.label,
    this.onPressed,
    this.icon,
    this.kind = VButtonKind.primary,
    this.busy = false,
    this.height = 56,
    this.tag,
  });

  final String label;
  final VoidCallback? onPressed;
  final IconData? icon;
  final VButtonKind kind;
  final bool busy;
  final double height;

  /// A small mono chip after the label (e.g. "Android").
  final String? tag;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final enabled = onPressed != null && !busy;
    final primary = kind == VButtonKind.primary;
    final fg = primary ? v.accentInk : v.ink;
    final content = Row(
      mainAxisAlignment: MainAxisAlignment.center,
      mainAxisSize: MainAxisSize.min,
      children: [
        if (busy)
          SizedBox.square(dimension: 18, child: CircularProgressIndicator(strokeWidth: 2, color: fg))
        else if (icon != null)
          Icon(icon, size: 19, color: fg),
        if (busy || icon != null) const SizedBox(width: 8),
        Flexible(
          child: Text(
            label,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: v.text(17, FontWeight.w600, color: fg, tracking: -0.01),
          ),
        ),
        if (tag != null) ...[
          const SizedBox(width: 8),
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
            decoration: BoxDecoration(color: v.bgAlt, borderRadius: BorderRadius.circular(6)),
            child: Text(tag!, style: v.text(11, FontWeight.w500, mono: true, color: v.muted)),
          ),
        ],
      ],
    );
    final r = v.rPill.clamp(0.0, height / 2);
    final Widget body = switch (kind) {
      VButtonKind.primary => DecoratedBox(
        decoration: BoxDecoration(
          color: v.accent,
          borderRadius: BorderRadius.circular(r),
          border: v.material == VMaterial.brutal ? Border.all(color: v.ink, width: 2) : null,
          boxShadow: v.material == VMaterial.brutal
              ? [BoxShadow(color: v.ink, offset: const Offset(4, 4))]
              : [
                  BoxShadow(
                    color: v.accent.withValues(alpha: 0.35),
                    blurRadius: 18,
                    offset: const Offset(0, 8),
                    spreadRadius: -8,
                  ),
                ],
        ),
        child: Center(child: content),
      ),
      VButtonKind.secondary => VSurface(
        radius: r,
        color: v.raised,
        child: Center(child: content),
      ),
      VButtonKind.ghost => Center(child: content),
    };
    return AnimatedOpacity(
      opacity: enabled || busy ? 1 : 0.45,
      duration: VMotion.fast,
      child: VPressable(
        onTap: enabled ? onPressed : null,
        semanticLabel: label,
        child: SizedBox(height: height, child: body),
      ),
    );
  }
}

/// A round 36 px glass button (back, close).
class VCircleButton extends StatelessWidget {
  const VCircleButton({
    super.key,
    required this.icon,
    required this.onTap,
    this.label,
    this.size = 36,
    this.dark = false,
  });
  final IconData icon;
  final VoidCallback onTap;
  final String? label;
  final double size;
  final bool dark;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return VPressable(
      onTap: onTap,
      semanticLabel: label,
      child: SizedBox.square(
        dimension: size,
        child: dark
            ? DecoratedBox(
                decoration: const BoxDecoration(color: Color(0x33FFFFFF), shape: BoxShape.circle),
                child: Icon(icon, size: 18, color: Colors.white),
              )
            : VSurface(
                radius: size / 2,
                color: v.raised,
                child: Icon(icon, size: 18, color: v.ink),
              ),
      ),
    );
  }
}

// ── Header ───────────────────────────────────────────────────────────────────────────────────────────────────────

/// Back button left, title centred (S3-1, S4-1).
class VNavHeader extends StatelessWidget {
  const VNavHeader({super.key, required this.title, this.onBack, this.trailing});
  final String title;
  final VoidCallback? onBack;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final back = onBack ?? (Navigator.of(context).canPop() ? () => Navigator.of(context).maybePop() : null);
    return SizedBox(
      height: 52,
      child: Stack(
        alignment: Alignment.center,
        children: [
          Text(title, style: v.text(17, FontWeight.w600)),
          if (back != null)
            Positioned(
              left: 16,
              child: VCircleButton(icon: Icons.chevron_left_rounded, onTap: back, label: 'Orqaga'),
            ),
          if (trailing != null) Positioned(right: 16, child: trailing!),
        ],
      ),
    );
  }
}

// ── Device pill ──────────────────────────────────────────────────────────────────────────────────────────────────

/// "● XSTUDIO · Windows  000092": the connected device, its platform and the check code both screens show.
class VDevicePill extends StatelessWidget {
  const VDevicePill({super.key, required this.name, this.platform, this.code, this.lost = false, this.onTap});
  final String name;
  final String? platform;
  final String? code;
  final bool lost;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final pill = VSurface(
      radius: v.rPill.clamp(0, 18),
      padding: const EdgeInsets.fromLTRB(12, 6, 6, 6),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          _Dot(color: lost ? v.muted : v.ok, halo: !lost),
          const SizedBox(width: 8),
          // Name and platform share one line that shortens as a whole: with large text the code still fits.
          Flexible(
            child: Text.rich(
              TextSpan(
                text: name,
                style: v.text(13.5, FontWeight.w500, color: lost ? v.muted : v.ink),
                children: [
                  TextSpan(
                    text: lost
                        ? ' · uzildi'
                        : (platform == null || platform!.isEmpty ? '' : ' · ${platformName(platform!)}'),
                    style: v.text(13.5, FontWeight.w400, color: v.muted),
                  ),
                ],
              ),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
          ),
          if (code != null && !lost) ...[
            const SizedBox(width: 8),
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
              decoration: BoxDecoration(color: v.bgAlt, borderRadius: BorderRadius.circular(v.rPill.clamp(0, 10))),
              child: Text(code!, style: v.text(12, FontWeight.w600, mono: true, tracking: 0.04)),
            ),
          ] else
            const SizedBox(width: 6),
        ],
      ),
    );
    // The label says it all; the visible texts are excluded so they are not read a second time. The tap action
    // stays on VPressable's own node, so a screen reader can still open the session sheet.
    final label = lost ? '$name bilan aloqa uzildi' : '$name ga ulangan, tekshiruv kodi $code';
    return onTap == null
        ? Semantics(label: label, excludeSemantics: true, child: pill)
        : VPressable(
            onTap: onTap,
            semanticLabel: label,
            child: ExcludeSemantics(child: pill),
          );
  }
}

/// Rust reports "windows", "android", "ios", "macos", "linux"; the pill shows the product name.
String platformName(String p) => switch (p.toLowerCase()) {
  'windows' => 'Windows',
  'android' => 'Android',
  'ios' => 'iPhone',
  'macos' => 'Mac',
  'linux' => 'Linux',
  _ => p,
};

class _Dot extends StatelessWidget {
  const _Dot({required this.color, required this.halo});
  final Color color;
  final bool halo;

  @override
  Widget build(BuildContext context) => Container(
    width: 8,
    height: 8,
    decoration: BoxDecoration(
      color: color,
      shape: BoxShape.circle,
      boxShadow: halo ? [BoxShadow(color: color.withValues(alpha: 0.35), spreadRadius: 3)] : null,
    ),
  );
}

// ── Notices ──────────────────────────────────────────────────────────────────────────────────────────────────────

/// Green result pill near the thumb: "✓ Yetib bordi · 38 ms". Fades in with an 8 px rise (220 ms).
class VResultPill extends StatelessWidget {
  const VResultPill({super.key, required this.text, this.mono, this.icon = Icons.check_rounded, this.busy = false});
  final String text;
  final String? mono;
  final IconData icon;
  final bool busy;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return VRise(
      child: VSurface(
        radius: v.rPill.clamp(0, 20),
        color: busy ? v.raised : Color.alphaBlend(v.ok.withValues(alpha: 0.14), v.raised),
        padding: const EdgeInsets.fromLTRB(8, 6, 14, 6),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (busy)
              SizedBox.square(dimension: 18, child: CircularProgressIndicator(strokeWidth: 2, color: v.accent))
            else
              Container(
                width: 20,
                height: 20,
                decoration: BoxDecoration(color: v.ok, shape: BoxShape.circle),
                child: Icon(icon, size: 14, color: Colors.white),
              ),
            const SizedBox(width: 8),
            Flexible(
              child: Text(text, maxLines: 2, overflow: TextOverflow.ellipsis, style: v.text(14, FontWeight.w500)),
            ),
            if (mono != null) ...[
              Text(' · ', style: v.text(14, FontWeight.w400, color: v.muted)),
              Text(mono!, style: v.text(13, FontWeight.w600, mono: true)),
            ],
          ],
        ),
      ),
    );
  }
}

/// Error card near the thumb: red "!" badge, a title and one or two lines on what happened and what to do.
class VNotice extends StatelessWidget {
  const VNotice({super.key, required this.title, this.body, this.error = true, this.onClose});
  final String title;
  final String? body;
  final bool error;
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final c = error ? v.err : v.accent;
    return VRise(
      child: Semantics(
        liveRegion: true,
        child: VSurface(
          color: v.raised,
          padding: const EdgeInsets.fromLTRB(14, 14, 10, 14),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Container(
                width: 22,
                height: 22,
                alignment: Alignment.center,
                decoration: BoxDecoration(color: c, shape: BoxShape.circle),
                child: Icon(
                  error ? Icons.priority_high_rounded : Icons.info_outline_rounded,
                  size: 15,
                  color: Colors.white,
                ),
              ),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(title, style: v.text(15, FontWeight.w600)),
                    if (body != null) ...[const SizedBox(height: 3), Text(body!, style: v.caption)],
                  ],
                ),
              ),
              if (onClose != null)
                GestureDetector(
                  onTap: onClose,
                  child: Padding(
                    padding: const EdgeInsets.all(4),
                    child: Icon(Icons.close_rounded, size: 18, color: v.muted),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Fade in with an 8 px rise (220 ms); a plain 200 ms fade under reduced motion.
class VRise extends StatelessWidget {
  const VRise({super.key, required this.child});
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final reduced = context.reducedMotion;
    return TweenAnimationBuilder<double>(
      tween: Tween(begin: 0, end: 1),
      duration: reduced ? VMotion.reduced : VMotion.resultIn,
      curve: VMotion.out,
      builder: (context, t, child) => Opacity(
        opacity: t,
        child: Transform.translate(offset: Offset(0, reduced ? 0 : 8 * (1 - t)), child: child),
      ),
      child: child,
    );
  }
}

// ── Empty and icon tiles ─────────────────────────────────────────────────────────────────────────────────────────

/// A 52 px rounded square holding one icon (empty states, list rows).
class VIconTile extends StatelessWidget {
  const VIconTile({super.key, required this.icon, this.size = 52, this.tint, this.background});
  final IconData icon;
  final double size;
  final Color? tint;
  final Color? background;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return Container(
      width: size,
      height: size,
      decoration: BoxDecoration(
        color: background ?? v.raised,
        borderRadius: BorderRadius.circular(v.rCard.clamp(0, size * 0.32)),
        border: v.material == VMaterial.brutal ? Border.all(color: v.ink, width: 2) : null,
        boxShadow: v.material == VMaterial.glass ? v.glassShadow : null,
      ),
      child: Icon(icon, size: size * 0.42, color: tint ?? v.ink),
    );
  }
}

/// Centered icon tile, title and a muted line: every empty or blocked state.
class VEmpty extends StatelessWidget {
  const VEmpty({super.key, required this.icon, required this.title, this.body, this.tint, this.tileColor, this.extra});
  final IconData icon;
  final String title;
  final String? body;
  final Color? tint;
  final Color? tileColor;
  final Widget? extra;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return Center(
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            VIconTile(icon: icon, size: 64, tint: tint, background: tileColor),
            const SizedBox(height: 18),
            Text(title, textAlign: TextAlign.center, style: v.title),
            if (body != null) ...[
              const SizedBox(height: 8),
              Text(
                body!,
                textAlign: TextAlign.center,
                style: v.text(14, FontWeight.w400, color: v.muted, height: 1.4),
              ),
            ],
            if (extra != null) ...[const SizedBox(height: 12), extra!],
          ],
        ),
      ),
    );
  }
}

/// A list row card: icon tile, title, muted subtitle, chevron (Home "Rasm qabul qilish", S4-1 choices).
class VRowCard extends StatelessWidget {
  const VRowCard({super.key, required this.icon, required this.title, this.subtitle, this.onTap, this.trailing});
  final IconData icon;
  final String title;
  final String? subtitle;
  final VoidCallback? onTap;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return VPressable(
      onTap: onTap,
      semanticLabel: title,
      child: VSurface(
        color: v.raised,
        padding: const EdgeInsets.all(14),
        child: Row(
          children: [
            VIconTile(icon: icon, size: 40, background: v.bgAlt),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(title, style: v.text(16, FontWeight.w600, tracking: -0.01)),
                  if (subtitle != null) ...[const SizedBox(height: 2), Text(subtitle!, style: v.caption)],
                ],
              ),
            ),
            trailing ?? Icon(Icons.chevron_right_rounded, color: v.muted),
          ],
        ),
      ),
    );
  }
}

// ── Tab bar ──────────────────────────────────────────────────────────────────────────────────────────────────────

class VTab {
  const VTab(this.icon, this.label);
  final IconData icon;
  final String label;
}

/// The floating Liquid-Glass tab bar with a separate round button on the right (QR).
class VGlassTabBar extends StatelessWidget {
  const VGlassTabBar({
    super.key,
    required this.tabs,
    required this.index,
    required this.onTap,
    required this.sideIcon,
    required this.onSide,
    this.sideLabel,
  });

  final List<VTab> tabs;
  final int index;
  final ValueChanged<int> onTap;
  final IconData sideIcon;
  final VoidCallback onSide;
  final String? sideLabel;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return Row(
      children: [
        Expanded(
          child: VSurface(
            radius: v.rPill.clamp(0, 32),
            padding: const EdgeInsets.all(4),
            child: Row(
              children: [
                for (final (i, t) in tabs.indexed)
                  Expanded(
                    child: VPressable(
                      onTap: () => onTap(i),
                      semanticLabel: t.label,
                      haptic: i != index,
                      child: AnimatedContainer(
                        duration: VMotion.fast,
                        curve: VMotion.out,
                        constraints: const BoxConstraints(minHeight: 52),
                        padding: const EdgeInsets.symmetric(vertical: 4),
                        decoration: BoxDecoration(
                          color: i == index ? v.bgAlt : Colors.transparent,
                          borderRadius: BorderRadius.circular(v.rPill.clamp(0, 28)),
                        ),
                        // Tab labels grow only to 1.3x, as on iOS: the bar stays one row; the label is still
                        // read in full by the screen reader (semanticLabel).
                        child: MediaQuery.withClampedTextScaling(
                          maxScaleFactor: 1.3,
                          child: Column(
                            mainAxisSize: MainAxisSize.min,
                            mainAxisAlignment: MainAxisAlignment.center,
                            children: [
                              Icon(t.icon, size: 22, color: i == index ? v.accent : v.ink),
                              const SizedBox(height: 2),
                              Text(
                                t.label,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: v.text(10.5, FontWeight.w600, color: i == index ? v.accent : v.ink),
                              ),
                            ],
                          ),
                        ),
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ),
        const SizedBox(width: 10),
        VPressable(
          onTap: onSide,
          semanticLabel: sideLabel,
          child: SizedBox.square(
            dimension: 60,
            child: VSurface(
              radius: v.rPill.clamp(0, 30),
              child: Icon(sideIcon, size: 24, color: v.ink),
            ),
          ),
        ),
      ],
    );
  }
}

/// Segmented control in the theme's material (Rasmlar / Fayllar).
class VSegmented extends StatelessWidget {
  const VSegmented({super.key, required this.labels, required this.index, required this.onChanged});
  final List<String> labels;
  final int index;
  final ValueChanged<int> onChanged;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    // The track looks 36 tall (the handoff) and grows with the text; the touch area is never under 48.
    final track = math.max(36.0, MediaQuery.textScalerOf(context).scale(13.5) * 1.35 + 12);
    final touch = math.max(48.0, track);
    return SizedBox(
      height: touch,
      child: Stack(
        alignment: Alignment.center,
        children: [
          Container(
            height: track,
            decoration: BoxDecoration(color: v.bgAlt, borderRadius: BorderRadius.circular(v.rPill.clamp(0, 18))),
          ),
          Row(
            children: [
              for (final (i, l) in labels.indexed)
                Expanded(
                  // The semantics node covers the whole 48-tall touch area, not just the visible segment.
                  child: Semantics(
                    selected: i == index,
                    button: true,
                    label: l,
                    excludeSemantics: true,
                    child: GestureDetector(
                      behavior: HitTestBehavior.opaque,
                      onTap: () {
                        if (i != index) HapticFeedback.selectionClick();
                        onChanged(i);
                      },
                      child: Center(
                        child: Padding(
                          padding: EdgeInsets.fromLTRB(i == 0 ? 2 : 1, 0, i == labels.length - 1 ? 2 : 1, 0),
                          child: SizedBox(height: track - 4, child: _segment(v, i, l)),
                        ),
                      ),
                    ),
                  ),
                ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _segment(VutricTokens v, int i, String l) => AnimatedContainer(
    duration: VMotion.fast,
    curve: VMotion.out,
    alignment: Alignment.center,
    padding: const EdgeInsets.symmetric(horizontal: 6),
    decoration: BoxDecoration(
      color: i == index ? v.raised : Colors.transparent,
      borderRadius: BorderRadius.circular(v.rPill.clamp(0, 16)),
      boxShadow: i == index && v.material == VMaterial.glass
          ? const [BoxShadow(color: Color(0x1A000000), blurRadius: 6, offset: Offset(0, 2))]
          : null,
      border: i == index && v.material == VMaterial.brutal ? Border.all(color: v.ink, width: 2) : null,
    ),
    child: Text(
      l,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: v.text(13.5, i == index ? FontWeight.w600 : FontWeight.w500),
    ),
  );
}

/// The photo card of S3: rounded image, a 5 px raised ring and the deep drop shadow.
class VPhotoCard extends StatelessWidget {
  const VPhotoCard({super.key, required this.child, this.radius = 22, this.lifted = 0});
  final Widget child;
  final double radius;

  /// 0..1 while dragging: the shadow grows a little.
  final double lifted;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final ring = v.material == VMaterial.brutal ? v.ink : v.raised;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: ring,
        borderRadius: BorderRadius.circular(radius + 5),
        boxShadow: [
          BoxShadow(
            color: Colors.black.withValues(alpha: 0.5),
            offset: Offset(0, 36 + 12 * lifted),
            blurRadius: 60 + 20 * lifted,
            spreadRadius: -24,
          ),
        ],
      ),
      child: Padding(
        padding: const EdgeInsets.all(5),
        child: ClipRRect(borderRadius: BorderRadius.circular(radius), child: child),
      ),
    );
  }
}
