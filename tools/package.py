"""Build the release executable and put the release files into dist/.

    python tools/package.py            # build, then pack
    python tools/package.py --no-build # pack an existing target/release build

Produces dist/fortnite-chest-counter.exe (the download) and its SHA-256 in
dist/fortnite-chest-counter.exe.sha256 (the app checks updates against it).

The licenses of the crates compiled into the executable, which ask for their text to go
along with it, are built into the app (About > Show licenses) from
crates/app/assets/licenses.json, generated from `cargo metadata` and the license files
each crate ships. It is regenerated here; if it changed, the script stops so the new
file gets committed before a release.

    python tools/package.py --licenses # only rewrite crates/app/assets/licenses.json
"""

import hashlib
import json
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-msvc"
EXE = "fortnite-chest-counter.exe"
LICENSE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE)", re.IGNORECASE)


def run(cmd):
    return subprocess.run(cmd, cwd=ROOT, check=True, capture_output=True, text=True, encoding="utf-8").stdout


LICENSES_ASSET = ROOT / "crates" / "app" / "assets" / "licenses.json"


def shipped_crates():
    meta = json.loads(run(["cargo", "metadata", "--format-version", "1", "--filter-platform", TARGET]))
    shipped = shipped_packages(meta, "fortnite-chest-counter")
    return sorted(
        (p for p in meta["packages"] if p["id"] in shipped and p["source"]),
        key=lambda p: (p["name"], p["version"]),
    )


def license_files(p):
    folder = pathlib.Path(p["manifest_path"]).parent
    files = sorted(f for f in folder.iterdir() if f.is_file() and LICENSE_FILE.match(f.name))
    if p.get("license_file"):
        extra = folder / p["license_file"]
        if extra.is_file() and extra not in files:
            files.append(extra)
    return files


def licenses_asset(packages):
    """The list for the app: each crate with indexes into `texts`, where every distinct
    license text is stored once."""
    texts, index, crates = [], {}, []
    for p in packages:
        ids = []
        for f in license_files(p):
            text = f.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n").strip()
            if text not in index:
                index[text] = len(texts)
                texts.append(text)
            ids.append(index[text])
        crates.append({
            "name": p["name"],
            "version": p["version"],
            "license": p.get("license") or "",
            "repository": p.get("repository") or "",
            "texts": ids,
        })
    return json.dumps({"crates": crates, "texts": texts}, ensure_ascii=False, separators=(",", ":")) + "\n"


def write_licenses_asset(packages):
    """Rewrites the app's list. Returns True if it changed."""
    new = licenses_asset(packages)
    old = LICENSES_ASSET.read_text(encoding="utf-8") if LICENSES_ASSET.is_file() else ""
    if new == old:
        return False
    LICENSES_ASSET.write_text(new, encoding="utf-8", newline="\n")
    return True


def shipped_packages(meta, root_name):
    """Ids of the crates compiled into `root_name`: its normal dependencies, transitively.
    Dev- and build-dependencies, and other workspace members such as the sync server, are
    left out."""
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    names = {p["id"]: p["name"] for p in meta["packages"]}
    root = next(pid for pid in meta["workspace_members"] if names[pid] == root_name)
    shipped = set()
    stack = [root]
    while stack:
        pid = stack.pop()
        if pid in shipped:
            continue
        shipped.add(pid)
        for dep in nodes[pid]["deps"]:
            if None in {k["kind"] for k in dep["dep_kinds"]}:
                stack.append(dep["pkg"])
    return shipped


def main():
    if "--licenses" in sys.argv:
        changed = write_licenses_asset(shipped_crates())
        print(LICENSES_ASSET, "updated" if changed else "unchanged")
        return
    if "--no-build" not in sys.argv:
        subprocess.run(["cargo", "build", "--release", "-p", "fortnite-chest-counter"], cwd=ROOT, check=True)
    exe = ROOT / "target" / "release" / EXE
    if not exe.is_file():
        sys.exit(f"{exe} not found; build first")
    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    shutil.copy2(exe, dist / EXE)
    digest = hashlib.sha256((dist / EXE).read_bytes()).hexdigest()
    (dist / f"{EXE}.sha256").write_text(f"{digest}  {EXE}\n", encoding="ascii", newline="\n")
    print(dist / EXE)
    if write_licenses_asset(shipped_crates()):
        sys.exit(f"{LICENSES_ASSET} was out of date and is rewritten: commit it and build again")


if __name__ == "__main__":
    main()
