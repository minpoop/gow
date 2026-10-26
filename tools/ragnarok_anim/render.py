import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
from PIL import Image, ImageDraw
from sent import bases
from anim import skeleton_pose
CHAINS = [([5, 79, 163, 165, 175, 193, 219, 318], (40, 40, 40)), ([163, 167, 181, 208, 315], (200, 40, 40)),
          ([163, 168, 187, 213, 316], (40, 40, 200)), ([193, 220, 319, 936, 1064, 1156], (40, 160, 200)),
          ([193, 221, 329, 955, 1103, 1203], (200, 120, 40))]
CELL = 180
def sheet(world, frames, follow=True, title=''):
    img = Image.new('RGB', (CELL * len(frames), CELL * 2 + 14), 'white')
    d = ImageDraw.Draw(img)
    d.text((4, 0), title, fill='black')
    for i, f in enumerate(frames):
        P = world[f, :, :3, 3]
        off = np.zeros(3)
        if follow:  # into the root's frame (undo its travel and turn)
            R = world[f, 5, :3, :3]; P = (P - world[f, 5, :3, 3]) @ R
        for row, (a, sgn) in enumerate(((2, 1), (0, 1))):  # side view: z horizontal; front view: x horizontal
            ox, oy = CELL * i + CELL // 2, 14 + CELL * row + CELL - 8
            sc = CELL / 2.2
            d.line([(CELL * i, oy), (CELL * i + CELL, oy)], fill=(180, 180, 180))
            for ch, col in CHAINS:
                q = P[ch] - off
                pts = [(ox + sgn * q[k, a] * sc, oy - q[k, 1] * sc) for k in range(len(ch))]
                d.line(pts, fill=col, width=2)
            d.text((CELL * i + 3, 14 + CELL * row + 2), f'f{f} ' + ('side' if row == 0 else 'front'), fill='black')
    return img
def draw(path, ci, out, mode=None, n=8, follow=True):
    b = open(path, 'rb').read(); c = list(bases(b))[ci]
    h, world = skeleton_pose(b, c, mode)
    F = h['frames']; frames = list(range(0, F, max(1, F // n)))[:n]
    sheet(world, frames, follow, f'{os.path.basename(path)} #{ci} {mode}').save(out)
    return h, world
if __name__ == '__main__':
    import json
    mode = json.loads(sys.argv[4]) if len(sys.argv) > 4 else None
    h, world = draw(sys.argv[1], int(sys.argv[2]), sys.argv[3], mode)
    P = world[:, :, :3, 3]
    for j, nm in ((208, 'ankleR'), (213, 'ankleL'), (315, 'toeR'), (316, 'toeL')):
        print(nm, 'height', np.round(P[:, j, 1], 2).tolist())
