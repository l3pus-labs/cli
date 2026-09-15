#!/usr/bin/env python3
"""Schreibt die manifest.json des Bundles mit der Fassung aus Cargo.toml.

Die Fassung steht damit an einer Stelle und nicht an zweien: ein Manifest,
das 0.1.1 sagt, während das Binär darin 0.1.2 ist, fällt niemandem auf,
bis jemand einen Fehler meldet, den es in seiner Fassung nicht gibt.
"""

import json
import pathlib
import sys

if len(sys.argv) != 3:
    print("Aufruf: bundle-manifest.py <fassung> <zieldatei>", file=sys.stderr)
    raise SystemExit(2)

fassung, ziel = sys.argv[1], pathlib.Path(sys.argv[2])
quelle = pathlib.Path(__file__).resolve().parent.parent / "bundle" / "manifest.json"

manifest = json.loads(quelle.read_text(encoding="utf-8"))
manifest["version"] = fassung
ziel.parent.mkdir(parents=True, exist_ok=True)
ziel.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
print(f"{ziel}: Fassung {fassung}")
