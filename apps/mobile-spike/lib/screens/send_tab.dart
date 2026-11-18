// The Otish tab: S1 Home ("Nimani yuboramiz?" and the dark "Rasm otish" panel) on top of the phone's own gallery
// and files (ADR-0008: gallery first, many at once, one send button). Tap to select in order, then "Yuborish";
// with one photo selected it can also be thrown.
//
// Light by design: thumbnails come from the OS thumbnail cache at 200 px, the grid builds only the visible cells,
// and the library is read 120 items at a time as you scroll.

import 'dart:async';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';
import 'package:photo_manager/photo_manager.dart';
import 'package:photo_manager_image_provider/photo_manager_image_provider.dart';

import '../design/tokens.dart';
import '../design/widgets.dart';
import '../session.dart';
import '../transfer_ui.dart';
import '../ui.dart';
import 'connect.dart';
import 'throw_page.dart';

const _page = 120;

class SendTab extends StatefulWidget {
  const SendTab({super.key, required this.header});
  final Widget header;

  @override
  State<SendTab> createState() => SendTabState();
}

class SendTabState extends State<SendTab> {
  final _s = Session.instance;
  int _tab = 0;
  PermissionState? _perm;
  List<AssetPathEntity> _albums = const [];
  AssetPathEntity? _album;
  final _assets = <AssetEntity>[];
  int _total = 0;
  bool _loading = false;

  /// Selected gallery items in the order they were tapped (the order they are sent).
  final _picked = <String, AssetEntity>{};

  /// Selected files from the Fayllar tab: path → name.
  final _files = <String, String>{};
  List<FileSystemEntity> _received = const [];
  bool _sending = false;

  @override
  void initState() {
    super.initState();
    _s.addListener(_changed);
    _open();
  }

  @override
  void dispose() {
    _s.removeListener(_changed);
    super.dispose();
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  Future<void> _open() async {
    final p = await PhotoManager.requestPermissionExtend();
    if (!mounted) return;
    setState(() => _perm = p);
    if (!p.hasAccess) return;
    final albums = await PhotoManager.getAssetPathList(type: RequestType.common, hasAll: true);
    if (!mounted) return;
    setState(() => _albums = albums);
    if (albums.isNotEmpty) await _show(albums.first);
  }

  Future<void> _show(AssetPathEntity album) async {
    final total = await album.assetCountAsync;
    if (!mounted) return;
    setState(() {
      _album = album;
      _assets.clear();
      _total = total;
    });
    await _more();
  }

  Future<void> _more() async {
    final album = _album;
    if (album == null || _loading || _assets.length >= _total) return;
    _loading = true;
    try {
      final next = await album.getAssetListPaged(page: _assets.length ~/ _page, size: _page);
      if (!mounted || album != _album) return;
      setState(() => _assets.addAll(next));
    } finally {
      _loading = false;
    }
  }

  Future<void> _loadReceived() async {
    final dir = Directory(await receiveDir());
    final items = await dir.exists() ? await dir.list().toList() : <FileSystemEntity>[];
    final when = {for (final f in items) f.path: (await f.stat()).modified};
    items.sort((a, b) => when[b.path]!.compareTo(when[a.path]!));
    if (mounted) setState(() => _received = items);
  }

  void _toggle(AssetEntity a) {
    HapticFeedback.selectionClick();
    setState(() {
      if (_picked.remove(a.id) == null) _picked[a.id] = a;
    });
  }

  void _toggleFile(String path, String name) {
    HapticFeedback.selectionClick();
    setState(() {
      if (_files.remove(path) == null) _files[path] = name;
    });
  }

  int get _count => _picked.length + _files.length;

  void _clear() => setState(() {
    _picked.clear();
    _files.clear();
  });

  Future<void> _pickFromFiles() async {
    try {
      final p = await pickAnyFiles();
      if (!mounted) return;
      setState(() {
        for (var i = 0; i < p.paths.length; i++) {
          _files[p.paths[i]] = p.names[i];
        }
      });
    } catch (e) {
      _toast('Tanlab boʻlmadi: ${errorText(e)}');
    }
  }

  Future<void> _pickFolder() async {
    try {
      // The Rust sender reads real paths; pickDirectory()'s content:// URI it cannot open.
      // ignore: deprecated_member_use
      final dir = await FilePicker.getDirectoryPath();
      if (dir == null || !mounted) return;
      setState(() => _files[dir] = baseName(dir));
    } catch (e) {
      _toast('Papkani ochib boʻlmadi: ${errorText(e)}');
    }
  }

  void _toast(String text) {
    if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(text)));
  }

  /// Sends everything selected, connecting first when needed.
  Future<void> _send() async {
    if (_sending || _count == 0) return;
    setState(() => _sending = true);
    try {
      if (!await _s.stillConnected()) {
        if (!mounted) return;
        final c = await connectFlow(context);
        if (c == null || !mounted) return;
      }
      // Gallery items become real files (iCloud originals download here), in the order they were tapped.
      final paths = <String>[];
      final names = <String>[];
      var missing = 0;
      for (final a in _picked.values) {
        final f = await a.originFile;
        if (f == null) {
          missing++;
          continue;
        }
        paths.add(f.path);
        names.add(await a.titleAsync);
      }
      for (final e in _files.entries) {
        paths.add(e.key);
        names.add(e.value);
      }
      if (paths.isEmpty || !mounted) return;
      final r = await sendPaths(context, paths, names);
      if (!mounted || r == null) return;
      HapticFeedback.mediumImpact();
      _clear();
      _toast(doneText(r, sending: true) + (missing > 0 ? '\n$missing tasi telefonda topilmadi' : ''));
    } catch (e) {
      _toast(errorText(e));
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  void _throwSelected() {
    final a = _picked.values.first;
    Navigator.of(context).push(MaterialPageRoute(builder: (_) => ThrowPage(asset: a)));
  }

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final single = _picked.length == 1 && _files.isEmpty && _picked.values.first.type == AssetType.image;
    return Stack(
      children: [
        CustomScrollView(
          slivers: [
            SliverSafeArea(
              bottom: false,
              sliver: SliverPadding(
                padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
                sliver: SliverList.list(
                  children: [
                    widget.header,
                    const SizedBox(height: 18),
                    Text('Nimani\nyuboramiz?', style: v.largeTitle),
                    const SizedBox(height: 16),
                    _HeroPanel(
                      onTap: () => Navigator.of(context).push(MaterialPageRoute(builder: (_) => const ThrowPage())),
                    ),
                    const SizedBox(height: 18),
                    VSegmented(
                      labels: const ['Rasmlar', 'Fayllar'],
                      index: _tab,
                      onChanged: (i) {
                        setState(() => _tab = i);
                        if (i == 1) _loadReceived();
                      },
                    ),
                    const SizedBox(height: 10),
                  ],
                ),
              ),
            ),
            if (_tab == 0) ..._gallery(v) else _filesTab(v),
            SliverToBoxAdapter(child: SizedBox(height: _count > 0 ? 184 : 112)),
          ],
        ),
        // The selection bar floats above the tab bar, near the thumb.
        Positioned(
          left: 16,
          right: 16,
          bottom: 92 + MediaQuery.paddingOf(context).bottom,
          child: AnimatedSwitcher(
            duration: VMotion.fast,
            switchInCurve: VMotion.out,
            transitionBuilder: (child, a) => FadeTransition(
              opacity: a,
              child: SlideTransition(
                position: Tween(begin: const Offset(0, 0.25), end: Offset.zero).animate(a),
                child: child,
              ),
            ),
            child: _count == 0
                ? const SizedBox.shrink()
                : VSurface(
                    key: const ValueKey('bar'),
                    radius: v.rPill.clamp(0, 36),
                    padding: const EdgeInsets.all(6),
                    child: Row(
                      children: [
                        VCircleButton(icon: Icons.close_rounded, onTap: _clear, label: 'Tanlovni tozalash', size: 44),
                        if (single) ...[
                          const SizedBox(width: 6),
                          VCircleButton(
                            icon: Icons.flight_takeoff_rounded,
                            onTap: _throwSelected,
                            label: 'Rasmni otish',
                            size: 44,
                          ),
                        ],
                        const SizedBox(width: 8),
                        Expanded(
                          child: VButton(
                            height: 48,
                            busy: _sending,
                            icon: Icons.arrow_upward_rounded,
                            label: _s.peer == null ? 'Ulanib yuborish · $_count' : 'Yuborish · $_count',
                            onPressed: _send,
                          ),
                        ),
                      ],
                    ),
                  ),
          ),
        ),
      ],
    );
  }

  List<Widget> _gallery(VutricTokens v) {
    final p = _perm;
    if (p == null) {
      return [
        const SliverToBoxAdapter(
          child: Padding(padding: EdgeInsets.all(48), child: Center(child: CircularProgressIndicator())),
        ),
      ];
    }
    if (!p.hasAccess) {
      return [
        SliverToBoxAdapter(
          child: Padding(
            padding: const EdgeInsets.symmetric(vertical: 40),
            child: VEmpty(
              icon: Icons.photo_library_outlined,
              title: 'Galereyaga ruxsat',
              body: 'Rasm va videolarni shu yerda koʻrish va yuborish uchun ruxsat bering.',
              extra: SizedBox(
                width: 220,
                child: VButton(label: 'Ruxsat berish', height: 48, onPressed: PhotoManager.openSetting),
              ),
            ),
          ),
        ),
      ];
    }
    return [
      if (p == PermissionState.limited)
        SliverToBoxAdapter(
          child: Padding(
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 10),
            child: VPressable(
              onTap: () async {
                await PhotoManager.presentLimited();
                await _open();
              },
              child: VNotice(error: false, title: 'Faqat tanlangan rasmlar koʻrinadi', body: 'Koʻproq ruxsat berish uchun bosing'),
            ),
          ),
        ),
      if (_albums.length > 1)
        SliverToBoxAdapter(
          child: SizedBox(
            height: 40,
            child: ListView.separated(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.symmetric(horizontal: 16),
              itemCount: _albums.length,
              separatorBuilder: (_, _) => const SizedBox(width: 6),
              itemBuilder: (_, i) {
                final a = _albums[i];
                final on = a == _album;
                return Center(
                  child: VPressable(
                    onTap: () => _show(a),
                    haptic: !on,
                    child: AnimatedContainer(
                      duration: VMotion.fast,
                      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 7),
                      decoration: BoxDecoration(
                        color: on ? v.ink : v.bgAlt,
                        borderRadius: BorderRadius.circular(v.rPill.clamp(0, 16)),
                      ),
                      child: Text(
                        a.isAll ? 'Hammasi' : a.name,
                        style: v.text(13.5, FontWeight.w500, color: on ? v.bg : v.ink),
                      ),
                    ),
                  ),
                );
              },
            ),
          ),
        ),
      const SliverToBoxAdapter(child: SizedBox(height: 8)),
      if (_assets.isEmpty && !_loading)
        const SliverToBoxAdapter(
          child: Padding(
            padding: EdgeInsets.symmetric(vertical: 40),
            child: VEmpty(icon: Icons.photo_outlined, title: 'Rasm yoʻq', body: 'Bu albomda rasm yoki video yoʻq.'),
          ),
        )
      else
        SliverPadding(
          padding: const EdgeInsets.symmetric(horizontal: 2),
          sliver: SliverGrid.builder(
            gridDelegate: const SliverGridDelegateWithFixedCrossAxisCount(
              crossAxisCount: 4,
              mainAxisSpacing: 2,
              crossAxisSpacing: 2,
            ),
            itemCount: _assets.length,
            itemBuilder: (_, i) {
              if (i >= _assets.length - 40) _more();
              final a = _assets[i];
              final order = _picked.containsKey(a.id) ? _picked.keys.toList().indexOf(a.id) + 1 : 0;
              return _Cell(asset: a, order: order, onTap: () => _toggle(a));
            },
          ),
        ),
    ];
  }

  Widget _filesTab(VutricTokens v) {
    return SliverPadding(
      padding: const EdgeInsets.fromLTRB(16, 4, 16, 0),
      sliver: SliverList.list(
        children: [
          Row(
            children: [
              Expanded(
                child: VButton(
                  label: 'Fayl tanlash',
                  icon: Icons.description_outlined,
                  kind: VButtonKind.secondary,
                  height: 48,
                  onPressed: _pickFromFiles,
                ),
              ),
              const SizedBox(width: 10),
              Expanded(
                child: VButton(
                  label: 'Papka',
                  icon: Icons.create_new_folder_outlined,
                  kind: VButtonKind.secondary,
                  height: 48,
                  onPressed: _pickFolder,
                ),
              ),
            ],
          ),
          if (_files.isNotEmpty) ...[
            const SizedBox(height: 20),
            Text('TANLANGAN', style: v.text(12, FontWeight.w600, color: v.muted, tracking: 0.06)),
            const SizedBox(height: 6),
            for (final e in _files.entries) _FileRow(name: e.value, picked: true, onTap: () => _toggleFile(e.key, e.value)),
          ],
          const SizedBox(height: 20),
          Text('VUTRIC ORQALI KELGANLAR', style: v.text(12, FontWeight.w600, color: v.muted, tracking: 0.06)),
          const SizedBox(height: 2),
          Text(
            Platform.isIOS ? 'Fayllar ilovasida: Mening iPhone’im → Vutric' : 'Telefon xotirasida: Android/data/…/Vutric',
            style: v.caption,
          ),
          const SizedBox(height: 6),
          if (_received.isEmpty)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 28),
              child: Text('Hali hech narsa kelmagan.', textAlign: TextAlign.center, style: v.caption),
            ),
          for (final f in _received)
            _FileRow(
              name: baseName(f.path),
              folder: f is Directory,
              picked: _files.containsKey(f.path),
              onTap: () => _toggleFile(f.path, baseName(f.path)),
            ),
        ],
      ),
    );
  }
}

/// The dark "Rasm otish" panel: a photo tilting out through the top edge, a round accent button.
class _HeroPanel extends StatelessWidget {
  const _HeroPanel({required this.onTap});
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return VPressable(
      onTap: onTap,
      semanticLabel: 'Rasm otish',
      child: Container(
        height: 168,
        clipBehavior: Clip.antiAlias,
        decoration: BoxDecoration(
          color: v.hero,
          borderRadius: BorderRadius.circular(v.rPanel),
          border: v.material == VMaterial.brutal ? Border.all(color: v.ink, width: 2) : null,
          boxShadow: v.material == VMaterial.brutal ? [BoxShadow(color: v.ink, offset: const Offset(4, 4))] : null,
        ),
        child: Stack(
          children: [
            // Accent light from the top right, the portal the photo flies through.
            Positioned(
              right: -60,
              top: -80,
              child: Container(
                width: 240,
                height: 240,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  gradient: RadialGradient(colors: [v.accent.withValues(alpha: 0.55), v.accent.withValues(alpha: 0)]),
                ),
              ),
            ),
            Positioned(right: 54, top: -18, child: _MiniPhoto(rotate: 0.18, ghost: true)),
            Positioned(right: 38, top: -6, child: _MiniPhoto(rotate: 0.12)),
            Positioned(
              left: 18,
              bottom: 18,
              right: 90,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text('Rasm otish', style: v.text(22, FontWeight.w600, color: v.heroInk, tracking: -0.03)),
                  const SizedBox(height: 2),
                  Text('Kompyuterga yoki telefonga', style: v.text(13, FontWeight.w400, color: v.heroMuted)),
                ],
              ),
            ),
            Positioned(
              right: 16,
              bottom: 16,
              child: Container(
                width: 56,
                height: 56,
                decoration: BoxDecoration(
                  color: v.accent,
                  shape: BoxShape.circle,
                  boxShadow: [BoxShadow(color: v.accent.withValues(alpha: 0.5), blurRadius: 20, spreadRadius: -4)],
                ),
                child: Icon(Icons.arrow_upward_rounded, color: v.accentInk),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _MiniPhoto extends StatelessWidget {
  const _MiniPhoto({required this.rotate, this.ghost = false});
  final double rotate;
  final bool ghost;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return Transform.rotate(
      angle: rotate,
      child: Container(
        width: 74,
        height: 98,
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: ghost ? v.accent.withValues(alpha: 0.5) : Colors.white, width: ghost ? 1 : 3),
          gradient: ghost
              ? null
              : const LinearGradient(
                  begin: Alignment.topCenter,
                  end: Alignment.bottomCenter,
                  stops: [0, 0.58, 0.59, 1],
                  colors: [Color(0xFF2B3A78), Color(0xFFC46D5A), Color(0xFF23284F), Color(0xFF121838)],
                ),
        ),
      ),
    );
  }
}

class _FileRow extends StatelessWidget {
  const _FileRow({required this.name, required this.picked, required this.onTap, this.folder = false});
  final String name;
  final bool picked;
  final bool folder;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return Semantics(
      checked: picked,
      label: name,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(12),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 8),
          child: Row(
            children: [
              VIconTile(icon: folder ? Icons.folder_outlined : fileIcon(name), size: 38, background: v.bgAlt),
              const SizedBox(width: 12),
              Expanded(
                child: Text(name, maxLines: 1, overflow: TextOverflow.ellipsis, style: v.text(15, FontWeight.w500)),
              ),
              _Check(on: picked),
            ],
          ),
        ),
      ),
    );
  }
}

class _Check extends StatelessWidget {
  const _Check({required this.on, this.order = 0});
  final bool on;
  final int order;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    return AnimatedContainer(
      duration: VMotion.fast,
      width: 24,
      height: 24,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        shape: BoxShape.circle,
        color: on ? v.accent : const Color(0x33000000),
        border: Border.all(color: Colors.white, width: 1.5),
      ),
      child: on
          ? (order > 0
                ? Text('$order', style: v.text(12, FontWeight.w700, color: v.accentInk))
                : Icon(Icons.check_rounded, size: 15, color: v.accentInk))
          : null,
    );
  }
}

class _Cell extends StatelessWidget {
  const _Cell({required this.asset, required this.order, required this.onTap});
  final AssetEntity asset;
  final int order;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final v = context.v;
    final picked = order > 0;
    return Semantics(
      checked: picked,
      label: asset.type == AssetType.video ? 'Video' : 'Rasm',
      child: GestureDetector(
        onTap: onTap,
        child: Stack(
          fit: StackFit.expand,
          children: [
            AnimatedScale(
              duration: VMotion.fast,
              curve: VMotion.out,
              scale: picked ? 0.86 : 1,
              child: ClipRRect(
                borderRadius: BorderRadius.circular(picked ? 10 : 2),
                child: AssetEntityImage(
                  asset,
                  isOriginal: false,
                  thumbnailSize: const ThumbnailSize.square(200),
                  thumbnailFormat: ThumbnailFormat.jpeg,
                  fit: BoxFit.cover,
                  gaplessPlayback: true,
                  errorBuilder: (_, _, _) => ColoredBox(color: v.bgAlt),
                ),
              ),
            ),
            if (asset.type == AssetType.video)
              Positioned(left: 6, bottom: 6, child: _Badge(text: _clock(asset.duration))),
            Positioned(right: 6, top: 6, child: _Check(on: picked, order: order)),
          ],
        ),
      ),
    );
  }
}

String _clock(int seconds) => '${seconds ~/ 60}:${(seconds % 60).toString().padLeft(2, '0')}';

class _Badge extends StatelessWidget {
  const _Badge({required this.text});
  final String text;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 5, vertical: 1),
      decoration: BoxDecoration(color: const Color(0x99000000), borderRadius: BorderRadius.circular(6)),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Icon(Icons.play_arrow_rounded, size: 12, color: Colors.white),
          Text(text, style: context.v.text(11, FontWeight.w600, color: Colors.white)),
        ],
      ),
    );
  }
}
