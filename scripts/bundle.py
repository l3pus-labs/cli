#!/usr/bin/env python3
"""Baut das .mcpb-Bundle für Claude Desktop.

**Alles in Python und nichts in der Shell**, und der Grund ist gemessen:
der erste Lauf am 15.09.2026 ist auf Windows an `zip: command not found`
gescheitert. Das Git-bash eines Windows-Läufers bringt kein `zip` mit,
und drei von vier Plattformen hatten da schon veröffentlicht. Ein Werkzeug,
das überall dasselbe tut, ist hier billiger als vier Sonderfälle.

Ein .mcpb ist ein Zip mit einer manifest.json und dem Server darin, wie
eine Chrome-Erweiterung.

    scripts/bundle.py 0.1.2 target/release/rakete rakete-x86_64.mcpb
"""

import json
import pathlib
import shutil
import sys
import tempfile
import zipfile

WURZEL = pathlib.Path(__file__).resolve().parent.parent


def main() -> int:
    if len(sys.argv) != 4:
        print("Aufruf: bundle.py <fassung> <binaer> <zieldatei>", file=sys.stderr)
        return 2

    fassung, binaer, ziel = sys.argv[1], pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3])
    if not binaer.is_file():
        print(f"{binaer} gibt es nicht.", file=sys.stderr)
        return 1

    manifest = json.loads((WURZEL / "bundle" / "manifest.json").read_text(encoding="utf-8"))
    # Die Fassung steht damit an einer Stelle und nicht an zweien: ein
    # Manifest, das 0.1.1 sagt, während das Binär darin 0.1.2 ist, fällt
    # niemandem auf, bis jemand einen Fehler meldet, den es in seiner
    # Fassung nicht gibt.
    manifest["version"] = fassung

    with tempfile.TemporaryDirectory() as tmp:
        bau = pathlib.Path(tmp)
        (bau / "server").mkdir()
        (bau / "manifest.json").write_text(
            json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
        )
        # Der Name im Bundle trägt kein .exe: das Manifest nennt
        # `server/rakete`, und die Apps hängen das .exe auf Windows
        # selbst an.
        shutil.copy2(binaer, bau / "server" / "rakete")
        shutil.copy2(WURZEL / "plugins" / "rakete" / "README.md", bau / "README.md")
        shutil.copy2(WURZEL / "LICENSE", bau / "LICENSE")

        ziel.parent.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(ziel, "w", zipfile.ZIP_DEFLATED) as archiv:
            for datei in sorted(bau.rglob("*")):
                if datei.is_file():
                    eintrag = zipfile.ZipInfo(str(datei.relative_to(bau)).replace("\\", "/"))
                    # **Ein selbst gebauter Eintrag speichert ohne
                    # Kompression**, egal was das Archiv als Vorgabe hat.
                    # Ohne diese Zeile war das Bundle doppelt so groß wie
                    # nötig, und zwar unauffällig: es funktioniert ja.
                    eintrag.compress_type = zipfile.ZIP_DEFLATED
                    # **Das Ausführungsrecht muss mit ins Archiv.** Auf
                    # macOS und Linux packt die App aus und ruft die Datei
                    # auf; ohne das Bit ist sie da und startet nicht.
                    eintrag.external_attr = (0o755 if datei.parent.name == "server" else 0o644) << 16
                    eintrag.date_time = (2026, 1, 1, 0, 0, 0)
                    archiv.writestr(eintrag, datei.read_bytes())

    print(f"{ziel}: {ziel.stat().st_size} Bytes")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
