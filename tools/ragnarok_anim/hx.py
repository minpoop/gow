import sys
def hexdump(b, start=0, n=512, base=0):
    for i in range(start, min(start+n, len(b)), 16):
        row=b[i:i+16]
        print(f"{base+i:08x}  "+' '.join(f"{x:02x}" for x in row).ljust(48)+'  '+''.join(chr(x) if 32<=x<127 else '.' for x in row))
if __name__=='__main__':
    b=open(sys.argv[1],'rb').read()
    s=int(sys.argv[2],0) if len(sys.argv)>2 else 0
    n=int(sys.argv[3],0) if len(sys.argv)>3 else 512
    print('size',len(b)); hexdump(b,s,n)
