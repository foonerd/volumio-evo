#!/usr/bin/env python3
"""Overwrite NETWORK.SMB_* strings in locale files using scripts/data/network-smb-i18n.json.

Leaves strings_en.json unchanged (canonical English lives there)."""

from __future__ import annotations

import json
import sys
from pathlib import Path


def main() -> int:
    repo = Path(__file__).resolve().parents[1]
    data_path = repo / "scripts" / "data" / "network-smb-i18n.json"
    translations: dict[str, dict[str, str]] = json.loads(data_path.read_text(encoding="utf-8"))
    layer = repo / "layer" / "web"
    themes = ["classic", "contemporary", "manifest"]
    smb_keys = list(next(iter(translations.values())).keys())
    updated = 0

    for theme in themes:
        i18n = layer / theme / "app" / "i18n"
        if not i18n.is_dir():
            continue
        for path in sorted(i18n.glob("strings_*.json")):
            lang = path.stem.replace("strings_", "")
            if lang == "en":
                continue
            block = translations.get(lang)
            if not block:
                print(f"missing translations for locale {lang} in {data_path}", file=sys.stderr)
                return 1
            raw = json.loads(path.read_text(encoding="utf-8"))
            net = raw.setdefault("NETWORK", {})
            for k in smb_keys:
                net[k] = block[k]
            path.write_text(json.dumps(raw, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
            updated += 1
            print(path.relative_to(repo))

    print(f"wrote SMB strings in {updated} locale files", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
