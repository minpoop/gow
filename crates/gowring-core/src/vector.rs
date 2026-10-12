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
    /// One of Ragnarök's own HUD pieces (a white shape, coloured `color`) mapped onto a quad: texture coordinate
    /// `uv[i]` lands on `pts[i]`. Drawn only when the renderer has the picture (see `art_flags`).
    Tex { asset: &'static str, pts: [(f32, f32); 4], uv: [(f32, f32); 4], color: u32, alpha: f32 },
}

/// Which of Ragnarök's own HUD pieces the renderer has loaded, as bits: the runic plates, the shield's octagon, Rage's
/// mark, the L3 / R3 buttons, the Triangle. Each falls back to the mod's own drawing when its pieces are missing.
pub const ART_PLATE: u32 = 1;
pub const ART_RELIC: u32 = 2;
pub const ART_RAGE: u32 = 4;
pub const ART_STICKS: u32 = 8;
pub const ART_TRIANGLE: u32 = 16;

const ART_GROUPS: [(u32, &[&str]); 5] = [
    (ART_PLATE, &["hud_part_plate_face", "hud_part_plate_housing", "hud_part_plate_spec", "hud_part_plate_shadow"]),
    (ART_RELIC, &["hud_part_relic_face", "hud_part_relic_spec", "hud_part_relic_shadow"]),
    (ART_RAGE, &["hud_icon_rage"]),
    (ART_STICKS, &["hud_btn_l3", "hud_btn_r3"]),
    (ART_TRIANGLE, &["hud_btn_triangle"]),
];

/// The `ART_*` bits for the pieces `has` reports loaded; a group counts only when all of its pieces are there.
pub fn art_flags(has: impl Fn(&str) -> bool) -> u32 {
    ART_GROUPS.iter().filter(|(_, ids)| ids.iter().all(|id| has(id))).fold(0, |f, (bit, _)| f | bit)
}

/// Ragnarök's pieces that are placed by their texel coordinates, so the renderer must keep them whole (not cropped).
pub fn is_placed_piece(asset: &str) -> bool {
    ART_GROUPS.iter().any(|(_, ids)| ids.contains(&asset))
}

/// A 64x64 piece placed so its texel `pivot` lands on `centre`, turned `q` quarter turns clockwise, then scaled by
/// `sx` / `sy` screen pixels per texel along the screen's axes.
fn piece(asset: &'static str, pivot: (f32, f32), centre: (f32, f32), sx: f32, sy: f32, q: u32, color: u32, alpha: f32) -> Prim {
    let at = |x: f32, y: f32| {
        let (mut lx, mut ly) = (x - pivot.0, y - pivot.1);
        for _ in 0..q {
            (lx, ly) = (-ly, lx);
        }
        (centre.0 + lx * sx, centre.1 + ly * sy)
    };
    // The game's sampler wraps, so a texture's outermost half texel blends with the opposite edge. Sample from half a
    // texel in; an edge on the pivot (where the turned quarters meet) still reaches the pivot, so they meet without a gap.
    let edge = |t: f32, pivot: f32| (if t == pivot { t } else { t.clamp(0.5, 63.5) }, t.clamp(0.5, 63.5) / 64.0);
    let corners = [(0.0, 0.0), (64.0, 0.0), (64.0, 64.0), (0.0, 64.0)].map(|(x, y)| {
        let ((px, u), (py, v)) = (edge(x, pivot.0), edge(y, pivot.1));
        (at(px, py), (u, v))
    });
    Prim::Tex { asset, pts: corners.map(|c| c.0), uv: corners.map(|c| c.1), color, alpha }
}

/// The part of an upright textured quad (turned by whole quarter turns only) between y = `top` and y = `bottom`, its
/// texture coordinates cut to match; `None` when nothing is left.
fn tex_band(p: &Prim, top: f32, bottom: f32) -> Option<Prim> {
    let Prim::Tex { asset, pts, uv, color, alpha } = p else { return None };
    let (y0, y1) = (pts.iter().map(|q| q.1).fold(f32::MAX, f32::min), pts.iter().map(|q| q.1).fold(f32::MIN, f32::max));
    let (a, b) = (top.max(y0), bottom.min(y1));
    if b - a < 0.01 {
        return None;
    }
    // texture coordinates are affine over the quad: uv(p) = uv0 + s * (uv1 - uv0) + t * (uv3 - uv0)
    let (e1, e2) = ((pts[1].0 - pts[0].0, pts[1].1 - pts[0].1), (pts[3].0 - pts[0].0, pts[3].1 - pts[0].1));
    let det = e1.0 * e2.1 - e1.1 * e2.0;
    if det.abs() < 1e-6 {
        return None;
    }
    let uv_at = |q: (f32, f32)| {
        let (dx, dy) = (q.0 - pts[0].0, q.1 - pts[0].1);
        let s = (dx * e2.1 - dy * e2.0) / det;
        let t = (e1.0 * dy - e1.1 * dx) / det;
        (uv[0].0 + s * (uv[1].0 - uv[0].0) + t * (uv[3].0 - uv[0].0), uv[0].1 + s * (uv[1].1 - uv[0].1) + t * (uv[3].1 - uv[0].1))
    };
    let cut = pts.map(|q| (q.0, q.1.clamp(a, b)));
    Some(Prim::Tex { asset, pts: cut, uv: cut.map(uv_at), color: *color, alpha: *alpha })
}

fn mix(c: u32, to: u32, t: f32) -> u32 {
    let ch = |s: u32| {
        let a = ((c >> s) & 255) as f32;
        let b = ((to >> s) & 255) as f32;
        (a + (b - a) * t).clamp(0.0, 255.0) as u32
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}
/// `a` blended toward `b` by `t` (0..1).
pub fn lerp_color(a: u32, b: u32, t: f32) -> u32 {
    mix(a, b, t.clamp(0.0, 1.0))
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
    plate_fx(shape, cx, cy, rx, ry, mid, light, charge, 0.0, alpha)
}

/// `plate` with a white `flash` (0..1) over it. A plate still charging is drawn grey, as Ragnarök greys a runic that is
/// recharging; the part already recharged rises from the bottom in a cooler grey, and the colour returns when ready.
pub fn plate_fx(shape: PlateShape, cx: f32, cy: f32, rx: f32, ry: f32, mid: u32, light: u32, charge: f32, flash: f32, alpha: f32) -> Vec<Prim> {
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
    let cooling = charge < 0.999;
    let (mid, light) = if cooling { (mix(shade(mid, 0.35), 0x707378, 0.75), mix(light, 0x9A9EA4, 0.75)) } else { (mid, light) };
    if cooling {
        let dim = clip_y(&face, cut, false);
        c.poly(&dim, mix(shade(mid, 0.55), 0x2A2C30, 0.5), 0.92 * alpha);
    }
    let lit = if cooling { clip_y(&face, cut, true) } else { face.clone() };
    if lit.len() >= 3 {
        c.poly(&lit, mid, alpha);
        // light from the top: the upper half brighter, the lowest fifth darker
        c.poly(&clip_y(&lit, cy, false), light, 0.45 * alpha);
        c.poly(&clip_y(&lit, cy + ry * 0.6, true), shade(mid, 0.35), 0.5 * alpha);
    }
    if flash > 0.0 {
        c.poly(&face, 0xF4FAFF, 0.85 * flash * alpha);
        c.stroke(&grow(&face, ctr, edge * 3.0), true, edge * 2.0, 0xFFFFFF, 0.8 * flash * alpha);
    }
    c.stroke(&grow(&face, ctr, -edge * 1.6), true, edge, lighten(light, 0.35), 0.55 * alpha);
    if charge < 0.999 && charge > 0.001 {
        let w = rx * 0.9;
        c.line((cx - w, cut), (cx + w, cut), edge, lighten(light, 0.6), 0.9 * alpha);
    }
    c.out
}

/// A runic plate in its weapon's colours, with an optional soft halo around it (`aura` 0..1; the Blades' plates glow).
/// `art`: the `ART_*` pieces loaded; with `ART_PLATE` the plate is built from Ragnarök's own pieces.
pub fn runic_plate(cx: f32, cy: f32, rx: f32, ry: f32, mid: u32, light: u32, charge: f32, aura: f32, alpha: f32, art: u32) -> Vec<Prim> {
    runic_plate_fx(cx, cy, rx, ry, mid, light, charge, aura, 0.0, alpha, art)
}

/// `runic_plate` with a white `flash` (used, or ready again).
pub fn runic_plate_fx(cx: f32, cy: f32, rx: f32, ry: f32, mid: u32, light: u32, charge: f32, aura: f32, flash: f32, alpha: f32, art: u32) -> Vec<Prim> {
    let mut out = Vec::new();
    if aura > 0.0 && charge >= 0.999 {
        let face = plate_outline(PlateShape::Diamond, cx, cy, rx, ry);
        let glow = mix(light, 0xFF7A2A, 0.5);
        for (d, a) in [(rx * 0.4, 0.16f32), (rx * 0.26, 0.26), (rx * 0.13, 0.4)] {
            out.push(Prim::Poly { pts: grow(&face, (cx, cy), d), color: glow, alpha: a * aura * alpha * (0.35 + 0.65 * charge.clamp(0.0, 1.0)) });
        }
    }
    if art & ART_PLATE != 0 {
        out.extend(tex_plate(PlateShape::Diamond, cx, cy, rx, ry, mid, light, charge, flash, alpha));
    } else {
        out.extend(plate_fx(PlateShape::Diamond, cx, cy, rx, ry, mid, light, charge, flash, alpha));
    }
    out
}

/// The shield's octagon in `lead`'s green: Ragnarök's own pieces with `ART_RELIC`, otherwise the mod's drawing.
pub fn relic_plate(cx: f32, cy: f32, rx: f32, ry: f32, lead: u32, alpha: f32, art: u32) -> Vec<Prim> {
    if art & ART_RELIC != 0 {
        tex_plate(PlateShape::Octagon, cx, cy, rx, ry, lead, lead, 1.0, 0.0, alpha)
    } else {
        plate(PlateShape::Octagon, cx, cy, rx, ry, shade(lead, 0.35), lead, 1.0, alpha)
    }
}

/// A runic plate (`Diamond`) or the shield's octagon (`Octagon`) built from Ragnarök's own pieces, the way its HUD
/// layers them: soft shadow, the face in the plate's colour lit from the top, the highlight, the silver outline. Each
/// face, outline and shadow piece is one quarter, turned four times about the plate's centre. Charging and the flash
/// behave as in `plate_fx`.
fn tex_plate(shape: PlateShape, cx: f32, cy: f32, rx: f32, ry: f32, mid: u32, light: u32, charge: f32, flash: f32, alpha: f32) -> Vec<Prim> {
    let mut out = Vec::new();
    let ctr = (cx, cy);
    let diamond = shape == PlateShape::Diamond;
    // how many texels of each piece reach from the centre to the face's edge (measured on the pieces)
    let (face_id, shadow_id, spec_id) = if diamond { ("hud_part_plate_face", "hud_part_plate_shadow", "hud_part_plate_spec") } else { ("hud_part_relic_face", "hud_part_relic_shadow", "hud_part_relic_spec") };
    let (fx, fy) = if diamond { (rx / 35.0, ry / 35.0) } else { (rx / 31.0, ry / 32.0) };
    let spec_reach = if diamond { 29.0 } else { 27.0 };
    // The face pieces are plain white: the shading is fitted to the player's screenshot (rim colours within about 5
    // levels): the runic face a little darker than its colour and lit strongly from the top; the shield's octagon a
    // greyed green, lit faintly.
    let (mid, light_a, spec_a) = if diamond { (shade(mid, 0.15), 0.55, 0.2) } else { (mix(shade(light, 0.25), 0x6E7064, 0.3), 0.15, 0.2) };
    let quarters = |id: &'static str, sx: f32, sy: f32, color: u32, a: f32| -> Vec<Prim> { (0..4).map(|q| piece(id, (0.0, 64.0), ctr, sx, sy, q, color, a)).collect() };
    out.extend(quarters(shadow_id, fx, fy, 0x000000, 0.8 * alpha));
    let outline = plate_outline(shape, cx, cy, rx, ry);
    let edge = (rx.min(ry) * 0.05).max(1.0);
    if !diamond {
        // the octagon has no outline piece: a dark border under the face
        out.push(Prim::Poly { pts: grow(&outline, ctr, edge * 1.8), color: 0x0A0E14, alpha: 0.92 * alpha });
    }
    let charge = charge.clamp(0.0, 1.0);
    let cooling = charge < 0.999;
    let (top, bottom) = (cy - ry, cy + ry);
    let cut = top + 2.0 * ry * (1.0 - charge);
    let (mid, light) = if cooling { (mix(shade(mid, 0.35), 0x707378, 0.75), mix(light, 0x9A9EA4, 0.75)) } else { (mid, light) };
    let face = |color: u32, a: f32| quarters(face_id, fx, fy, color, a);
    let lit_from = if cooling {
        // the part still to charge, dimmed; the part already charged rises from the bottom
        out.extend(face(mix(shade(mid, 0.55), 0x2A2C30, 0.5), 0.92 * alpha));
        cut
    } else {
        top
    };
    let lit = face(mid, alpha);
    out.extend(lit.iter().filter_map(|p| tex_band(p, lit_from, bottom)));
    // light from the top: the lit tone over the face, strongest at the top and gone at the bottom, in thin bands
    let bands = 12;
    for i in 0..bands {
        let (a, b) = (top + 2.0 * ry * i as f32 / bands as f32, top + 2.0 * ry * (i + 1) as f32 / bands as f32);
        let strength = light_a * (1.0 - (i as f32 + 0.5) / bands as f32);
        out.extend(face(light, strength * alpha).iter().filter_map(|p| tex_band(p, a.max(lit_from), b)));
    }
    if flash > 0.0 {
        out.extend(face(0xF4FAFF, 0.85 * flash * alpha));
    }
    out.push(piece(spec_id, (32.0, 32.0), ctr, rx / spec_reach, ry / spec_reach, 0, 0xFFFFFF, spec_a * alpha));
    let silver = lerp_color(0xD0D6DE, 0xFFFFFF, flash);
    if diamond {
        // the outline piece sits a fixed step outside the face
        let gap = rx * 0.15;
        out.extend(quarters("hud_part_plate_housing", (rx + gap) / 58.0, (ry + gap) / 58.0, silver, 0.9 * alpha));
    } else {
        let ring = grow(&outline, ctr, edge * 3.0);
        for i in 0..ring.len() {
            out.push(Prim::Line { a: ring[i], b: ring[(i + 1) % ring.len()], thick: edge * 0.9, color: silver, alpha: 0.75 * alpha });
        }
    }
    if cooling && charge > 0.001 {
        // the bright edge of the charge, across the face's width at that height
        let xs: Vec<f32> = clip_y(&outline, cut, true).iter().filter(|q| (q.1 - cut).abs() < 1e-3).map(|q| q.0).collect();
        if let (Some(x0), Some(x1)) = (xs.iter().copied().reduce(f32::min), xs.iter().copied().reduce(f32::max)) {
            out.push(Prim::Line { a: (x0 + edge, cut), b: (x1 - edge, cut), thick: edge, color: lighten(light, 0.6), alpha: 0.9 * alpha });
        }
    }
    out
}

/// One of Ragnarök's whole pictures (`asset`, a white shape) centred on (`cx`, `cy`), `size` pixels across.
pub fn tex_icon(asset: &'static str, cx: f32, cy: f32, size: f32, color: u32, alpha: f32) -> Prim {
    piece(asset, (32.0, 32.0), (cx, cy), size / 64.0, size / 64.0, 0, color, alpha)
}

/// One of Ragnarök's button pictures (`hud_btn_*`: a white disc with the button's mark cut out) over a dark disc, so
/// the mark reads dark; `r` is the white disc's radius.
pub fn tex_button(asset: &'static str, cx: f32, cy: f32, r: f32, alpha: f32) -> Vec<Prim> {
    // the button's disc is 26 texels across from its centre in the 64-texel picture
    vec![Prim::Circle { c: (cx, cy), r: r * 0.98, fill: true, thick: 0.0, color: 0x161616, alpha: 0.94 * alpha }, tex_icon(asset, cx, cy, r * 64.0 / 26.0, 0xF2F2F0, alpha)]
}

/// A PlayStation face button: white disc, dark rim, a dark triangle (Ragnarök's recall / detonate prompt). With
/// `ART_TRIANGLE` in `art`, Ragnarök's own Triangle.
pub fn face_button(cx: f32, cy: f32, r: f32, alpha: f32, art: u32) -> Vec<Prim> {
    if art & ART_TRIANGLE != 0 {
        return tex_button("hud_btn_triangle", cx, cy, r, alpha);
    }
    let mut c = Canvas { out: Vec::new() };
    c.circle((cx, cy), r * 1.1, true, 0.0, 0x161616, 0.9 * alpha);
    c.circle((cx, cy), r, true, 0.0, 0xF2F2F0, alpha);
    let tri: Vec<(f32, f32)> = (0..3)
        .map(|k| {
            let a = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::TAU / 3.0;
            (cx + r * 0.55 * a.cos(), cy + r * 0.08 + r * 0.55 * a.sin())
        })
        .collect();
    c.stroke(&tri, true, (r * 0.14).max(1.2), 0x262626, alpha);
    c.out
}

/// Spartan Rage's mark for the weapon disc while Rage burns: a fierce face with the red stripe over one eye.
pub fn rage_mark(rect: &Rect, alpha: f32) -> Vec<Prim> {
    let (sx, sy, s) = square(rect);
    let p = |nx: f32, ny: f32| (sx + nx * s, sy + ny * s);
    let mut c = Canvas { out: Vec::new() };
    let head = [p(0.3, 0.1), p(0.7, 0.1), p(0.82, 0.34), p(0.78, 0.6), p(0.64, 0.92), p(0.36, 0.92), p(0.22, 0.6), p(0.18, 0.34)];
    c.fan(p(0.5, 0.5), &head, 0x101010, 0.6 * alpha);
    let head_in: Vec<(f32, f32)> = head.iter().map(|q| (q.0 + (p(0.5, 0.5).0 - q.0) * 0.06, q.1 + (p(0.5, 0.5).1 - q.1) * 0.06)).collect();
    c.fan(p(0.5, 0.5), &head_in, 0xF2F2F0, alpha);
    // angry brows and eyes, the set mouth, the beard's edge
    c.fan(p(0.36, 0.44), &[p(0.24, 0.36), p(0.46, 0.44), p(0.44, 0.5), p(0.26, 0.44)], 0x1A1A1A, alpha);
    c.fan(p(0.64, 0.44), &[p(0.76, 0.36), p(0.54, 0.44), p(0.56, 0.5), p(0.74, 0.44)], 0x1A1A1A, alpha);
    c.line(p(0.38, 0.74), p(0.62, 0.74), (s * 0.05).max(1.0), 0x1A1A1A, alpha);
    c.stroke(&[p(0.26, 0.64), p(0.34, 0.86), p(0.5, 0.94), p(0.66, 0.86), p(0.74, 0.64)], false, (s * 0.035).max(1.0), 0x1A1A1A, 0.8 * alpha);
    // the red stripe from the crown down over the left eye
    c.fan(p(0.4, 0.34), &[p(0.36, 0.11), p(0.45, 0.11), p(0.44, 0.6), p(0.36, 0.6)], 0xC8281E, alpha);
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
        Prim::Tex { asset, pts, uv, alpha, .. } => Prim::Tex { asset, pts: pts.map(|q| (q.0 + dx, q.1 + dx)), uv: *uv, color: col, alpha: alpha * a },
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
        "hud_icon_runic_rime_ring" => {
            // a ring of rime with frost barbs pointing out
            c.circle(p(0.5, 0.5), s * 0.24, false, th(0.07), color, a);
            for k in 0..8 {
                let ang = k as f32 * std::f32::consts::FRAC_PI_4;
                let (cx, cy) = (ang.cos(), ang.sin());
                c.line(p(0.5 + 0.28 * cx, 0.5 + 0.28 * cy), p(0.5 + 0.44 * cx, 0.5 + 0.44 * cy), th(0.05), color, a);
                c.circle(p(0.5 + 0.44 * cx, 0.5 + 0.44 * cy), s * 0.035, true, 0.0, color, a);
            }
            c.circle(p(0.5, 0.5), s * 0.07, true, 0.0, color, a);
        }
        "hud_icon_runic_ice_lance" => {
            // a long ice lance flying up and to the right, with shards breaking off its base
            c.fan(p(0.46, 0.54), &[p(0.08, 0.92), p(0.22, 0.56), p(0.92, 0.08), p(0.44, 0.78)], color, a);
            c.tri(p(0.08, 0.5), p(0.2, 0.46), p(0.16, 0.36), color, a);
            c.tri(p(0.5, 0.92), p(0.54, 0.8), p(0.64, 0.86), color, a);
        }
        "hud_icon_runic_winters_maw" => {
            // jaws of ice: fangs down from the top, fangs up from the bottom
            c.line(p(0.1, 0.2), p(0.9, 0.2), th(0.08), color, a);
            c.line(p(0.1, 0.84), p(0.9, 0.84), th(0.08), color, a);
            for i in 0..4 {
                let x = 0.2 + 0.2 * i as f32;
                c.tri(p(x - 0.08, 0.22), p(x + 0.08, 0.22), p(x, 0.5), color, a);
                let x = 0.3 + 0.2 * i as f32;
                if x < 0.85 {
                    c.tri(p(x - 0.08, 0.82), p(x + 0.08, 0.82), p(x, 0.56), color, a);
                }
            }
        }
        "hud_icon_runic_frostfall" => {
            // a great icicle falling, with the cracks of its landing
            c.fan(p(0.5, 0.4), &[p(0.3, 0.06), p(0.7, 0.06), p(0.58, 0.6), p(0.5, 0.86), p(0.42, 0.6)], color, a);
            for (dx, dy) in [(-0.3f32, 0.02f32), (-0.18, 0.08), (0.18, 0.08), (0.3, 0.02)] {
                c.line(p(0.5 + dx * 0.4, 0.9), p(0.5 + dx, 0.9 + dy), th(0.05), color, a);
            }
        }
        "hud_icon_runic_ember_whip" => {
            // a whip of chain curling up to a flaming tip
            c.stroke(&[p(0.14, 0.88), p(0.26, 0.66), p(0.44, 0.64), p(0.58, 0.5), p(0.58, 0.34)], false, th(0.07), color, a);
            c.fan(p(0.66, 0.24), &[p(0.68, 0.02), p(0.78, 0.16), p(0.84, 0.3), p(0.72, 0.42), p(0.54, 0.36), p(0.56, 0.18)], color, a);
        }
        "hud_icon_runic_cinder_rush" => {
            // three chevrons charging right, the last one alight
            for (i, x) in [0.14f32, 0.36, 0.58].iter().enumerate() {
                let w = 0.05 + 0.02 * i as f32;
                c.stroke(&[p(*x, 0.24), p(x + 0.2, 0.5), p(*x, 0.76)], false, th(w), color, a);
            }
            c.fan(p(0.86, 0.5), &[p(0.86, 0.26), p(0.94, 0.44), p(0.9, 0.62), p(0.8, 0.62), p(0.78, 0.46)], color, a);
        }
        "hud_icon_runic_pyre_pillar" => {
            // a pillar of fire rising from the ground
            c.fan(p(0.5, 0.64), &[p(0.3, 0.88), p(0.7, 0.88), p(0.74, 0.62), p(0.8, 0.38), p(0.66, 0.46), p(0.64, 0.22), p(0.56, 0.36), p(0.5, 0.04), p(0.44, 0.36), p(0.36, 0.2), p(0.34, 0.46), p(0.2, 0.38), p(0.26, 0.62)], color, a);
            c.fan(p(0.5, 0.74), &[p(0.5, 0.5), p(0.58, 0.68), p(0.56, 0.84), p(0.44, 0.84), p(0.42, 0.68)], 0x101010, 0.5 * a);
            c.line(p(0.12, 0.93), p(0.88, 0.93), th(0.06), color, a);
        }
        "hud_icon_runic_scorched_arc" => {
            // a wide fan of fire sweeping out in front
            let arc: Vec<(f32, f32)> = (0..=8)
                .map(|i| {
                    let ang = std::f32::consts::PI * (1.15 + 0.7 * i as f32 / 8.0);
                    p(0.5 + 0.42 * ang.cos(), 0.86 + 0.6 * ang.sin())
                })
                .collect();
            let mut wedge = vec![p(0.5, 0.88)];
            wedge.extend(arc.iter().copied());
            c.fan(p(0.5, 0.6), &wedge, color, a);
            for q in arc.iter().step_by(2) {
                let (dx, dy) = (q.0 - p(0.5, 0.88).0, q.1 - p(0.5, 0.88).1);
                let l = (dx * dx + dy * dy).sqrt().max(1e-3);
                c.tri((q.0 - dy / l * s * 0.05, q.1 + dx / l * s * 0.05), (q.0 + dx / l * s * 0.12, q.1 + dy / l * s * 0.12 - s * 0.03), (q.0 + dy / l * s * 0.05, q.1 - dx / l * s * 0.05), color, a);
            }
            c.fan(p(0.5, 0.72), &[p(0.5, 0.56), p(0.58, 0.7), p(0.5, 0.84), p(0.42, 0.7)], 0x101010, 0.55 * a);
        }
        "hud_icon_runic_shard_burst" => {
            // shards bursting up and out in a fan
            for k in 0..5 {
                let ang = -std::f32::consts::FRAC_PI_2 + (k as f32 - 2.0) * 0.36;
                let (cx, cy) = (ang.cos(), ang.sin());
                let (tx, ty) = (-cy, cx);
                let o = (0.5 + 0.12 * cx, 0.8 + 0.12 * cy);
                c.tri(p(o.0 - 0.05 * tx, o.1 - 0.05 * ty), p(0.5 + 0.7 * cx, 0.8 + 0.7 * cy), p(o.0 + 0.05 * tx, o.1 + 0.05 * ty), color, a);
            }
            c.circle(p(0.5, 0.82), s * 0.07, true, 0.0, color, a);
        }
        "hud_icon_runic_thunder_step" => {
            // a spear point driving right, speed lines behind it
            c.fan(p(0.74, 0.5), &[p(0.56, 0.4), p(0.92, 0.5), p(0.56, 0.6), p(0.62, 0.5)], color, a);
            c.line(p(0.3, 0.5), p(0.6, 0.5), th(0.06), color, a);
            for (y, x0) in [(0.32f32, 0.1f32), (0.5, 0.06), (0.68, 0.1)] {
                c.line(p(x0, y), p(x0 + 0.22, y), th(0.04), color, a);
            }
        }
        "hud_icon_runic_blast_ring" => {
            // spear heads planted in a ring, pointing out
            for k in 0..8 {
                let ang = k as f32 * std::f32::consts::FRAC_PI_4 - std::f32::consts::FRAC_PI_2;
                let (cx, cy) = (ang.cos(), ang.sin());
                let (tx, ty) = (-cy, cx);
                c.tri(p(0.5 + 0.24 * cx - 0.07 * tx, 0.5 + 0.24 * cy - 0.07 * ty), p(0.5 + 0.46 * cx, 0.5 + 0.46 * cy), p(0.5 + 0.24 * cx + 0.07 * tx, 0.5 + 0.24 * cy + 0.07 * ty), color, a);
            }
            c.circle(p(0.5, 0.5), s * 0.12, true, 0.0, color, a);
        }
        "hud_icon_runic_sky_lance" => {
            // a giant spear striking down, lightning beside it
            c.line(p(0.4, 0.06), p(0.4, 0.62), th(0.07), color, a);
            c.fan(p(0.4, 0.74), &[p(0.3, 0.6), p(0.5, 0.6), p(0.4, 0.94)], color, a);
            c.stroke(&[p(0.74, 0.08), p(0.62, 0.36), p(0.76, 0.4), p(0.62, 0.7)], false, th(0.05), color, a);
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
    rbar_fx(rect, frac, trail, 0xF4EBD0, 0.5, mid, lead, segments, 0.0, alpha, None)
}

/// A repeatable 0..1 value for particle `i` (no state is kept between frames).
fn hash01(i: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

/// `rbar` with the lost chunk (`trail`) drawn in `trail_color` at `trail_alpha`, and a white-hot `flash` over the fill.
/// `live`: the HUD clock in seconds for Kratos' own bars, whose fill is alive the way Ragnarök's is (measured on the
/// player's Hrolf Kraki clips): soft patches of light and shade flowing back through the health fill from its tip and
/// churning in the Rage segments, a soft glow around the fill and light spilling from its tip. `None`: a still bar (the
/// patches stand still). The clips show no sharp sparks: a time slice of the bars has only broad, slow bands.
pub fn rbar_fx(rect: &Rect, frac: f32, trail: f32, trail_color: u32, trail_alpha: f32, mid: u32, lead: u32, segments: u32, flash: f32, alpha: f32, live: Option<f32>) -> Vec<Prim> {
    let mut c = Canvas { out: Vec::new() };
    let (x0, y0, y1) = (rect.x, rect.y, rect.y + rect.h);
    let h = rect.h;
    let frac = frac.clamp(0.0, 1.0);
    let trail = trail.clamp(0.0, 1.0);
    let n = segments.max(1);
    let gap = if n > 1 { (h * 0.38).max(2.0) } else { 0.0 };
    let sw = (rect.w - gap * (n - 1) as f32) / n as f32;
    let seg_x = |i: u32| x0 + i as f32 * (sw + gap);
    // a fraction of the whole bar to an x, skipping the gaps
    let at = |f: f32| {
        let u = f * n as f32;
        let i = (u.floor() as u32).min(n - 1);
        seg_x(i) + (u - i as f32) * sw
    };
    let fill_end = at(frac);
    // the glow around what is filled (bars big enough to show it), breathing slowly, strongest at the leading end
    let (alive, t) = (live.is_some(), live.unwrap_or(0.0));
    let fill_to = fill_end;
    if alive && frac > 0.002 {
        let breathe = 0.85 + 0.15 * (t * 2.2).sin();
        let glow = mix(mid, lead, 0.6);
        for (d, a) in [(h * 0.75, 0.04f32), (h * 0.45, 0.06), (h * 0.22, 0.08)] {
            c.poly(&[(x0 - d * 0.3, y0 - d), (fill_to + d, y0 - d), (fill_to + d, y1 + d), (x0 - d * 0.3, y1 + d)], glow, a * breathe * alpha);
        }
    }
    // housing: one piece per segment, so the gaps stay dark
    let rim = (h * 0.08).max(1.0);
    for i in 0..n {
        let (a, b) = (seg_x(i), seg_x(i) + sw);
        c.rect(a, y0 - rim, b, y1 + rim, 0x000000, 0.55 * alpha);
        c.rect(a, y0, b, y1, 0x5A5658, 0.45 * alpha);
    }
    if trail > frac && trail_alpha > 0.0 {
        c.rect(fill_end, y0, at(trail), y1, trail_color, trail_alpha * alpha);
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
    let blot = |c: &mut Canvas, cx: f32, rx: f32, ry: f32, col: u32, al: f32, lo: f32, hi: f32| {
        for (k, sc) in [1.0f32, 0.88, 0.76, 0.64, 0.52, 0.4, 0.28].iter().enumerate() {
            // kept inside the fill: an ellipse squeezed against [lo, hi] stays convex
            let pts: Vec<(f32, f32)> = (0..24)
                .map(|i| {
                    let t = i as f32 * std::f32::consts::TAU / 24.0;
                    ((cx + rx * sc * t.cos()).clamp(lo, hi), (y0 + y1) / 2.0 + ry * sc * t.sin())
                })
                .collect();
            c.poly(&pts, col, al * (0.34 + 0.05 * k as f32) * alpha);
        }
    };
    // a soft patch of light or shade the bar's height, strongest (`al`) in its middle, kept inside [lo, hi]
    let patch = |c: &mut Canvas, cx: f32, rx: f32, col: u32, al: f32, lo: f32, hi: f32| {
        if cx + rx <= lo || cx - rx >= hi || al <= 0.0 {
            return;
        }
        for sc in [1.0f32, 0.78, 0.56, 0.34] {
            let pts: Vec<(f32, f32)> = (0..20)
                .map(|i| {
                    let a = i as f32 * std::f32::consts::TAU / 20.0;
                    ((cx + rx * sc * a.cos()).clamp(lo, hi), ((y0 + y1) / 2.0 + h * 0.62 * sc * a.sin()).clamp(y0, y1))
                })
                .collect();
            c.poly(&pts, col, al / 4.0 * alpha);
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
            // The fill's texture (measured on the player's 1080p clip): soft light and dark patches about two bar heights
            // wide and the bar's full height, swelling and fading, flowing back from the tip toward the start at about
            // 1.4 bar heights a second (25-28 px/s on the 19 px bar), about 9 levels lighter or darker (up to 15).
            // Laid out over the whole bar, so they do not jump when health changes.
            let speed = h * 1.4;
            let period = rect.w + h * 4.0;
            let count = ((period / (h * 2.4)) as u32).max(3);
            for k in 0..count {
                // each patch also changes shape and strength as it goes, as the clip's do
                let rx = h * (0.8 + 0.5 * hash01(k * 4 + 21)) * (1.0 + 0.3 * (t * 1.9 + k as f32 * 2.9).sin());
                let u = (period * k as f32 / count as f32 + h * hash01(k * 4 + 22) - t * speed * (0.9 + 0.2 * hash01(k * 4 + 23))).rem_euclid(period);
                let swell = 0.55 + 0.45 * (t * 2.2 + k as f32 * 1.7).sin();
                let (col, al) = if hash01(k * 4 + 24) > 0.5 { (0xFFFFFF, 0.13) } else { (0x000000, 0.19) };
                patch(&mut c, x0 - h * 2.0 + u, rx, col, al * swell, x0, fill_end);
            }
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
                // a smoky core, and smaller light and dark patches churning in it (measured: about one bar height wide,
                // the light ones warm, +13 / -9 levels, no steady drift)
                let wob = (t * 1.3 + i as f32 * 1.7).sin();
                blot(&mut c, a + sw * (0.45 + 0.03 * wob), sw * (0.38 + 0.02 * (t * 0.9 + i as f32).cos()), h * 0.4, shade(warm, 0.75), 0.5 - 0.1 * i as f32, a, b);
                let period = sw + h * 2.0;
                for k in 0..5u32 {
                    let id = i * 16 + k * 4;
                    let rx = h * (0.45 + 0.25 * hash01(id + 31));
                    let u = (period * k as f32 / 5.0 + t * h * 0.6 * (0.85 + 0.3 * hash01(id + 32)) + h * 0.8 * (t * 1.7 + k as f32).sin()).rem_euclid(period);
                    let swell = 0.5 + 0.5 * (t * 4.0 + k as f32 * 2.3 + i as f32).sin();
                    let (col, al) = if hash01(id + 33) > 0.45 { (0xFFE4DA, 0.2) } else { (0x000000, 0.12) };
                    patch(&mut c, a - h + u, rx, col, al * swell, a, b);
                }
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
    if alive && frac > 0.002 {
        // light spilling from the leading edge: soft rings that fade outward
        let breathe = 0.8 + 0.2 * (t * 2.2 + 0.7).sin();
        // many faint layers, so the light fades smoothly instead of in rings
        for k in 0..8 {
            let (sc, a) = (1.6 - 0.16 * k as f32, 0.035f32);
            let ring: Vec<(f32, f32)> = (0..20)
                .map(|i| {
                    let ang = i as f32 * std::f32::consts::TAU / 20.0;
                    (fill_end + h * sc * 0.9 * ang.cos(), (y0 + y1) / 2.0 + h * sc * 0.75 * ang.sin())
                })
                .collect();
            c.poly(&ring, lighten(lead, 0.5), a * breathe * alpha);
        }
    }
    if n > 1 && frac > 0.002 && frac < 0.999 {
        let tip = (h * 0.2).max(1.5);
        c.rect(fill_end - tip, y0, fill_end, y1, lighten(lead, 0.7), 0.95 * alpha);
    }
    if flash > 0.0 {
        // white-hot, segment by segment so the gaps stay dark
        for i in 0..n {
            let (a, b) = (seg_x(i), (seg_x(i) + sw).min(fill_end));
            if b > a {
                c.rect(a, y0, b, y1, 0xFFEDE4, 0.85 * flash * alpha);
            }
        }
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

/// A stick button glyph ("L3" / "R3"): white disc, dark rim and lettering, ringed by burning fire when `fire` > 0.
/// `t` is a running clock in seconds: the flames rise, sway and flicker, and embers drift up off the top. With
/// `ART_STICKS` in `art`, the button itself is Ragnarök's own.
pub fn stick_button(cx: f32, cy: f32, r: f32, label: &str, fire: f32, t: f32, alpha: f32, art: u32) -> Vec<Prim> {
    let mut c = Canvas { out: Vec::new() };
    if fire > 0.0 {
        let fa = fire * alpha;
        // a soft halo that breathes, taller above the button
        let breathe = 0.85 + 0.15 * (t * 6.3).sin() * (t * 2.9 + 1.0).cos();
        c.circle((cx, cy - r * 0.14), r * 1.55, true, 0.0, 0x8E2010, 0.3 * fa * breathe);
        c.circle((cx, cy - r * 0.07), r * 1.38, true, 0.0, 0xC8381F, 0.5 * fa * breathe);
        c.circle((cx, cy), r * 1.22, true, 0.0, 0xF06A34, 0.5 * fa);
        // three layers of tongues (red, orange, yellow), each tongue with its own rhythm; fire rises, so the tongues
        // on top are long and lean up, the ones underneath short
        for (layer, (col, reach, n)) in [(0xC8381Fu32, 1.0f32, 14usize), (0xFF8A3A, 0.68, 12), (0xFFD27A, 0.38, 9)].iter().enumerate() {
            for k in 0..*n {
                let ph = k as f32 * 1.37 + layer as f32 * 0.71;
                let ang = k as f32 * std::f32::consts::TAU / *n as f32 + layer as f32 * 0.4 + 0.15 * (t * 1.7 + ph).sin();
                let (ca, sa) = (ang.cos(), ang.sin());
                let up = (-sa).max(0.0); // 1 at the top, 0 at the sides and below
                let lick = 0.55 + 0.3 * (t * 9.0 + ph).sin() + 0.15 * (t * 14.3 + ph * 2.0).sin();
                let len = r * reach * (0.22 + 0.5 * up) * lick.max(0.15) * 1.25;
                let w = r * (0.2 + 0.08 * up) * (1.0 - 0.25 * layer as f32);
                let base = r * (1.08 - 0.06 * layer as f32);
                let (bx, by) = (cx + ca * base, cy + sa * base);
                let sway = len * 0.35 * (t * 6.0 + ph).sin();
                let tip = (bx + ca * len * 0.6 - sa * sway, by + sa * len * 0.6 - len * 0.55 + ca * sway * 0.3);
                let side = (-sa * w, ca * w);
                let a = if layer == 2 { 0.75 } else { 0.85 } * fa * (0.75 + 0.25 * lick.min(1.0));
                c.tri((bx - side.0, by - side.1), tip, (bx + side.0, by + side.1), *col, a);
            }
        }
        // embers drifting up off the top and fading
        for i in 0..6 {
            let u = (t * 0.75 + i as f32 / 6.0 + label.len() as f32 * 0.13).fract();
            let x = cx + r * 0.75 * ((i as f32 * 2.3 + t * 1.9).sin()) * (1.0 - 0.5 * u);
            let y = cy - r * 1.05 - u * r * 1.7;
            c.circle((x, y), (r * 0.085 * (1.0 - u)).max(0.6), true, 0.0, if i % 2 == 0 { 0xFFB060 } else { 0xFF7A3A }, 0.9 * (1.0 - u) * fa);
        }
    }
    let own = match label {
        "L3" => Some("hud_btn_l3"),
        "R3" => Some("hud_btn_r3"),
        _ => None,
    };
    if let Some(id) = own.filter(|_| art & ART_STICKS != 0) {
        c.circle((cx, cy), r * 1.08, true, 0.0, 0x141414, 0.95 * alpha);
        c.out.extend(tex_button(id, cx, cy, r, alpha));
        return c.out;
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

/// Ragnarök's Spartan Rage prompt in `rect`: L3 + R3, both buttons on fire (`t`: running clock, seconds).
pub fn rage_prompt(rect: &Rect, t: f32, alpha: f32, art: u32) -> Vec<Prim> {
    let r = rect.h * 0.34;
    let cy = rect.y + rect.h / 2.0;
    let (lx, rx) = (rect.x + rect.w * 0.2, rect.x + rect.w * 0.8);
    let mut out = stick_button(lx, cy, r, "L3", 1.0, t, alpha, art);
    out.extend(stick_button(rx, cy, r, "R3", 1.0, t + 0.37, alpha, art));
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
            Prim::Tex { asset, pts, uv, color, alpha } => Prim::Tex { asset, pts: pts.map(rot), uv: *uv, color: *color, alpha: *alpha },
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
            Prim::Tex { pts, .. } => pts.to_vec(),
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
        // a ready plate is in its colour; one recharging is grey, its recharged part rising from the bottom
        let lit_top = |charge: f32| {
            let face = if charge >= 0.999 { 0x4A7FB4 } else { mix(shade(0x4A7FB4, 0.35), 0x707378, 0.75) };
            plate(PlateShape::Diamond, 50.0, 50.0, 30.0, 26.0, 0x4A7FB4, 0x8AB4DC, charge, 1.0)
                .iter()
                .filter_map(|p| if let Prim::Poly { pts, color, .. } = p { (*color == face).then(|| pts.iter().map(|q| q.1).fold(f32::MAX, f32::min)) } else { None })
                .fold(f32::MAX, f32::min)
        };
        assert!((lit_top(1.0) - 24.0).abs() < 0.01, "a ready plate is lit to its top");
        assert!((lit_top(0.5) - 50.0).abs() < 0.01, "half charged: lit up to the middle");
        assert!(lit_top(0.0) > 75.0, "nothing lit when it has just been used");
    }

    #[test]
    fn every_ragnarok_piece_the_hud_places_is_an_asset_picked_from_the_gallery() {
        for (_, ids) in ART_GROUPS {
            for id in ids {
                let a = crate::sheets::ASSETS.iter().find(|a| a.id == *id).unwrap_or_else(|| panic!("{id} is not in assets.json"));
                assert_ne!(a.ragnarok_pick, "none", "{id} has no Ragnarök picture picked");
                assert!(is_placed_piece(id));
            }
        }
        assert!(!is_placed_piece("hud_icon_shield"), "whole icons are still cropped to their shape");
    }

    #[test]
    fn a_group_of_pieces_counts_only_when_all_of_it_is_loaded() {
        assert_eq!(art_flags(|_| false), 0);
        assert_eq!(art_flags(|_| true), ART_PLATE | ART_RELIC | ART_RAGE | ART_STICKS | ART_TRIANGLE);
        let no_spec = art_flags(|id| id != "hud_part_plate_spec");
        assert_eq!(no_spec & ART_PLATE, 0, "a plate missing a piece falls back to the drawing");
        assert_ne!(no_spec & ART_RELIC, 0);
    }

    #[test]
    fn a_textured_plate_is_four_quarters_meeting_at_its_centre() {
        let (cx, cy) = (200.0, 300.0);
        let prims = runic_plate_fx(cx, cy, 30.0, 26.0, 0x4A7FB4, 0x8AB4DC, 1.0, 0.0, 0.0, 1.0, ART_PLATE);
        let faces: Vec<&[(f32, f32); 4]> = prims.iter().filter_map(|p| if let Prim::Tex { asset: "hud_part_plate_face", pts, color, .. } = p { (*color == shade(0x4A7FB4, 0.15)).then_some(pts) } else { None }).collect();
        assert_eq!(faces.len(), 4, "one face piece per quarter");
        for pts in &faces {
            assert!(pts.iter().any(|q| (q.0 - cx).abs() < 1e-3 && (q.1 - cy).abs() < 1e-3), "each quarter reaches the centre: {pts:?}");
        }
        // one quarter in each direction from the centre
        let mut dirs: Vec<(bool, bool)> = faces.iter().map(|p| (p.iter().map(|q| q.0).sum::<f32>() > 4.0 * cx, p.iter().map(|q| q.1).sum::<f32>() > 4.0 * cy)).collect();
        dirs.sort();
        dirs.dedup();
        assert_eq!(dirs.len(), 4);
        // without the pieces it is the mod's own drawing
        assert!(!runic_plate_fx(cx, cy, 30.0, 26.0, 0x4A7FB4, 0x8AB4DC, 1.0, 0.0, 0.0, 1.0, 0).iter().any(|p| matches!(p, Prim::Tex { .. })));
    }

    #[test]
    fn a_band_of_a_piece_keeps_its_texture_in_step() {
        let p = piece("hud_part_plate_face", (0.0, 64.0), (100.0, 100.0), 1.0, 1.0, 0, 0xFFFFFF, 1.0);
        let Prim::Tex { pts: full, uv: full_uv, .. } = &p else { unreachable!() };
        // the whole piece: v runs linearly from its top edge to its bottom edge
        let (top, bottom) = (full[0].1, full[3].1);
        let v_at = |y: f32| full_uv[0].1 + (y - top) / (bottom - top) * (full_uv[3].1 - full_uv[0].1);
        let Some(Prim::Tex { pts, uv, .. }) = tex_band(&p, 70.0, 80.0) else { panic!("a band inside the piece") };
        for (q, t) in pts.iter().zip(uv.iter()) {
            assert!(q.1 >= 70.0 - 1e-3 && q.1 <= 80.0 + 1e-3);
            assert!((t.1 - v_at(q.1)).abs() < 1e-5, "{q:?} {t:?}");
        }
        // the pivot edge reaches the pivot; the texture is sampled from half a texel in (the game's sampler wraps)
        assert!((bottom - 100.0).abs() < 1e-4 && (full_uv[3].1 - 63.5 / 64.0).abs() < 1e-6);
        assert!(tex_band(&p, 120.0, 130.0).is_none(), "below the piece");
    }

    #[test]
    fn kratos_bars_move_with_the_clock_and_others_stay_still() {
        let r = Rect { x: 0.0, y: 0.0, w: 392.0, h: 19.0 };
        let at = |live| rbar_fx(&r, 0.8, 0.8, 0, 0.0, 0x55914A, 0x8EE05A, 1, 0.0, 1.0, live);
        let (a, b) = (at(Some(1.0)), at(Some(1.4)));
        assert_ne!(format!("{a:?}"), format!("{b:?}"), "the patches of light and shade move");
        assert_eq!(format!("{:?}", at(None)), format!("{:?}", at(None)));
        assert!(at(None).len() < a.len(), "a still bar has no glow");
        // the patches of light stay inside the fill
        for p in &a {
            if let Prim::Poly { pts, color: 0xFFFFFF, .. } = p {
                assert!(pts.iter().all(|q| q.0 >= -1e-3 && q.0 <= 392.0 * 0.8 + 1e-3 && q.1 >= -1e-3 && q.1 <= 19.0 + 1e-3), "{pts:?}");
            }
        }
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
