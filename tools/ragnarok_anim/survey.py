import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import paths
import numpy as np, pickle, time
from cado import parse
from dec import decode_group
HERE = os.path.dirname(os.path.abspath(__file__))
b = open(paths.ANM, 'rb').read()
from sent import bases
rows = [(c,) for c in bases(b)]
out = []; t0 = time.time(); fails = 0
KEY = [79, 164, 166, 167, 179, 181, 183, 198, 201]  # hips, spine, thighs, knees, chest, ankles (g1 slots)
for i, r in enumerate(rows):
    c = r[0]
    try:
        h, _, _ = parse(b, c)
        g, res = decode_group(b, c, h, 1)
    except Exception as ex:
        fails += 1; out.append((c, None, str(ex)[:60])); continue
    F = h['frames']; fps = h['fps']
    root = res.get(5, {})
    t = root.get('t'); q = root.get('r')
    disp = (t[-1] - t[0]) if t is not None else np.zeros(3)
    dur = (F - 1) / fps if fps else 0
    yaw = 0.0
    if q is not None:
        def yawof(qq): return 2 * np.arctan2(qq[1], qq[3])
        yaw = np.degrees(((yawof(q[-1]) - yawof(q[0]) + np.pi) % (2 * np.pi)) - np.pi)
    loop = []
    for k in KEY:
        d = res.get(k)
        if d and 'r' in d: loop.append(1 - abs(d['r'][0] @ d['r'][-1]))
    hips = res.get(79, {}).get('t')
    out.append((c, dict(F=F, fps=fps, dur=dur, disp=disp, speed=np.linalg.norm(disp[[0, 2]]) / dur if dur else 0,
                        yaw=yaw, loop=max(loop) if loop else None, hipy=float(hips[:, 1].mean()) if hips is not None else None,
                        nrot=sum(1 for d in res.values() if 'r' in d)), None))
    if i % 200 == 0: print(i, time.time() - t0, flush=True)
pickle.dump(out, open(os.path.join(paths.OUT, 'survey.pkl'), 'wb'))
print('done', len(out), 'fails', fails, time.time() - t0)
