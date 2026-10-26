import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct, numpy as np
def load(path):
    b=open(path,'rb').read()
    n=struct.unpack_from('<H',b,0x10)[0]
    recs=[struct.unpack_from('<4h',b,0x18+8*i) for i in range(n)]
    parent=[r[3] for r in recs]
    loc=np.array([np.array(struct.unpack_from('<16f',b,0xb6b0+64*i)).reshape(4,4).T for i in range(n)])  # column-major -> row-major
    world=np.zeros_like(loc)
    for i in range(n):
        world[i]=loc[i] if parent[i]<0 else world[parent[i]]@loc[i]
    return n,parent,loc,world
if __name__=='__main__':
    n,parent,loc,world=load(sys.argv[1])
    print(n)
    for i in [int(x) for x in sys.argv[2:]]:
        print(i,'parent',parent[i],'pos',np.round(world[i][:3,3],3))
