# ADR-0057: Storage engine rules in the core (T2, T4–T7)

Status: accepted (2026-11-18) · Stream: W3 · Backlog: T2, T4, T5, T6, T7 · Builds on: ADR-0007 part 4

## Context

The storage settings (`vutric_core::storage::Settings`) already held every user choice: folder, conflict rule,
free-space limit, what to do after receiving, how long parts and history are kept. Several had no engine
behind them. The screens belong to W1 (phone) and W2 (desktop); this ADR covers what the core and `vutric-net`
do, so both screens get the same behaviour.

## Decision

All changes are additive: new enum variants, new fields with defaults, new functions.

| Item | Engine | Where |
|---|---|---|
| T2 | `SaveRules::usable(is_dir)` swaps a chosen folder that is gone (unplugged drive, deleted folder) for the default. `receive` does this itself, never recreates the missing folder, and reports it in `Summary.missing_folder`. `Settings::tell_missing_folder` says yes once per missing folder, and resets when the folder is back. | `storage.rs`, `transfer.rs` |
| T4 | The guard compares free space with what is still to arrive plus `min_free_bytes` (1 GiB by default). It now takes the smaller of the target's volume and the default folder's volume, because phones stage files in app storage before the gallery or SAF copy. | `transfer.rs` |
| T5 | `After::action(saved) -> AfterAction`: notify with a count, open the file, or open the folder. With several files, "open file" opens their common folder instead of many apps at once. Nothing saved means nothing to do. The host carries the action out. | `storage.rs` |
| T6 | `disk::sweep_parts(root, keep_days, now) -> Swept`. It deletes only plain `*.vutric-part` files older than `keep_days`, never follows links, and goes at most 8 folders deep. `0` means "delete now". `migrate` already clamps the setting to 1–30. Today's parts are never old enough, so a daily sweep cannot race a transfer. History expiry (`history_days`) stays with the history store's owner. | `disk.rs` |
| T7 | `Conflict::Ask` plus an optional `SaveTo.ask` callback, asked only when the name is taken. Questions come one at a time, because the rename lock is held. The answer is `Replace` or `Skip`. Anything else, no callback, or no answer within 5 minutes keeps both files. A host that has not wired the callback therefore behaves exactly as before. | `storage.rs`, `transfer.rs` |

## Measured

| Check | Result |
|---|---|
| `tests/save_rules.rs`: a clash answered "replace" and one answered "skip"; asked only twice for three files; no copies or parts left | pass |
| `tests/save_rules.rs`: a missing folder falls back to the default, is reported and is not recreated | pass |
| `tests/save_rules.rs`: an impossible free-space limit: receiver `NoRoom`, sender `NoRoom`, nothing written | pass |
| `disk::sweep_tests`: fresh parts stay, old parts go, other files stay, "delete now" | pass |
| `storage` unit tests: `After::action`, folder fallback told once, `"ask"` round-trips | pass |

## Handed over

- **W1:** add the `ask` value to the conflict list in `settings_page.dart`; pass a `SaveTo.ask` that shows the
  question; show `missing_folder` once using `tell_missing_folder`; run `sweep_parts` on the save folder and the
  staging folder once a day, plus a "delete now" button; carry out `After::action`; SAF and iPhone bookmark
  folders (T2) are platform code.
- **W2:** the same for the desktop window (T8), with a folder dialog.

## Addendum (2026-11-18, issue #8)

`SaveRules.android_tree: Option<AndroidTree { uri, label }>` keeps the Storage Access Framework tree the user
picked on Android. Rust still writes into `folder` (app storage); the phone moves each verified file into the tree
(W1 `SafStore`, ADR-0012). `migrate` drops a URI that is not `content://…`, longer than 2048 bytes or contains
whitespace or control characters, and cuts the label to 64 characters. Older files read as `None`. Test:
`storage::tests::android_tree_is_kept_and_checked`.
