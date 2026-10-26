"""Read gowring.dll's live pose checks (er_pose_live_<n>.bin, 0.1.90 on) against the skeleton. Usage:
python3 -I er_pose_live.py er_skeleton.json er_pose_live_1.bin [...]

Per buffer: the links at rest in each snapshot, the main joints in his own frame and how far they went, the cape
check (its bones' offsets, and their skinning against their parent's, with the cape on and off), and a stick-figure
contact sheet of the pose over the snapshots (er_pose_live_<n>_<buffer>.png in RAGNAROK_ANIM_OUT).

The tests are livepose.rs's: `links_at_rest` and `locate` mirror it (`locate` on a memory image, for testing it
offline), so a dump reads here as it did in the log. The skinning is per bone world x the inverse of the bind
(0.1.89's dumps: the model's bind equals the skeleton's), row-major 3x4."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import json
import numpy as np
from er_pose_find import read_dump_meta, skeleton, matrices
from paths import OUT

ENTRY = 48
MATCH, MIN_LINKS, MAX_BONES = 0.8, 30, 400
JOINTS = ['Pelvis', 'Spine2', 'Head', 'L_UpperArm', 'L_Hand', 'R_UpperArm', 'R_Hand', 'L_Foot', 'R_Foot']
CHAINS = [(['Pelvis', 'Spine', 'Spine1', 'Spine2', 'Neck', 'Head'], (40, 40, 40)),
          (['Spine2', 'L_Clavicle', 'L_UpperArm', 'L_Forearm', 'L_Hand'], (200, 40, 40)),
          (['Spine2', 'R_Clavicle', 'R_UpperArm', 'R_Forearm', 'R_Hand'], (40, 40, 200)),
          (['Pelvis', 'L_Thigh', 'L_Calf', 'L_Foot', 'L_Toe0'], (200, 120, 40)),
          (['Pelvis', 'R_Thigh', 'R_Calf', 'R_Foot', 'R_Toe0'], (40, 160, 200))]


def as4(e):
    m = np.eye(4); m[:3, :4] = np.asarray(e, float).reshape(3, 4); return m


def links_at_rest(y, bind, bones):
    """(at rest, links) over the links whose parent and child entries differ: livepose.rs's test."""
    y = np.asarray(y, float).reshape(-1, 12); at = links = 0
    for i, b in enumerate(bones):
        p = b['parent']
        if p < 0 or p >= i or np.array_equal(y[p], y[i]): continue
        wp = as4(y[p]) @ bind[p]
        if abs(np.linalg.det(wp[:3, :3])) < 1e-9: continue
        links += 1
        loc = np.linalg.inv(wp) @ (as4(y[i]) @ bind[i])
        at += np.linalg.norm(loc[:3, 3] - np.array(b['t'])) < 0.01
    return at, links


def locate(mem, base, offsets, bones, bind):
    """livepose.rs's search on a memory image `mem` starting at address `base`: (skinning address, bone count)."""
    n0 = len(bones)
    for n in range(n0, MAX_BONES + 1):
        at = offsets - n * ENTRY
        if at < base: return None
        y = np.frombuffer(mem, '<f4', count=n0 * 12, offset=at - base)
        if not np.all(np.isfinite(y)): continue
        r, links = links_at_rest(y, bind, bones)
        if links >= MIN_LINKS and r >= MATCH * links: return at, n
    return None


def sheet(world, names, frames, title, out):
    from PIL import Image, ImageDraw
    cell = 170
    img = Image.new('RGB', (cell * len(frames), cell * 2 + 14), 'white'); d = ImageDraw.Draw(img)
    d.text((4, 0), title, fill='black')
    idx = {n: i for i, n in enumerate(names)}
    for c, f in enumerate(frames):
        P = world[f]
        for row, (a, sgn) in enumerate(((2, 1), (0, 1))):  # side view (z across), front view (x across)
            ox, oy, sc = cell * c + cell // 2, 14 + cell * row + cell - 8, cell / 2.2
            d.line([(cell * c, oy), (cell * c + cell, oy)], fill=(180, 180, 180))
            for ch, col in CHAINS:
                pts = [(ox + sgn * P[idx[n], a] * sc, oy - P[idx[n], 1] * sc) for n in ch if n in idx]
                d.line(pts, fill=col, width=2)
            d.text((cell * c + 3, 14 + cell * row + 2), f's{f} ' + ('side' if row == 0 else 'front'), fill='black')
    img.save(out)


def report(skel, path):
    names, parent, local, bind = skeleton(skel)
    bones = json.load(open(skel))['bones']
    arrays, meta = read_dump_meta(path)
    off = meta.get('cape_off', range(0))
    print(f"== {path}: {meta['snapshots']} snapshots over {meta.get('ms', [0])[-1]} ms, cape off in {off.start}-{off.stop - 1}")
    by = {}
    for a in arrays:
        buf, what = a['path'].split(' ', 2)[1], a['path'].split(' ', 2)[2]
        by.setdefault(buf, {})[what] = a
    for buf, g in sorted(by.items()):
        Y, Z = g['skinning']['snaps'], g['offsets']['snaps']          # [K, n, 12]
        K, n = Y.shape[:2]
        good = [k for k in range(K) if np.any(Y[k]) and np.all(np.isfinite(Y[k]))]
        print(f"buffer {buf}: skinning at {g['skinning']['address']:#x}, {n} bones, {len(good)} of {K} snapshots read")
        rest = [links_at_rest(Y[k, :len(bones)], bind, bones) for k in good]
        print(f"  links at rest: {min(r[0] for r in rest)}..{max(r[0] for r in rest)} of {max(r[1] for r in rest)} differing")
        S = np.array([[as4(e) for e in Y[k, :len(bones)]] for k in good])
        W = np.einsum('knab,nbc->knac', S, bind)
        # joints in model space, the bind's: Master's skinning undone (its bind turns it 90 degrees, so not its world)
        own = np.einsum('kab,knbc->knac', np.linalg.inv(S[:, 0]), W)[:, :, :3, 3]
        travel = np.linalg.norm(W[:, 0, :3, 3] - W[0, 0, :3, 3], axis=1).max()
        print(f"  the root (Master) went up to {travel:.2f} m")
        for nm in JOINTS:
            j = names.index(nm); p0 = own[0, j]
            print(f"  {nm:11s} {np.round(p0, 2).tolist()}  {np.linalg.norm(p0 - bind[j][:3, 3]):.2f} m off bind, went {np.linalg.norm(own[:, j] - p0, axis=1).max():.2f} m")
        on = [i for i, k in enumerate(good) if good[i] not in off]
        offk = [i for i, k in enumerate(good) if good[i] in off]
        chain = [j for j in range(1, len(bones)) if any(np.abs(Z[good[i], j] - np.eye(4)[:3].ravel()).max() > 1e-4 for i in on)]
        for j in chain:
            p = parent[j]
            def dev(ks):
                o = max((np.abs(Z[good[i], j] - np.eye(4)[:3].ravel()).max() for i in ks), default=0)
                ang = max((np.degrees(np.arccos(np.clip((np.trace(as4(Z[good[i], j])[:3, :3]) - 1) / 2, -1, 1))) for i in ks), default=0)
                y = max((np.abs(np.linalg.inv(as4(Y[good[i], p])) @ as4(Y[good[i], j]) - np.eye(4))[:3].max() for i in ks), default=0)
                return o, ang, y
            o1, a1, y1 = dev(on); o0, a0, y0 = dev(offk)
            print(f"  cape bone {names[j]}: offset up to {a1:.1f} deg ({o1:.4f}) on, {a0:.1f} deg ({o0:.4f}) off; skinning off its parent's by {y1:.4f} on, {y0:.4f} off")
        frames = list(range(0, len(good), max(1, len(good) // 8)))[:8]
        out = os.path.join(OUT, os.path.basename(path).replace('.bin', f'_{buf}.png'))
        sheet(own, names, frames, f"{os.path.basename(path)} buffer {buf}: model space", out)
        print(f"  contact sheet: {out}")


if __name__ == '__main__':
    for p in sys.argv[2:]:
        report(sys.argv[1], p)
