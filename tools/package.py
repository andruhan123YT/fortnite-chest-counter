"""Build the release executable and pack it with its licenses into dist/.

    python tools/package.py            # build, then pack
    python tools/package.py --no-build # pack an existing target/release build

Produces dist/fortnite-chest-counter.exe (the standalone download), its SHA-256 in
dist/fortnite-chest-counter.exe.sha256 (the app checks updates against it) and
dist/fortnite-chest-counter-<version>-windows-x64.zip containing the
executable, the Epic profile script, README.md, LICENSE, NOTICE and
THIRD_PARTY_LICENSES.txt. The third-party
file is generated from `cargo metadata`: every crate compiled into the executable, with
the license files shipped in its source package.
"""

import hashlib
import json
import pathlib
import re
import shutil
import subprocess
import sys
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-msvc"
EXE = "fortnite-chest-counter.exe"
LICENSE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE)", re.IGNORECASE)


def run(cmd):
    return subprocess.run(cmd, cwd=ROOT, check=True, capture_output=True, text=True, encoding="utf-8").stdout


def version():
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    return re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE).group(1)


def third_party_licenses():
    meta = json.loads(run(["cargo", "metadata", "--format-version", "1", "--filter-platform", TARGET]))
    used = {node["id"] for node in meta["resolve"]["nodes"]}
    dev_only = dev_only_packages(meta)
    packages = sorted(
        (p for p in meta["packages"] if p["id"] in used and p["source"] and p["name"] not in dev_only),
        key=lambda p: (p["name"], p["version"]),
    )
    parts = [
        "Third-party licenses\n"
        "====================\n\n"
        f"Fortnite Chest Counter includes the following {len(packages)} Rust crates.\n"
    ]
    for p in packages:
        header = f"{p['name']} {p['version']}  ({p.get('license') or 'see below'})"
        parts.append("\n" + "-" * 78 + "\n" + header + "\n")
        if p.get("repository"):
            parts.append(p["repository"] + "\n")
        folder = pathlib.Path(p["manifest_path"]).parent
        files = sorted(f for f in folder.iterdir() if f.is_file() and LICENSE_FILE.match(f.name))
        if p.get("license_file"):
            extra = folder / p["license_file"]
            if extra.is_file() and extra not in files:
                files.append(extra)
        for f in files:
            parts.append(f"\n[{f.name}]\n\n" + f.read_text(encoding="utf-8", errors="replace").strip() + "\n")
        if not files:
            parts.append("\n(No license file in the package; licensed as stated above.)\n")
    return "".join(parts)


def dev_only_packages(meta):
    """Names of crates reachable only through dev- or build-dependencies of workspace crates."""
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    names = {p["id"]: p["name"] for p in meta["packages"]}
    normal = set()
    stack = list(meta["workspace_members"])
    while stack:
        pid = stack.pop()
        if pid in normal:
            continue
        normal.add(pid)
        for dep in nodes[pid]["deps"]:
            kinds = {k["kind"] for k in dep["dep_kinds"]}
            if None in kinds:
                stack.append(dep["pkg"])
    return {names[pid] for pid in nodes if pid not in normal} - {names[pid] for pid in normal}


def main():
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
    licenses = third_party_licenses()
    (dist / "THIRD_PARTY_LICENSES.txt").write_text(licenses, encoding="utf-8")
    archive = dist / f"fortnite-chest-counter-{version()}-windows-x64.zip"
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as z:
        z.write(exe, EXE)
        for name in ["README.md", "LICENSE", "NOTICE"]:
            z.write(ROOT / name, name)
        scripts = ROOT / "crates" / "app" / "scripts"
        for script in sorted(p for p in scripts.iterdir() if p.suffix in {".bat", ".ps1"}):
            text = script.read_text(encoding="utf-8").replace("\r\n", "\n").replace("\n", "\r\n")
            z.writestr(script.name, text)
        z.writestr("THIRD_PARTY_LICENSES.txt", licenses)
    print(archive)


if __name__ == "__main__":
    main()
