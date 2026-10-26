class Bits:
    """MSB-first bit reader."""
    def __init__(self, data, pos=0): self.d=data; self.p=pos*8
    def read(self, n):
        v=0
        for _ in range(n):
            byte=self.d[self.p>>3]; bit=(byte>>(7-(self.p&7)))&1
            v=(v<<1)|bit; self.p+=1
        return v
def popmask(bs):
    """frames keyed by a MSB-first mask over frames 1..; frame 0 always"""
    fr=[0]
    for i,byte in enumerate(bs):
        for k in range(8):
            if byte>>(7-k)&1: fr.append(i*8+k+1)
    return fr
