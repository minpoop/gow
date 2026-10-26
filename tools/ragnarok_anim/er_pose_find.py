"""Read gowring.dll's pose search dumps (er_pose_find_<n>.bin) and work out which array is Elden Ring's animated pose
and in what bone order. Usage: python3 -I er_pose_find.py er_skeleton.json er_pose_find_1.bin [...]

For each array that moved: how it reads in the skeleton's own order (as local, model-space or skinning transforms),
then, for model-space and skinning reads, the bone order recovered by walking the skeleton from its root: a child's
joint must sit where its parent's transform puts it in every snapshot (model space: parent x the child's rest offset
= the child's translation; skinning: the parent's and the child's matrices take the child's rest joint to the same
place)."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import json, struct
import numpy as np

LAYOUTS = ['4x4', '3x4', '4x3', 'hkQsTransform']


def qmat(q):
    x, y, z, w = q
    return np.array([[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
                     [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                     [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]])


def skeleton(path):
    bones = json.load(open(path))['bones']
    parent = [b['parent'] for b in bones]
    local = []
    for b in bones:
        m = np.eye(4); m[:3, :3] = qmat(b['r']) * np.array(b['s']); m[:3, 3] = b['t']; local.append(m)
    bind = []
    for i, m in enumerate(local):
        bind.append(m if parent[i] < 0 else bind[parent[i]] @ m)
    return [b['name'] for b in bones], parent, np.array(local), np.array(bind)


def read_dump(path):
    return read_dump_meta(path)[0]


def read_dump_meta(path):
    """(arrays, meta). Version 2 (er_pose_live, 0.1.90 on) adds after the header the first and last+1 snapshot taken
    with the cape off and each snapshot's ms since the first."""
    b = open(path, 'rb').read()
    assert b[:4] == b'GRPF', b[:4]
    ver, n, snaps = struct.unpack_from('<3I', b, 4); o = 16
    meta = dict(version=ver, snapshots=snaps)
    if ver >= 2:
        off0, off1 = struct.unpack_from('<2I', b, o); o += 8
        meta['cape_off'] = range(off0, off1)
        meta['ms'] = list(struct.unpack_from(f'<{snaps}I', b, o)); o += 4 * snaps
    out = []
    for _ in range(n):
        ln, = struct.unpack_from('<I', b, o); o += 4
        name = b[o:o + ln].decode(); o += ln
        addr, = struct.unpack_from('<Q', b, o); o += 8
        layout, count, stride, kept = struct.unpack_from('<4I', b, o); o += 16
        f = np.frombuffer(b, '<f4', count=kept * count * stride, offset=o).reshape(kept, count, stride).astype(float)
        o += kept * count * stride * 4
        out.append(dict(path=name, address=addr, layout=LAYOUTS[layout], count=count, snaps=f))
    return out, meta


def matrices(a):
    """[snapshot, entry, 4, 4] from an array's floats (matrices as stored: row-major 3x4, 4x3 = 3x3 rows then the
    translation, 4x4 tried with the translation in the last row (column-major) and last column)."""
    f = a['snaps']; K, N, S = f.shape
    M = np.tile(np.eye(4), (K, N, 1, 1))
    if a['layout'] == '3x4':
        M[:, :, :3, :4] = f.reshape(K, N, 3, 4)
    elif a['layout'] == '4x3':
        M[:, :, :3, :3] = f[:, :, :9].reshape(K, N, 3, 3); M[:, :, :3, 3] = f[:, :, 9:12]
    elif a['layout'] == '4x4':
        g = f.reshape(K, N, 4, 4)
        if np.abs(g[:, :, 3, :3]).sum() > np.abs(g[:, :, :3, 3]).sum():  # translation in the last row: column-major
            g = np.swapaxes(g, 2, 3)
        M[:] = g; M[:, :, 3] = [0, 0, 0, 1]
    else:  # hkQsTransform: translation, quaternion, scale
        for k in range(K):
            for e in range(N):
                t, q, s = f[k, e, 0:3], f[k, e, 4:8], f[k, e, 8:11]
                M[k, e, :3, :3] = qmat(q / np.linalg.norm(q)) * s; M[k, e, :3, 3] = t
    return M


def in_order(M, parent, local, bind, tol=0.01):
    """links that read right in the skeleton's order: as local, model-space, skinning transforms"""
    n = min(M.shape[1], len(parent)); res = {}
    lt = local[:, :3, 3]
    res['local'] = sum(1 for i in range(n) if parent[i] >= 0 and np.all(np.linalg.norm(M[:, i, :3, 3] - lt[i], axis=1) < tol))
    res['model'] = sum(1 for i in range(n) if parent[i] >= 0 and parent[i] < n and np.all(
        np.linalg.norm(np.einsum('kab,b->ka', M[:, parent[i]], np.r_[lt[i], 1])[:, :3] - M[:, i, :3, 3], axis=1) < tol))
    res['skinning'] = sum(1 for i in range(n) if parent[i] >= 0 and parent[i] < n and np.all(
        np.linalg.norm(np.einsum('kab,b->ka', M[:, parent[i]], np.r_[bind[i][:3, 3], 1]) - np.einsum('kab,b->ka', M[:, i], np.r_[bind[i][:3, 3], 1]), axis=1) < tol))
    return res


def recover(M, parent, local, bind, kind, tol=0.01):
    """bone -> entry. Each child's candidates are the entries that put its joint where its parent's entry does in every
    snapshot; siblings at the same spot are told apart by how much of their subtree each candidate explains (scored
    bottom-up, then assigned top-down without reusing an entry)."""
    K, N = M.shape[:2]; nb = len(parent)
    children = [[] for _ in range(nb)]
    for i, p in enumerate(parent):
        if p >= 0: children[p].append(i)
    lt = local[:, :3, 3]
    if kind == 'model':
        at = lambda b: M[:, :, :3, 3]                                           # [K, N, 3] each entry as bone b
        put = lambda ep, b: np.einsum('kab,b->ka', M[:, ep], np.r_[lt[b], 1])[:, :3]
    else:
        at = lambda b: np.einsum('knab,b->kna', M, np.r_[bind[b][:3, 3], 1])[:, :, :3]
        put = lambda ep, b: np.einsum('kab,b->ka', M[:, ep], np.r_[bind[b][:3, 3], 1])[:, :3]
    AT = [at(b) for b in range(nb)] if kind != 'model' else None
    POS = M[:, :, :3, 3]
    # entries that cannot be told apart (exact copies, padding identities) would let one entry stand for many bones
    flat = M.reshape(K, N, 16).transpose(1, 0, 2).reshape(N, -1)
    _, first = np.unique(np.round(flat, 6), axis=0, return_index=True)
    usable = np.zeros(N, bool); usable[first] = True
    usable &= ~np.all(np.abs(M - np.eye(4)) < 1e-6, axis=(0, 2, 3))
    def cands(ep, b):
        tg = put(ep, b)
        d = np.linalg.norm((POS if kind == 'model' else AT[b]) - tg[:, None, :], axis=2).max(0)
        return [int(e) for e in np.where((d < tol) & usable)[0]]
    memo = {}
    def score(b, e):
        key = (b, e)
        if key in memo: return memo[key]
        memo[key] = 1  # guards cycles
        total = 1; pick = {}
        for c in children[b]:
            best = (0, None)
            for ec in cands(e, c):
                if ec == e: continue
                sc = score(c, ec)[0]
                if sc > best[0]: best = (sc, ec)
            total += best[0]; pick[c] = best[1]
        memo[key] = (total, pick)
        return memo[key]
    root = parent.index(-1)
    best = (0, None)
    for er in np.where(usable)[0]:
        sc = score(root, int(er))
        if sc[0] > best[0]: best = (sc[0], int(er))
    if best[1] is None: return 0, {}
    assign = {}; used = set()
    def place(b, e):
        if e is None or e in used: return
        assign[b] = e; used.add(e)
        for c, ec in score(b, e)[1].items(): place(c, ec)
    place(root, best[1])
    return len(assign), assign


def main():
    names, parent, local, bind = skeleton(sys.argv[1])
    for path in sys.argv[2:]:
        print('==', path)
        for a in read_dump(path):
            f = a['snaps']
            moved = int((np.abs(f - f[:1]).max(axis=(0, 2)) > 1e-4).sum()) if len(f) > 1 else 0
            if moved == 0: continue
            M = matrices(a)
            io = in_order(M, parent, local, bind)
            line = f"{a['path']} {a['address']:#x} {a['count']} x {a['layout']}, {moved} moved; in order {io}"
            if a['layout'] != 'hkQsTransform':
                for kind in ('model', 'skinning'):
                    n, assign = recover(M, parent, local, bind, kind)
                    if n > 20:
                        shifts = {}
                        for b_, e_ in assign.items(): shifts[e_ - b_] = shifts.get(e_ - b_, 0) + 1
                        sh, cnt = max(shifts.items(), key=lambda x: x[1])
                        main = [names.index(x) for x in ('Pelvis', 'Spine', 'Neck', 'Head', 'L_UpperArm', 'R_UpperArm', 'L_Thigh', 'R_Thigh') if x in names]
                        line += (f"; {kind} walk places {n}/{len(parent)} bones, {cnt} of them at entry = bone {sh:+d}; "
                                 + ', '.join(f"{names[b_]} -> {assign.get(b_, '?')}" for b_ in main))
            print(line)


if __name__ == '__main__':
    main()
