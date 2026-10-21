// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Cameras. The default is the isometric 2.5D projection. A `View` adds a
//! general camera (target, yaw, pitch, distance) that covers the isometric
//! view (infinite distance, iso pixel basis), third-person and first-person
//! perspective, and every blend between them. Joints and capsules are the
//! same in every mode: points go through `project`, which also returns a
//! per-point scale `s` (focal / z) that the rasterizer applies to radii.
use crate::math::{v3, Vec3};

/// Pixels per world metre along iso axes. A 1 m tile is a 64x32 diamond and a
/// 1.8 m stick figure is ~80 px tall.
pub const ISO_X: f32 = 32.0;
pub const ISO_Y: f32 = 16.0;
pub const HEIGHT_PX: f32 = 44.0;

/// Px per metre across the screen in the iso view (|(32, 0, -32)|).
pub const ISO_SCALE: f32 = 45.254_834;
/// Azimuth of the iso view direction: the camera looks toward -x, -z.
pub const ISO_YAW: f32 = -3.0 * std::f32::consts::FRAC_PI_4;
/// Elevation of the iso view direction: the null vector of the iso basis is
/// (1, 32/44, 1), i.e. atan((32/44) / sqrt 2) ~ 27.2 degrees down.
pub fn iso_pitch() -> f32 {
    (32.0f32 / 44.0 / std::f32::consts::SQRT_2).atan()
}
/// Anything nearer to the eye than this (metres) is culled.
pub const NEAR: f32 = 0.08;
/// Ground beyond this distance (metres) from the eye is drawn as sky/fog.
pub const FAR: f32 = 60.0;

/// Projected point: screen x/y in pixels plus depth (smaller = nearer) and
/// the radius scale `s` (1 in iso; focal / z in perspective; 0 = culled).
#[derive(Clone, Copy, Debug)]
pub struct P2 {
    pub x: f32,
    pub y: f32,
    pub d: f32,
    pub s: f32,
}

impl P2 {
    pub fn new(x: f32, y: f32, d: f32) -> P2 {
        P2 { x, y, d, s: 1.0 }
    }
    /// True when the point is behind the near plane and must not be drawn.
    pub fn culled(&self) -> bool {
        self.s <= 0.0
    }
}

/// General camera parameters. `inv_d` = 1 / distance from eye to target
/// (0 = orthographic). `scale` = px per metre on the focus plane through
/// `target`, so focal length in px = `scale / inv_d`. `skew` blends the
/// orthonormal camera basis (0) into the classic iso pixel basis (1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub inv_d: f32,
    pub scale: f32,
    pub skew: f32,
    /// Screen anchor of `target` in pixels.
    pub ax: f32,
    pub ay: f32,
}

impl View {
    /// The iso projection expressed as a `View` (orthographic, iso basis).
    pub fn iso(cx: f32, cz: f32, w: f32, h: f32) -> View {
        View { target: v3(cx, 0.0, cz), yaw: ISO_YAW, pitch: iso_pitch(), inv_d: 0.0, scale: ISO_SCALE, skew: 1.0, ax: w * 0.5, ay: h * 0.55 }
    }
    /// Unit view direction (into the screen).
    pub fn fwd(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        v3(cp * sy, -sp, cp * cy)
    }
    /// Pixel basis: screen-right and screen-down vectors (px per metre on the focus plane).
    pub fn basis(&self) -> (Vec3, Vec3) {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let r = v3(-cy, 0.0, sy) * self.scale;
        let up = v3(sy * sp, cp, cy * sp);
        let dn = up * -self.scale;
        if self.skew <= 0.0 {
            return (r, dn);
        }
        let (ri, di) = (v3(ISO_X, 0.0, -ISO_X), v3(ISO_Y, -HEIGHT_PX, ISO_Y));
        (r.lerp(ri, self.skew), dn.lerp(di, self.skew))
    }
    /// Eye position (None when orthographic).
    pub fn eye(&self) -> Option<Vec3> {
        (self.inv_d > 0.0).then(|| self.target - self.fwd() * (1.0 / self.inv_d))
    }
    /// Blend toward `o` by `t` (0..1). Two perspective views blend along the
    /// boom (distance lerp, constant focal length); a blend with an
    /// orthographic view lerps `inv_d` at constant focus scale (dolly zoom).
    pub fn lerp(&self, o: &View, t: f32) -> View {
        let l = |a: f32, b: f32| a + (b - a) * t;
        let (inv_d, scale) = if self.inv_d > 0.0 && o.inv_d > 0.0 {
            let d = l(1.0 / self.inv_d, 1.0 / o.inv_d);
            let f = l(self.scale / self.inv_d, o.scale / o.inv_d);
            (1.0 / d, f / d)
        } else {
            (l(self.inv_d, o.inv_d), l(self.scale, o.scale))
        };
        View {
            target: self.target.lerp(o.target, t),
            yaw: self.yaw + crate::math::angle_diff(self.yaw, o.yaw) * t,
            pitch: l(self.pitch, o.pitch),
            inv_d,
            scale,
            skew: l(self.skew, o.skew),
            ax: l(self.ax, o.ax),
            ay: l(self.ay, o.ay),
        }
    }
}

/// Camera: the iso view (`view == None`, unchanged legacy path) or a general `View`.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub cx: f32,
    pub cz: f32,
    pub w: f32,
    pub h: f32,
    pub view: Option<View>,
}

impl Camera {
    pub fn new(w: usize, h: usize) -> Self {
        Camera { cx: 0.0, cz: 0.0, w: w as f32, h: h as f32, view: None }
    }

    /// The view in effect (the iso view when none is set).
    pub fn effective_view(&self) -> View {
        self.view.unwrap_or_else(|| View::iso(self.cx, self.cz, self.w, self.h))
    }

    pub fn project(&self, p: Vec3) -> P2 {
        match &self.view {
            None => Self::project_iso(self.cx, self.cz, self.w, self.h, p),
            Some(v) => Self::project_view(v, p),
        }
    }

    fn project_iso(cx: f32, cz: f32, w: f32, h: f32, p: Vec3) -> P2 {
        let u = (p.x - cx) - (p.z - cz);
        let v = (p.x - cx) + (p.z - cz);
        P2 { x: w * 0.5 + u * ISO_X, y: h * 0.55 + v * ISO_Y - p.y * HEIGHT_PX, d: -(p.x + p.z), s: 1.0 }
    }

    /// Perspective (or ortho) projection: x = ax + (q.R) k, y = ay + (q.D) k
    /// with k = 1 / (1 + z inv_d), z = q.F the depth past the focus plane.
    /// k is focal / (eye distance) relative to the focus scale, so radii
    /// scale by `s = k * scale / ISO_SCALE`. Depth is view z.
    pub fn project_view(v: &View, p: Vec3) -> P2 {
        let q = p - v.target;
        let f = v.fwd();
        let (r, dn) = v.basis();
        let z = q.dot(f);
        let den = 1.0 + z * v.inv_d;
        if v.inv_d > 0.0 && den <= NEAR * v.inv_d {
            return P2 { x: 0.0, y: 0.0, d: f32::INFINITY, s: 0.0 };
        }
        let k = 1.0 / den;
        P2 { x: v.ax + q.dot(r) * k, y: v.ay + q.dot(dn) * k, d: z, s: k * v.scale / ISO_SCALE }
    }

    /// Unit vector from `p` toward the eye (for "facing away" tests).
    pub fn to_eye(&self, p: Vec3) -> Vec3 {
        let v = self.effective_view();
        match v.eye() {
            Some(e) => (e - p).norm(),
            None => v.fwd() * -1.0,
        }
    }

    /// Screen pixel -> point on the ground plane (y = 0). In iso this is the
    /// legacy affine map; in perspective it falls back to the camera centre
    /// above the horizon (use `pick_ground` to detect that).
    pub fn unproject_ground(&self, sx: f32, sy: f32) -> (f32, f32) {
        match &self.view {
            None => {
                let u = (sx - self.w * 0.5) / ISO_X;
                let v = (sy - self.h * 0.55) / ISO_Y;
                (self.cx + (u + v) * 0.5, self.cz + (v - u) * 0.5)
            }
            Some(_) => self.pick_ground(sx, sy).unwrap_or((self.cx, self.cz)),
        }
    }

    /// Ground point under a pixel, or None above the horizon / past `FAR`.
    pub fn pick_ground(&self, sx: f32, sy: f32) -> Option<(f32, f32)> {
        if self.view.is_none() {
            return Some(self.unproject_ground(sx, sy));
        }
        let h = GroundMap::new(&self.effective_view());
        h.at(sx, sy)
    }
}

/// Screen -> ground homography for a view: numerators and denominator of the
/// 2x2 Cramer solve are affine in screen x/y (the XY terms cancel), so the
/// ground under a pixel is ((a.x, a.y, 1) / (c.x, c.y, 1)) and can be stepped
/// incrementally across a row with one divide per pixel.
#[derive(Clone, Copy, Debug)]
pub struct GroundMap {
    /// [nx, nz, det] = c0 + cx * X + cy * Y, with X, Y relative to the anchor.
    c0: [f32; 3],
    cx: [f32; 3],
    cy: [f32; 3],
    ax: f32,
    ay: f32,
    target: Vec3,
    fwd: Vec3,
    inv_d: f32,
}

impl GroundMap {
    pub fn new(v: &View) -> GroundMap {
        let raw = |x: f32, y: f32| -> [f32; 3] {
            let (r, dn) = v.basis();
            let f = v.fwd();
            let i = v.inv_d;
            let qy = -v.target.y;
            let c1 = 1.0 + i * f.y * qy;
            let (a11, a12, b1) = (r.x - x * i * f.x, r.z - x * i * f.z, x * c1 - r.y * qy);
            let (a21, a22, b2) = (dn.x - y * i * f.x, dn.z - y * i * f.z, y * c1 - dn.y * qy);
            [b1 * a22 - a12 * b2, a11 * b2 - b1 * a21, a11 * a22 - a12 * a21]
        };
        let c0 = raw(0.0, 0.0);
        let (c1, c2) = (raw(1.0, 0.0), raw(0.0, 1.0));
        let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        GroundMap { c0, cx: sub(c1, c0), cy: sub(c2, c0), ax: v.ax, ay: v.ay, target: v.target, fwd: v.fwd(), inv_d: v.inv_d }
    }
    /// Homogeneous ground coords for a pixel: (nx, nz, det).
    #[inline]
    pub fn raw(&self, sx: f32, sy: f32) -> [f32; 3] {
        let (x, y) = (sx - self.ax, sy - self.ay);
        [0, 1, 2].map(|k| self.c0[k] + self.cx[k] * x + self.cy[k] * y)
    }
    /// Per-pixel increment of `raw` along screen x.
    pub fn step_x(&self) -> [f32; 3] {
        self.cx
    }
    /// Resolve homogeneous coords to a ground point in front of the eye.
    #[inline]
    pub fn resolve(&self, n: [f32; 3]) -> Option<(f32, f32)> {
        if n[2].abs() < 1e-9 {
            return None;
        }
        let (qx, qz) = (n[0] / n[2], n[1] / n[2]);
        if self.inv_d > 0.0 {
            let z = qx * self.fwd.x - self.target.y * self.fwd.y + qz * self.fwd.z;
            let dist = 1.0 / self.inv_d + z;
            if dist <= NEAR || dist > FAR {
                return None;
            }
        }
        Some((self.target.x + qx, self.target.z + qz))
    }
    pub fn at(&self, sx: f32, sy: f32) -> Option<(f32, f32)> {
        self.resolve(self.raw(sx, sy))
    }
}
