//! Over-the-shoulder camera: eases between the walk and aim framings.

use crate::data::Data;
use crate::types::CameraRequest;

pub struct Camera {
    d: &'static Data,
    cur: CameraRequest,
}

impl Camera {
    pub fn new(d: &'static Data) -> Self {
        let c = d.camera;
        Camera { d, cur: CameraRequest { shoulder_x_m: c.shoulder_x_m, height_m: c.height_m, distance_m: c.distance_m, fov_deg: c.fov_deg } }
    }

    pub fn update(&mut self, dt: f32, aiming: bool, active: bool) -> CameraRequest {
        let c = self.d.camera;
        let target = if !active {
            CameraRequest::default()
        } else if aiming {
            CameraRequest { shoulder_x_m: c.aim_shoulder_x_m, height_m: c.height_m, distance_m: c.aim_distance_m, fov_deg: c.aim_fov_deg }
        } else {
            CameraRequest { shoulder_x_m: c.shoulder_x_m, height_m: c.height_m, distance_m: c.distance_m, fov_deg: c.fov_deg }
        };
        let k = if c.smooth_s <= 1e-4 { 1.0 } else { 1.0 - (-dt / c.smooth_s).exp() };
        let mix = |a: f32, b: f32| a + (b - a) * k;
        self.cur = CameraRequest {
            shoulder_x_m: mix(self.cur.shoulder_x_m, target.shoulder_x_m),
            height_m: mix(self.cur.height_m, target.height_m),
            distance_m: mix(self.cur.distance_m, target.distance_m),
            fov_deg: mix(self.cur.fov_deg, target.fov_deg),
        };
        self.cur
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eases_toward_the_aim_framing_and_back() {
        let d = Data::get();
        let mut cam = Camera::new(d);
        let walk = cam.update(0.016, false, true);
        assert!((walk.shoulder_x_m - d.camera.shoulder_x_m).abs() < 0.05);
        let mut r = walk;
        for _ in 0..120 {
            r = cam.update(0.016, true, true);
        }
        assert!((r.shoulder_x_m - d.camera.aim_shoulder_x_m).abs() < 0.01);
        assert!((r.fov_deg - d.camera.aim_fov_deg).abs() < 0.1);
        for _ in 0..120 {
            r = cam.update(0.016, false, true);
        }
        assert!((r.distance_m - d.camera.distance_m).abs() < 0.01);
    }
}
