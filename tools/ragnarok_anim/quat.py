import numpy as np, struct
S=1/np.sqrt(2)
def q48(bs):
    """48-bit smallest-three, MSB first: x 16 bits, y 15, z 15, index 2; the largest is swapped into w's place."""
    v=int.from_bytes(bs,'little')
    idx=v&3; z=(v>>2)&0x7fff; y=(v>>17)&0x7fff; x=(v>>32)&0xffff
    q=[(x-32768)/32768*S,(y-16384)/16384*S,(z-16384)/16384*S]
    q.append(np.sqrt(max(0.0,1-sum(c*c for c in q))))
    q[idx],q[3]=q[3],q[idx]
    return np.array(q)  # x,y,z,w
def mat2quat(m):
    m=np.asarray(m)[:3,:3]
    t=np.trace(m)
    if t>0:
        s=np.sqrt(t+1)*2; w=0.25*s; x=(m[2,1]-m[1,2])/s; y=(m[0,2]-m[2,0])/s; z=(m[1,0]-m[0,1])/s
    else:
        i=np.argmax([m[0,0],m[1,1],m[2,2]])
        if i==0:
            s=np.sqrt(1+m[0,0]-m[1,1]-m[2,2])*2; w=(m[2,1]-m[1,2])/s; x=0.25*s; y=(m[0,1]+m[1,0])/s; z=(m[0,2]+m[2,0])/s
        elif i==1:
            s=np.sqrt(1+m[1,1]-m[0,0]-m[2,2])*2; w=(m[0,2]-m[2,0])/s; x=(m[0,1]+m[1,0])/s; y=0.25*s; z=(m[1,2]+m[2,1])/s
        else:
            s=np.sqrt(1+m[2,2]-m[0,0]-m[1,1])*2; w=(m[1,0]-m[0,1])/s; x=(m[0,2]+m[2,0])/s; y=(m[1,2]+m[2,1])/s; z=0.25*s
    q=np.array([x,y,z,w]); return q/np.linalg.norm(q)
def qmul(a, b):
    ax, ay, az, aw = a; bx, by, bz, bw = b
    return np.array([aw*bx + ax*bw + ay*bz - az*by,
                     aw*by - ax*bz + ay*bw + az*bx,
                     aw*bz + ax*by - ay*bx + az*bw,
                     aw*bw - ax*bx - ay*by - az*bz])
def qconj(a): return np.array([-a[0], -a[1], -a[2], a[3]])
def qrot(q, v): return qmul(qmul(q, np.array([v[0], v[1], v[2], 0.0])), qconj(q))[:3]
def qmat(q):
    x, y, z, w = q
    return np.array([[1-2*(y*y+z*z), 2*(x*y-z*w), 2*(x*z+y*w)],
                     [2*(x*y+z*w), 1-2*(x*x+z*z), 2*(y*z-x*w)],
                     [2*(x*z-y*w), 2*(y*z+x*w), 1-2*(x*x+y*y)]])
