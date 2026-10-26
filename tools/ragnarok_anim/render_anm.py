import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import paths
import numpy as np
from anim import skeleton_pose
from render import sheet
HERE = os.path.dirname(os.path.abspath(__file__))
b = open(paths.ANM, 'rb').read()
for arg in sys.argv[1:]:
    c = int(arg, 16)
    h, world = skeleton_pose(b, c)
    F = h['frames']; frames = list(range(0, F, max(1, F // 8)))[:8]
    sheet(world, frames, True, f'ANM {arg} F={F}').save(os.path.join(paths.OUT, f'anm_{arg}.png'))
    P = world[:, :, :3, 3]; R = world[:, 5, :3, :3]
    rel = np.einsum('fjk,fkl->fjl', P - world[:, 5:6, :3, 3], R)
    print(arg, 'ankle min/max R %.2f %.2f L %.2f %.2f' % (P[:, 208, 1].min(), P[:, 208, 1].max(), P[:, 213, 1].min(), P[:, 213, 1].max()),
          'head y %.2f' % P[:, 318, 1].mean())
