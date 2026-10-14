//! The player's live pose, for fitting Kratos's model offline against how Elden Ring really poses the character (its
//! idle holds the arms differently from any pose made up from the rest skeleton). Where the game keeps the posed bones
//! is not mapped, so this searches: from the player's model item (its `mtx43_array_entity`, the bone matrices the name
//! suggests) and from the behaviour module, it follows pointers two steps deep and looks at what they point to for an
//! array of at least 100 consecutive bone transforms: 4x4, 3x4 or 4x3 matrices whose rotation is orthonormal, or
//! Havok `hkQsTransform`s (translation, unit quaternion, scale 1). What it finds goes to `%LOCALAPPDATA%\GowRing\
//! er_pose.json` (a few captures a few seconds apart; which bone is which is worked out offline from the bone lengths).
//! Read-only, `ReadProcessMemory` copies, on the probe thread, within a read budget.

use std::collections::HashSet;
use std::fmt::Write as _;

use crate::skeleton::{read_vec, reads};

/// `mtx43_array_entity` in `CSFD4ModelItem` (eldenring 0.14: after 0x78 + 0x5c8 bytes of other fields).
pub const MTX43_ARRAY_ENTITY: usize = 0x640;

/// Bytes of each object looked at for pointers, and of each candidate array read.
const OBJECT: usize = 0x800;
const ARRAY: usize = 176 * 64;
const READ_BUDGET: u64 = 4000;

/// The layouts tried: name, stride in floats, and where the 3x3 rotation's rows (or the quaternion) sit.
#[derive(Clone, Copy)]
enum Layout {
    /// 16 floats; rotation in 0-2, 4-6, 8-10.
    M44,
    /// 12 floats, rotation in 0-2, 4-6, 8-10, translation in 3, 7, 11.
    M34,
    /// 12 floats, rotation in 0-8 (three rows of three), translation in 9-11.
    M43,
    /// 12 floats: translation 0-2 (+pad), quaternion 4-7, scale 8-10 (+pad).
    Qs,
}

impl Layout {
    fn name(self) -> &'static str {
        match self {
            Layout::M44 => "4x4",
            Layout::M34 => "3x4",
            Layout::M43 => "4x3",
            Layout::Qs => "hkQsTransform",
        }
    }
    fn stride(self) -> usize {
        match self {
            Layout::M44 => 16,
            _ => 12,
        }
    }
    /// Does this element look like a bone transform?
    fn valid(self, f: &[f32]) -> bool {
        if f.iter().any(|x| !x.is_finite() || x.abs() > 1e4) {
            return false;
        }
        let unit = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2] - 1.0).abs() < 0.02;
        let dot = |a: [f32; 3], b: [f32; 3]| (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).abs() < 0.02;
        let rows = |r: [[f32; 3]; 3]| unit(r[0]) && unit(r[1]) && unit(r[2]) && dot(r[0], r[1]) && dot(r[1], r[2]) && dot(r[0], r[2]);
        match self {
            Layout::M44 | Layout::M34 => rows([[f[0], f[1], f[2]], [f[4], f[5], f[6]], [f[8], f[9], f[10]]]),
            Layout::M43 => rows([[f[0], f[1], f[2]], [f[3], f[4], f[5]], [f[6], f[7], f[8]]]),
            Layout::Qs => {
                let q = f[4] * f[4] + f[5] * f[5] + f[6] * f[6] + f[7] * f[7];
                (q - 1.0).abs() < 0.01 && (0..3).all(|k| (f[8 + k] - 1.0).abs() < 0.02)
            }
        }
    }
}

/// The longest run of valid transforms starting within the first 16 floats of `f` (an array may follow a small
/// header), for each layout; the best (start in floats, layout, count).
fn best_run(f: &[f32]) -> Option<(usize, Layout, usize)> {
    (0..16)
        .step_by(4)
        .flat_map(|start| [Layout::M44, Layout::M34, Layout::M43, Layout::Qs].into_iter().map(move |l| (start, l)))
        .map(|(start, l)| (start, l, f[start..].chunks_exact(l.stride()).take_while(|e| l.valid(e)).count()))
        .filter(|(_, _, n)| *n >= 100)
        .max_by_key(|(_, _, n)| *n)
}

/// Search from `roots` (name, address) and write what is found as capture `label`. Returns a line for the log.
pub fn capture(roots: &[(&str, usize)], label: &str) -> String {
    let (t0, r0) = (std::time::Instant::now(), reads());
    let mut seen: HashSet<usize> = HashSet::new();
    let mut found: Vec<(String, usize, Layout, Vec<f32>)> = Vec::new();
    let looks_like_pointer = |w: usize| (0x10000..0x7fff_ffff_0000).contains(&w) && w % 8 == 0;
    // breadth first: (path, address of an object to look into), two levels
    let mut level: Vec<(String, usize)> = roots.iter().map(|(n, a)| (n.to_string(), *a)).collect();
    for depth in 0..3 {
        let mut next = Vec::new();
        for (path, at) in level {
            if reads() - r0 > READ_BUDGET || found.len() >= 8 {
                break;
            }
            if !seen.insert(at) {
                continue;
            }
            // the object itself might be the array
            if let Some(f) = read_vec::<f32>(at, ARRAY / 4) {
                if let Some((start, layout, n)) = best_run(&f) {
                    found.push((path.clone(), at + start * 4, layout, f[start..start + n * layout.stride()].to_vec()));
                    continue;
                }
            }
            if depth == 2 {
                continue;
            }
            let Some(words) = read_vec::<usize>(at, OBJECT / 8) else { continue };
            for (k, w) in words.into_iter().enumerate() {
                if looks_like_pointer(w) && !seen.contains(&w) {
                    next.push((format!("{path}+{:#x}", k * 8), w));
                }
            }
        }
        level = next;
    }
    let mut j = format!("{{\n  \"capture\": {label:?},\n  \"arrays\": [\n");
    for (i, (path, at, layout, f)) in found.iter().enumerate() {
        let n = f.len() / layout.stride();
        let _ = write!(j, "    {{\"path\": {path:?}, \"address\": \"{at:#x}\", \"layout\": {:?}, \"count\": {n}, \"floats\": [", layout.name());
        for (k, x) in f.iter().enumerate() {
            let _ = write!(j, "{}{x}", if k > 0 { "," } else { "" });
        }
        let _ = write!(j, "]}}{}\n", if i + 1 < found.len() { "," } else { "" });
    }
    j.push_str("  ]\n}\n");
    let path = crate::paths::get().local.join(format!("er_pose_{label}.json"));
    let wrote = std::fs::create_dir_all(&crate::paths::get().local).and_then(|_| std::fs::write(&path, j));
    let list: Vec<String> = found.iter().map(|(p, a, l, f)| format!("{p} at {a:#x}: {} x {}", f.len() / l.stride(), l.name())).collect();
    format!(
        "pose capture {label}: {} array(s) [{}]; {} ({} objects looked at, {} reads, {} ms)",
        found.len(),
        list.join("; "),
        match wrote {
            Ok(()) => format!("written to {}", path.display()),
            Err(e) => format!("could not write it: {e}"),
        },
        seen.len(),
        reads() - r0,
        t0.elapsed().as_millis()
    )
}
