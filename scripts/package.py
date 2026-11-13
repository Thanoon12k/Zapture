#!/usr/bin/env python3
"""Package a native release with documentation and dependency license notices."""
import hashlib
import json
import pathlib
import platform
import shutil
import subprocess
import tarfile
import tomllib
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
SYSTEM = platform.system()
HOST = next(line.split(": ", 1)[1] for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
ARCH = "arm64" if HOST.startswith("aarch64") else "x64" if HOST.startswith("x86_64") else HOST.split("-")[0]
OS_NAME = {"Windows": "Windows", "Linux": "Linux", "Darwin": "macOS"}[SYSTEM]
NAME = f"Zapture-v{VERSION}-{OS_NAME}-{ARCH}"
DIST = ROOT / "dist"
STAGE = DIST / NAME
if STAGE.exists():
    shutil.rmtree(STAGE)
STAGE.mkdir(parents=True)
for file in ["README.md", "LICENSE", "THIRD_PARTY.md"]:
    shutil.copy2(ROOT / file, STAGE / file)
for folder in ["docs", "examples"]:
    shutil.copytree(ROOT / folder, STAGE / folder)
exe = ROOT / "target" / "release" / ("zapture.exe" if SYSTEM == "Windows" else "zapture")
if not exe.is_file():
    raise SystemExit("Build the native release executable first")
if SYSTEM == "Darwin":
    app = STAGE / "Zapture.app"
    binary = app / "Contents" / "MacOS" / "Zapture"
    binary.parent.mkdir(parents=True)
    shutil.copy2(exe, binary)
    plist = (ROOT / "packaging/macos/Info.plist").read_text().replace("<string>0.1.0</string>", f"<string>{VERSION.split(chr(45))[0]}</string>")
    (app / "Contents/Info.plist").write_text(plist)
    subprocess.run(["codesign", "--force", "--deep", "--sign", "-", str(app)], check=True)
else:
    shutil.copy2(exe, STAGE / ("Zapture.exe" if SYSTEM == "Windows" else "zapture"))
metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", HOST], cwd=ROOT, text=True))
resolved = {node["id"] for node in metadata["resolve"]["nodes"]}
licenses = STAGE / "licenses"
licenses.mkdir()
inventory = []
for package in metadata["packages"]:
    if package["id"] not in resolved or package["name"] == "zapture":
        continue
    base = pathlib.Path(package["manifest_path"]).parent
    copied = []
    for file in base.rglob("*"):
        if not file.is_file():
            continue
        lower = file.name.lower()
        if lower.startswith(("license", "licence", "copying", "notice")):
            dest = licenses / f'{package["name"]}-{package["version"]}' / file.relative_to(base)
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(file, dest)
            copied.append(str(dest.relative_to(STAGE)))
    inventory.append({"name": package["name"], "version": package["version"], "license": package.get("license"), "repository": package.get("repository"), "included_notices": copied})
(licenses / "inventory.json").write_text(json.dumps(inventory, indent=2) + "\n")
if SYSTEM == "Windows":
    archive = DIST / f"{NAME}.zip"
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as z:
        for file in STAGE.rglob("*"):
            if file.is_file():
                z.write(file, f"{NAME}/{file.relative_to(STAGE).as_posix()}")
else:
    archive = DIST / f"{NAME}.tar.gz"
    with tarfile.open(archive, "w:gz") as tar:
        tar.add(STAGE, arcname=NAME)
(archive.with_name(archive.name + ".sha256")).write_text(hashlib.sha256(archive.read_bytes()).hexdigest() + "  " + archive.name + "\n")
print(archive)
