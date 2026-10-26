"""Animated stick-figure preview of decoded Ragnarök clips (in place, 3/4 view)."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import paths
import numpy as np
from PIL import Image, ImageDraw
from anim import skeleton_pose, PARENT, S2J
HERE = os.path.dirname(os.path.abspath(__file__))
b = open(paths.ANM, 'rb').read()
CLIPS = [(0xff610, 'idle'), (0x182fbd0, 'walk 1.5 m/s'), (0x1852180, 'run 3 m/s'), (0x18b41c0, 'sprint 6 m/s')]
# draw every animated joint's bone back to its nearest drawn ancestor, plus the main chains thicker
MAIN = {79, 163, 165, 175, 193, 219, 318, 167, 181, 208, 315, 168, 187, 213, 316, 220, 319, 936, 1064, 1156, 221, 329, 955, 1103, 1203}
used = set(j for j in S2J if j is not None)
# main chains plus the hand / finger chains under each wrist (descendants of 1064 / 1103 at depth <= 4)
DRAW = set(MAIN)
def kids(j): return [k for k in range(len(PARENT)) if PARENT[k] == j]
for wrist in (1064, 1103):
    front = [wrist]
    for depth in range(4):
        nxt = []
        for j in front:
            for k in kids(j):
                if k in used: DRAW.add(k); nxt.append(k)
        front = nxt
SIDE = {}
from anim import LOC
import skel as _sk
_, _, _, W0 = _sk.load(paths.SKELETON)
DRAW = {j for j in DRAW if j in MAIN or min(np.linalg.norm(W0[j][:3, 3] - W0[w][:3, 3]) for w in (1064, 1103)) < 0.22}
for j in DRAW: SIDE[j] = 1 if W0[j][0, 3] > 0.02 else -1 if W0[j][0, 3] < -0.02 else 0
W, H = 300, 380
def proj(p, yaw):
    c, s = np.cos(yaw), np.sin(yaw)
    x = c * p[..., 0] + s * p[..., 2]; z = -s * p[..., 0] + c * p[..., 2]
    return np.stack([W / 2 + x * 150, H - 30 - p[..., 1] * 150 + z * 25], -1)
poses = []
for c, name in CLIPS:
    h, world = skeleton_pose(b, c)
    P = world[:, :, :3, 3]; R = world[:, 5, :3, :3]
    rel = np.einsum('fjk,fkl->fjl', P - world[:, 5:6, :3, 3], R)  # into the root's frame: in place
    poses.append((name, rel, h['frames']))
n = max(p[2] for p in poses) if False else 90
frames = []
for f in range(n):
    img = Image.new('RGB', (W * len(poses), H), (250, 250, 247))
    d = ImageDraw.Draw(img)
    for i, (name, rel, F) in enumerate(poses):
        ox = i * W
        q = proj(rel[f % F], 0.6)
        for gx in np.linspace(-0.8, 0.8, 5):  # ground grid
            a = proj(np.array([[gx, 0, -0.8], [gx, 0, 0.8]]), 0.6); d.line([tuple(a[0] + [ox, 0]), tuple(a[1] + [ox, 0])], fill=(215, 215, 210))
            a = proj(np.array([[-0.8, 0, gx], [0.8, 0, gx]]), 0.6); d.line([tuple(a[0] + [ox, 0]), tuple(a[1] + [ox, 0])], fill=(215, 215, 210))
        for j in sorted(DRAW):
            p = PARENT[j]
            if p not in DRAW and p != 163: continue
            if j not in MAIN and np.linalg.norm(rel[f % F, j] - rel[f % F, p]) > 0.12: continue
            side = SIDE.get(j, 0)
            col = (190, 60, 50) if side > 0 else (50, 90, 180) if side < 0 else (40, 40, 40)
            d.line([tuple(q[p] + [ox, 0]), tuple(q[j] + [ox, 0])], fill=col, width=4 if j in MAIN else 2)
        d.text((ox + 8, 8), name, fill=(20, 20, 20))
    frames.append(img)
out = os.path.join(paths.OUT, 'kratos_motion.gif')
frames[0].save(out, save_all=True, append_images=frames[1:], duration=33, loop=0)
print(out)
