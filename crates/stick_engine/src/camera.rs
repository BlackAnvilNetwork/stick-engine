use crate::math::Vec3;

/// Pixels per world metre along iso axes. A 1 m tile is a 64x32 diamond and a
/// 1.8 m stick figure is ~80 px tall.
pub const ISO_X: f32 = 32.0;
pub const ISO_Y: f32 = 16.0;
pub const HEIGHT_PX: f32 = 44.0;

/// Projected point: screen x/y in pixels plus depth (smaller = nearer).
#[derive(Clone, Copy, Debug)]
pub struct P2 {
    pub x: f32,
    pub y: f32,
    pub d: f32,
}

/// Camera abstraction. Only the iso projection exists today; the 3D
/// perspective mode (see PLAN.md) will be another `project` implementation
/// over the same 3D joints.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub cx: f32,
    pub cz: f32,
    pub w: f32,
    pub h: f32,
}

impl Camera {
    pub fn new(w: usize, h: usize) -> Self {
        Camera { cx: 0.0, cz: 0.0, w: w as f32, h: h as f32 }
    }
    pub fn project(&self, p: Vec3) -> P2 {
        let u = (p.x - self.cx) - (p.z - self.cz);
        let v = (p.x - self.cx) + (p.z - self.cz);
        P2 {
            x: self.w * 0.5 + u * ISO_X,
            y: self.h * 0.55 + v * ISO_Y - p.y * HEIGHT_PX,
            d: -(p.x + p.z),
        }
    }
    /// Screen pixel -> point on the ground plane (y = 0).
    pub fn unproject_ground(&self, sx: f32, sy: f32) -> (f32, f32) {
        let u = (sx - self.w * 0.5) / ISO_X;
        let v = (sy - self.h * 0.55) / ISO_Y;
        (self.cx + (u + v) * 0.5, self.cz + (v - u) * 0.5)
    }
}
