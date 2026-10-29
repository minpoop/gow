"""Read gowr_probe's camera-search recordings (camsearch_<n>_proj.bin / _pos.bin) and say where Ragnarök keeps its
camera. Usage: python3 -I camsearch.py camsearch_1_proj.bin [camsearch_1_pos.bin ...]

Per window (memory around a projection or a found position), over all snapshots:
  * projections: the 16-float blocks with the screen's shape (as the probe tests them), and whether they change;
  * rotations: 3x3 blocks (rows 16 or 12 bytes apart) orthonormal in at least 70 % of the snapshots (Ragnarök
    reuses these buffers between frames) that turned, how far, and the translation next to them read several ways
    (stored row / fourth column, each also taken back through the rotation), over those snapshots; only readings
    that stay within 100 m and move are printed;
  * positions (pos files): which 3-float groups move, how far in total, and the box they stay in.
A camera block shows as one rotation that turns with the camera and a translation that traces the same path as a
position found elsewhere."""
import os, struct, sys
import numpy as np


def load(path):
    with open(path, 'rb') as f:
        data = f.read()
    if data[:4] != b'GWCS':
        sys.exit(f'{path}: not a camera-search recording')
    ver, kind, n, win, sw, sh = struct.unpack_from('<6I', data, 4)
    starts = struct.unpack_from(f'<{n}Q', data, 28)
    off = 28 + 8 * n
    rec = 4 + 8 + n + n * win
    snaps = []
    while off + rec <= len(data):
        t, frame = struct.unpack_from('<IQ', data, off)
        ok = data[off + 12:off + 12 + n]
        body = data[off + 12 + n:off + rec]
        snaps.append((t, frame, ok, [body[k * win:(k + 1) * win] for k in range(n)]))
        off += rec
    return dict(kind=kind, n=n, win=win, w=sw, h=sh, starts=starts, snaps=snaps)


def floats(b):
    return np.frombuffer(b, dtype='<f4')


def rot(f, i, stride):
    s = stride // 4
    if i + 2 * s + 3 > len(f):
        return None
    r = np.array([f[i + k * s:i + k * s + 3] for k in range(3)], dtype=np.float64)
    if not np.all(np.isfinite(r)):
        return None
    if np.abs(r @ r.T - np.eye(3)).max() > 2e-3:
        return None
    return r


def angle(a, b):
    return np.degrees(np.arccos(np.clip((np.trace(a @ b.T) - 1) / 2, -1, 1)))


def projections(f, aspect):
    out = []
    for i in range(0, len(f) - 15):
        p = f[i:i + 16]
        if p[15] != 0 or not (abs(abs(p[11]) - 1) < 1e-6 or abs(abs(p[14]) - 1) < 1e-6):
            continue
        sx, sy = p[0], p[5]
        if not (0.2 < sx < 20 and 0.2 < sy < 20) or abs(sy / sx / aspect - 1) > 0.03:
            continue
        if abs(p[1]) > 1e-5 or abs(p[4]) > 1e-5:
            continue
        out.append((i * 4, float(np.degrees(2 * np.arctan(1 / sy))), p.copy()))
    return out


def report_window(d, k):
    snaps = [s for s in d['snaps'] if s[2][k]]
    if len(snaps) < 2:
        print(f'  window {k}: fewer than 2 readable snapshots')
        return
    fs = [floats(s[3][k]) for s in snaps]
    start = d['starts'][k]
    print(f'  window {k} at {start:#x} ({len(snaps)} snapshots, {snaps[0][0] / 1000:.1f}-{snaps[-1][0] / 1000:.1f} s)')
    if d['kind'] == 1:
        for off, fov, p in projections(fs[0], d['w'] / max(d['h'], 1)):
            moved = max(np.abs(f[off // 4:off // 4 + 16] - p).max() for f in fs)
            print(f'    projection at +{off:#x} ({start + off:#x}): fov {fov:.2f} deg, A {p[10]:.6f}, '
                  f'B {p[14] if abs(abs(p[11]) - 1) < 1e-6 else p[11]:.6f}, changes up to {moved:.6f}')
    for stride in (16, 12):
        for i in range(0, len(fs[0]) - 12):
            pairs = [(r, f) for r, f in ((rot(f, i, stride), f) for f in fs) if r is not None]
            if len(pairs) < 0.7 * len(fs):
                continue
            rs = [r for r, _ in pairs]
            turn = max(angle(rs[0], r) for r in rs)
            if turn < 15:
                continue
            reads = {}
            if stride == 16 and i + 15 < len(fs[0]):
                reads['row after'] = [f[i + 12:i + 15] for _, f in pairs]
            if stride == 16:
                reads['4th column'] = [np.array([f[i + 3], f[i + 7], f[i + 11]]) for _, f in pairs]
            if stride == 12 and i + 12 < len(fs[0]):
                reads['row after'] = [f[i + 9:i + 12] for _, f in pairs]
            lines = []
            for name, ts in reads.items():
                for how, conv in (('as stored', lambda r, t: t), ('-R^T t', lambda r, t: -(r.T @ t)), ('-R t', lambda r, t: -(r @ t))):
                    path = np.array([conv(r, np.asarray(t, dtype=np.float64)) for r, t in zip(rs, ts)])
                    if not np.all(np.isfinite(path)):
                        continue
                    span = path.max(0) - path.min(0)
                    length = np.linalg.norm(np.diff(path, axis=0), axis=1).sum()
                    if 0.05 < span.max() < 100:
                        lines.append(f'      {name:10} {how:9}: first ({path[0][0]:.2f}, {path[0][1]:.2f}, {path[0][2]:.2f}), '
                                     f'box {span[0]:.2f} x {span[1]:.2f} x {span[2]:.2f}, path {length:.2f}')
            print(f'    rotation at +{i * 4:#x} ({start + i * 4:#x}), rows {stride} bytes apart, in {len(rs)} snapshots: '
                  f'turned up to {turn:.0f} deg' + ('' if lines else '; no moving position beside it'))
            for l in lines:
                print(l)
    if d['kind'] == 2:
        arr = np.array(fs, dtype=np.float64)
        for i in range(0, arr.shape[1] - 2):
            path = arr[:, i:i + 3]
            if not np.all(np.isfinite(path)) or np.abs(path).max() > 1e6:
                continue
            length = np.linalg.norm(np.diff(path, axis=0), axis=1).sum()
            span = path.max(0) - path.min(0)
            if length > 0.5 and span.max() < 100 and np.abs(path[0]).max() > 0.5:
                print(f'    moving triple at +{i * 4:#x} ({start + i * 4:#x}): first ({path[0][0]:.2f}, {path[0][1]:.2f}, '
                      f'{path[0][2]:.2f}), path {length:.2f}, box {span[0]:.2f} x {span[1]:.2f} x {span[2]:.2f}')


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    for path in sys.argv[1:]:
        d = load(path)
        kind = {1: 'projection windows', 2: 'position windows'}.get(d['kind'], '?')
        print(f'{os.path.basename(path)}: {kind}, {d["n"]} windows of {d["win"]} bytes, {len(d["snaps"])} snapshots, '
              f'screen {d["w"]}x{d["h"]}')
        for k in range(d['n']):
            report_window(d, k)


if __name__ == '__main__':
    main()
