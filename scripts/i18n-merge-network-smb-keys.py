#!/usr/bin/env python3
"""Merge NETWORK.SMB_* keys from strings_en.json into every strings_*.json per theme.

Uses English strings as placeholders for locales missing those keys. After merging, run
``scripts/i18n-apply-network-smb-translations.py`` so ``scripts/data/network-smb-i18n.json``
overwrites those keys with real translations."""
from __future__ import annotations

import json
import sys
from pathlib import Path


def main() -> int:
    repo = Path(__file__).resolve().parents[1]
    layer = repo / "layer" / "web"
    themes = ["classic", "contemporary", "manifest"]
    changed = 0
    for theme in themes:
        i18n = layer / theme / "app" / "i18n"
        en_path = i18n / "strings_en.json"
        if not en_path.is_file():
            print(f"skip missing {en_path}", file=sys.stderr)
            continue
        en_data = json.loads(en_path.read_text(encoding="utf-8"))
        net_en = en_data.get("NETWORK") or {}
        smb_kv = {k: v for k, v in net_en.items() if k.startswith("SMB_")}
        if not smb_kv:
            print(f"no SMB_* keys under NETWORK in {en_path}", file=sys.stderr)
            continue

        for path in sorted(i18n.glob("strings_*.json")):
            raw = path.read_text(encoding="utf-8")
            data = json.loads(raw)
            net = data.setdefault("NETWORK", {})
            updated = False
            for k, v in smb_kv.items():
                if k not in net:
                    net[k] = v
                    updated = True
            if updated:
                path.write_text(
                    json.dumps(data, ensure_ascii=False, indent=2) + "\n",
                    encoding="utf-8",
                )
                changed += 1
                print(path.relative_to(repo))
    print(f"updated {changed} files", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
