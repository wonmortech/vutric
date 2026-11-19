# ADR-0063: Settings schema versions and per-device overrides (T9, T3)

Status: accepted (2026-11-18) · Stream: W3 · Backlog: T3, T9 · Builds on: ADR-0007 part 4, ADR-0057

## Context

`Settings` has carried a `version` field since the first release, and `Settings::migrate` brings older files
forward (every field is `#[serde(default)]`, values are clamped). Two gaps remained:

1. **Downgrade loses data.** A user who installs an older Vutric over a newer one gets the newer file read,
   `version` stamped down to ours, and every field we do not know dropped the next time the host saves
   (`vutric-mobile` saves `s.migrate()` after every change). Going back to the newer version then finds those
   settings gone.
2. **`devices` (T3) is unchecked.** Keys are meant to be 64-hex endpoint ids, but a hand-edited or damaged file
   could hold any key, any number of entries, and empty entries; a lookup with an upper-case id missed.

## Decision

**Versions (T9).**

- `SETTINGS_VERSION` goes up only when a field changes meaning or is removed; adding a field with a default does
  not need a bump.
- Each bump adds one step to `Settings::migrate`, keyed on the old `version`, with a test that loads a file of the
  old version.
- `vutric_net::settings::save` reads the file it is about to replace. When that file's `version` is newer than
  ours, our values are merged over it object by object: fields only the newer version knows stay, nested ones
  too, and its version number stays, so the newer app reads its own file back. For everything we know, our value
  wins. An older or same-version file is simply replaced.
- A file that is not JSON is still kept as `.bad` and replaced by the defaults (unchanged).

**Per-device overrides (T3, engine).**

- `Settings::set_device_rules(id, rules)` checks the id (64 hex, stored lower case), removes the entry when all
  overrides are `None`, and refuses past `MAX_DEVICE_RULES` (1024).
- `migrate` drops malformed keys and empty entries, folds keys to lower case and caps the count.
- `rules_for` matches the id in any case.
- The device detail screen that edits these is W1/W2.

## Measured

| Check | Result |
|---|---|
| `vutric-net` `settings::tests::a_newer_file_keeps_what_we_do_not_know`: a version 9 file with top-level and nested unknown fields survives a load, change and save; our change is written; an older file is replaced | pass |
| `vutric-core` `storage::tests::device_rules_are_checked_and_bounded` | pass |
| existing `round_trip_damaged_and_missing`, `migrate_clamps` | pass |

## Consequences

- No public API changed; `set_device_rules` and `MAX_DEVICE_RULES` are additions.
- A host that writes the settings file by another path than `vutric_net::settings::save` loses the downgrade
  protection; all current hosts use it.
