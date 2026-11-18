// First start only (backlog A2): one screen that says what Vutric does, a second that names this phone and asks
// for the one permission the home screen needs (the gallery). The camera is asked for when the scanner first
// opens, by the OS, with the reason in the Info.plist / manifest text: asking for it up front would be a prompt
// with no context. Nothing here blocks: the gallery can be skipped, and the Otish tab asks again in place.

import 'dart:io';

import 'package:material_ui/material_ui.dart';
import 'package:photo_manager/photo_manager.dart';

import '../design/tokens.dart';
import '../design/widgets.dart';
import '../session.dart';

class Onboarding extends StatefulWidget {
  const Onboarding({super.key, required this.onDone});
  final VoidCallback onDone;

  @override
  State<Onboarding> createState() => _OnboardingState();
}

class _OnboardingState extends State<Onboarding> {
  final _pages = PageController();
  late final _name = TextEditingController(text: Session.instance.deviceName);
  int _page = 0;
  PermissionState? _gallery;
  bool _finishing = false;

  @override
  void initState() {
    super.initState();
    PhotoManager.getPermissionState(requestOption: const PermissionRequestOption()).then((p) {
      if (mounted) setState(() => _gallery = p);
    });
  }

  @override
  void dispose() {
    _pages.dispose();
    _name.dispose();
    super.dispose();
  }

  void _next() => _pages.animateToPage(
    1,
    duration: context.reducedMotion ? VMotion.reduced : VMotion.open,
    curve: VMotion.out,
  );

  Future<void> _askGallery() async {
    final p = await PhotoManager.requestPermissionExtend();
    if (mounted) setState(() => _gallery = p);
  }

  Future<void> _finish() async {
    if (_finishing) return;
    setState(() => _finishing = true);
    final s = Session.instance;
    final renamed = await s.setDeviceName(_name.text);
    await s.finishOnboarding();
    widget.onDone();
    if (renamed) await s.restart();
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return Scaffold(
      backgroundColor: v.bg,
      body: SafeArea(
        child: Column(
          children: [
            Expanded(
              child: PageView(
                controller: _pages,
                physics: const NeverScrollableScrollPhysics(),
                onPageChanged: (i) => setState(() => _page = i),
                children: [_welcome(v), _setup(v)],
              ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 8, 16, 16),
              child: Column(
                children: [
                  Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      for (var i = 0; i < 2; i++)
                        AnimatedContainer(
                          duration: VMotion.fast,
                          margin: const EdgeInsets.symmetric(horizontal: 3),
                          width: i == _page ? 18 : 6,
                          height: 6,
                          decoration: BoxDecoration(
                            color: i == _page ? v.accent : v.lineStrong,
                            borderRadius: BorderRadius.circular(3),
                          ),
                        ),
                    ],
                  ),
                  const SizedBox(height: 16),
                  VButton(
                    label: _page == 0 ? 'Davom etish' : 'Boshlash',
                    icon: _page == 0 ? Icons.arrow_forward_rounded : Icons.check_rounded,
                    busy: _finishing,
                    onPressed: _page == 0 ? _next : _finish,
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _welcome(VutricTokens v) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const SizedBox(height: 24),
          const VWordmark(),
          const Spacer(),
          const Center(child: _ThrowArt()),
          const Spacer(),
          Text('Rasmni oting.\nKompyuterda\npaydo boʻladi.', style: v.display),
          const SizedBox(height: 12),
          Text(
            'Telefon va kompyuter oʻrtasida rasm, video va istalgan fayl: shu Wi-Fi’da yoki internet orqali, '
            'shifrlangan. Hisob ochish shart emas.',
            style: v.text(15.5, FontWeight.w400, color: v.muted, height: 1.4),
          ),
          const SizedBox(height: 12),
        ],
      ),
    );
  }

  Widget _setup(VutricTokens v) {
    final g = _gallery;
    final granted = g?.hasAccess ?? false;
    final blocked = g == PermissionState.denied || g == PermissionState.restricted;
    return ListView(
      padding: const EdgeInsets.fromLTRB(24, 24, 24, 8),
      children: [
        Text('Bu telefon', style: v.largeTitle),
        const SizedBox(height: 6),
        Text('Boshqa qurilmalarda shu nom chiqadi.', style: v.text(15, FontWeight.w400, color: v.muted)),
        const SizedBox(height: 14),
        TextField(
          controller: _name,
          maxLength: 32,
          textCapitalization: TextCapitalization.sentences,
          style: v.text(17, FontWeight.w600),
          decoration: InputDecoration(
            hintText: Platform.isIOS ? 'iPhone' : 'Android',
            prefixIcon: Icon(Icons.smartphone_rounded, color: v.muted),
          ),
        ),
        const SizedBox(height: 18),
        Text('RUXSATLAR', style: v.text(12, FontWeight.w600, color: v.muted, tracking: 0.06)),
        const SizedBox(height: 8),
        VRowCard(
          icon: granted ? Icons.check_rounded : Icons.photo_library_outlined,
          title: 'Galereya',
          subtitle: switch (g) {
            PermissionState.authorized => 'Berildi',
            PermissionState.limited => 'Faqat tanlangan rasmlar',
            PermissionState.denied || PermissionState.restricted => 'Berilmadi: sozlamalardan yoqing',
            _ => 'Rasm va videolarni shu yerda koʻrib, yuborish uchun',
          },
          onTap: granted ? null : (blocked ? PhotoManager.openSetting : _askGallery),
          trailing: granted ? null : Text('Ruxsat berish', style: v.text(14, FontWeight.w600, color: v.accent)),
        ),
        const SizedBox(height: 8),
        const VRowCard(
          icon: Icons.qr_code_scanner_rounded,
          title: 'Kamera',
          subtitle: 'Faqat QR kodni skanerlaganda soʻraladi',
        ),
        if (Platform.isIOS) ...[
          const SizedBox(height: 8),
          const VRowCard(
            icon: Icons.wifi_rounded,
            title: 'Mahalliy tarmoq',
            subtitle: 'Birinchi ulanishda soʻraladi: shu Wi-Fi’dagi kompyuterni topish uchun',
          ),
        ],
        const SizedBox(height: 14),
        Text(
          'Fayllar faqat siz tanlagan qurilmaga boradi. Vutric serverida hech narsa saqlanmaydi.',
          style: v.caption,
        ),
      ],
    );
  }
}

/// The welcome picture: a photo leaving the phone through its top edge toward the accent light.
class _ThrowArt extends StatelessWidget {
  const _ThrowArt();

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return SizedBox(
      width: 260,
      height: 220,
      child: Stack(
        alignment: Alignment.center,
        clipBehavior: Clip.none,
        children: [
          Positioned(
            top: -30,
            child: Container(
              width: 260,
              height: 180,
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                gradient: RadialGradient(colors: [v.accent.withValues(alpha: 0.35), v.accent.withValues(alpha: 0)]),
              ),
            ),
          ),
          Positioned(
            bottom: 0,
            child: Container(
              width: 120,
              height: 150,
              decoration: BoxDecoration(
                color: v.raised,
                borderRadius: BorderRadius.circular(24),
                border: Border.all(color: v.lineStrong, width: 1.5),
              ),
            ),
          ),
          Positioned(
            top: 6,
            child: Transform.rotate(
              angle: 0.14,
              child: Container(
                width: 92,
                height: 120,
                decoration: BoxDecoration(
                  borderRadius: BorderRadius.circular(14),
                  border: Border.all(color: Colors.white, width: 4),
                  boxShadow: const [BoxShadow(color: Color(0x40000000), blurRadius: 24, offset: Offset(0, 12))],
                  gradient: const LinearGradient(
                    begin: Alignment.topCenter,
                    end: Alignment.bottomCenter,
                    stops: [0, 0.58, 0.59, 1],
                    colors: [Color(0xFF2B3A78), Color(0xFFC46D5A), Color(0xFF23284F), Color(0xFF121838)],
                  ),
                ),
              ),
            ),
          ),
          Positioned(top: -10, right: 50, child: Icon(Icons.north_east_rounded, color: v.accent, size: 28)),
        ],
      ),
    );
  }
}
