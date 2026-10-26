import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np, glob, struct
from cado import parse
from dec import Group, decode_block, blocks_of
from sent import bases
slack=[]; bad=[]; skipped=[]
for path in sorted(glob.glob(sys.argv[1] if len(sys.argv) > 1 else os.path.join(__import__('paths').SETS, '*.bin'))):
    b=open(path,'rb').read()
    for c in bases(b):
        h,_,groups=parse(b,c); F=h['frames']
        for gi in range(1,8):
            g=Group(b,c+groups[gi][0])
            if g.unsupported: skipped.append((hex(c),gi,g.unsupported)); continue
            if not g.tracks(): continue
            n=groups[gi][4]
            offs=struct.unpack_from('<%dI'%(n+1),b,c+groups[gi][3]+(((F+7)//8+3)&~3)+((6*((F-1)//128+1)+3)&~3))
            for (s,e,start),end in zip(blocks_of(b,c,groups[gi],F),[c+o for o in offs[1:]]):
                a,out,used=decode_block(g,b,start,s,e)
                sl=(end-start)*8-used; slack.append(sl)
                if sl<0 or sl>=128: bad.append((os.path.basename(path)[13:40],hex(c),gi,s,e,used,(end-start)*8))
print('skipped groups',len(skipped),skipped[:3]); print('blocks',len(slack),'slack min',min(slack),'max',max(slack),'bad',len(bad))
for x in bad[:10]: print(x)
