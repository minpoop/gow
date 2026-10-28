//! Elden Ring's live animated pose: step 2 of giving Kratos his own movement, found in 0.1.89's dumps offline and
//! checked in play in 0.1.90.
//!
//! What the dumps showed (MODLOG, 0.1.89): each offset array the cape writes (`bonepose`) is the last of three arrays
//! of one row-major 3x4 per model bone, back to back (181 bones for the player: the Havok skeleton's 150 in its order,
//! then 31 of the model's own), and the game keeps two such buffers. The middle array is the game's skinning: per
//! bone, the world transform times the inverse of the bind pose (the model's bind equals the one built from the
//! skeleton, for all 150 bones). Read that way it puts 74 of the 78 links whose two entries differ at their rest
//! offset (Pelvis and RootRotY carry animated translation, the SpineArmor bones the armour's), the left hand 0.54 m
//! from its bind position (the two-handed idle), and the cape's bones exactly on their parents' while the cape's
//! offsets turn them 3 to 5 degrees: the offsets go on after it, so the offset that shows a pose is this array's
//! entry inverted times the pose's skinning. The searches missed it because a run of good matrices read the end of
//! the first array and this one as one, and because bone 84 (L_Pectoral) carries the body's scale, which their 2 %
//! unit-row test refused.
//!
//! 0.1.90 checked it in play while the player moved (MODLOG): links at rest throughout, joints moving up to 1.4 m,
//! the cape's offsets (up to 74 degrees) never in it. `moves` reads it every frame to put Kratos's own moves on.

use crate::bonepose::{inverse, mul};
use crate::skeleton::{read_vec, Bone};

/// The model's bone count is looked for from the skeleton's up to this.
const MAX_BONES: usize = 400;
/// A block reads as the skinning when this share of its links whose two entries differ (at least `MIN_LINKS` of
/// them) sit at their rest offset; links between equal entries pass whatever the block is (0.1.89: 74 of 78, 29 equal).
const MATCH: f64 = 0.8;
const MIN_LINKS: usize = 30;
const ENTRY: usize = 48;

/// One of the game's buffers: its skinning array, the model's bone count (each array's length) and its offsets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Buffer {
    pub skinning: usize,
    pub bones: usize,
    pub offsets: usize,
}

/// 12 floats (row-major 3x4) as a 4x4.
pub(crate) fn matrix(f: &[f32]) -> [[f64; 4]; 4] {
    let mut m = [[0.0; 4]; 4];
    for r in 0..3 {
        for c in 0..4 {
            m[r][c] = f[r * 4 + c] as f64;
        }
    }
    m[3][3] = 1.0;
    m
}

/// Bone `b`'s world transform in skinning `y` (its entry times its bind).
fn world(y: &[f32], bind: &[[[f64; 4]; 4]], b: usize) -> [[f64; 4]; 4] {
    mul(&matrix(&y[b * 12..b * 12 + 12]), &bind[b])
}

/// The links of skinning `y` whose parent and child entries differ, and how many of them sit at their rest offset
/// (the child's joint, in its parent's frame, where the skeleton puts it).
fn links_at_rest(y: &[f32], bind: &[[[f64; 4]; 4]], bones: &[Bone]) -> (usize, usize) {
    let (mut at_rest, mut links) = (0, 0);
    for (i, b) in bones.iter().enumerate() {
        let Some(p) = usize::try_from(b.parent).ok().filter(|p| *p < i) else { continue };
        if y[p * 12..p * 12 + 12] == y[i * 12..i * 12 + 12] {
            continue;
        }
        let Some(inv) = inverse(&world(y, bind, p)) else { continue };
        links += 1;
        let local = mul(&inv, &world(y, bind, i));
        if (0..3).map(|k| (local[k][3] - b.t[k] as f64).powi(2)).sum::<f64>().sqrt() < 0.01 {
            at_rest += 1;
        }
    }
    (at_rest, links)
}

/// The skinning before the offset array at `offsets`: the block of `n` entries just before it, for the `n` (the model's
/// bone count) at which it reads best as skinning: the most of its differing links at their rest offset, the smallest
/// `n` of those within 5 % of the best (the array before it can read as well). The smallest `n` that merely passed
/// was taken until 0.1.94, where a block 31 entries short passed with him standing still just after loading, and
/// nothing played (it read as a 150-bone model, which `moves` will not write).
pub(crate) fn locate(offsets: usize, bones: &[Bone], bind: &[[[f64; 4]; 4]]) -> Option<Buffer> {
    let scored: Vec<(usize, usize, usize)> = (bones.len()..=MAX_BONES)
        .filter_map(|n| {
            let at = offsets.checked_sub(n * ENTRY)?;
            let y = read_vec::<f32>(at, bones.len() * 12)?;
            if y.iter().any(|x| !x.is_finite()) {
                return None;
            }
            let (at_rest, links) = links_at_rest(&y, bind, bones);
            (links >= MIN_LINKS && at_rest as f64 >= MATCH * links as f64).then_some((n, at, at_rest))
        })
        .collect();
    let best = scored.iter().map(|s| s.2).max()?;
    scored.iter().find(|s| s.2 as f64 >= 0.95 * best as f64).map(|&(n, at, _)| Buffer { skinning: at, bones: n, offsets })
}
