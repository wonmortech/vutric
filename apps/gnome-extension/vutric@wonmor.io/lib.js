// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0
//
// Pure helpers of the Vutric extension. No `gi://` imports, so they are unit-tested with `node --test` in CI.

/** API level of `io.wonmor.Vutric.Shell1`; must match `vutric_host_linux::gnome::API`. */
export const API = 1;

/** The only binary the extension ever launches, in this order of preference. */
export function surfaceCandidates(home) {
    const list = ['/usr/libexec/vutric/vutric-surface', '/usr/lib/vutric/vutric-surface'];
    if (typeof home === 'string' && home.startsWith('/'))
        list.push(`${home.replace(/\/+$/, '')}/.local/libexec/vutric/vutric-surface`);
    return list;
}

/**
 * The daemon's socket must be a `*.sock` file directly inside `$XDG_RUNTIME_DIR/vutric/`. Anything else
 * (other folders, `..`, odd characters) is refused, so the call cannot point the surface somewhere else.
 */
export function validSocket(path, runtimeDir) {
    if (typeof path !== 'string' || typeof runtimeDir !== 'string' || !runtimeDir.startsWith('/'))
        return false;
    const prefix = `${runtimeDir.replace(/\/+$/, '')}/vutric/`;
    if (!path.startsWith(prefix))
        return false;
    const name = path.slice(prefix.length);
    return /^[A-Za-z0-9_-][A-Za-z0-9._-]{0,63}\.sock$/.test(name) && !name.includes('..');
}

/** The id of the hit rectangle under (x, y) in buffer pixels; rectangles are half-open. */
export function hitAt(hits, x, y) {
    for (const [id, x0, y0, x1, y1] of hits) {
        if (x >= x0 && x < x1 && y >= y0 && y < y1)
            return id;
    }
    return null;
}

/** Where a card of `width` x `height` logical pixels goes inside a work area: bottom-right, 16 px margin. */
export function cardPosition(workArea, width, height, margin = 16) {
    return [
        Math.round(workArea.x + workArea.width - width - margin),
        Math.round(workArea.y + workArea.height - height - margin),
    ];
}

/** Validates a card raster from D-Bus before it reaches Cogl. */
export function validCard(width, height, scale, length) {
    return Number.isInteger(width) && Number.isInteger(height) && Number.isInteger(scale) &&
        width > 0 && height > 0 && width <= 4096 && height <= 4096 &&
        scale >= 1 && scale <= 4 && length === width * height * 4;
}

/** Feature detection for the Mutter API move in GNOME 49 (see ADR-0090). */
export function mutterApi(WaylandClient, windowProto) {
    return {
        spawn: typeof WaylandClient?.new_subprocess === 'function' ? 'new_subprocess' : 'new+spawnv',
        retype: typeof windowProto?.set_type === 'function' ? 'window.set_type' : 'client.make_dock',
        hide: typeof windowProto?.hide_from_window_list === 'function'
            ? 'window.hide_from_window_list' : 'client.hide_from_window_list',
    };
}
