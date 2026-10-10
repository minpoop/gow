"""Assemble dist/GowRing-<version>.zip (what Melty installs into {managed}) and dist/package.json.

Contents: GowRing.exe, gowring.dll, tools/ (vgmstream-cli + its DLLs, ISC), assets_map.json, mod/.keep, README + third-party notices.
ModEngine2 is NOT included: Melty installs it. No game files are included, ever.
"""
import hashlib, json, os, shutil, sys, tempfile, urllib.request, zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGET = "x86_64-pc-windows-msvc"
VERSION = "0.1.1"
VGM_URL = "https://github.com/vgmstream/vgmstream/releases/latest/download/vgmstream-win64.zip"
CACHE = os.path.join(ROOT, "target", "package-cache")
DIST = os.path.join(ROOT, "dist")

def vgmstream():
    os.makedirs(CACHE, exist_ok=True)
    z = os.path.join(CACHE, "vgmstream-win64.zip")
    if not os.path.exists(z):
        print("downloading vgmstream from its official release page...")
        urllib.request.urlretrieve(VGM_URL, z)
    out = os.path.join(CACHE, "vgmstream")
    shutil.rmtree(out, ignore_errors=True)
    with zipfile.ZipFile(z) as f:
        f.extractall(out)
    return out

def main():
    launcher = os.path.join(ROOT, "target", TARGET, "release", "GowRing.exe")
    dll = os.path.join(ROOT, "crates", "gowring-dll", "target", TARGET, "release", "gowring.dll")
    for p in (launcher, dll):
        if not os.path.exists(p):
            sys.exit(f"missing build output: {p}")
    stage_root = tempfile.mkdtemp(prefix="gowring-stage-")
    # Everything lives in GowRing/ so it cannot collide with other Elden Ring mashups that share {managed}.
    stage = os.path.join(stage_root, "GowRing")
    os.makedirs(stage)
    def put(src, dst):
        d = os.path.join(stage, dst)
        os.makedirs(os.path.dirname(d), exist_ok=True)
        shutil.copy2(src, d)
    put(launcher, "GowRing.exe")
    put(dll, "gowring.dll")
    vg = vgmstream()
    for name in sorted(os.listdir(vg)):
        if name.lower().endswith(".dll") or name == "vgmstream-cli.exe":
            put(os.path.join(vg, name), f"tools/{name}")
    put(os.path.join(vg, "COPYING"), "tools/vgmstream-COPYING.txt")
    put(os.path.join(ROOT, "README.md"), "README.md")
    put(os.path.join(ROOT, "docs", "STATUS.md"), "STATUS.md")
    with open(os.path.join(stage, "assets_map.json"), "w") as f:
        f.write("{}\n")
    os.makedirs(os.path.join(stage, "mod"), exist_ok=True)
    with open(os.path.join(stage, "mod", ".keep"), "w") as f:
        f.write("ModEngine2 loose-file folder (intentionally empty)\n")
    with open(os.path.join(stage, "THIRD-PARTY.txt"), "w") as f:
        f.write("""Third-party software in this package
=====================================
* vgmstream (tools/) - ISC licence, see tools/vgmstream-COPYING.txt. Converts Wwise audio from your own God of War Ragnarok install. https://github.com/vgmstream/vgmstream
* eldenring / fromsoftware-rs (compiled into gowring.dll) - MIT OR Apache-2.0. https://github.com/vswarte/eldenring-rs
* hudhook, imgui-rs, retour, rodio, png, serde - MIT/Apache-2.0 (compiled into gowring.dll / GowRing.exe)
* The Ragnarok audio-pack layout follows gowr_soundtool by iArtorias (MIT). https://github.com/iArtorias/gowr_soundtool

This package contains no files from Elden Ring or God of War Ragnarok. ModEngine2 (MIT) is installed separately by Melty.
Built with AI (Claude).
""")
    os.makedirs(DIST, exist_ok=True)
    zname = f"GowRing-{VERSION}.zip"
    zpath = os.path.join(DIST, zname)
    if os.path.exists(zpath):
        os.remove(zpath)
    entries = []
    with zipfile.ZipFile(zpath, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for base, dirs, files in os.walk(stage_root):
            dirs.sort()
            for fn in sorted(files):
                full = os.path.join(base, fn)
                rel = os.path.relpath(full, stage_root).replace(os.sep, "/")
                info = zipfile.ZipInfo(rel, date_time=(2026, 10, 10, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = 0o644 << 16
                with open(full, "rb") as fh:
                    z.writestr(info, fh.read())
                entries.append({"path": rel, "size": os.path.getsize(full)})
    sha = hashlib.sha256(open(zpath, "rb").read()).hexdigest()
    meta = {"fileName": zname, "size": os.path.getsize(zpath), "sha256": sha, "entries": entries}
    json.dump(meta, open(os.path.join(DIST, "package.json"), "w"), indent=1)
    shutil.rmtree(stage_root, ignore_errors=True)
    print(f"{zname}: {meta['size']} bytes, sha256 {sha}")
    for e in entries:
        print(f"  {e['size']:>10}  {e['path']}")

main()
