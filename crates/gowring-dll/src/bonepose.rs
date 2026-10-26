//! Moving the player's bones from the mod. Elden Ring's own capes run on Havok cloth data inside each armour file,
//! which the mod cannot make, so Kratos's cape hangs from bones the mod moves itself (`cape`), through the offset
//! arrays found here.
//!
//! What the probes found (0.1.67-0.1.69, the debug pose key): arrays reached from the model item's bone-matrix entity
//! hold one row-major 3x4 per bone of the Havok skeleton, in its order, the identity in play; the game fills two of
//! them on alternate frames. They are offsets on top of the animated pose, applied in rest (bind) space per bone:
//! final skinning = the game's skinning x offset, not passed to the bone's children (the right hand turned about its
//! rest position stayed on the spear; the upper arm turned without the forearm). Their path from the model item moves
//! between sessions, so they are found by search each time. The animated pose itself is `posefind`'s question.

use crate::pose::{Found, Layout};
use crate::skeleton::Bone;

/// A 3x4 entry is a bone's when each row of its 3x3 part is this close to unit length (rotation, scale about 1).
const ROW_SLACK: f32 = 0.2;
/// The offset search (the cape's): further than the pose capture's (4,000 reads, 8 arrays, 2 pointers), on its own
/// thread.
pub(crate) const SEARCH_READS: u64 = 200_000;
pub(crate) const SEARCH_ARRAYS: usize = 48;
pub(crate) const SEARCH_DEPTH: usize = 4;
/// Rest (bind) transforms in model space, 4x4 row-major, from the skeleton's parent-relative ones.
pub(crate) fn bind_pose(bones: &[Bone]) -> Vec<[[f64; 4]; 4]> {
    let mut out: Vec<[[f64; 4]; 4]> = Vec::with_capacity(bones.len());
    for b in bones {
        let [x, y, z, w] = b.r.map(|v| v as f64);
        let s = b.s.map(|v| v as f64);
        let r = [
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
            [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
            [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
        ];
        let mut local = [[0.0; 4]; 4];
        for i in 0..3 {
            for j in 0..3 {
                local[i][j] = r[i][j] * s[j];
            }
            local[i][3] = b.t[i] as f64;
        }
        local[3][3] = 1.0;
        let m = match usize::try_from(b.parent).ok().and_then(|p| out.get(p)) {
            Some(parent) => mul(parent, &local),
            None => local,
        };
        out.push(m);
    }
    out
}

fn mul(a: &[[f64; 4]; 4], b: &[[f64; 4]; 4]) -> [[f64; 4]; 4] {
    let mut m = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            m[i][j] = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

/// The inverse of an affine 4x4 (3x3 by its adjugate), if its 3x3 part is not singular.
fn inverse(m: &[[f64; 4]; 4]) -> Option<[[f64; 4]; 4]> {
    let a = |i: usize, j: usize| m[i][j];
    let det = a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1)) - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0)) + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0));
    if det.abs() < 1e-9 {
        return None;
    }
    let mut inv = [[0.0; 4]; 4];
    inv[0][0] = (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1)) / det;
    inv[0][1] = (a(0, 2) * a(2, 1) - a(0, 1) * a(2, 2)) / det;
    inv[0][2] = (a(0, 1) * a(1, 2) - a(0, 2) * a(1, 1)) / det;
    inv[1][0] = (a(1, 2) * a(2, 0) - a(1, 0) * a(2, 2)) / det;
    inv[1][1] = (a(0, 0) * a(2, 2) - a(0, 2) * a(2, 0)) / det;
    inv[1][2] = (a(0, 2) * a(1, 0) - a(0, 0) * a(1, 2)) / det;
    inv[2][0] = (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0)) / det;
    inv[2][1] = (a(0, 1) * a(2, 0) - a(0, 0) * a(2, 1)) / det;
    inv[2][2] = (a(0, 0) * a(1, 1) - a(0, 1) * a(1, 0)) / det;
    for i in 0..3 {
        inv[i][3] = -(0..3).map(|k| inv[i][k] * m[k][3]).sum::<f64>();
    }
    inv[3][3] = 1.0;
    Some(inv)
}

/// How many of the skeleton's parent-child links sit at their rest offset when `floats` (row-major 3x4) are read as
/// skinning matrices in the skeleton's order: (matched, links). `None` when the array is shorter than the skeleton.
pub fn skin_match(floats: &[f32], bones: &[Bone]) -> Option<(usize, usize)> {
    skin_match_except(floats, bones, &[])
}

/// [`skin_match`] leaving out the links of the bones in `skip` (bones the mod writes itself, see `cape`).
pub fn skin_match_except(floats: &[f32], bones: &[Bone], skip: &[usize]) -> Option<(usize, usize)> {
    if floats.len() < bones.len() * 12 || bones.is_empty() {
        return None;
    }
    let bind = bind_pose(bones);
    let model: Vec<[[f64; 4]; 4]> = (0..bones.len())
        .map(|i| {
            let f = &floats[i * 12..i * 12 + 12];
            let mut s = [[0.0; 4]; 4];
            for r in 0..3 {
                for c in 0..4 {
                    s[r][c] = f[r * 4 + c] as f64;
                }
            }
            s[3][3] = 1.0;
            mul(&s, &bind[i])
        })
        .collect();
    let (mut matched, mut links) = (0, 0);
    for (i, b) in bones.iter().enumerate() {
        let Some(p) = usize::try_from(b.parent).ok().filter(|p| *p < i) else { continue };
        if skip.contains(&i) || skip.contains(&p) {
            continue;
        }
        let Some(inv) = inverse(&model[p]) else { continue };
        links += 1;
        let local = mul(&inv, &model[i]);
        let d: f64 = (0..3).map(|k| (local[k][3] - b.t[k] as f64).powi(2)).sum::<f64>().sqrt();
        if d < 0.01 {
            matched += 1;
        }
    }
    Some((matched, links))
}

/// Write `v` at `addr` through `WriteProcessMemory` on our own process (fails cleanly on memory that is not there).
pub(crate) fn write_floats(addr: usize, v: &[f32]) -> bool {
    use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut done = 0usize;
    let ok = unsafe { WriteProcessMemory(GetCurrentProcess(), addr as *const _, v.as_ptr() as *const _, v.len() * 4, Some(&mut done)) };
    ok.is_ok() && done == v.len() * 4
}

/// Does this 3x4 read as a bone's (rows of its 3x3 near unit length, all finite)?
pub(crate) fn bone_like(m: &[f32; 12]) -> bool {
    m.iter().all(|x| x.is_finite()) && (0..3).all(|r| ((m[r * 4] * m[r * 4] + m[r * 4 + 1] * m[r * 4 + 1] + m[r * 4 + 2] * m[r * 4 + 2]).sqrt() - 1.0).abs() < ROW_SLACK)
}

/// An array of the offsets' kind: 3x4, at least the skeleton long, the identity for at least 95 % of the skeleton's
/// bones.
pub(crate) fn offsets_like(f: &Found, bones: &[Bone]) -> bool {
    let n = bones.len();
    f.layout == Layout::M34 && f.floats.len() >= n * 12 && f.floats.chunks_exact(12).take(n).filter(|e| e.iter().zip(IDENTITY).all(|(a, b)| (a - b).abs() < 1e-4)).count() * 100 >= n * 95
}

pub(crate) const IDENTITY: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
