//! Stand-in sound effects, generated in code (filtered noise, sine sweeps, bell partials), played only when the
//! real Ragnarök sound for an effect could not be read from the player's install. They are original, short and
//! quiet on purpose; when the converted Ragnarök sounds exist they are used instead and none of this runs.

pub const RATE: u32 = 44_100;

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

fn buf(seconds: f32) -> Vec<f32> {
    vec![0.0; (seconds * RATE as f32) as usize]
}

/// Smooth rise over `attack` seconds then exponential fall with time constant `tau`.
fn env(t: f32, attack: f32, tau: f32) -> f32 {
    if t < attack {
        let x = t / attack.max(1e-4);
        x * x * (3.0 - 2.0 * x)
    } else {
        (-(t - attack) / tau.max(1e-4)).exp()
    }
}

/// One-pole low-pass coefficient for a cutoff in Hz.
fn alpha(cutoff: f32) -> f32 {
    1.0 - (-std::f32::consts::TAU * cutoff / RATE as f32).exp()
}

fn add_noise_sweep(out: &mut [f32], rng: &mut Rng, from_hz: f32, to_hz: f32, attack: f32, tau: f32, gain: f32, start: f32) {
    let n0 = (start * RATE as f32) as usize;
    let len = out.len().saturating_sub(n0);
    let (mut lp1, mut lp2) = (0.0f32, 0.0f32);
    for i in 0..len {
        let t = i as f32 / RATE as f32;
        let k = i as f32 / len.max(1) as f32;
        let a = alpha(from_hz + (to_hz - from_hz) * k);
        lp1 += a * (rng.next() - lp1);
        lp2 += a * (lp1 - lp2);
        out[n0 + i] += lp2 * env(t, attack, tau) * gain * 3.0;
    }
}

fn add_sine_sweep(out: &mut [f32], from_hz: f32, to_hz: f32, attack: f32, tau: f32, gain: f32, start: f32) {
    let n0 = (start * RATE as f32) as usize;
    let len = out.len().saturating_sub(n0);
    let mut phase = 0.0f32;
    for i in 0..len {
        let t = i as f32 / RATE as f32;
        let k = i as f32 / len.max(1) as f32;
        let f = from_hz * (to_hz / from_hz).powf(k);
        phase += std::f32::consts::TAU * f / RATE as f32;
        out[n0 + i] += phase.sin() * env(t, attack, tau) * gain;
    }
}

fn add_bell(out: &mut [f32], base_hz: f32, partials: &[(f32, f32)], tau: f32, gain: f32, start: f32) {
    let n0 = (start * RATE as f32) as usize;
    let len = out.len().saturating_sub(n0);
    for i in 0..len {
        let t = i as f32 / RATE as f32;
        let mut s = 0.0;
        for (ratio, amp) in partials {
            s += (std::f32::consts::TAU * base_hz * ratio * t).sin() * amp * (-t / (tau / (1.0 + ratio * 0.35))).exp();
        }
        out[n0 + i] += s * gain * env(t, 0.002, 10.0);
    }
}

fn add_click(out: &mut [f32], rng: &mut Rng, ms: f32, gain: f32, start: f32) {
    let n0 = (start * RATE as f32) as usize;
    let len = ((ms / 1000.0) * RATE as f32) as usize;
    for i in 0..len {
        if n0 + i < out.len() {
            out[n0 + i] += rng.next() * gain * (1.0 - i as f32 / len as f32);
        }
    }
}

/// Fade the very start and end, and keep the peak under `peak`.
fn finish(mut v: Vec<f32>, peak: f32) -> Vec<f32> {
    let fade = (0.004 * RATE as f32) as usize;
    let n = v.len();
    for i in 0..fade.min(n / 2) {
        let g = i as f32 / fade as f32;
        v[i] *= g;
        v[n - 1 - i] *= g;
    }
    let max = v.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(1e-6);
    let g = (peak / max).min(1.0);
    // soft clip so a stack of layers never rasps
    v.iter().map(|x| (x * g * 1.4).tanh() / 1.4f32.tanh() * peak).collect()
}

/// A generated sound for a sheet asset id (`sfx_*`), or `None` for ids with no stand-in.
pub fn sfx(id: &str) -> Option<Vec<f32>> {
    let mut rng = Rng(0x1234_5678 ^ id.len() as u32);
    let r = &mut rng;
    let v = match id {
        "sfx_axe_throw" => {
            let mut o = buf(0.5);
            add_noise_sweep(&mut o, r, 500.0, 3200.0, 0.06, 0.14, 0.9, 0.0);
            add_sine_sweep(&mut o, 220.0, 90.0, 0.005, 0.1, 0.18, 0.0);
            finish(o, 0.5)
        }
        "sfx_axe_recall" => {
            let mut o = buf(0.7);
            add_noise_sweep(&mut o, r, 3000.0, 600.0, 0.22, 0.2, 0.8, 0.0);
            add_sine_sweep(&mut o, 90.0, 260.0, 0.2, 0.18, 0.15, 0.0);
            finish(o, 0.45)
        }
        "sfx_axe_catch" => {
            let mut o = buf(0.6);
            add_sine_sweep(&mut o, 130.0, 48.0, 0.002, 0.1, 0.8, 0.0);
            add_click(&mut o, r, 18.0, 0.5, 0.0);
            add_bell(&mut o, 1150.0, &[(1.0, 0.5), (2.4, 0.3), (3.7, 0.15)], 0.28, 0.25, 0.0);
            finish(o, 0.6)
        }
        "sfx_rage_on" => {
            let mut o = buf(1.5);
            add_sine_sweep(&mut o, 70.0, 40.0, 0.12, 0.55, 0.9, 0.0);
            add_sine_sweep(&mut o, 140.0, 80.0, 0.1, 0.4, 0.35, 0.0);
            add_noise_sweep(&mut o, r, 900.0, 160.0, 0.18, 0.5, 0.6, 0.0);
            add_bell(&mut o, 420.0, &[(1.0, 0.3), (1.51, 0.2)], 0.9, 0.12, 0.05);
            finish(o, 0.7)
        }
        "sfx_rage_off" => {
            let mut o = buf(0.7);
            add_sine_sweep(&mut o, 260.0, 55.0, 0.01, 0.25, 0.7, 0.0);
            add_noise_sweep(&mut o, r, 700.0, 150.0, 0.02, 0.18, 0.3, 0.0);
            finish(o, 0.45)
        }
        "sfx_parry" => {
            let mut o = buf(0.7);
            add_click(&mut o, r, 12.0, 0.8, 0.0);
            add_bell(&mut o, 780.0, &[(1.0, 0.6), (1.67, 0.5), (2.63, 0.4), (3.92, 0.25)], 0.45, 0.55, 0.0);
            add_sine_sweep(&mut o, 150.0, 70.0, 0.002, 0.08, 0.3, 0.0);
            finish(o, 0.7)
        }
        "sfx_block" => {
            let mut o = buf(0.4);
            add_sine_sweep(&mut o, 160.0, 70.0, 0.002, 0.07, 0.7, 0.0);
            add_click(&mut o, r, 10.0, 0.4, 0.0);
            add_bell(&mut o, 540.0, &[(1.0, 0.4), (2.2, 0.2)], 0.12, 0.2, 0.0);
            finish(o, 0.5)
        }
        "sfx_runic_ready" => {
            let mut o = buf(0.6);
            add_bell(&mut o, 880.0, &[(1.0, 0.6), (2.0, 0.2), (3.0, 0.1)], 0.3, 0.5, 0.0);
            add_bell(&mut o, 1318.5, &[(1.0, 0.6), (2.0, 0.2)], 0.3, 0.4, 0.09);
            finish(o, 0.3)
        }
        "sfx_stun_full" => {
            let mut o = buf(0.5);
            add_click(&mut o, r, 14.0, 0.9, 0.0);
            add_bell(&mut o, 1800.0, &[(1.0, 0.6), (2.7, 0.3), (4.1, 0.15)], 0.22, 0.45, 0.0);
            add_sine_sweep(&mut o, 200.0, 90.0, 0.002, 0.06, 0.4, 0.0);
            finish(o, 0.55)
        }
        "sfx_finisher" => {
            let mut o = buf(0.9);
            add_sine_sweep(&mut o, 110.0, 38.0, 0.002, 0.18, 1.0, 0.0);
            add_noise_sweep(&mut o, r, 2500.0, 300.0, 0.002, 0.12, 0.8, 0.0);
            add_bell(&mut o, 650.0, &[(1.0, 0.5), (1.9, 0.4), (3.1, 0.2)], 0.5, 0.3, 0.02);
            finish(o, 0.75)
        }
        "sfx_weapon_swap" => {
            let mut o = buf(0.3);
            add_noise_sweep(&mut o, r, 1200.0, 4200.0, 0.02, 0.05, 0.6, 0.0);
            add_click(&mut o, r, 8.0, 0.5, 0.0);
            add_click(&mut o, r, 6.0, 0.35, 0.09);
            add_bell(&mut o, 1700.0, &[(1.0, 0.3), (2.5, 0.15)], 0.1, 0.18, 0.09);
            finish(o, 0.4)
        }
        "sfx_blades_swing" => {
            let mut o = buf(0.45);
            add_noise_sweep(&mut o, r, 900.0, 5200.0, 0.05, 0.1, 0.8, 0.0);
            for k in 0..5 {
                add_click(&mut o, r, 5.0, 0.18, 0.05 + 0.045 * k as f32);
            }
            finish(o, 0.4)
        }
        "sfx_spear_throw" => {
            let mut o = buf(0.4);
            add_noise_sweep(&mut o, r, 1800.0, 6000.0, 0.03, 0.08, 0.7, 0.0);
            add_sine_sweep(&mut o, 420.0, 200.0, 0.004, 0.07, 0.18, 0.0);
            finish(o, 0.4)
        }
        "sfx_spear_blast" => {
            let mut o = buf(1.1);
            add_sine_sweep(&mut o, 90.0, 34.0, 0.003, 0.3, 1.0, 0.0);
            add_noise_sweep(&mut o, r, 3200.0, 200.0, 0.003, 0.28, 1.0, 0.0);
            add_bell(&mut o, 380.0, &[(1.0, 0.3), (2.1, 0.2)], 0.4, 0.15, 0.0);
            finish(o, 0.8)
        }
        _ => return None,
    };
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDS: [&str; 14] = [
        "sfx_axe_throw", "sfx_axe_recall", "sfx_axe_catch", "sfx_rage_on", "sfx_rage_off", "sfx_parry", "sfx_block", "sfx_runic_ready", "sfx_stun_full", "sfx_finisher", "sfx_weapon_swap", "sfx_blades_swing", "sfx_spear_throw", "sfx_spear_blast",
    ];

    #[test]
    fn every_sheet_effect_has_a_short_finite_quiet_stand_in() {
        for id in IDS {
            let v = sfx(id).unwrap_or_else(|| panic!("{id}"));
            let secs = v.len() as f32 / RATE as f32;
            assert!((0.25..=1.6).contains(&secs), "{id}: {secs}s");
            assert!(v.iter().all(|x| x.is_finite()), "{id}: not finite");
            let peak = v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!(peak > 0.1 && peak <= 0.81, "{id}: peak {peak}");
            assert!(v[0].abs() < 0.01 && v[v.len() - 1].abs() < 0.01, "{id}: clicks at the ends");
            let rms = (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
            assert!(rms > 0.01, "{id}: nearly silent ({rms})");
        }
        assert!(sfx("sfx_unknown").is_none());
    }
}
