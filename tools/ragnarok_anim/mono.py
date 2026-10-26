import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import paths
import pickle, numpy as np
cands = pickle.load(open(os.path.join(paths.OUT, 'cands.pkl'), 'rb'))
N = len(cands); J = 1458
# DP: f[k][j] = can slots 0..k be assigned increasing joints with slot k -> j (allowing 'none' slots to take any unused joint?)
# Simple greedy check: is there a strictly increasing choice? use DP over sorted candidate lists.
import bisect
best = [None] * N  # best[k] = smallest joint index achievable for slot k given a valid increasing prefix
prev = -1; choice = []
ok = True
# exact DP for existence with minimal last index (greedy is optimal for existence)
for k in range(N):
    c = sorted(cands[k]) if cands[k] else None
    if c is None:
        choice.append(None); continue
    i = bisect.bisect_right(c, prev)
    if i == len(c):
        print('fail at slot', k, 'prev', prev, 'cands', c[:10]); ok = False; break
    prev = c[i]; choice.append(prev)
print('ok', ok)
if ok:
    pickle.dump(choice, open(os.path.join(paths.OUT, 'slot2joint.pkl'), 'wb'))
    skipped = sorted(set(range(J)) - set(x for x in choice if x is not None))
    print('skipped joints', len(skipped), skipped[:80])
    print('none slots', [k for k in range(N) if choice[k] is None])
# latest-feasible assignment (backward greedy)
nxt = 1458; late = [None] * N
for k in range(N - 1, -1, -1):
    c = sorted(cands[k]) if cands[k] else None
    if c is None: continue
    i = bisect.bisect_left(c, nxt) - 1
    assert i >= 0, k
    nxt = c[i]; late[k] = nxt
amb = [k for k in range(N) if choice[k] != late[k]]
print('ambiguous slots', len(amb))
import struct
b = open(paths.SKELETON, 'rb').read()
par = [struct.unpack_from('<4h', b, 0x18 + 8 * i)[3] for i in range(1458)]
runs = []
for k in amb:
    if runs and runs[-1][1] == k - 1: runs[-1][1] = k
    else: runs.append([k, k])
for r in runs[:60]:
    print('slots %d-%d early %s late %s' % (r[0], r[1], choice[r[0]:r[1] + 1][:6], late[r[0]:r[1] + 1][:6]))
pickle.dump((choice, late), open(os.path.join(paths.OUT, 'slot2joint_range.pkl'), 'wb'))
