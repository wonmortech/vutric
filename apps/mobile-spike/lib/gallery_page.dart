// The first screen: the phone's whole gallery and files, inside Vutric. Tap to select (as many as you like),
// then one button sends them to the connected device. No detour through the system picker.
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

import 'src/rust/api.dart' as vutric;
import 'transfer_ui.dart';
import 'ui.dart';

const _page = 120;

class GalleryPage extends StatefulWidget {
  const GalleryPage({super.key, required this.connect, required this.openMenu});

  /// Opens the connect sheet; resolves to the peer's name once a session is open, null when the user gave up.
  final Future<String?> Function() connect;

  /// The "more" menu: receive, throw a photo, settings, measurement.
  final VoidCallback openMenu;

  @override
  State<GalleryPage> createState() => GalleryPageState();
}

class GalleryPageState extends State<GalleryPage> {
  int _tab = 0;
  PermissionState? _perm;
  List<AssetPathEntity> _albums = const [];
  AssetPathEntity? _album;
  final _assets = <AssetEntity>[];
  int _total = 0;
  bool _loading = false;

  /// Selected gallery items in the order they were tapped (the order they are sent).
  final _picked = <String, AssetEntity>{};

  /// Selected files from the Files tab: path → name.
  final _files = <String, String>{};
  List<FileSystemEntity> _received = const [];
  String? _receivedDir;
  String? _peer;
  String? _note;
  bool _sending = false;

  @override
  void initState() {
    super.initState();
    _open();
  }

  /// Called by the home after a session opened or closed elsewhere (menu pages).
  void peerChanged(String? name) => setState(() => _peer = name);

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
    final dir = await receiveDir();
    final d = Directory(dir);
    final items = await d.exists() ? await d.list().toList() : <FileSystemEntity>[];
    final when = {for (final f in items) f.path: (await f.stat()).modified};
    items.sort((a, b) => when[b.path]!.compareTo(when[a.path]!));
    if (!mounted) return;
    setState(() {
      _receivedDir = dir;
      _received = items;
    });
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
      if (mounted) setState(() => _note = 'Tanlab boʻlmadi: ${errorText(e)}');
    }
  }

  Future<void> _pickFolder() async {
    try {
      // The Rust sender reads real paths; pickDirectory()'s content:// URI it cannot open. Android may still hand
      // back a folder the app cannot read: then the send fails with that error, shown in the bar.
      // ignore: deprecated_member_use
      final dir = await FilePicker.getDirectoryPath();
      if (dir == null || !mounted) return;
      setState(() => _files[dir] = _baseName(dir));
    } catch (e) {
      if (mounted) setState(() => _note = 'Papkani ochib boʻlmadi: ${errorText(e)}');
    }
  }

  /// Sends everything selected, connecting first when needed.
  Future<void> _send() async {
    if (_sending || _count == 0) return;
    setState(() {
      _sending = true;
      _note = null;
    });
    try {
      var peer = (await vutric.currentRoute()).isEmpty ? null : _peer;
      if (peer == null) {
        if (!mounted) return;
        peer = await widget.connect();
        if (peer == null || !mounted) return;
        setState(() => _peer = peer);
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
      final r = await runTransfer(
        context,
        title: '$peer ga yuborilmoqda',
        stream: vutric.sendFiles(paths: paths, names: names),
      );
      if (!mounted) return;
      if (r != null) {
        setState(() {
          _note = doneText(r, sending: true) + (missing > 0 ? '\n$missing tasi telefonda topilmadi' : '');
          _picked.clear();
          _files.clear();
        });
      } else if ((await vutric.currentRoute()).isEmpty && mounted) {
        setState(() => _peer = null);
      }
    } catch (e) {
      if (mounted) setState(() => _note = errorText(e));
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  Future<void> _connectOnly() async {
    final peer = await widget.connect();
    if (peer != null && mounted) setState(() => _peer = peer);
  }

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context).textTheme;
    return Scaffold(
      appBar: AppBar(
        backgroundColor: sand,
        titleSpacing: 16,
        title: Row(
          children: [
            Text('Vutric', style: t.titleLarge?.copyWith(color: cobalt, fontWeight: FontWeight.w700)),
            const SizedBox(width: 12),
            Flexible(child: _PeerChip(peer: _peer, onTap: _connectOnly)),
          ],
        ),
        actions: [IconButton(onPressed: widget.openMenu, icon: const Icon(Icons.more_horiz), tooltip: 'Yana')],
        bottom: PreferredSize(
          preferredSize: const Size.fromHeight(52),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
            child: SegmentedButton<int>(
              segments: const [
                ButtonSegment(value: 0, label: Text('Galereya'), icon: Icon(Icons.photo_library_outlined)),
                ButtonSegment(value: 1, label: Text('Fayllar'), icon: Icon(Icons.folder_outlined)),
              ],
              selected: {_tab},
              showSelectedIcon: false,
              onSelectionChanged: (s) {
                setState(() => _tab = s.first);
                if (_tab == 1) _loadReceived();
              },
            ),
          ),
        ),
      ),
      body: _tab == 0 ? _gallery() : _filesTab(t),
      bottomNavigationBar: _bar(t),
    );
  }

  Widget _gallery() {
    final p = _perm;
    if (p == null) return const Center(child: CircularProgressIndicator());
    if (!p.hasAccess) {
      return _Empty(
        icon: Icons.photo_library_outlined,
        text: 'Rasm va videolarni shu yerda koʻrish uchun galereyaga ruxsat bering.',
        action: 'Ruxsat berish',
        onAction: PhotoManager.openSetting,
      );
    }
    return Column(
      children: [
        if (p == PermissionState.limited)
          Material(
            color: paper,
            child: ListTile(
              dense: true,
              leading: const Icon(Icons.info_outline, color: cobalt),
              title: const Text('Faqat tanlangan rasmlar koʻrinadi'),
              trailing: TextButton(
                onPressed: () async {
                  await PhotoManager.presentLimited();
                  await _open();
                },
                child: const Text('Koʻproq'),
              ),
            ),
          ),
        if (_albums.length > 1)
          SizedBox(
            height: 44,
            child: ListView.separated(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
              itemCount: _albums.length,
              separatorBuilder: (_, _) => const SizedBox(width: 6),
              itemBuilder: (_, i) {
                final a = _albums[i];
                return ChoiceChip(
                  label: Text(a.isAll ? 'Hammasi' : a.name),
                  selected: a == _album,
                  onSelected: (_) => _show(a),
                );
              },
            ),
          ),
        Expanded(
          child: _assets.isEmpty && !_loading
              ? const _Empty(icon: Icons.photo_outlined, text: 'Bu albomda rasm yoki video yoʻq.')
              : GridView.builder(
                  padding: const EdgeInsets.all(2),
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
      ],
    );
  }

  Widget _filesTab(TextTheme t) {
    return ListView(
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 16),
      children: [
        Row(
          children: [
            Expanded(
              child: FilledButton.tonalIcon(
                onPressed: _pickFromFiles,
                icon: const Icon(Icons.description_outlined),
                label: const Text('Fayl tanlash'),
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: FilledButton.tonalIcon(
                onPressed: _pickFolder,
                icon: const Icon(Icons.create_new_folder_outlined),
                label: const Text('Papka'),
              ),
            ),
          ],
        ),
        if (_files.isNotEmpty) ...[
          const SizedBox(height: 16),
          Text('Tanlangan', style: t.titleSmall?.copyWith(color: cobalt)),
          for (final e in _files.entries)
            ListTile(
              contentPadding: EdgeInsets.zero,
              leading: Icon(_iconFor(e.value), color: cobalt),
              title: Text(e.value, maxLines: 1, overflow: TextOverflow.ellipsis),
              trailing: IconButton(
                icon: const Icon(Icons.close),
                onPressed: () => setState(() => _files.remove(e.key)),
              ),
            ),
        ],
        const SizedBox(height: 16),
        Text('Vutric orqali kelganlar', style: t.titleSmall?.copyWith(color: cobalt)),
        if (_receivedDir != null)
          Text(
            Platform.isIOS ? 'Fayllar ilovasida: Mening iPhone’im → Vutric spike → Vutric' : _receivedDir!,
            style: t.bodySmall,
          ),
        if (_received.isEmpty)
          const Padding(
            padding: EdgeInsets.symmetric(vertical: 24),
            child: Text('Hali hech narsa kelmagan.', textAlign: TextAlign.center),
          ),
        for (final f in _received)
          ListTile(
            contentPadding: EdgeInsets.zero,
            leading: Icon(f is Directory ? Icons.folder : _iconFor(f.path), color: cobalt),
            title: Text(_baseName(f.path), maxLines: 1, overflow: TextOverflow.ellipsis),
            trailing: Checkbox(
              value: _files.containsKey(f.path),
              onChanged: (_) => _toggleFile(f.path, _baseName(f.path)),
            ),
            onTap: () => _toggleFile(f.path, _baseName(f.path)),
          ),
      ],
    );
  }

  Widget? _bar(TextTheme t) {
    if (_count == 0 && _note == null) return null;
    return SafeArea(
      child: Container(
        padding: const EdgeInsets.fromLTRB(16, 10, 16, 10),
        decoration: const BoxDecoration(
          color: paper,
          boxShadow: [BoxShadow(blurRadius: 12, color: Color(0x22000000))],
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (_note != null)
              Padding(
                padding: const EdgeInsets.only(bottom: 8),
                child: Row(
                  children: [
                    Expanded(child: Text(_note!, style: t.bodySmall)),
                    IconButton(
                      visualDensity: VisualDensity.compact,
                      onPressed: () => setState(() => _note = null),
                      icon: const Icon(Icons.close, size: 18),
                    ),
                  ],
                ),
              ),
            if (_count > 0)
              Row(
                children: [
                  TextButton(
                    onPressed: _sending
                        ? null
                        : () => setState(() {
                            _picked.clear();
                            _files.clear();
                          }),
                    child: const Text('Tozalash'),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: FilledButton.icon(
                      style: FilledButton.styleFrom(minimumSize: const Size.fromHeight(48), backgroundColor: cobalt),
                      onPressed: _sending ? null : _send,
                      icon: _sending
                          ? const SizedBox.square(dimension: 18, child: CircularProgressIndicator(strokeWidth: 2))
                          : const Icon(Icons.north),
                      label: Text(_peer == null ? 'Ulanib yuborish · $_count ta' : 'Yuborish · $_count ta'),
                    ),
                  ),
                ],
              ),
          ],
        ),
      ),
    );
  }
}

String _baseName(String path) =>
    path.split(RegExp(r'[/\\]')).lastWhere((s) => s.isNotEmpty, orElse: () => 'fayl');

IconData _iconFor(String name) {
  final dot = name.lastIndexOf('.');
  final ext = dot < 0 ? '' : name.substring(dot + 1).toLowerCase();
  return switch (ext) {
    'jpg' || 'jpeg' || 'png' || 'heic' || 'webp' || 'gif' => Icons.image_outlined,
    'mp4' || 'mov' || 'm4v' || 'webm' || 'mkv' => Icons.movie_outlined,
    'mp3' || 'm4a' || 'wav' || 'flac' || 'ogg' => Icons.audiotrack_outlined,
    'zip' || 'rar' || '7z' || 'gz' || 'tar' => Icons.folder_zip_outlined,
    'pdf' => Icons.picture_as_pdf_outlined,
    _ => Icons.insert_drive_file_outlined,
  };
}

class _Cell extends StatelessWidget {
  const _Cell({required this.asset, required this.order, required this.onTap});
  final AssetEntity asset;
  final int order;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final picked = order > 0;
    return GestureDetector(
      onTap: onTap,
      child: Stack(
        fit: StackFit.expand,
        children: [
          AnimatedPadding(
            duration: const Duration(milliseconds: 120),
            padding: EdgeInsets.all(picked ? 6 : 0),
            child: AssetEntityImage(
              asset,
              isOriginal: false,
              thumbnailSize: const ThumbnailSize.square(200),
              thumbnailFormat: ThumbnailFormat.jpeg,
              fit: BoxFit.cover,
              gaplessPlayback: true,
              errorBuilder: (_, _, _) => const ColoredBox(color: paper),
            ),
          ),
          if (asset.type == AssetType.video)
            Positioned(left: 6, bottom: 6, child: _Badge(text: _clock(asset.duration))),
          Positioned(
            right: 6,
            top: 6,
            child: Container(
              width: 24,
              height: 24,
              alignment: Alignment.center,
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                color: picked ? signal : const Color(0x33000000),
                border: Border.all(color: Colors.white, width: 1.5),
              ),
              child: picked
                  ? Text(
                      '$order',
                      style: const TextStyle(color: Colors.white, fontSize: 12, fontWeight: FontWeight.w700),
                    )
                  : null,
            ),
          ),
        ],
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
      padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1),
      decoration: BoxDecoration(color: const Color(0x99000000), borderRadius: BorderRadius.circular(4)),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Icon(Icons.play_arrow_rounded, size: 12, color: Colors.white),
          Text(text, style: const TextStyle(color: Colors.white, fontSize: 11)),
        ],
      ),
    );
  }
}

class _PeerChip extends StatelessWidget {
  const _PeerChip({required this.peer, required this.onTap});
  final String? peer;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final on = peer != null;
    return ActionChip(
      onPressed: onTap,
      avatar: Icon(Icons.circle, size: 10, color: on ? ok : Colors.grey),
      label: Text(on ? peer! : 'Ulanish', overflow: TextOverflow.ellipsis),
      backgroundColor: paper,
      side: BorderSide.none,
    );
  }
}

class _Empty extends StatelessWidget {
  const _Empty({required this.icon, required this.text, this.action, this.onAction});
  final IconData icon;
  final String text;
  final String? action;
  final Future<void> Function()? onAction;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 48, color: cobalt),
            const SizedBox(height: 12),
            Text(text, textAlign: TextAlign.center),
            if (action != null) ...[
              const SizedBox(height: 12),
              FilledButton(onPressed: onAction, child: Text(action!)),
            ],
          ],
        ),
      ),
    );
  }
}
