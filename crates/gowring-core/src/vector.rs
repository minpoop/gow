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
    fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: u32, alpha: f32) {
        self.tri((x0, y0), (x1, y0), (x1, y1), color, alpha);
        self.tri((x0, y0), (x1, y1), (x0, y1), color, alpha);
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

/// A framed bar: dark well, the pale "recent damage" trail, a lit fill and small end caps.
pub fn bar(rect: &Rect, frac: f32, trail: f32, color: u32, alpha: f32) -> Vec<Prim> {
    let mut c = Canvas { out: Vec::new() };
    let (x0, y0, x1, y1) = (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
    let frac = frac.clamp(0.0, 1.0);
    let trail = trail.clamp(0.0, 1.0);
    c.rect(x0, y0, x1, y1, 0x0B0B0E, 0.72 * alpha);
    if trail > frac {
        c.rect(x0, y0, x0 + rect.w * trail, y1, 0xE8E2C8, 0.42 * alpha);
    }
    if frac > 0.0 {
        let fx = x0 + rect.w * frac;
        c.rect(x0, y0, fx, y1, color, alpha);
        // light from above: a bright upper third and a shaded lower third
        c.rect(x0, y0, fx, y0 + rect.h * 0.34, lighten(color, 0.38), 0.55 * alpha);
        c.rect(x0, y1 - rect.h * 0.30, fx, y1, shade(color, 0.45), 0.5 * alpha);
        // a bright leading edge
        if frac < 0.999 {
            c.line((fx, y0), (fx, y1), (rect.h * 0.09).max(1.0), lighten(color, 0.7), 0.9 * alpha);
        }
    }
    let t = (rect.h * 0.07).clamp(1.0, 2.5);
    c.stroke(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)], true, t, 0xC9BB8E, 0.6 * alpha);
    if rect.h >= 14.0 {
        // notches every tenth, and a small diamond at each end
        for i in 1..10 {
            let x = x0 + rect.w * i as f32 / 10.0;
            c.line((x, y1 - rect.h * 0.28), (x, y1), 1.0, 0x000000, 0.28 * alpha);
        }
        let d = rect.h * 0.55;
        for (cx, dir) in [(x0, -1.0f32), (x1, 1.0)] {
            let cy = (y0 + y1) / 2.0;
            let p = [(cx, cy - d), (cx + dir * d * 0.8, cy), (cx, cy + d), (cx - dir * d * 0.0, cy)];
            c.fan((cx + dir * d * 0.2, cy), &p, 0xC9BB8E, 0.75 * alpha);
        }
    }
    c.out
}

/// A cooldown ring: the pie still to run is dimmed clockwise from 12 o'clock; a ready ring glows.
pub fn ring(cx: f32, cy: f32, r: f32, remaining: f32, ready: bool, color: u32, alpha: f32, icon: Option<&str>) -> Vec<Prim> {
    let mut c = Canvas { out: Vec::new() };
    let centre = (cx, cy);
    c.circle(centre, r, true, 0.0, 0x0B0B0E, 0.62 * alpha);
    c.circle(centre, r - 1.5, false, (r * 0.07).max(2.0), color, alpha);
    c.circle(centre, r * 0.80, false, 1.2, 0xFFFFFF, 0.22 * alpha);
    if let Some(id) = icon {
        let inner = Rect { x: cx - r * 0.56, y: cy - r * 0.56, w: r * 1.12, h: r * 1.12 };
        c.out.extend(icon_prims(id, &inner, lighten(color, 0.5), 0.9 * alpha).unwrap_or_default());
    }
    if remaining > 0.001 {
        let start = -std::f32::consts::FRAC_PI_2;
        let sweep = std::f32::consts::TAU * remaining.min(1.0);
        let steps = ((sweep / std::f32::consts::TAU) * 48.0).ceil().max(1.0) as usize;
        let ri = r - 2.0;
        for i in 0..steps {
            let a0 = start + sweep * i as f32 / steps as f32;
            let a1 = start + sweep * (i + 1) as f32 / steps as f32;
            c.tri(centre, (cx + ri * a0.cos(), cy + ri * a0.sin()), (cx + ri * a1.cos(), cy + ri * a1.sin()), 0x000000, 0.64);
        }
    } else if ready {
        c.circle(centre, r * 1.07, false, (r * 0.07).max(2.5), lighten(color, 0.35), alpha);
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::FRAC_PI_4 + 0.39;
            c.line((cx + r * 1.14 * a.cos(), cy + r * 1.14 * a.sin()), (cx + r * 1.28 * a.cos(), cy + r * 1.28 * a.sin()), 2.0, lighten(color, 0.4), 0.8 * alpha);
        }
    }
    c.out
}

/// A plate behind an icon: a disc (`sides` 0) or a regular polygon with a flat top, dark with a pale rim.
pub fn badge(cx: f32, cy: f32, r: f32, sides: u32, fill: u32, rim: u32, alpha: f32) -> Vec<Prim> {
    let mut c = Canvas { out: Vec::new() };
    let thick = (r * 0.07).max(1.5);
    if sides < 3 {
        c.circle((cx, cy), r, true, 0.0, fill, alpha);
        c.circle((cx, cy), r - thick / 2.0, false, thick, rim, alpha);
        return c.out;
    }
    let step = std::f32::consts::TAU / sides as f32;
    let pts: Vec<(f32, f32)> = (0..sides)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + step * (i as f32 + 0.5);
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect();
    c.fan((cx, cy), &pts, fill, alpha);
    c.stroke(&pts, true, thick, rim, alpha);
    c.out
}

/// A small solid triangle pointing up or down with its tip at (`x`, `tip_y`): which way on the D-pad.
pub fn pointer(x: f32, tip_y: f32, size: f32, up: bool, color: u32, alpha: f32) -> Vec<Prim> {
    let base = if up { tip_y + size } else { tip_y - size };
    vec![Prim::Tri { p: [(x, tip_y), (x + size, base), (x - size, base)], color, alpha }]
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

    #[test]
    fn every_hud_icon_has_a_drawing_inside_its_rect() {
        for id in ["hud_icon_axe", "hud_icon_blades", "hud_icon_spear", "hud_icon_shield", "hud_crosshair_throw", "hud_enemy_stunned"] {
            let prims = icon_prims(id, &R, 0x9FD8FF, 1.0).unwrap_or_else(|| panic!("{id} has no drawing"));
            assert!(prims.len() >= 6, "{id}");
            for p in &prims {
                let pts: Vec<(f32, f32)> = match p {
                    Prim::Tri { p, .. } => p.to_vec(),
                    Prim::Line { a, b, .. } => vec![*a, *b],
                    Prim::Circle { c, r, .. } => vec![(c.0 - r, c.1 - r), (c.0 + r, c.1 + r)],
                };
                for (x, y) in pts {
                    assert!(x >= R.x - 6.0 && x <= R.x + R.w + 6.0 && y >= R.y - 6.0 && y <= R.y + R.h + 6.0, "{id}: ({x}, {y}) outside the rect");
                }
            }
        }
        assert!(icon_prims("hud_btn_r3", &R, 0xFFFFFF, 1.0).is_none());
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

    #[test]
    fn bars_scale_with_the_fraction_and_rings_with_the_cooldown() {
        let r = Rect { x: 0.0, y: 0.0, w: 400.0, h: 26.0 };
        let max_x = |v: &[Prim], color: u32| v.iter().filter_map(|p| if let Prim::Tri { p, color: c, .. } = p { (*c == color).then(|| p.iter().map(|q| q.0).fold(0.0f32, f32::max)) } else { None }).fold(0.0f32, f32::max);
        let full = bar(&r, 1.0, 1.0, 0x3FAE4F, 1.0);
        let half = bar(&r, 0.5, 0.5, 0x3FAE4F, 1.0);
        assert!((max_x(&full, 0x3FAE4F) - 400.0).abs() < 0.01);
        assert!((max_x(&half, 0x3FAE4F) - 200.0).abs() < 0.01);
        let idle = ring(50.0, 50.0, 38.0, 0.0, false, 0x66B3FF, 1.0, None);
        let cooling = ring(50.0, 50.0, 38.0, 0.5, false, 0x66B3FF, 1.0, None);
        let dim = |v: &[Prim]| v.iter().filter(|p| matches!(p, Prim::Tri { color: 0, .. })).count();
        assert_eq!(dim(&idle), 0);
        assert!(dim(&cooling) >= 20);
    }
}
