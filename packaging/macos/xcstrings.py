#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
# SPDX-License-Identifier: Apache-2.0
"""Compiles String Catalogs (.xcstrings) into <lang>.lproj/<Name>.strings without Xcode's build system.

    xcstrings.py OUT_DIR CATALOG.xcstrings [CATALOG.xcstrings ...]
    xcstrings.py --check CATALOG.xcstrings [...]

Every key must have a translated value in every language the catalog uses; a missing one fails the build, so no
key ever shows up raw in the UI. `--check` validates without writing.
"""

import json
import pathlib
import sys


def escape(value: str) -> str:
    return value.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")


def load(path: pathlib.Path) -> tuple[str, dict[str, dict[str, str]]]:
    data = json.loads(path.read_text(encoding="utf-8"))
    source = data["sourceLanguage"]
    table: dict[str, dict[str, str]] = {}
    langs: set[str] = {source}
    for key, entry in data["strings"].items():
        for lang, loc in entry.get("localizations", {}).items():
            unit = loc.get("stringUnit")
            if not unit or unit.get("state") != "translated":
                raise SystemExit(f"{path}: {key!r} [{lang}] is not translated")
            table.setdefault(lang, {})[key] = unit["value"]
            langs.add(lang)
    for lang in langs:
        missing = sorted(set(data["strings"]) - set(table.get(lang, {})))
        if missing:
            raise SystemExit(f"{path}: [{lang}] misses {', '.join(missing)}")
    return source, table


def main(argv: list[str]) -> int:
    check = argv[:1] == ["--check"]
    if check:
        catalogs = argv[1:]
        out = None
    else:
        if len(argv) < 2:
            print(__doc__)
            return 2
        out = pathlib.Path(argv[0])
        catalogs = argv[1:]
    for c in map(pathlib.Path, catalogs):
        _, table = load(c)
        for lang, strings in table.items():
            if out is None:
                continue
            d = out / f"{lang}.lproj"
            d.mkdir(parents=True, exist_ok=True)
            body = "".join(f'"{escape(k)}" = "{escape(v)}";\n' for k, v in sorted(strings.items()))
            (d / f"{c.stem}.strings").write_text(body, encoding="utf-8")
        print(f"{c}: {len(table)} languages, {max(len(s) for s in table.values())} keys")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
