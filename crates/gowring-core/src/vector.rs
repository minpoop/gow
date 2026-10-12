//! Original vector art for the HUD: bars, rune rings, weapon icons and the aim reticle drawn from a few
//! triangles, lines and circles. It is what the HUD shows until (and wherever) Ragnarök's own art cannot be
//! read from the player's install, and it is shared by the in-game overlay and the layout preview so both
//! draw exactly the same shapes. Nothing here is taken from either game.

use crate::hud::Rect;

#[derive(Clone, Debug)]
pub enum Prim {
    Tri { p: [(f32, f32); 3], color: u32, alpha: f32 },
    Line { a: (f32, f32), b: (f32, f32), thick: f32, color: u32, alpha: f32 },
    Circle { c: (f32, f32), r: f32, fill: bool, thick: f32, color: u32, alpha: f32 },
    /// A filled convex polygon, as one shape, so a see-through fill shows no inner seams.
    Poly { pts: Vec<(f32, f32)>, color: u32, alpha: f32 },
    /// A rectangle shaded from `left` to `right`.
    Grad { x0: f32, y0: f32, x1: f32, y1: f32, left: u32, right: u32, alpha: f32 },
}

fn mix(c: u32, to: u32, t: f32) -> u32 {
    let ch = |s: u32| {
        let a = ((c >> s) & 255) as f32;
        let b = ((to >> s) & 255) as f32;
        (a + (b - a) * t).clamp(0.0, 255.0) as u32
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}
pub fn lighten(c: u32, t: f32) -> u32 {
    mix(c, 0xFFFFFF, t)
}
pub fn shade(c: u32, t: f32) -> u32 {
    mix(c, 0x000000, t)
}

struct Canvas {
    out: Vec<Prim>,
}

impl Canvas {
    fn tri(&mut self, a: (f32, f32), b: (f32, f32), c: (f32, f32), color: u32, alpha: f32) {
        self.out.push(Prim::Tri { p: [a, b, c], color, alpha });
    }
    fn line(&mut self, a: (f32, f32), b: (f32, f32), thick: f32, color: u32, alpha: f32) {
        self.out.push(Prim::Line { a, b, thick, color, alpha });
    }
    fn circle(&mut self, c: (f32, f32), r: f32, fill: bool, thick: f32, color: u32, alpha: f32) {
        self.out.push(Prim::Circle { c, r, fill, thick, color, alpha });
    }
    fn poly(&mut self, pts: &[(f32, f32)], color: u32, alpha: f32) {
        if pts.len() >= 3 {
            self.out.push(Prim::Poly { pts: pts.to_vec(), color, alpha });
        }
    }
    fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: u32, alpha: f32) {
        // one shape, so a see-through rectangle has no diagonal seam
        self.poly(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)], color, alpha);
    }
    /// Fill a polygon that is star-shaped about `centre` (every point visible from it).
    fn fan(&mut self, centre: (f32, f32), pts: &[(f32, f32)], color: u32, alpha: f32) {
        // Adjacent triangles of a fan are grown by a hair so their shared edges overlap instead of showing a seam.
        for i in 0..pts.len() {
            let t = [centre, pts[i], pts[(i + 1) % pts.len()]];
            let g = ((t[0].0 + t[1].0 + t[2].0) / 3.0, (t[0].1 + t[1].1 + t[2].1) / 3.0);
            let grow = |q: (f32, f32)| {
                let (dx, dy) = (q.0 - g.0, q.1 - g.1);
                let l = (dx * dx + dy * dy).sqrt().max(1e-4);
                (q.0 + dx / l * 0.6, q.1 + dy / l * 0.6)
            };
            self.tri(grow(t[0]), grow(t[1]), grow(t[2]), color, alpha);
        }
    }
    fn stroke(&mut self, pts: &[(f32, f32)], closed: bool, thick: f32, color: u32, alpha: f32) {
        let n = pts.len();
        for i in 0..n {
            if i + 1 < n || closed {
                self.line(pts[i], pts[(i + 1) % n], thick, color, alpha);
            }
        }
    }
    fn arc(&mut self, c: (f32, f32), r: f32, a0: f32, a1: f32, thick: f32, color: u32, alpha: f32) {
        let steps = (((a1 - a0).abs() / std::f32::consts::TAU) * 56.0).ceil().max(2.0) as usize;
        let pt = |i: usize| {
            let a = a0 + (a1 - a0) * i as f32 / steps as f32;
            (c.0 + r * a.cos(), c.1 + r * a.sin())
        };
        for i in 0..steps {
            self.line(pt(i), pt(i + 1), thick, color, alpha);
        }
    }
}

/// The largest centred square inside `r`, as (x, y, side).
fn square(r: &Rect) -> (f32, f32, f32) {
    let s = r.w.min(r.h);
    (r.x + (r.w - s) / 2.0, r.y + (r.h - s) / 2.0, s)
}

/// The outline of a HUD plate around (`cx`, `cy`), `rx` by `ry` from the centre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlateShape {
    /// A diamond with its four tips cut flat (Ragnarök's runic plates).
    Diamond,
    /// An octagon with a flat top (Ragnarök's relic plate).
    Octagon,
    Disc,
}

pub fn plate_outline(shape: PlateShape, cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<(f32, f32)> {
    match shape {
        PlateShape::Diamond => {
            // short flat tips, long 45-degree sides
            let t = rx * 0.21;
            let tv = (ry - (rx - t)).max(ry * 0.08);
            vec![(cx - t, cy - ry), (cx + t, cy - ry), (cx + rx, cy - tv), (cx + rx, cy + tv), (cx + t, cy + ry), (cx - t, cy + ry), (cx - rx, cy + tv), (cx - rx, cy - tv)]
        }
        PlateShape::Octagon => {
            let (ex, ey) = (rx * 0.414, ry * 0.414);
            vec![(cx - ex, cy - ry), (cx + ex, cy - ry), (cx + rx, cy - ey), (cx + rx, cy + ey), (cx + ex, cy + ry), (cx - ex, cy + ry), (cx - rx, cy + ey), (cx - rx, cy - ey)]
        }
        PlateShape::Disc => (0..40)
            .map(|i| {
                let a = i as f32 * std::f32::consts::TAU / 40.0;
                (cx + rx * a.cos(), cy + ry * a.sin())
            })
            .collect(),
    }
}

/// The part of a convex polygon below (`below`) or above the line y = `y`.
fn clip_y(pts: &[(f32, f32)], y: f32, below: bool) -> Vec<(f32, f32)> {
    let inside = |q: (f32, f32)| if below { q.1 >= y } else { q.1 <= y };
    let mut out = Vec::new();
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        if inside(a) {
            out.push(a);
        }
        if inside(a) != inside(b) {
            let t = (y - a.1) / (b.1 - a.1);
            out.push((a.0 + (b.0 - a.0) * t, y));
        }
    }
    out
}

/// The same outline pushed out (or in, for negative `d`) by `d` pixels from its centre.
fn grow(pts: &[(f32, f32)], c: (f32, f32), d: f32) -> Vec<(f32, f32)> {
    pts.iter()
        .map(|q| {
            let (dx, dy) = (q.0 - c.0, q.1 - c.1);
            let l = (dx * dx + dy * dy).sqrt().max(1e-4);
            (q.0 + dx / l * d, q.1 + dy / l * d)
        })
        .collect()
}

/// A Ragnarök HUD plate: a thin silver outline, a dark border, the coloured face lit from the top, and an inner
/// bevel. `charge` (0..1) is how much of the face is lit from the bottom up; the rest is dimmed (a runic
/// recharging). A disc is the weapon's plain grey plate.
pub fn plate(shape: PlateShape, cx: f32, cy: f32, rx: f32, ry: f32, mid: u32, light: u32, charge: f32, alpha: f32) -> Vec<Prim> {
    let mut c = Canvas { out: Vec::new() };
    let ctr = (cx, cy);
    let face = plate_outline(shape, cx, cy, rx, ry);
    let edge = (rx.min(ry) * 0.05).max(1.0);
    if shape == PlateShape::Disc {
        c.poly(&face, mid, 0.86 * alpha);
        c.stroke(&face, true, edge * 1.1, light, 0.85 * alpha);
        c.stroke(&grow(&face, ctr, edge * 1.2), true, edge * 0.8, 0x000000, 0.35 * alpha);
        return c.out;
    }
    c.stroke(&grow(&face, ctr, edge * 3.0), true, edge * 0.9, 0xB8BEC6, 0.75 * alpha);
    c.poly(&grow(&face, ctr, edge * 1.8), 0x0A0E14, 0.92 * alpha);
    let charge = charge.clamp(0.0, 1.0);
    let top = cy - ry;
    let cut = top + 2.0 * ry * (1.0 - charge);
    // the dimmed part still to charge, then the lit part
    if charge < 0.999 {
        let dim = clip_y(&face, cut, false);
        c.poly(&dim, shade(mid, 0.72), 0.95 * alpha);
    }
    let lit = if charge >= 0.999 { face.clone() } else { clip_y(&face, cut, true) };
    if lit.len() >= 3 {
        c.poly(&lit, mid, alpha);
        // light from the top: the upper half brighter, the lowest fifth darker
        c.poly(&clip_y(&lit, cy, false), light, 0.45 * alpha);
        c.poly(&clip_y(&lit, cy + ry * 0.6, true), shade(mid, 0.35), 0.5 * alpha);
    }
    c.stroke(&grow(&face, ctr, -edge * 1.6), true, edge, lighten(light, 0.35), 0.55 * alpha);
    if charge < 0.999 && charge > 0.001 {
        let w = rx * 0.9;
        c.line((cx - w, cut), (cx + w, cut), edge, lighten(light, 0.6), 0.9 * alpha);
    }
    c.out
}

/// An icon as a white (or `color`) silhouette with a soft dark outline, the way Ragnarök draws its glyphs.
pub fn mono_icon(asset: &str, rect: &Rect, color: u32, alpha: f32) -> Option<Vec<Prim>> {
    let shapes = glyph_prims(asset, rect, color, alpha).or_else(|| icon_prims(asset, rect, color, alpha))?;
    let recolour = |p: &Prim, col: u32, a: f32, dx: f32, grow_by: f32| match p {
        Prim::Tri { p, alpha, .. } => Prim::Tri { p: p.map(|q| (q.0 + dx, q.1 + dx)), color: col, alpha: alpha * a },
        Prim::Line { a: qa, b, thick, alpha, .. } => Prim::Line { a: (qa.0 + dx, qa.1 + dx), b: (b.0 + dx, b.1 + dx), thick: thick + grow_by, color: col, alpha: alpha * a },
        Prim::Circle { c, r, fill, thick, alpha, .. } => Prim::Circle { c: (c.0 + dx, c.1 + dx), r: *r, fill: *fill, thick: thick + grow_by, color: col, alpha: alpha * a },
        Prim::Poly { pts, alpha, .. } => Prim::Poly { pts: pts.iter().map(|q| (q.0 + dx, q.1 + dx)).collect(), color: col, alpha: alpha * a },
        Prim::Grad { x0, y0, x1, y1, alpha, .. } => Prim::Grad { x0: x0 + dx, y0: y0 + dx, x1: x1 + dx, y1: y1 + dx, left: col, right: col, alpha: alpha * a },
    };
    let sh = (rect.w * 0.025).max(1.0);
    let mut out: Vec<Prim> = shapes.iter().map(|p| recolour(p, 0x000000, 0.55, sh, sh)).collect();
    out.extend(shapes.iter().map(|p| recolour(p, color, 1.0, 0.0, 0.0)));
    Some(out)
}

/// Flat white glyphs in the manner of Ragnarök's HUD icons (weapons, shield and the runic attacks), drawn in
/// `color`. Original drawings; Ragnarök's own pictures replace them when chosen in ragnarok_hud.json.
pub fn glyph_prims(asset: &str, rect: &Rect, color: u32, alpha: f32) -> Option<Vec<Prim>> {
    let (sx, sy, s) = square(rect);
    let p = |nx: f32, ny: f32| (sx + nx * s, sy + ny * s);
    let th = |f: f32| (s * f).max(1.0);
    let mut c = Canvas { out: Vec::new() };
    let a = alpha;
    match asset {
        "hud_icon_axe" => {
            // haft with a pommel, a broad bearded blade to the right and a short back blade to the left
            c.fan(p(0.45, 0.5), &[p(0.40, 0.10), p(0.49, 0.10), p(0.49, 0.86), p(0.40, 0.86)], color, a);
            c.fan(p(0.445, 0.9), &[p(0.37, 0.85), p(0.52, 0.85), p(0.50, 0.95), p(0.39, 0.95)], color, a);
            c.fan(p(0.66, 0.30), &[p(0.49, 0.13), p(0.64, 0.10), p(0.80, 0.06), p(0.90, 0.08), p(0.88, 0.24), p(0.84, 0.40), p(0.76, 0.54), p(0.68, 0.60), p(0.62, 0.46), p(0.55, 0.38), p(0.49, 0.36)], color, a);
            c.fan(p(0.31, 0.22), &[p(0.40, 0.13), p(0.26, 0.10), p(0.13, 0.13), p(0.18, 0.24), p(0.24, 0.33), p(0.40, 0.34)], color, a);
            c.line(p(0.50, 0.14), p(0.50, 0.37), th(0.025), 0x101010, 0.5 * a);
        }
        "hud_icon_blades" => {
            // two curved blades crossed, hilts down
            for dir in [-1.0f32, 1.0] {
                let q = |x: f32, y: f32| p(0.5 + dir * (x - 0.5), y);
                c.fan(q(0.45, 0.42), &[q(0.16, 0.06), q(0.30, 0.10), q(0.52, 0.40), q(0.70, 0.68), q(0.64, 0.72), q(0.42, 0.46), q(0.22, 0.20)], color, a);
                c.line(q(0.60, 0.62), q(0.78, 0.60), th(0.06), color, a);
                c.line(q(0.70, 0.70), q(0.84, 0.92), th(0.07), color, a);
            }
        }
        "hud_icon_spear" => {
            c.line(p(0.18, 0.86), p(0.66, 0.38), th(0.07), color, a);
            c.fan(p(0.76, 0.26), &[p(0.62, 0.40), p(0.70, 0.22), p(0.90, 0.10), p(0.78, 0.30)], color, a);
            c.line(p(0.56, 0.36), p(0.66, 0.46), th(0.05), color, a);
        }
        "hud_icon_shield" => {
            // the round Guardian Shield: rim, boss and spokes
            c.circle(p(0.5, 0.5), s * 0.42, false, th(0.08), color, a);
            c.circle(p(0.5, 0.5), s * 0.12, true, 0.0, color, a);
            for k in 0..8 {
                let ang = k as f32 * std::f32::consts::FRAC_PI_4;
                c.line(p(0.5 + 0.14 * ang.cos(), 0.5 + 0.14 * ang.sin()), p(0.5 + 0.36 * ang.cos(), 0.5 + 0.36 * ang.sin()), th(0.045), color, a);
            }
        }
        "hud_icon_runic_frost_wave" => {
            // a six-armed frost star with barbs
            for k in 0..6 {
                let ang = k as f32 * std::f32::consts::PI / 3.0 - std::f32::consts::FRAC_PI_2;
                let (cx, cy) = (ang.cos(), ang.sin());
                c.line(p(0.5, 0.5), p(0.5 + 0.42 * cx, 0.5 + 0.42 * cy), th(0.075), color, a);
                for side in [-0.5f32, 0.5] {
                    let b = ang + side;
                    c.line(p(0.5 + 0.26 * cx, 0.5 + 0.26 * cy), p(0.5 + 0.26 * cx + 0.13 * b.cos(), 0.5 + 0.26 * cy + 0.13 * b.sin()), th(0.05), color, a);
                }
            }
            c.circle(p(0.5, 0.5), s * 0.09, true, 0.0, color, a);
        }
        "hud_icon_runic_glacial" => {
            // ice shards bursting up from the ground
            c.fan(p(0.5, 0.55), &[p(0.42, 0.86), p(0.5, 0.08), p(0.6, 0.86)], color, a);
            c.fan(p(0.3, 0.66), &[p(0.2, 0.86), p(0.26, 0.34), p(0.4, 0.86)], color, a);
            c.fan(p(0.72, 0.66), &[p(0.62, 0.86), p(0.8, 0.30), p(0.82, 0.86)], color, a);
            c.line(p(0.12, 0.9), p(0.88, 0.9), th(0.06), color, a);
        }
        "hud_icon_runic_flame_lash" => {
            // a single tall flame with a cut-out heart
            c.fan(p(0.5, 0.6), &[p(0.5, 0.06), p(0.62, 0.28), p(0.76, 0.48), p(0.78, 0.68), p(0.66, 0.88), p(0.5, 0.94), p(0.34, 0.88), p(0.22, 0.68), p(0.26, 0.48), p(0.38, 0.36), p(0.44, 0.22)], color, a);
            c.fan(p(0.5, 0.74), &[p(0.5, 0.52), p(0.6, 0.68), p(0.58, 0.82), p(0.5, 0.86), p(0.42, 0.82), p(0.4, 0.68)], 0x101010, 0.55 * a);
        }
        "hud_icon_runic_inferno" => {
            // a ring of fire with flames licking outward
            c.circle(p(0.5, 0.5), s * 0.24, false, th(0.08), color, a);
            for k in 0..6 {
                let ang = k as f32 * std::f32::consts::PI / 3.0 + 0.3;
                let (cx, cy) = (ang.cos(), ang.sin());
                let (tx, ty) = (-cy, cx);
                c.tri(p(0.5 + 0.26 * cx - 0.08 * tx, 0.5 + 0.26 * cy - 0.08 * ty), p(0.5 + 0.46 * cx + 0.1 * tx, 0.5 + 0.46 * cy + 0.1 * ty), p(0.5 + 0.26 * cx + 0.08 * tx, 0.5 + 0.26 * cy + 0.08 * ty), color, a);
            }
        }
        "hud_icon_runic_spark" => {
            // a lightning bolt
            c.fan(p(0.48, 0.5), &[p(0.58, 0.06), p(0.30, 0.52), p(0.48, 0.52), p(0.38, 0.94), p(0.70, 0.42), p(0.52, 0.42), p(0.66, 0.06)], color, a);
        }
        "hud_icon_runic_spear_storm" => {
            // three spears driving down
            for (x, top) in [(0.28f32, 0.12f32), (0.5, 0.06), (0.72, 0.12)] {
                c.line(p(x, top), p(x, 0.66), th(0.06), color, a);
                c.fan(p(x, 0.76), &[p(x - 0.07, 0.64), p(x + 0.07, 0.64), p(x, 0.92)], color, a);
            }
        }
        _ => return None,
    }
    Some(c.out)
}

/// A Ragnarök player bar: a see-through grey housing with a dark rim, and the fill.
///
/// One segment (health, enemies): dark at its start and brightening toward the leading edge (`mid` -> `lead`),
/// lit along the bottom, with a pale tip. Several segments (the Rage meter): each full segment dark red with a bright
/// upper band and glowing ends, warming from one segment to the next; the segment still filling burns in `lead`.
pub fn rbar(rect: &Rect, frac: f32, trail: f32, mid: u32, lead: u32, segments: u32, alpha: f32) -> Vec<Prim> {
    let mut c = Canvas { out: Vec::new() };
    let (x0, y0, y1) = (rect.x, rect.y, rect.y + rect.h);
    let h = rect.h;
    let frac = frac.clamp(0.0, 1.0);
    let trail = trail.clamp(0.0, 1.0);
    let n = segments.max(1);
    let gap = if n > 1 { (h * 0.38).max(2.0) } else { 0.0 };
    let sw = (rect.w - gap * (n - 1) as f32) / n as f32;
    let seg_x = |i: u32| x0 + i as f32 * (sw + gap);
    // housing: one piece per segment, so the gaps stay dark
    let rim = (h * 0.08).max(1.0);
    for i in 0..n {
        let (a, b) = (seg_x(i), seg_x(i) + sw);
        c.rect(a, y0 - rim, b, y1 + rim, 0x000000, 0.55 * alpha);
        c.rect(a, y0, b, y1, 0x5A5658, 0.45 * alpha);
    }
    // a fraction of the whole bar to an x, skipping the gaps
    let at = |f: f32| {
        let u = f * n as f32;
        let i = (u.floor() as u32).min(n - 1);
        seg_x(i) + (u - i as f32) * sw
    };
    let fill_end = at(frac);
    if trail > frac {
        c.rect(fill_end, y0, at(trail), y1, 0xF4EBD0, 0.5 * alpha);
    }
    // from `a` to `b`, coloured by `f(x)`: a few smooth gradient pieces
    let slices = |c: &mut Canvas, a: f32, b: f32, f: &dyn Fn(f32) -> u32| {
        let n = (((b - a) / 10.0).ceil() as usize).clamp(1, 24);
        for k in 0..n {
            // inner boundaries on whole pixels, so neighbouring pieces meet without a seam
            let at = |j: usize| if j == 0 { a } else if j == n { b } else { (a + (b - a) * j as f32 / n as f32).round() };
            let (p, q) = (at(k), at(k + 1));
            c.out.push(Prim::Grad { x0: p, y0, x1: q, y1, left: f(p), right: f(q), alpha });
        }
    };
    // a soft dark blot, like the smoky core of Ragnarök's meters: nested ellipses that darken toward the middle
    let blot = |c: &mut Canvas, cx: f32, rx: f32, ry: f32, col: u32, al: f32| {
        for (k, sc) in [1.0f32, 0.88, 0.76, 0.64, 0.52, 0.4, 0.28].iter().enumerate() {
            let pts: Vec<(f32, f32)> = (0..24)
                .map(|i| {
                    let t = i as f32 * std::f32::consts::TAU / 24.0;
                    (cx + rx * sc * t.cos(), (y0 + y1) / 2.0 + ry * sc * t.sin())
                })
                .collect();
            c.poly(&pts, col, al * (0.34 + 0.05 * k as f32) * alpha);
        }
    };
    let band = |c: &mut Canvas, a: f32, b: f32, f0: f32, f1: f32, col: u32, al: f32| {
        if b > a {
            c.rect(a, y0 + h * f0, b, y0 + h * f1, col, al * alpha);
        }
    };
    if n == 1 {
        if fill_end > x0 {
            let base = shade(mid, 0.36);
            let from = x0 + (fill_end - x0) * 0.45;
            let head = (h * 0.7).min((fill_end - x0) * 0.3);
            slices(&mut c, x0, fill_end, &|x| {
                let g = ((x - from) / (fill_end - from).max(1.0)).clamp(0.0, 1.0);
                let col = mix(base, lead, 0.45 * g + 0.55 * g.powi(3));
                // the glowing head of the bar pales to white at its tip
                mix(col, 0xFFFFFF, ((x - (fill_end - head)) / head.max(1.0)).clamp(0.0, 1.0).powi(3) * 0.8)
            });
            let len = fill_end - x0;
            blot(&mut c, x0 + len * 0.3, len * 0.28, h * 0.32, shade(mid, 0.72), 0.16);
            blot(&mut c, x0 + len * 0.62, len * 0.16, h * 0.28, shade(mid, 0.72), 0.12);
            band(&mut c, x0, fill_end, 0.0, 0.14, 0xFFFFFF, 0.12);
            for (f0, f1, a) in [(0.5, 0.62, 0.2), (0.62, 0.74, 0.42), (0.74, 0.86, 0.62), (0.86, 0.94, 0.72)] {
                band(&mut c, x0, fill_end, f0, f1, lighten(mid, 0.15), a);
            }
            band(&mut c, x0, fill_end, 0.94, 1.0, 0x000000, 0.3);
        }
    } else {
        for i in 0..n {
            let (a, b) = (seg_x(i), seg_x(i) + sw);
            let end = b.min(fill_end);
            if end <= a {
                break;
            }
            if end >= b - 0.01 {
                // a full segment: dark core, bright upper band, glowing ends, warmer the further along it is
                // a full segment: bright red, a smoky dark core, glowing ends, warmer the further along it is
                let warm = mix(mid, lead, 0.14 * i as f32);
                let glow = mix(warm, 0xFF5A4C, 0.55);
                slices(&mut c, a, b, &|x| {
                    let t = (x - a) / sw;
                    let e = ((t - 0.8) / 0.2).clamp(0.0, 1.0).max(((0.12 - t) / 0.12).clamp(0.0, 1.0) * 0.7);
                    mix(warm, glow, e)
                });
                blot(&mut c, a + sw * 0.45, sw * 0.4, h * 0.4, shade(warm, 0.75), 0.5 - 0.1 * i as f32);
                band(&mut c, a, b, 0.0, 0.1, lighten(warm, 0.3), 0.45);
                band(&mut c, a, b, 0.9, 1.0, 0x000000, 0.2);
            } else {
                // the segment still filling burns in the lead colour, paling to its tip
                let fire = lead;
                slices(&mut c, a, end, &|x| {
                    let t = ((x - a) / (end - a).max(1.0)).clamp(0.0, 1.0);
                    mix(mix(fire, mid, 0.15), lighten(fire, 0.25), t)
                });
                band(&mut c, a, end, 0.0, 0.35, lighten(fire, 0.1), 0.35);
                band(&mut c, a, end, 0.5, 1.0, mix(fire, 0xE8A040, 0.6), 0.45);
            }
        }
    }
    if n > 1 && frac > 0.002 && frac < 0.999 {
        let tip = (h * 0.2).max(1.5);
        c.rect(fill_end - tip, y0, fill_end, y1, lighten(lead, 0.7), 0.95 * alpha);
    }
    c.out
}

/// Thin strokes for the few characters the HUD writes itself, in a box 0.5 wide and 1 tall.
fn strokes(ch: char) -> Vec<Vec<(f32, f32)>> {
    match ch {
        'L' => vec![vec![(0.04, 0.0), (0.04, 1.0), (0.48, 1.0)]],
        'R' => vec![
            vec![(0.04, 1.0), (0.04, 0.0), (0.3, 0.0), (0.44, 0.08), (0.48, 0.24), (0.44, 0.4), (0.3, 0.5), (0.04, 0.5)],
            vec![(0.24, 0.5), (0.5, 1.0)],
        ],
        '3' => vec![
            vec![(0.02, 0.1), (0.14, 0.0), (0.36, 0.0), (0.48, 0.12), (0.48, 0.32), (0.36, 0.46), (0.18, 0.48)],
            vec![(0.18, 0.48), (0.36, 0.5), (0.5, 0.64), (0.5, 0.86), (0.37, 1.0), (0.14, 1.0), (0.0, 0.9)],
        ],
        _ => vec![],
    }
}

/// A stick button glyph ("L3" / "R3"): white disc, dark rim and lettering, ringed by flames when `fire` > 0.
pub fn stick_button(cx: f32, cy: f32, r: f32, label: &str, fire: f32, flicker: f32, alpha: f32) -> Vec<Prim> {
    let mut c = Canvas { out: Vec::new() };
    if fire > 0.0 {
        // a soft fiery halo, a little taller above the button, with short flickering tongues
        let up = r * 0.12;
        c.circle((cx, cy - up), r * 1.5, true, 0.0, 0x9A2414, 0.35 * fire * alpha);
        c.circle((cx, cy - up * 0.5), r * 1.36, true, 0.0, 0xD03A22, 0.6 * fire * alpha);
        c.circle((cx, cy), r * 1.22, true, 0.0, 0xF06A34, 0.55 * fire * alpha);
        for k in 0..12 {
            let a = k as f32 * std::f32::consts::TAU / 12.0 + 0.2;
            let wob = 0.8 + 0.3 * ((flicker * std::f32::consts::TAU * 3.0) + k as f32 * 1.7).sin();
            let lean = if a.sin() < 0.0 { 1.2 } else { 0.7 };
            let len = r * 0.38 * wob * lean;
            let (ca, sa) = (a.cos(), a.sin());
            let base_l = (cx + (ca * 1.2 - sa * 0.22) * r, cy + (sa * 1.2 + ca * 0.22) * r);
            let base_r = (cx + (ca * 1.2 + sa * 0.22) * r, cy + (sa * 1.2 - ca * 0.22) * r);
            let tip = (cx + ca * (r * 1.2 + len), cy + sa * (r * 1.2 + len) - 0.3 * len);
            c.tri(base_l, tip, base_r, if k % 2 == 0 { 0xD8452A } else { 0xFF8A3A }, 0.7 * fire * alpha);
        }
    }
    c.circle((cx, cy), r * 1.08, true, 0.0, 0x141414, 0.95 * alpha);
    c.circle((cx, cy), r, true, 0.0, 0xF2F2F0, alpha);
    c.circle((cx, cy), r * 0.86, false, (r * 0.06).max(1.0), 0xBDBDBD, 0.8 * alpha);
    let h = r * 0.9;
    let w = h * 0.5;
    let n = label.chars().count() as f32;
    let space = w * 0.28;
    let total = n * w + (n - 1.0) * space;
    let thick = (h * 0.2).max(1.4);
    for (i, ch) in label.chars().enumerate() {
        let ox = cx - total / 2.0 + i as f32 * (w + space);
        let oy = cy - h / 2.0;
        for line in strokes(ch) {
            let pts: Vec<(f32, f32)> = line.iter().map(|q| (ox + q.0 * h, oy + q.1 * h)).collect();
            c.stroke(&pts, false, thick, 0x262626, alpha);
        }
    }
    c.out
}

/// Ragnarök's Spartan Rage prompt in `rect`: L3 + R3, both buttons on fire.
pub fn rage_prompt(rect: &Rect, flicker: f32, alpha: f32) -> Vec<Prim> {
    let r = rect.h * 0.34;
    let cy = rect.y + rect.h / 2.0;
    let (lx, rx) = (rect.x + rect.w * 0.2, rect.x + rect.w * 0.8);
    let mut out = stick_button(lx, cy, r, "L3", 1.0, flicker, alpha);
    out.extend(stick_button(rx, cy, r, "R3", 1.0, flicker + 0.37, alpha));
    let mx = (lx + rx) / 2.0;
    let arm = r * 0.42;
    let t = (r * 0.22).max(1.5);
    out.push(Prim::Line { a: (mx - arm, cy), b: (mx + arm, cy), thick: t, color: 0xF2F2F0, alpha });
    out.push(Prim::Line { a: (mx, cy - arm), b: (mx, cy + arm), thick: t, color: 0xF2F2F0, alpha });
    out
}

/// The same shapes turned by `angle` radians about `centre` (for thrown weapons that spin in flight).
pub fn rotated(prims: &[Prim], centre: (f32, f32), angle: f32) -> Vec<Prim> {
    let (sn, cs) = angle.sin_cos();
    let rot = |q: (f32, f32)| {
        let (dx, dy) = (q.0 - centre.0, q.1 - centre.1);
        (centre.0 + dx * cs - dy * sn, centre.1 + dx * sn + dy * cs)
    };
    prims
        .iter()
        .map(|p| match p {
            Prim::Tri { p, color, alpha } => Prim::Tri { p: [rot(p[0]), rot(p[1]), rot(p[2])], color: *color, alpha: *alpha },
            Prim::Line { a, b, thick, color, alpha } => Prim::Line { a: rot(*a), b: rot(*b), thick: *thick, color: *color, alpha: *alpha },
            Prim::Circle { c, r, fill, thick, color, alpha } => Prim::Circle { c: rot(*c), r: *r, fill: *fill, thick: *thick, color: *color, alpha: *alpha },
            Prim::Poly { pts, color, alpha } => Prim::Poly { pts: pts.iter().map(|q| rot(*q)).collect(), color: *color, alpha: *alpha },
            Prim::Grad { x0, y0, x1, y1, left, right, alpha } => Prim::Poly { pts: vec![rot((*x0, *y0)), rot((*x1, *y0)), rot((*x1, *y1)), rot((*x0, *y1))], color: mix(*left, *right, 0.5), alpha: *alpha },
        })
        .collect()
}

/// Vector art for the HUD asset ids that have a drawing; `None` for any other id.
pub fn icon_prims(asset: &str, rect: &Rect, color: u32, alpha: f32) -> Option<Vec<Prim>> {
    let (sx, sy, s) = square(rect);
    let p = |nx: f32, ny: f32| (sx + nx * s, sy + ny * s);
    let th = |f: f32| (s * f).max(1.0);
    let mut c = Canvas { out: Vec::new() };
    // The HUD sheet tints icons white; the art keeps its own metal palette and only borrows a little of the tint.
    let steel = mix(0xA3AEBA, color, 0.12);
    let dark = 0x1E242B;
    let wood = 0x7A5636;
    let glint = 0xEAF6FF;
    match asset {
        "hud_icon_axe" => {
            // haft
            c.fan(p(0.5, 0.5), &[p(0.46, 0.1), p(0.54, 0.1), p(0.54, 0.96), p(0.46, 0.96)], wood, 0.98 * alpha);
            c.stroke(&[p(0.46, 0.1), p(0.54, 0.1), p(0.54, 0.96), p(0.46, 0.96)], true, th(0.018), dark, 0.9 * alpha);
            for k in 0..4 {
                let y = 0.62 + 0.07 * k as f32;
                c.line(p(0.46, y), p(0.54, y + 0.03), th(0.014), dark, 0.7 * alpha);
            }
            // blade, back spike and the light on its edge
            let blade = [p(0.54, 0.16), p(0.72, 0.06), p(0.90, 0.13), p(0.98, 0.30), p(0.93, 0.46), p(0.78, 0.54), p(0.54, 0.47)];
            c.fan(p(0.72, 0.30), &blade, steel, alpha);
            c.tri(p(0.46, 0.2), p(0.26, 0.3), p(0.46, 0.4), steel, alpha);
            c.stroke(&blade, true, th(0.025), dark, 0.9 * alpha);
            c.stroke(&[p(0.72, 0.06), p(0.90, 0.13), p(0.98, 0.30), p(0.93, 0.46)], false, th(0.03), glint, 0.95 * alpha);
            for k in 0..3 {
                let y = 0.2 + 0.1 * k as f32;
                c.line(p(0.60, y), p(0.80 - 0.02 * k as f32, y + 0.05), th(0.018), glint, 0.7 * alpha);
            }
        }
        "hud_icon_blades" => {
            for dir in [-1.0f32, 1.0] {
                let end = (0.5 + dir * 0.34, 0.36);
                let start = (0.5, 0.94);
                // chain links
                let links = 8;
                for i in 0..links {
                    let t = i as f32 / (links - 1) as f32;
                    let q = (start.0 + (end.0 - start.0) * t, start.1 + (end.1 - start.1) * t);
                    c.circle(p(q.0, q.1), s * 0.032, false, th(0.016), steel, 0.9 * alpha);
                }
                // blade on the end of the chain, pointing away from the hand
                let d = ((end.0 - start.0), (end.1 - start.1));
                let len = (d.0 * d.0 + d.1 * d.1).sqrt();
                let d = (d.0 / len, d.1 / len);
                let perp = (-d.1, d.0);
                let tip = (end.0 + d.0 * 0.34, end.1 + d.1 * 0.34);
                let a = (end.0 + perp.0 * 0.075, end.1 + perp.1 * 0.075);
                let b = (end.0 - perp.0 * 0.075, end.1 - perp.1 * 0.075);
                c.tri(p(a.0, a.1), p(tip.0, tip.1), p(b.0, b.1), steel, alpha);
                c.stroke(&[p(a.0, a.1), p(tip.0, tip.1), p(b.0, b.1)], true, th(0.02), dark, 0.9 * alpha);
                c.line(p(end.0, end.1), p(tip.0, tip.1), th(0.016), 0xFF8A3A, 0.95 * alpha);
            }
        }
        "hud_icon_spear" => {
            let (a, b) = ((0.10, 0.94), (0.78, 0.26));
            c.line(p(a.0, a.1), p(b.0, b.1), th(0.055), dark, 0.95 * alpha);
            c.line(p(a.0, a.1), p(b.0, b.1), th(0.032), wood, 0.98 * alpha);
            let d = (0.7071f32, -0.7071f32);
            let perp = (0.7071f32, 0.7071f32);
            let base = b;
            let tip = (base.0 + d.0 * 0.26, base.1 + d.1 * 0.26);
            let head = [
                (base.0 + perp.0 * 0.095 + d.0 * 0.03, base.1 + perp.1 * 0.095 + d.1 * 0.03),
                tip,
                (base.0 - perp.0 * 0.095 + d.0 * 0.03, base.1 - perp.1 * 0.095 + d.1 * 0.03),
                (base.0 - d.0 * 0.05, base.1 - d.1 * 0.05),
            ];
            let hp: Vec<(f32, f32)> = head.iter().map(|q| p(q.0, q.1)).collect();
            c.fan(p(base.0 + d.0 * 0.06, base.1 + d.1 * 0.06), &hp, steel, alpha);
            c.stroke(&hp, true, th(0.022), dark, 0.9 * alpha);
            c.line(p(base.0, base.1), p(tip.0, tip.1), th(0.014), glint, 0.9 * alpha);
            // rune marks down the shaft
            for k in 1..4 {
                let t = 0.2 + 0.17 * k as f32;
                let q = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                c.line(p(q.0 - 0.03, q.1 - 0.03), p(q.0 + 0.03, q.1 + 0.03), th(0.016), 0x7FD8FF, 0.85 * alpha);
            }
        }
        "hud_icon_shield" => {
            let outline = [p(0.18, 0.12), p(0.82, 0.12), p(0.88, 0.45), p(0.72, 0.76), p(0.5, 0.94), p(0.28, 0.76), p(0.12, 0.45)];
            c.fan(p(0.5, 0.5), &outline, dark, 0.92 * alpha);
            c.stroke(&outline, true, th(0.045), steel, alpha);
            let inner = [p(0.26, 0.22), p(0.74, 0.22), p(0.78, 0.46), p(0.64, 0.68), p(0.5, 0.8), p(0.36, 0.68), p(0.22, 0.46)];
            c.stroke(&inner, true, th(0.016), glint, 0.55 * alpha);
            c.line(p(0.5, 0.24), p(0.5, 0.78), th(0.03), glint, 0.8 * alpha);
            c.stroke(&[p(0.3, 0.42), p(0.5, 0.28), p(0.7, 0.42)], false, th(0.03), glint, 0.8 * alpha);
            c.circle(p(0.5, 0.5), s * 0.075, true, 0.0, steel, alpha);
        }
        "hud_crosshair_throw" => {
            let m = p(0.5, 0.5);
            c.circle(m, s * 0.30, false, th(0.035), color, 0.95 * alpha);
            c.circle(m, s * 0.22, false, th(0.012), 0xFFFFFF, 0.35 * alpha);
            c.circle(m, s * 0.035, true, 0.0, lighten(color, 0.5), alpha);
            for k in 0..4 {
                let a = k as f32 * std::f32::consts::FRAC_PI_2;
                c.line((m.0 + s * 0.36 * a.cos(), m.1 + s * 0.36 * a.sin()), (m.0 + s * 0.48 * a.cos(), m.1 + s * 0.48 * a.sin()), th(0.04), color, alpha);
            }
        }
        "hud_enemy_stunned" => {
            let m = p(0.5, 0.5);
            let d = s * 0.46;
            let pts = [(m.0, m.1 - d), (m.0 + d, m.1), (m.0, m.1 + d), (m.0 - d, m.1)];
            c.fan(m, &pts, 0x7A0F0F, 0.8 * alpha);
            c.stroke(&pts, true, th(0.05), color, alpha);
            c.line(p(0.5, 0.28), p(0.5, 0.58), th(0.09), 0xFFFFFF, alpha);
            c.circle(p(0.5, 0.72), s * 0.05, true, 0.0, 0xFFFFFF, alpha);
        }
        _ => return None,
    }
    Some(c.out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: Rect = Rect { x: 100.0, y: 50.0, w: 80.0, h: 80.0 };

    fn corners(p: &Prim) -> Vec<(f32, f32)> {
        match p {
            Prim::Tri { p, .. } => p.to_vec(),
            Prim::Line { a, b, .. } => vec![*a, *b],
            Prim::Circle { c, r, .. } => vec![(c.0 - r, c.1 - r), (c.0 + r, c.1 + r)],
            Prim::Poly { pts, .. } => pts.clone(),
            Prim::Grad { x0, y0, x1, y1, .. } => vec![(*x0, *y0), (*x1, *y1)],
        }
    }

    fn inside(id: &str, prims: &[Prim], r: &Rect, slack: f32) {
        for p in prims {
            for (x, y) in corners(p) {
                assert!(x >= r.x - slack && x <= r.x + r.w + slack && y >= r.y - slack && y <= r.y + r.h + slack, "{id}: ({x}, {y}) outside {r:?}");
            }
        }
    }

    #[test]
    fn every_hud_icon_has_a_drawing_inside_its_rect() {
        for id in ["hud_icon_axe", "hud_icon_blades", "hud_icon_spear", "hud_icon_shield", "hud_crosshair_throw", "hud_enemy_stunned"] {
            let prims = icon_prims(id, &R, 0x9FD8FF, 1.0).unwrap_or_else(|| panic!("{id} has no drawing"));
            assert!(prims.len() >= 6, "{id}");
            inside(id, &prims, &R, 6.0);
        }
        assert!(icon_prims("hud_btn_r3", &R, 0xFFFFFF, 1.0).is_none());
    }

    #[test]
    fn every_runic_and_weapon_has_a_white_glyph_inside_its_rect() {
        let mut ids: Vec<&str> = crate::sheets::RUNICS.iter().filter_map(|r| r.icon).collect();
        ids.extend(["hud_icon_axe", "hud_icon_blades", "hud_icon_spear", "hud_icon_shield"]);
        for id in ids {
            let g = glyph_prims(id, &R, 0xFFFFFF, 1.0).unwrap_or_else(|| panic!("{id} has no glyph"));
            inside(id, &g, &R, 4.0);
            // the mono icon is the glyph plus its shadow, in the glyph colour
            let m = mono_icon(id, &R, 0xFFFFFF, 1.0).unwrap();
            assert_eq!(m.len(), g.len() * 2, "{id}");
        }
    }

    #[test]
    fn segmented_bar_fills_its_segments_in_order_and_keeps_the_gaps_dark() {
        let r = Rect { x: 0.0, y: 0.0, w: 400.0, h: 13.0 };
        let fill_right = |prims: &[Prim]| prims.iter().filter_map(|p| if let Prim::Grad { x1, .. } = p { Some(*x1) } else { None }).fold(0.0f32, f32::max);
        // 4 segments with gaps of 0.38 x the height between them; 2.5 segments end half way into the third
        let gap = 13.0 * 0.38;
        let sw = (400.0 - 3.0 * gap) / 4.0;
        let half = rbar(&r, 0.625, 0.625, 0xA82A26, 0xFF8954, 4, 1.0);
        assert!((fill_right(&half) - (2.0 * (sw + gap) + 0.5 * sw)).abs() < 0.01, "{}", fill_right(&half));
        // no fill inside a gap
        for p in &half {
            if let Prim::Grad { x0, x1, .. } = p {
                assert!(!(*x0 < 97.0 && *x1 > 98.0), "fill across the first gap: {x0}..{x1}");
            }
        }
        let full = rbar(&r, 1.0, 1.0, 0xA82A26, 0xFF8954, 4, 1.0);
        assert!((fill_right(&full) - 400.0).abs() < 0.6);
        assert!(fill_right(&rbar(&r, 0.0, 0.0, 0x55914A, 0x8EE05A, 1, 1.0)) == 0.0, "an empty bar has no fill");
    }

    #[test]
    fn a_recharging_plate_is_lit_from_the_bottom_up() {
        let lit_top = |charge: f32| {
            plate(PlateShape::Diamond, 50.0, 50.0, 30.0, 26.0, 0x4A7FB4, 0x8AB4DC, charge, 1.0)
                .iter()
                .filter_map(|p| if let Prim::Poly { pts, color: 0x4A7FB4, .. } = p { Some(pts.iter().map(|q| q.1).fold(f32::MAX, f32::min)) } else { None })
                .fold(f32::MAX, f32::min)
        };
        assert!((lit_top(1.0) - 24.0).abs() < 0.01, "a ready plate is lit to its top");
        assert!((lit_top(0.5) - 50.0).abs() < 0.01, "half charged: lit up to the middle");
        assert!(lit_top(0.0) > 75.0, "nothing lit when it has just been used");
    }

    #[test]
    fn rotation_keeps_the_shape_and_moves_it_about_the_centre() {
        let prims = icon_prims("hud_icon_axe", &R, 0xFFFFFF, 1.0).unwrap();
        let centre = (R.x + R.w / 2.0, R.y + R.h / 2.0);
        let turned = rotated(&prims, centre, std::f32::consts::FRAC_PI_2);
        assert_eq!(prims.len(), turned.len());
        let (Prim::Line { a: a0, b: b0, .. }, Prim::Line { a: a1, b: b1, .. }) = (prims.iter().find(|p| matches!(p, Prim::Line { .. })).unwrap(), turned.iter().find(|p| matches!(p, Prim::Line { .. })).unwrap()) else { unreachable!() };
        let len = |a: &(f32, f32), b: &(f32, f32)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
        assert!((len(a0, b0) - len(a1, b1)).abs() < 1e-3, "a quarter turn must not change lengths");
        assert!(len(a0, a1) > 0.1, "points must actually move");
    }
}
