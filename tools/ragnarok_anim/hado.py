import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import paths
import struct, numpy as np
ANM = paths.ANM
H = 0xcc5a0
def load():
    b = open(ANM, 'rb').read()
    nslot = struct.unpack_from('<H', b, H + 0x12)[0]
    groups = []
    for i in range(8):
        r = b[H + 0xc0 + 10 * i:H + 0xca + 10 * i]
        _, size, isf, _, _, start = struct.unpack('<HHHBBH', r)
        groups.append((size or 256 if not isf else size, start, isf))
    rest = np.frombuffer(b, '<f4', count=nslot * 12, offset=H + 0x630).reshape(nslot, 3, 4)
    return b, nslot, groups, rest
if __name__ == '__main__':
    b, nslot, groups, rest = load()
    print(nslot, groups)
    for k in (0, 5, 79, 167, 181, 187):
        print(k, np.round(rest[k], 4).tolist())
