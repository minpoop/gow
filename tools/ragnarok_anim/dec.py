"""CADO decoder (work in progress)."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct, numpy as np
from cado import parse
from groups import records
from bits import Bits, popmask

def sf(v, w):  # signed truncated f32 (top w bits)
    return struct.unpack('<f', struct.pack('<I', (v << (32 - w)) & 0xffffffff))[0]
def uf(v, w):  # unsigned truncated f32 (no sign bit)
    return struct.unpack('<f', struct.pack('<I', (v << (31 - w)) & 0xffffffff))[0]
def fields(q, fn):
    return [fn(q & 0x1fffff, 21), fn((q >> 21) & 0x1fffff, 21), fn((q >> 42) & 0x3fffff, 22)]

def even(n): return n + (n & 1)

class Section:
    """One run of animated tracks: widths (0d), ranges (1b), rotation records (1c/1d/1e), lists (12/13)."""
    def __init__(self):
        self.trans, self.rots = [], []
        self.widths = []; self.ranges = []; self.splits = []
        self.c_splits = []; self.d_recs = []; self.e_recs = []
        self.nd = self.ne = self.nc = 0
        self.order = {}  # rotation category -> its scratch offset (the categories run in that order)
        self.b_start = 0; self.t_start = 0

KNOWN = {0x0d, 0x12, 0x13, 0x15, 0x16, 0x17, 0x18, 0x1b, 0x1c, 0x1d, 0x1e}

class Group:
    def __init__(self, b, gb):
        self.recs = records(b, gb)
        self.b, self.gb = b, gb
        self.const_s, self.const_r = [], []
        self.sections = []
        sec = None
        self.unsupported = sorted({hex(t) for t, k, f in self.recs if t not in KNOWN})
        if self.unsupported: return  # e.g. 2-frame pose clips with records 0b / 0f / 10 / 1a: not decoded yet
        for t, k, f in self.recs:
            n = f[0] & 0x7fff; o = gb + f[2]
            if t == 0x16: self.const_s = list(struct.unpack_from('<%dH' % n, b, o))
            elif t == 0x18: self.const_r = list(struct.unpack_from('<%dH' % n, b, o))
            elif t == 0x0d:
                sec = Section(); self.sections.append(sec)
                sec.widths = list(b[o:o + n]) if f[0] >> 15 else [b[o]] * n  # flag clear: one shared width
            elif t == 0x12: sec.trans = list(struct.unpack_from('<%dH' % n, b, o)); sec.t_start = f[3]
            elif t == 0x13: sec.rots = list(struct.unpack_from('<%dH' % n, b, o))
            elif t == 0x1b:
                sec.b_start = f[3]  # scratch offset of the first ranged track
                m = n if f[0] >> 15 else 1  # flag clear: one range and split shared by all its tracks
                for i in range(m):
                    lo, hi = struct.unpack_from('<QQ', b, o + 16 * i)
                    sec.ranges.append((fields(hi, sf), fields(lo, uf)))
                for i in range(m):
                    sec.splits.append(tuple(b[o + 16 * m + 4 * i:o + 16 * m + 4 * i + 4]))
                if m == 1: sec.ranges *= n; sec.splits *= n
            elif t == 0x1c:
                sec.nc = n; sec.order['c'] = f[3]
                per = f[0] >> 15  # flag: one split per track, else one shared split
                sec.c_splits = [tuple(b[o + 4 * i * per:o + 4 * i * per + 4]) for i in range(n)]
            elif t == 0x1d:
                sec.nd = n; sec.order['d'] = f[3]
                blk = (n + 1) // 2  # two tracks per 16-byte block, components interleaved
                raw = np.array(struct.unpack_from('<%dh' % (8 * blk), b, o), float).reshape(blk, 8)
                sec.d_recs = [raw[i // 2][(i & 1)::2] / 32768 for i in range(n)]
            elif t == 0x1e:
                sec.ne = n; sec.order['e'] = f[3]
                raw = [np.array(struct.unpack_from('<8h', b, o + 16 * i), float) for i in range(n)]
                sec.e_recs = [(r[0::2] / 32768, r[1::2] / 32768) for r in raw]

    def tracks(self):
        """(kind, joint, slot, section, record index) in bitstream order.
        Every track owns a 32-byte scratch slot; each category record (12 / 1c / 1d / 1e) gives its first slot,
        categories run in slot order and an odd one leaves a pad slot (a sentinel in the 13 list between
        rotation categories). 1b ranges start at the 1b record's slot: translations before it are raw f32 x3,
        and rotations left over after the categories are raw f32 x4."""
        out = []
        for si, sec in enumerate(self.sections):
            cats = [(sec.t_start, 't', len(sec.trans))] if sec.trans else []
            cats += [(sec.order[k], k, n) for k, n in (('d', sec.nd), ('e', sec.ne), ('c', sec.nc)) if n]
            cats.sort()
            pos = 0; p = 0; r = sec.rots
            def ranged(at): return (at - sec.b_start) // 32
            for start, kind, n in cats:
                while pos < start:
                    out.append(('pad', None, None, si, None)); pos += 32
                    if kind != 't' and out[-2:-1] and out[-2][0] != 't': p += 1  # sentinel between rotation lists
                for i in range(n):
                    if kind == 't':
                        v = sec.trans[i]
                        out.append(('traw', v // 48, v % 48, si, None) if pos < sec.b_start else ('t', v // 48, v % 48, si, ranged(pos)))
                    else:
                        v = r[p]; p += 1
                        out.append(('rc', v // 48, v % 48, si, i) if kind == 'c' else ('r' + kind, v // 48, v % 48, si, ranged(pos)))
                    pos += 32
            if p < len(r) and cats and cats[-1][2] & 1 and cats[-1][1] != 't':
                p += 1; out.append(('pad', None, None, si, None))  # odd last category before raw rotations
            for v in r[p:]: out.append(('rraw', v // 48, v % 48, si, None))
        return out

    @property
    def widths(self): return [w for sec in self.sections for w in sec.widths]

def blocks_of(b, c, gentry, frames):
    """[(start_frame, end_frame, data_offset)] for one group: an MSB-first frame mask marks block boundaries,
    padded to 4 bytes, then a seek table (per 128 frames k: u16 first mark >= 128k, u16 its block index, u16 last
    mark < 128k; padded to 4 bytes), then n + 1 u32 block offsets."""
    off, _, _, idx, n = gentry
    mb = ((frames + 7) // 8 + 3) & ~3
    mask = b[c + idx:c + idx + mb]
    marks = [i for i in range(frames) if mask[i >> 3] >> (7 - (i & 7)) & 1]
    seek = (6 * ((frames - 1) // 128 + 1) + 3) & ~3
    offs = struct.unpack_from('<%dI' % (n + 1), b, c + idx + mb + seek)
    assert len(marks) == n + 1, (marks, n)
    return [(marks[i], marks[i + 1], c + offs[i]) for i in range(n)]

def decode_block(g, b, start, s, e):
    """keys of one block covering frames s..e (both keyed by the block).
    Header: per section u16 (offset of its key masks | mask bytes per track - 1) and u16 mask byte count;
    the first offset is the header size. Each section: per-track MSB-first key masks, then an MSB-first
    track-major bitstream of keys."""
    a = struct.unpack_from('<H', b, start)[0]
    nsec, mb = (a & ~3) // 4, (a & 3) + 1
    assert nsec == len(g.sections), (nsec, len(g.sections))
    assert (e - s + 7) // 8 <= mb, (s, e, mb)
    hdr = struct.unpack_from('<%dH' % (2 * nsec), b, start)
    tr = g.tracks()
    out = []; used = 0
    for si in range(nsec):
        M = hdr[2 * si + 1]
        mo = start + (hdr[2 * si] & ~3)
        st = [t for t in tr if t[3] == si]
        assert M == mb * len(st), (si, M, mb, len(st))
        widths = g.sections[si].widths
        bits = Bits(b, mo + M)
        for ti, (kind, j, slot, _, ri) in enumerate(st):
            keys = [s + k for k in popmask(b[mo + mb * ti:mo + mb * (ti + 1)])]
            keys = [k for k in keys if k <= e]
            w = widths[ti]
            sec = g.sections[si]
            sp = sec.c_splits[ri] if kind == 'rc' else (sec.splits[ri] if kind in ('t', 'rd', 're') else (0, 0, 0, 0))
            vals = [bits.read(w) for k in keys]
            out.append((kind, j, slot, (si, ri), w, sp, keys, vals))
        used = bits.p - (start * 8)
    return a, out, used

S2 = 1 / np.sqrt(2)
from quat import qmul, qconj, q48

def split3(v, sp):
    bx, by, bz = sp[0], sp[1], sp[3]
    return v >> (by + bz), (v >> bz) & ((1 << by) - 1), v & ((1 << bz) - 1)

def deq(v, sp, rng):
    mn, st = rng
    qx, qy, qz = split3(v, sp)
    return np.array([mn[2] + qx * st[2], mn[1] + qy * st[1], mn[0] + qz * st[0]])

def smallest3(v, sp):
    idx = v & 3; v >>= 2
    comps = []
    for q, bb in zip(split3(v, sp), (sp[0], sp[1], sp[3])):
        comps.append((q - (1 << (bb - 1))) / (1 << (bb - 1)) * S2 if bb else 0.0)
    big = np.sqrt(max(0.0, 1 - sum(x * x for x in comps)))
    q = comps + [big]
    q[idx], q[3] = q[3], q[idx]  # the largest component was swapped into w's place
    return np.array(q)

def rawf(v, n):
    """n f32 packed MSB first in one key"""
    return np.array([struct.unpack('>f', ((v >> (32 * (n - 1 - i))) & 0xffffffff).to_bytes(4, 'big'))[0] for i in range(n)])

def lift(d):
    """small quaternion vector part -> unit quaternion"""
    return np.array([d[0], d[1], d[2], np.sqrt(max(0.0, 1 - d @ d))])

def interp(keys, vals, nframes):
    keys = np.asarray(keys); vals = np.asarray(vals, float)
    out = np.zeros((nframes,) + vals.shape[1:])
    for f in range(nframes):
        k = np.searchsorted(keys, f, side='right') - 1
        k = max(0, min(k, len(keys) - 1))
        if keys[k] == f or k == len(keys) - 1: out[f] = vals[k]; continue
        t = (f - keys[k]) / (keys[k + 1] - keys[k])
        out[f] = vals[k] * (1 - t) + vals[k + 1] * t
    return out

def decode_group(b, c, h, gi, mode=None):
    """{joint: {'t': [F,3], 'r': [F,4], 's': [F,3], 'rkind': ...}} local values of one group."""
    mode = mode or {}
    _, blocks, groups = parse(b, c)
    gentry = groups[gi]
    g = Group(b, c + gentry[0])
    F = h['frames']
    res = {}
    if not g.recs: return g, res
    if g.unsupported: raise NotImplementedError('group %d records %s' % (gi, g.unsupported))
    tr = g.tracks()
    per = {}
    for (s, e, start) in (blocks_of(b, c, gentry, F) if tr else []):
        a, out, used = decode_block(g, b, start, s, e)
        for kind, j, slot, (si, ri), w, sp, keys, vals in out:
            if kind == 'pad': continue
            sec = g.sections[si]
            ks, vs, _ = per.setdefault((kind, j), ([], [], (si, ri)))
            for k, v in zip(keys, vals):
                if ks and ks[-1] == k: ks.pop(); vs.pop()
                ks.append(k)
                if kind == 'rc': vs.append(smallest3(v, sp))
                elif kind in ('traw', 'rraw'): vs.append(rawf(v, 3 if kind == 'traw' else 4))
                else: vs.append(deq(v, sp, sec.ranges[ri]))
    # record index of each rotation track within its section's 1d / 1e list
    rec = {}
    for kind, j, slot, si, ri in tr:
        if kind in ('rd', 're'):
            sec = g.sections[si]; cnt = rec.setdefault((si, kind), 0); rec[(si, kind)] += 1
            rec[(kind, j)] = (si, cnt)
    for (kind, j), (ks, vs, _) in per.items():
        d = res.setdefault(j, {})
        arr = interp(ks, vs, F)
        if kind in ('t', 'traw'): d['t'] = arr
        elif kind == 'rraw':
            d['r'] = np.array([q / np.linalg.norm(q) for q in arr]); d['rkind'] = 'raw'
        elif kind == 'rc':
            d['r'] = np.array([q / np.linalg.norm(q) for q in arr]); d['rkind'] = 'c'
        elif kind == 'rd':
            si, i = rec[(kind, j)]
            ref = g.sections[si].d_recs[i]; d['rkind'] = 'd'; d['ref'] = ref; d['delta'] = arr
            d['r'] = np.array([qmul(ref, lift(x)) for x in arr])
        elif kind == 're':
            si, i = rec[(kind, j)]
            qa, qb = g.sections[si].e_recs[i]; d['rkind'] = 'e'; d['qa'] = qa; d['qb'] = qb; d['delta'] = arr
            d['r'] = np.array([qmul(qmul(qa, lift(x)), qb) for x in arr])
    for t, k, f in g.recs:
        n = f[0] & 0x7fff; o = g.gb + f[2]
        if t not in (0x15, 0x16, 0x17, 0x18): continue
        lst = struct.unpack_from('<%dH' % n, b, o)
        do = (o + 2 * n + 15) & ~15
        if t == 0x18:
            for i, v in enumerate(lst):
                res.setdefault(v // 48, {})['r'] = np.tile(q48(b[do + 6 * i:do + 6 * i + 6]), (F, 1))
                res[v // 48]['rkind'] = 'const'
        if t == 0x15:  # constant translations, f32 x3
            for i, v in enumerate(lst):
                res.setdefault(v // 48, {})['t'] = np.tile(np.array(struct.unpack_from('<3f', b, do + 12 * i)), (F, 1))
        if t == 0x17:  # constant rotations, f32 x4
            for i, v in enumerate(lst):
                q = np.array(struct.unpack_from('<4f', b, do + 16 * i))
                res.setdefault(v // 48, {})['r'] = np.tile(q / np.linalg.norm(q), (F, 1)); res[v // 48]['rkind'] = 'const'
        if t == 0x16:
            for i, v in enumerate(lst):
                val = np.array(struct.unpack_from('<3e', b, do + 6 * i), float)
                res.setdefault(v // 48, {})['s' if v % 48 == 0 else 't'] = np.tile(val, (F, 1))
    return g, res
