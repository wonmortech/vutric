// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0
//
// The extension's strings. Keys are the daemon's (`tray.*` from vutric-daemon::i18n, `linux.*` from
// vutric-host-linux::i18n); Uzbek (Latin) is the default, English the fallback.

const CATALOG = {
    'tray.tooltip': ['Vutric — qabul qilishga tayyor', 'Vutric — ready to receive'],
    'tray.tooltip.paused': ["Vutric — to'xtatilgan", 'Vutric — paused'],
    'tray.pair': ['Telefonni ulash', 'Connect a phone'],
    'tray.settings': ['Sozlamalar', 'Settings'],
    'tray.pause': ["To'xtatib turish", 'Pause'],
    'tray.resume': ['Davom ettirish', 'Resume'],
    'tray.quit': ['Chiqish', 'Quit'],
};

/** `lang` is what the daemon sent with SetStatus: `uz` or `en`. Unknown keys come back as the key. */
export function t(lang, key) {
    const row = CATALOG[key];
    if (!row)
        return key;
    return lang === 'en' ? row[1] : row[0];
}

export const KEYS = Object.keys(CATALOG);
