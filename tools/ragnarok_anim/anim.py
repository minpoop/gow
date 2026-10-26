"""Decode a CADO clip into skeleton poses (work in progress)."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import paths
import numpy as np, pickle
from cado import parse
from dec import decode_group
from quat import qmat
import hado, skel
HERE = os.path.dirname(os.path.abspath(__file__))
_, NSLOT, _, REST = hado.load()
STARTS = [0, 256, 512, 554, 743, 910, 1166, 1268]
choice, late = pickle.load(open(os.path.join(paths.OUT, 'slot2joint_range.pkl'), 'rb'))
NJ, PARENT, LOC, _ = skel.load(paths.SKELETON)
S2J = choice

def slot_pose(b, c, mode=None):
    h, _, _ = parse(b, c)
    F = h['frames']
    s = np.tile(REST[:, 0, :3], (F, 1, 1)).astype(float)
    r = np.tile(REST[:, 1, :], (F, 1, 1)).astype(float)
    t = np.tile(REST[:, 2, :3], (F, 1, 1)).astype(float)
    for gi in range(1, 8):
        g, res = decode_group(b, c, h, gi, mode)
        for j, d in res.items():
            k = STARTS[gi - 1] + j
            if k >= STARTS[gi]: continue  # sentinel / padding
            if 'r' in d: r[:, k] = d['r']
            if 't' in d: t[:, k] = d['t']
            if 's' in d: s[:, k] = d['s']
    return h, s, r, t

def skeleton_pose(b, c, mode=None):
    h, s, r, t = slot_pose(b, c, mode)
    F = h['frames']
    local = np.tile(LOC, (F, 1, 1, 1))
    for k in range(NSLOT):
        j = S2J[k]
        if j is None: continue
        for f in range(F):
            m = np.eye(4)
            m[:3, :3] = qmat(r[f, k] / np.linalg.norm(r[f, k])) * s[f, k]
            m[:3, 3] = t[f, k]
            local[f, j] = m
    world = np.zeros_like(local)
    for j in range(NJ):
        p = PARENT[j]
        world[:, j] = local[:, j] if p < 0 else world[:, p] @ local[:, j]
    return h, world
