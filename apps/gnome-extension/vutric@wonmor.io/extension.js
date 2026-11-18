// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0
//
// Vutric on GNOME Shell 46–50 (ADR-0090 §2). GNOME has no layer-shell, so this extension:
//   1. launches `vutric-surface` as a trusted Wayland client and turns its window into a dock that is kept above,
//      sticky, hidden from Alt+Tab and the overview, and stretched over the primary monitor;
//   2. draws the daemon's instant cards as Shell chrome (clickable, keyboard focus stays in the user's app);
//   3. shows the Vutric panel menu, because plain GNOME has no StatusNotifierItem host.
// The daemon is the only intended caller of io.wonmor.Vutric.Shell1; nothing here receives files.

import Clutter from 'gi://Clutter';
import Cogl from 'gi://Cogl';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import * as Config from 'resource:///org/gnome/shell/misc/config.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {t} from './i18n.js';
import {API, cardPosition, hitAt, mutterApi, surfaceCandidates, validCard, validSocket} from './lib.js';

const OBJECT_PATH = '/io/wonmor/Vutric/Shell';
const SURFACE_APP_ID = 'io.wonmor.Vutric.Surface';

function loadInterfaceXml(dir) {
    const file = dir.get_child('dbus').get_child('io.wonmor.Vutric.Shell1.xml');
    const [, bytes] = file.load_contents(null);
    return new TextDecoder().decode(bytes);
}

function setImageBytes(content, bytes, width, height) {
    const format = Cogl.PixelFormat.BGRA_8888_PRE;
    try {
        // GNOME 47+: the Cogl context comes first.
        const ctx = global.stage.context.get_backend().get_cogl_context();
        content.set_bytes(ctx, bytes, format, width, height, width * 4);
    } catch {
        content.set_bytes(bytes, format, width, height, width * 4);
    }
}

class Surface {
    constructor() {
        this._client = null;
        this._proc = null;
        this._windows = new Set();
        this._createdId = global.display.connect('window-created', (_d, win) => this._adopt(win));
        this._monitorsId = Main.layoutManager.connect('monitors-changed', () => this._placeAll());
        this._api = mutterApi(Meta.WaylandClient, Meta.Window.prototype);
    }

    get pid() {
        const id = this._proc?.get_identifier();
        return id ? Number(id) : 0;
    }

    spawn(socket) {
        if (this._proc && this.pid)
            return this.pid;
        const exe = surfaceCandidates(GLib.get_home_dir())
            .find(p => GLib.file_test(p, GLib.FileTest.IS_EXECUTABLE));
        if (!exe)
            throw new Error('vutric-surface is not installed');
        const argv = [exe, '--backend', 'gnome', '--socket', socket];
        const launcher = new Gio.SubprocessLauncher({flags: Gio.SubprocessFlags.NONE});
        if (this._api.spawn === 'new_subprocess') {
            this._client = Meta.WaylandClient.new_subprocess(global.context, launcher, argv);
            this._proc = this._client.get_subprocess();
        } else {
            this._client = Meta.WaylandClient.new(global.context, launcher);
            this._proc = this._client.spawnv(global.display, argv);
        }
        this._proc.wait_async(null, () => {
            this._proc = null;
            this._client = null;
            this._windows.clear();
        });
        return this.pid;
    }

    _adopt(win) {
        if (!this._client || !this._client.owns_window(win))
            return;
        if (win.get_wm_class() !== SURFACE_APP_ID && win.get_gtk_application_id?.() !== SURFACE_APP_ID) {
            // Our process, but not the effect window: leave it alone (there is no other window today).
            return;
        }
        if (this._api.retype === 'window.set_type')
            win.set_type(Meta.WindowType.DOCK);
        else
            this._client.make_dock(win);
        if (this._api.hide === 'window.hide_from_window_list')
            win.hide_from_window_list();
        else
            this._client.hide_from_window_list(win);
        win.make_above();
        win.stick();
        console.log(`Vutric: surface window adopted (${this._api.spawn}, ${this._api.retype}, ${this._api.hide})`);
        this._windows.add(win);
        win.connect('unmanaged', () => this._windows.delete(win));
        win.connect('shown', () => this._place(win));
        GLib.idle_add(GLib.PRIORITY_DEFAULT, () => {
            this._place(win);
            return GLib.SOURCE_REMOVE;
        });
    }

    _place(win) {
        const m = Main.layoutManager.primaryMonitor;
        if (m)
            win.move_resize_frame(false, m.x, m.y, m.width, m.height);
    }

    _placeAll() {
        for (const w of this._windows)
            this._place(w);
    }

    destroy() {
        global.display.disconnect(this._createdId);
        Main.layoutManager.disconnect(this._monitorsId);
        this._proc?.force_exit();
        this._proc = null;
        this._client = null;
    }
}

class Card {
    constructor(onClick) {
        this._onClick = onClick;
        this._actor = null;
        this._hits = [];
        this._scale = 1;
    }

    show(width, height, scale, bytes, hits) {
        this.hide();
        const content = St.ImageContent.new_with_preferred_size(width, height);
        setImageBytes(content, bytes, width, height);
        const w = width / scale;
        const h = height / scale;
        const actor = new Clutter.Actor({reactive: true, width: w, height: h, content});
        const index = Main.layoutManager.primaryIndex;
        const [x, y] = cardPosition(Main.layoutManager.getWorkAreaForMonitor(index), w, h);
        actor.set_position(x, y);
        this._hits = hits;
        this._scale = scale;
        actor.connect('button-release-event', (a, event) => {
            const [sx, sy] = event.get_coords();
            const [ok, lx, ly] = a.transform_stage_point(sx, sy);
            const id = ok ? hitAt(this._hits, lx * this._scale, ly * this._scale) : null;
            if (id)
                this._onClick(id);
            return Clutter.EVENT_STOP;
        });
        Main.layoutManager.addTopChrome(actor, {affectsInputRegion: true, trackFullscreen: false});
        this._actor = actor;
    }

    hide() {
        if (!this._actor)
            return;
        Main.layoutManager.removeChrome(this._actor);
        this._actor.destroy();
        this._actor = null;
        this._hits = [];
    }
}

class Indicator {
    constructor(dir, onAction) {
        this._icons = dir.get_child('icons');
        this._onAction = onAction;
        this._button = null;
        this._lang = 'uz';
        this._paused = false;
    }

    update(paused, lang) {
        this._paused = paused;
        this._lang = lang === 'en' ? 'en' : 'uz';
        this._button?.destroy();
        const b = new PanelMenu.Button(0.0, 'Vutric', false);
        const name = paused ? 'io.wonmor.Vutric-paused-symbolic.svg' : 'io.wonmor.Vutric-symbolic.svg';
        b.add_child(new St.Icon({
            gicon: new Gio.FileIcon({file: this._icons.get_child(name)}),
            fallback_icon_name: 'send-to-symbolic',
            style_class: 'system-status-icon',
        }));
        b.accessible_name = t(this._lang, paused ? 'tray.tooltip.paused' : 'tray.tooltip');
        const item = (key, action) => {
            const i = new PopupMenu.PopupMenuItem(t(this._lang, key));
            i.connect('activate', () => this._onAction(action));
            b.menu.addMenuItem(i);
        };
        item('tray.pair', 'pair');
        item('tray.settings', 'settings');
        b.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        item(paused ? 'tray.resume' : 'tray.pause', 'toggle_pause');
        b.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        item('tray.quit', 'quit');
        Main.panel.addToStatusArea('vutric', b);
        this._button = b;
    }

    destroy() {
        this._button?.destroy();
        this._button = null;
    }
}

export default class VutricExtension extends Extension {
    enable() {
        this._surface = new Surface();
        this._card = new Card(id => this._emit('CardClicked', id));
        this._indicator = new Indicator(this.dir, id => this._emit('MenuActivated', id));
        this._watch = 0;
        this._owner = null;
        this._dbus = Gio.DBusExportedObject.wrapJSObject(loadInterfaceXml(this.dir), this);
        this._dbus.export(Gio.DBus.session, OBJECT_PATH);
    }

    disable() {
        this._dbus?.unexport();
        this._dbus = null;
        if (this._watch)
            Gio.bus_unwatch_name(this._watch);
        this._watch = 0;
        this._owner = null;
        this._card?.hide();
        this._card = null;
        this._indicator?.destroy();
        this._indicator = null;
        this._surface?.destroy();
        this._surface = null;
    }

    _emit(signal, id) {
        this._dbus?.emit_signal(signal, new GLib.Variant('(s)', [id]));
    }

    // Follow the daemon that called us: when it leaves the bus, the menu, the card and the surface go too.
    _follow(sender) {
        if (!sender || sender === this._owner)
            return;
        if (this._watch)
            Gio.bus_unwatch_name(this._watch);
        this._owner = sender;
        this._watch = Gio.bus_watch_name(Gio.BusType.SESSION, sender, Gio.BusNameWatcherFlags.NONE,
            null, () => {
                this._card?.hide();
                this._indicator?.destroy();
                this._surface?.destroy();
                this._surface = new Surface();
                this._owner = null;
            });
    }

    // io.wonmor.Vutric.Shell1

    Ping() {
        return [API, Config.PACKAGE_VERSION];
    }

    SpawnSurfaceAsync([socket], invocation) {
        if (!validSocket(socket, GLib.get_user_runtime_dir())) {
            invocation.return_dbus_error('org.freedesktop.DBus.Error.InvalidArgs', 'socket outside $XDG_RUNTIME_DIR/vutric/');
            return;
        }
        try {
            this._follow(invocation.get_sender());
            invocation.return_value(new GLib.Variant('(i)', [this._surface.spawn(socket)]));
        } catch (e) {
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed', `${e.message}`);
        }
    }

    ShowCardAsync([width, height, scale, bgra, hits], invocation) {
        if (!validCard(width, height, scale, bgra.length)) {
            invocation.return_dbus_error('org.freedesktop.DBus.Error.InvalidArgs', 'bad card raster');
            return;
        }
        try {
            this._follow(invocation.get_sender());
            this._card.show(width, height, scale, new GLib.Bytes(bgra), hits);
            invocation.return_value(null);
        } catch (e) {
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed', `${e.message}`);
        }
    }

    HideCard() {
        this._card?.hide();
    }

    SetStatusAsync([paused, lang], invocation) {
        this._follow(invocation.get_sender());
        this._indicator.update(paused, lang);
        invocation.return_value(null);
    }
}
