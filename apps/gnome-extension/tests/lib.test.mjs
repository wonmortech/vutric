// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0
//
// node --test apps/gnome-extension/tests/

import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {test} from 'node:test';

import {API, cardPosition, hitAt, mutterApi, surfaceCandidates, validCard, validSocket} from '../vutric@wonmor.io/lib.js';
import {KEYS, t} from '../vutric@wonmor.io/i18n.js';

const ext = new URL('../vutric@wonmor.io/', import.meta.url);

test('socket must sit directly in $XDG_RUNTIME_DIR/vutric', () => {
    const rt = '/run/user/1000';
    assert.ok(validSocket('/run/user/1000/vutric/surface-42.sock', rt));
    assert.ok(validSocket('/run/user/1000/vutric/surface-42.sock', `${rt}/`));
    for (const bad of [
        '/run/user/1000/vutric/../evil.sock',
        '/run/user/1000/vutric/sub/x.sock',
        '/run/user/1000/vutric/x.socket',
        '/run/user/1000/vutric/.hidden.sock',
        '/tmp/vutric/x.sock',
        '/run/user/1000/vutricx/x.sock',
        '/run/user/1000/vutric/a b.sock',
        42,
    ])
        assert.equal(validSocket(bad, rt), false, String(bad));
    assert.equal(validSocket('/run/user/1000/vutric/x.sock', 'relative'), false);
});

test('only fixed binary paths are ever launched', () => {
    assert.deepEqual(surfaceCandidates('/home/ali/'), [
        '/usr/libexec/vutric/vutric-surface',
        '/usr/lib/vutric/vutric-surface',
        '/home/ali/.local/libexec/vutric/vutric-surface',
    ]);
    assert.equal(surfaceCandidates(undefined).length, 2);
    assert.equal(surfaceCandidates('../x').length, 2);
});

test('hit rectangles are half-open', () => {
    const hits = [['decline', 0, 100, 100, 140], ['accept', 100, 100, 200, 140]];
    assert.equal(hitAt(hits, 100, 120), 'accept');
    assert.equal(hitAt(hits, 99.9, 120), 'decline');
    assert.equal(hitAt(hits, 200, 120), null);
});

test('card sits bottom-right of the work area', () => {
    assert.deepEqual(cardPosition({x: 0, y: 32, width: 1920, height: 1048}, 400, 176), [1504, 888]);
    assert.ok(validCard(400, 176, 1, 400 * 176 * 4));
    assert.equal(validCard(400, 176, 1, 3), false);
    assert.equal(validCard(400, 176, 0, 400 * 176 * 4), false);
    assert.equal(validCard(5000, 1, 1, 5000 * 4), false);
});

test('mutter API is feature-detected', () => {
    assert.deepEqual(mutterApi({new: () => {}}, {}), {
        spawn: 'new+spawnv', retype: 'client.make_dock', hide: 'client.hide_from_window_list',
    });
    assert.deepEqual(mutterApi({new_subprocess: () => {}}, {set_type() {}, hide_from_window_list() {}}), {
        spawn: 'new_subprocess', retype: 'window.set_type', hide: 'window.hide_from_window_list',
    });
});

test('strings: Uzbek default, English fallback, no Cyrillic', () => {
    assert.equal(t('uz', 'tray.pair'), 'Telefonni ulash');
    assert.equal(t('en', 'tray.pair'), 'Connect a phone');
    assert.equal(t('ru', 'tray.quit'), 'Chiqish');
    assert.equal(t('uz', 'missing'), 'missing');
    for (const k of KEYS)
        assert.ok(!/[Ѐ-ӿ]/.test(t('uz', k)), k);
});

test('metadata and D-Bus contract', () => {
    const meta = JSON.parse(readFileSync(new URL('metadata.json', ext), 'utf8'));
    assert.equal(meta.uuid, 'vutric@wonmor.io');
    assert.deepEqual(meta['shell-version'], ['46', '47', '48', '49', '50']);
    const xml = readFileSync(new URL('dbus/io.wonmor.Vutric.Shell1.xml', ext), 'utf8');
    const methods = [...xml.matchAll(/<method name="(\w+)"/g)].map(m => m[1]);
    const src = readFileSync(new URL('extension.js', ext), 'utf8');
    for (const m of methods)
        assert.ok(new RegExp(`\\n    ${m}(Async)?\\(`).test(src), `extension implements ${m}`);
    assert.equal(API, 1);
});
