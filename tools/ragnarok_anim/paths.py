"""Where the probe copied the player's Ragnarök files (set KRATOS_PROBE to the kratos_probe folder)."""
import os
PROBE = os.environ.get('KRATOS_PROBE', os.path.join(os.path.dirname(os.path.abspath(__file__)), 'kratos_probe'))
SKELETON = os.path.join(PROBE, 'goProtoheroa00.bin')
ANM = os.path.join(PROBE, 'chunks', '0078_ANM_heroa00_id1_s2.bin')
SETS = os.path.join(PROBE, 'as_full')
OUT = os.environ.get('RAGNAROK_ANIM_OUT', os.path.dirname(os.path.abspath(__file__)))
