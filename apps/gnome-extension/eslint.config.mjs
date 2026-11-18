// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0
//
// Lints the extension as GJS ES modules (GNOME Shell 45+ style). Run from this folder:
//   npx --yes eslint@9.39.1 .

export default [
    {
        files: ['vutric@wonmor.io/**/*.js'],
        languageOptions: {
            ecmaVersion: 2022,
            sourceType: 'module',
            globals: {
                global: 'readonly',
                log: 'readonly',
                logError: 'readonly',
                print: 'readonly',
                printerr: 'readonly',
                TextDecoder: 'readonly',
                TextEncoder: 'readonly',
                console: 'readonly',
            },
        },
        rules: {
            'no-unused-vars': ['error', {argsIgnorePattern: '^_'}],
            'no-undef': 'error',
            'prefer-const': 'error',
            'no-var': 'error',
            eqeqeq: 'error',
            'no-eval': 'error',
            'no-implied-eval': 'error',
            'no-new-func': 'error',
            curly: ['error', 'multi-or-nest', 'consistent'],
        },
    },
    {
        files: ['tests/**/*.mjs'],
        languageOptions: {
            ecmaVersion: 2022,
            sourceType: 'module',
            globals: {URL: 'readonly'},
        },
    },
];
