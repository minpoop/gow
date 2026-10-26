import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import paths
import numpy as np, pickle
import hado, skel
from quat import mat2quat
b, nslot, groups, rest = hado.load()
n, parent, loc, world = skel.load(paths.SKELETON)
jq = np.array([mat2quat(loc[j]) for j in range(n)])
jt = np.array([loc[j][:3, 3] for j in range(n)])
js = np.array([np.linalg.norm(loc[j][:3, :3], axis=0) for j in range(n)])
cands = []
for k in range(nslot):
    s, q, t = rest[k][0][:3], rest[k][1], rest[k][2][:3]
    dq = np.minimum(np.abs(jq - q).max(1), np.abs(jq + q).max(1))
    dt = np.abs(jt - t).max(1)
    ok = np.where((dq < 2e-3) & (dt < 2e-3))[0]
    cands.append(ok.tolist())
cnt = [len(c) for c in cands]
print('unique', sum(1 for c in cnt if c == 1), 'none', sum(1 for c in cnt if c == 0), 'multi', sum(1 for c in cnt if c > 1))
pickle.dump(cands, open(os.path.join(paths.OUT, 'cands.pkl'), 'wb'))
for k in list(range(0, 12)) + [79, 163, 164, 165, 166, 167, 168, 175, 181, 187, 193, 219, 220]:
    print(k, cands[k][:12], len(cands[k]))
