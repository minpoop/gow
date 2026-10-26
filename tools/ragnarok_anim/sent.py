import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct, glob
from cado import parse
from groups import records
def bases(b):
    i=0
    while True:
        i=b.find(b'CADO',i)
        if i<0: return
        yield i-2; i+=4
def main():
  for path in sorted(glob.glob(sys.argv[1])):
      b=open(path,'rb').read()
      for c in bases(b):
          h,blocks,groups=parse(b,c)
          out=[]
          for gi,g in enumerate(groups):
              gb=c+g[0]; s=set()
              for t,k,f in records(b,gb):
                  if t in (0x12,0x13,0x16,0x18):
                      n=f[0]&0x7fff
                      v=struct.unpack_from('<%dH'%n,b,gb+f[2])
                      s|={x for x in v if x%48==0 and x>0}
                      # also padding after list
                      pad=struct.unpack_from('<H',b,gb+f[2]+2*n)[0] if True else 0
                      if pad%48==0 and pad>0: s.add(pad)
              out.append('/'.join(str(x//48) for x in sorted(s)) or '-')
          print(os.path.basename(path)[13:40].ljust(28), h['frames'], ' '.join(out))

if __name__=='__main__': main()
