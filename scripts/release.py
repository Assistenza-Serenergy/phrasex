"""Publishes the built installer as a GitHub Release for the auto-update (run scripts/build.sh first).
Usage: python scripts/release.py "short notes"   -- outward-facing: only when the user said yes."""
import datetime, json, pathlib, shutil, subprocess, sys

root = pathlib.Path(__file__).resolve().parent.parent
version = json.loads((root / "app/src-tauri/tauri.conf.json").read_text())["version"]
setup = root / f"target/release/bundle/nsis/Phrase X_{version}_x64-setup.exe"
sig = setup.with_name(setup.name + ".sig")
asset = f"PhraseX_{version}_x64-setup.exe"  # no spaces in the download URL
url = f"https://github.com/Assistenza-Serenergy/phrasex/releases/download/v{version}/{asset}"
latest = {
    "version": version,
    "notes": sys.argv[1] if len(sys.argv) > 1 else "",
    "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "platforms": {"windows-x86_64": {"signature": sig.read_text().strip(), "url": url}},
}
out = root / "target/release/latest.json"
out.write_text(json.dumps(latest, indent=2))
upload = out.with_name(asset)  # gh keeps the file's own name ("#label" only changes the label)
shutil.copyfile(setup, upload)
subprocess.run(["gh", "release", "create", f"v{version}", str(upload), str(out),
                "--repo", "Assistenza-Serenergy/phrasex", "--title", f"Phrase X {version}", "--notes", latest["notes"]], check=True)
