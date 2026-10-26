# Ragnarök animation (CADO) — decoding notes

Samples: `../kprobe3/kratos_probe/as_full/*.bin` (35 streamed sets, named), `../kprobe3/kratos_probe/chunks/0078_ANM_heroa00_id1_s2.bin`
(32 MB, 1,289 CADO clips: Kratos's gameplay moveset), skeleton `goProtoheroa00.bin` (1,458 joints; `skel.py`).

## Containers
- `.as` set: 0x98 header (+0x96 file count), u64 file offsets; each file record: u64 hash, name[56], +0x40 tag `0a 40`,
  +0x48 u32 offset to the CADO object (from the record), +0x4c i32 back to the set; +0x50 u16 id, u16 9; +0x58 f32
  duration (s), f32 1.0; then 8 x 16-byte entries.
- ANM_heroa00 chunk: clip records (hash, name[56], tag `12 00`, u32 offset, i32 -1, `03 00 09 00`, duration, 1.0, ...)
  in a table near the start; the CADO data elsewhere (offset base not yet known; the chunk has a pointer-fixup table,
  magic 0x5a7ada7a). Names: navAmbRun, attAxe*, def*, ...

## CADO object (base = the `04 01` before "CADO")
- +0x00 u16 0x0104; +0x02 "CADO"; +0x06 u16 0xeb3e (constant); +0x08 u32 size + 0x50; +0x0c u16 group count (8);
  +0x0e u16 ?; +0x14 u32 group-table offset; +0x18 u32 size; +0x1c f32 fps (30; 15 or 60 in a few);
  +0x20 u16 k (40/32/8/0); +0x22 u16 frames; f32 1/(frames-1); f32 frames-1; f32 1/fps.
- +0x30: one entry per group: N u32 (per time segment: 0x280 for the 15-frame walk; 0x80,0x80,0x80,0x88 for 101 frames),
  2 u32 zero, N+1 u32 data offsets (segment starts, then end), padded. Segment count N depends on frames.
- group table: per group 16 bytes (u32 descriptor offset, u16, u16, u32, u32 N?).
- 256 bytes after the group table: bitmasks (ff runs) — probably which joints/channels are present.
- group descriptor: records `[u8 type][u8 k = u16 count that follows][k x u16]`, ending with type 0; fields:
  count (bit 15 a flag), 3, offset (from the descriptor start), extra.
  Types seen: 0d/1b (same count: 0d = bytes (bit widths?), 1b = 16-byte records), 12/13 (counts adding to 0d's),
  1c/1d/1e, 14/0e/19 (group 0), 16/18 (small lists).

## Groups
- group 0: float curves. Data is big-endian f32: e.g. the walk's 15-frame curves 0.60 -> 0.18, 0.40 -> 0.82.
  Descriptor 19 (n=2: joint ids 340, 464 + constants 1.0,0,0,0), 0e (n=10: 0x20...), 14 (n=10: u16 ids 208..476).
- group 7: always 18[n=28] — 28 joints, likely the fingers (2 x 14). 0x12/0x13 lists hold multiples of 0x30 (48 bits:
  3 x 16-bit components?). Data block 192 bytes for 15 frames: compressed over time, not raw per frame.
- groups 3-5 often empty.

## Session 2 findings (2026-10-25)
- Group pose buffers (padding sentinel = byte size): g0 640 (160 floats), g1 15360 (320 joints x 48), g2 12288 (256), g3
  (empty; 168 by difference), g4 9072 (189), g5 8016 (167), g6 12288 (256), g7 4896 (102): 320+256+168+189+167+256+102
  = 1458 = the skeleton. Consecutive skeleton ranges [0,320) body (hierarchy inside), [320,576) helpers, [576,744) and
  [744,933) face, [933,1100) armour/cloth, [1100,1356) and [1356,1458) arms/hands helpers; flags uniform per range.
  g1 = [0,320) in order looks right (its animated joints are hips 79, pelvis 163, thighs 167/168, knees 181/187, ...).
  g7 = [1356,1458) in order does NOT fit its mirror pairs (local +16 apart; the skeleton's are +25 apart): order unknown.
- Per-joint pose buffer entry 48 bytes: +0 scale, +16 rotation, +32 translation (remainders in the track lists).
- Track lists: 12 = animated translations (+32), 13 = animated rotations (+16), 16 = constant scale/translation
  (f16 values, 3 per track, e.g. scale 1.0 = 0x3c00), 18 = constant rotations (48-bit quaternions), 15 = ?.
  0d count = 12 count + 13 count; 0d bytes = per-track bit widths (3-44, independent of clip length).
- 48-bit quaternion (`quat.q48`): little-endian 48 bits; bits 0-1 index of the largest component (x,y,z,w), then
  a 15-bit, b 15-bit, c 16-bit, each centred (0x4000 / 0x4000 / 0x8000) and scaled by 1/sqrt(2). Identity =
  `03 00 01 80 00 80`. Decoded constants are unit, mirror pairs flip x. They do not equal the rest pose (deltas?).
- Data block per group per segment: u16 count, u16 offset; then per-track u16 key masks for <=16 frames
  (walk g7: 0x5455 = keys 0,2,4,6,10,12,14; 0x0440 x 18 = keys 6, 10): keyframe-reduced, variable bit width.
- ANM chunk: u32 count 7511 at +0x60, u64 object offsets from +0x80; 'default' set at 0x3348 lists 0xe0-byte clip
  records (hash, name, tag 12 00, u32 offset...); clip -> CADO offset base still unknown.

## Session 3 (2026-10-26): full decode works (dec.py, anim.py, render.py)
Validated on all 22 streamed sets: 653 blocks, every bitstream ends inside its 16-byte padding; run clip feet plant at
exactly the rest ankle (0.09) / toe (0.03) heights, alternating.

### Pose layout: the HADO object (ANM_heroa00 chunk +0xcc5a0, magic `06 01 "HADO"`)
- +0x10 u16 1458 joints, u16 1268 pose slots, u16 157 float channels, ..., u16 8 groups.
- +0xc0: 8 x 10 bytes (u16 0, u16 size (0 = 256), u16 is_float, u8, u8, u16 first slot): g0 157 floats;
  slots g1 [0,256) g2 [256,512) g3 [512,554) g4 [554,743) g5 [743,910) g6 [910,1166) g7 [1166,1268).
  (The CADO group table's byte +6 is the same size, 0 meaning 256.)
- +0x630: default pose, 1268 x 48 bytes (scale xyz0, quat xyzw, trans xyz0), in slot order. +0xf3f0: g0 defaults.
- Slot -> skeleton joint: slots follow skeleton order with 190 procedural joints left out. Matching default pose to
  goProto rest (rot + trans within 2e-3) with an increasing assignment is unique except 82 helper slots
  (match.py / mono.py -> slot2joint_range.pkl). Main chain: root 5->5, hips 79->79, spine 165->164, 175->174,
  chest 193->183, neck 219->204, head 318->299, thighs 167/168->166/167, knees 181/187->179/181, ankles
  208/213->198/201, toes 315/316->296/297, clavicles 220/221->205/206, upper arms 319/329->300/308, elbows
  936/955->910/919, wrists 1064/1103->1019/1039.
- +0xf670..+0x17200: four "rig program" tables (records 28/31/32/33/34/36 with (u16,u16) operand pairs) — probably
  the procedural joints; not decoded. +0x17200: 1458 x 3x4 matrices.

### Group table (CADO groups_at): 16 bytes: u32 descriptor, u16 0, u8 0, u8 size, u32 block-table offset, u32 n blocks.
### Block table entry: MSB-first frame mask of block boundaries (ceil(F/8) bytes padded to 4), seek table
  (one 6-byte entry per 128 frames k = 0..(F-1)//128: u16 first mark >= 128k, u16 its block index, u16 last
  mark < 128k; padded to 4), then n + 1 u32 block offsets (from CADO base). Block i covers frames [mark i, mark i+1], both keyed.
  Each group chooses its own block lengths (g1 of navAmbRun: 0,8,15,22,29,35).
### Descriptor sections: a group can hold several sections, each starting at a 0d record:
  0d widths, 1b ranges, 1c/1d/1e rotation records, 12 translations list, 13 rotations list. Flag bit 15 of a
  count = per-track entries (1c without it: one split shared by all its tracks).
  Track order (tracks() in dec.py): every track owns a 32-byte scratch slot. The 12 / 1c / 1d / 1e records' last
  field is each category's first slot; categories run in slot order (usually 12, 1d, 1e, 1c, but 1c can come
  first) and an odd category leaves a pad slot (width 0, mask 0; a sentinel entry in the 13 list between
  rotation categories). The 1b record's last field is the slot of the first ranged track: range index =
  (slot - that) / 32. Translations before it are raw (3 x f32 = 96-bit keys, the root joints 5 / 6 in long
  clips); 13-list entries left after the categories are raw rotations (4 x f32 = 128-bit keys).
- Flag bit 15 clear on a 0d / 1b / 1c count: one entry (width, range + split, split) shared by all its tracks.
### Block data: header per section (u16 offset of its masks | (mask bytes per track - 1), u16 mask byte count);
  first offset = header size. Section: per-track MSB-first key masks over frames s+1..e (frame s implicit), then an
  MSB-first track-major bitstream (keys x width bits). Next section starts 4-byte aligned. Keys interpolate linearly.
### Values
- Key = [x bits][y bits][z bits] MSB first; split table (4 bytes per 1b track, after the 1b records): (x bits,
  y bits, x+y, z bits).
- 1b range (16 bytes = lo u64, hi u64): hi = min, lo = step; each u64 = z (bits 0-20), y (21-41), x (42-63);
  min fields are truncated signed f32 (top 21/22 bits), step fields unsigned (no sign bit). value = min + q*step.
- Translations (12): plain local translation (metres). Root 5 carries root motion (navAmbRun: ~4.2 m/s along -z).
- 1d rotations: per-track reference quaternion, i16 x4 /32768, two tracks per 16-byte block with components
  interleaved (track 2k even i16s, 2k+1 odd). q = ref * (dx, dy, dz, sqrt(1-|d|^2)). (ref*d beats d*ref 0.006 vs
  0.019 median against sibling clips; foot plants agree.)
- 1e rotations: per-track 16 bytes = qa (even i16s) and qb (odd i16s); q = qa * d * qb (best of 14 forms, median
  0.006 vs next 0.013 over 503 sibling comparisons).
- 1c rotations: absolute smallest-three in the key: [x][y][z] then 2-bit index (lowest bits); components centred
  ((q - 2^(b-1)) / 2^(b-1) / sqrt2); the largest component is swapped into w's place (q[idx], q[3] = q[3], q[idx]).
  Knees use (14,1,1): a clean hinge about x.
- 18 constant rotations: 48-bit LE, MSB first x 16 bits, y 15, z 15, index 2, same centring and swap (32 of 357
  non-identity constants equal the default pose exactly with this order, none with others).
- 16 constants: f16 x3, slot +0 scale, +32 translation. 15: translations f32 x3. 17: rotations f32 x4.
- Records 0b / 0f / 10 / 1a (2-frame pose clips, 169 groups in ANM_heroa00): not decoded.
- Unlisted slots keep the HADO default pose; skeleton joints without a slot keep the goProto rest local transform
  (their rig program is not run, so twist / corrective helpers stay at rest).
- Convention: column vectors, local = T * R * S, world = world[parent] @ local, y up, Kratos faces -z.

### Still open
- ANM_heroa00 clip name -> CADO offset (the 1,289 gameplay clips); the 82 ambiguous helper slots; the rig program.

### Kratos's moveset (ANM_heroa00, 1,289 CADOs): picked by motion (survey.py), names not linked yet
Clip names live in 'default' set records (0xe0 bytes, tag 0x12) at chunk +0x3300 + u64 offsets; the record's
+0x48 is a size, not a pointer, and no u32/u64 in the chunk points at a CADO base, so names stay unmatched.
Picked by root speed, straightness and loop error (first vs last frame of hips, spine, legs):
- idle 0xff610 (61 frames, 2 s; the first gameplay CADO); longer idles 0x32ac60, 0xc6e310, 0xc783d0 (201 frames)
- 4-way sets (forward / back / left / right, yaw 0): walk 1.5 m/s 0x182fbd0 (68 frames, fwd -z), 0xbc1eb0;
  2.05 m/s 0x18d75f0 (77); run 3.0 m/s 0x1852180 (45); 3.7-6 m/s 0x12a2640 (43, fwd 5.96); sprint 6 m/s
  0x18b41c0 (36); 7.8 m/s 0x1c8edd0 (72)
- After the fixes every block decodes: 15,569 in ANM_heroa00 and 653 in the streamed sets, each bitstream ending
  inside its 16-byte padding (validate_blocks.py).
