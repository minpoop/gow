# ragnarok_anim — research decoder for God of War Ragnarök animation (CADO)

Offline Python tools used to work out Ragnarök's animation format from the player's own files (copied by the
kratos probe). Nothing from the game is in this folder: point `KRATOS_PROBE` at the probe's `kratos_probe`
folder, and outputs (`*.pkl`, `*.png`, `*.gif`, all derived from the player's files) go to `RAGNAROK_ANIM_OUT`
(default: this folder, git-ignored). Run every script with `python3 -I`.

The format is written up in `NOTES.md`. The builder's Rust port is `crates/gowring-gow/src/kratos_anim.rs` (checked against `anim.py`).

| script | does |
|---|---|
| `match.py`, `mono.py` | map the 1,268 pose slots (HADO default pose) onto the 1,458 skeleton joints → `slot2joint_range.pkl` (run first) |
| `dec.py` | decode one group of a clip: tracks, keys, ranges, rotations (1c / 1d / 1e), constants |
| `anim.py` | whole clip → slot pose → skeleton local transforms → world matrices (FK) |
| `validate_blocks.py` | decode every block of every streamed set and check each bitstream ends inside its padding |
| `survey.py` | decode all 1,289 clips in ANM_heroa00 and record root speed, turn and loop error |
| `render.py`, `render_anm.py`, `gif.py` | stick-figure contact sheets and an animated preview |
| `cado.py`, `groups.py`, `bits.py`, `quat.py`, `skel.py`, `hado.py`, `sent.py`, `hx.py` | parsing helpers |
| `er_pose_find.py` | Elden Ring side: read gowring.dll's 0.1.89 pose search dumps (`er_pose_find_<n>.bin`) against `er_skeleton.json` |
| `er_pose_live.py` | Elden Ring side: read its live pose checks (`er_pose_live_<n>.bin`, 0.1.90 on): links at rest, joints, the cape check, a contact sheet |
