import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from cado import parse, u16
def records(b, gbase):
    out=[]; o=gbase
    while True:
        t,k=b[o],b[o+1]
        if t==0: break
        f=struct.unpack_from('<%dH'%k,b,o+2)
        out.append((t,k,f)); o+=2+2*k
    return out
if __name__=='__main__':
    b=open(sys.argv[1],'rb').read()
    c=b.find(b'CADO')-2
    h,blocks,groups=parse(b,c)
    print(h['frames'],'frames')
    for gi,g in enumerate(groups):
        gb=c+g[0]
        recs=records(b,gb)
        print(f'group {gi} @+{g[0]:#x}:', ' '.join(f"{t:02x}[n={f[0]&0x7fff}{'*' if f[0]&0x8000 else ''} c={f[1]} @{f[2]:#x}{' '+','.join(hex(x) for x in f[3:]) if len(f)>3 else ''}]" for t,k,f in recs))
