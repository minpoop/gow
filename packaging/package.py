"""Assemble dist/GowRing-<version>.zip (what Melty installs into {managed}) and dist/package.json.

Contents: GowRing.exe, gowring.dll, tools/ (vgmstream-cli + its DLLs, ISC), assets_map.json, mod/.keep, README + third-party notices.
ModEngine2 (official release, MIT) is bundled. No game files are included, ever.
"""
import hashlib, json, os, shutil, subprocess, sys, tempfile, urllib.request, zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGET = "x86_64-pc-windows-msvc"
VERSION = "0.1.46"
ME2_URL = "https://github.com/soulsmods/ModEngine2/releases/download/release-2.1.0/ModEngine-2.1.0.0-win64.zip"
ME2_LICENSE_URL = "https://raw.githubusercontent.com/soulsmods/ModEngine2/main/LICENSE-MIT"
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

def modengine2():
    """The official ModEngine2 release (MIT). Only what starting Elden Ring needs: no debug-menu assets, no headers,
    and not the bundled ScyllaHide anti-debug tool (this mod never touches anti-cheat or anti-debug)."""
    os.makedirs(CACHE, exist_ok=True)
    z = os.path.join(CACHE, "ModEngine-2.1.0.0-win64.zip")
    if not os.path.exists(z):
        print("downloading ModEngine2 from its official release page...")
        urllib.request.urlretrieve(ME2_URL, z)
    out = os.path.join(CACHE, "me2")
    shutil.rmtree(out, ignore_errors=True)
    with zipfile.ZipFile(z) as f:
        f.extractall(out)
    root = os.path.join(out, "ModEngine-2.1.0.0-win64")
    lic = os.path.join(CACHE, "ModEngine2-LICENSE-MIT.txt")
    if not os.path.exists(lic):
        try:
            urllib.request.urlretrieve(ME2_LICENSE_URL, lic)
        except Exception:
            with open(lic, "w") as f:
                f.write("ModEngine2 is MIT licensed. See https://github.com/soulsmods/ModEngine2/blob/main/LICENSE-MIT\n")
    return root, lic

VCR_URL = "https://aka.ms/vs/17/release/vc_redist.x64.exe"
VCR_DLLS = ["concrt140", "msvcp140", "msvcp140_1", "msvcp140_2", "msvcp140_atomic_wait", "msvcp140_codecvt_ids", "vcruntime140", "vcruntime140_1"]

def vc_runtime():
    """The Visual C++ runtime DLLs ModEngine2 is built against, taken from Microsoft's own redistributable
    (app-local deployment is allowed). Not every PC has the newest runtime, and without these ModEngine2 does not start."""
    os.makedirs(CACHE, exist_ok=True)
    exe = os.path.join(CACHE, "vc_redist.x64.exe")
    if not os.path.exists(exe):
        print("downloading the Visual C++ redistributable from Microsoft...")
        urllib.request.urlretrieve(VCR_URL, exe)
    out = os.path.join(CACHE, "vcr")
    shutil.rmtree(out, ignore_errors=True)
    os.makedirs(out)
    subprocess.run(["cabextract", "-s", "-q", "-d", out, exe], check=True)
    found = {}
    for cab in sorted(os.listdir(out)):
        p = os.path.join(out, cab)
        r = subprocess.run(["cabextract", "-q", "-d", os.path.join(out, "dll_" + cab), p], capture_output=True)
        d = os.path.join(out, "dll_" + cab)
        if os.path.isdir(d):
            for f in os.listdir(d):
                if f.endswith(".dll_amd64"):
                    found[f[: -len("_amd64")]] = os.path.join(d, f)
    missing = [n for n in VCR_DLLS if n + ".dll" not in found]
    if missing:
        sys.exit(f"the redistributable did not contain {missing}")
    sha = hashlib.sha256(open(exe, "rb").read()).hexdigest()
    return {n + ".dll": found[n + ".dll"] for n in VCR_DLLS}, sha

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
    me2, me2_lic = modengine2()
    vcr, vcr_sha = vc_runtime()
    for name, src in sorted(vcr.items()):
        # Beside the launcher, and beside modengine2.dll (the game process also gets this folder on its PATH).
        put(src, f"modengine2/{name}")
        put(src, f"modengine2/modengine2/bin/{name}")
    put(os.path.join(me2, "modengine2_launcher.exe"), "modengine2/modengine2_launcher.exe")
    put(os.path.join(me2, "README.txt"), "modengine2/README.txt")
    put(me2_lic, "modengine2/LICENSE-MIT.txt")
    for sub in ("bin", "crashpad"):
        for name in sorted(os.listdir(os.path.join(me2, "modengine2", sub))):
            put(os.path.join(me2, "modengine2", sub, name), f"modengine2/modengine2/{sub}/{name}")
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
* ModEngine2 (modengine2/) - MIT licence, see modengine2/LICENSE-MIT.txt. Loads gowring.dll into Elden Ring offline. https://github.com/soulsmods/ModEngine2
* Microsoft Visual C++ runtime (modengine2/*.dll) - redistributable DLLs from Microsoft's vc_redist.x64.exe (sha256 %s), shipped app-local because ModEngine2 needs them and not every PC has the current version. https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files
* eldenring / fromsoftware-rs (compiled into gowring.dll) - MIT OR Apache-2.0. https://github.com/vswarte/eldenring-rs
* hudhook, imgui-rs, retour, rodio, png, serde - MIT/Apache-2.0 (compiled into gowring.dll / GowRing.exe)
* The Ragnarok audio-pack layout follows gowr_soundtool by iArtorias (MIT). https://github.com/iArtorias/gowr_soundtool

This package contains no files from Elden Ring or God of War Ragnarok. ModEngine2 (MIT) is bundled unmodified from its official release.
Built with AI (Claude).
""".replace("%s", vcr_sha))
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
