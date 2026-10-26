import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct, sys
from hx import hexdump
def u16(b,o): return struct.unpack_from('<H',b,o)[0]
def u32(b,o): return struct.unpack_from('<I',b,o)[0]
def f32(b,o): return struct.unpack_from('<f',b,o)[0]
def parse(b, base):
    assert b[base+2:base+6]==b'CADO', b[base:base+8]
    h=dict(ver=u16(b,base), hash=hex(u32(b,base+6)), a=u16(b,base+10), ngroups=u16(b,base+12), nb=u16(b,base+14),
           z=u32(b,base+16), groups_at=u32(b,base+20), size=u32(b,base+24), fps=f32(b,base+28), k=u16(b,base+32), frames=u16(b,base+34))
    blocks=[struct.unpack_from('<8I',b,base+0x30+32*i) for i in range(h['nb'])]
    groups=[struct.unpack_from('<IHHII',b,base+h['groups_at']+16*i) for i in range(h['ngroups'])]
    return h, blocks, groups
if __name__=='__main__':
    b=open(sys.argv[1],'rb').read()
    c=b.find(b'CADO')-2
    h,blocks,groups=parse(b,c)
    print('base',hex(c),h)
    for bl in blocks: print('block',[hex(x) for x in bl])
    for g in groups: print('group off %#x u16 %#x %#x idx %#x n %d'%g)
    ends=[g[0] for g in groups]+[blocks[0][3]]
    for i,g in enumerate(groups):
        print(f'--- group {i} at base+{g[0]:#x} ({ends[i+1]-g[0]} bytes)')
        hexdump(b, c+g[0], min(ends[i+1]-g[0],0x60), 0)
