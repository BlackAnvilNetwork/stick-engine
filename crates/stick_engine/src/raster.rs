// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! CPU pixel rasterizer: 8-bit indexed colour (ramp model) + f32 depth buffer.
use crate::camera::P2;

/// Shades per colour ramp. A pixel index is `ramp * RAMP_LEN + shade`
/// (shade 0 = darkest).
pub const RAMP_LEN: usize = 6;

/// Largest radius (px) a projected primitive may reach near the camera.
pub const MAX_RADIUS: f32 = 160.0;

/// Built-in ramps (0x00RRGGBB, dark -> light). Body, gear and effects refer to
/// ramps through `RampMap` slots, never to literal indices.
pub const RAMPS: &[(&str, [u32; RAMP_LEN])] = &[
    ("void", [0x0d0b14, 0x0d0b14, 0x0d0b14, 0x0d0b14, 0x0d0b14, 0x0d0b14]),
    ("stone", [0x18161f, 0x221f2a, 0x2a2633, 0x34303f, 0x3b3546, 0x4a4458]),
    ("bone", [0x5a5446, 0x8a8068, 0xb0a582, 0xc9b98f, 0xe8e2d0, 0xf6f2e6]),
    ("blood", [0x3a1416, 0x5c1d20, 0x82282b, 0xb33939, 0xd0584f, 0xe88a78]),
    ("steel", [0x2c3240, 0x4a5466, 0x6c788c, 0x9aa6b8, 0xc4cede, 0xf4f7ff]),
    ("gold", [0x4a3414, 0x6e4f1e, 0x9a7028, 0xd9a441, 0xeec266, 0xfbe39a]),
    ("leather", [0x2a2016, 0x3e3022, 0x54422f, 0x6c5a42, 0x8a7558, 0xa8916f]),
    ("dust", [0x3a3540, 0x4e4856, 0x625b6c, 0x7d7486, 0x9a92a2, 0xb8b0c0]),
    ("arcane", [0x0f2a3a, 0x16455c, 0x22698a, 0x3a96bc, 0x5fc4e8, 0xa8ecff]),
    ("white", [0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff]),
    ("fire", [0x3a0e08, 0x6e1c0c, 0xa83410, 0xe0601a, 0xf89a32, 0xffd870]),
    ("ice", [0x1a2c44, 0x2c4a6e, 0x46729a, 0x6ea2c8, 0xa0cfe8, 0xe0f6ff]),
    ("team_blue", [0x101c3a, 0x1a2e5e, 0x284688, 0x3a64b8, 0x5c8ae0, 0x98b8f6]),
    ("team_green", [0x0e2a16, 0x184424, 0x246236, 0x34864a, 0x52aa64, 0x8ad296]),
    // hair colours
    ("hair_black", [0x0a0a0e, 0x121218, 0x1c1c24, 0x282834, 0x383846, 0x4c4c5c]),
    ("hair_brown", [0x1e140c, 0x2e1f12, 0x42301c, 0x5a4228, 0x725638, 0x8c6c4a]),
    ("hair_blonde", [0x5a4420, 0x8a6a30, 0xb89444, 0xd8b45c, 0xecd07e, 0xf8e8ac]),
    ("hair_red", [0x3a120a, 0x5e1e10, 0x86301a, 0xac4624, 0xc8623a, 0xe08a5e]),
    ("hair_grey", [0x3a3a40, 0x5a5a62, 0x7c7c84, 0x9e9ea6, 0xc0c0c6, 0xe0e0e4]),
];
/// Ramp names offered for the hair colour option.
pub const HAIR_RAMPS: [&str; 6] = ["hair_black", "hair_brown", "hair_blonde", "hair_red", "hair_grey", "arcane"];

/// Ramp id by name (`None` if unknown).
pub fn ramp_id(name: &str) -> Option<u8> {
    RAMPS.iter().position(|r| r.0 == name).map(|i| i as u8)
}

/// Palette index of `shade` in `ramp` (shade is clamped to the ramp).
#[inline]
pub const fn shade(ramp: u8, shade: u8) -> u8 {
    let s = if (shade as usize) < RAMP_LEN { shade } else { (RAMP_LEN - 1) as u8 };
    ramp * RAMP_LEN as u8 + s
}

const fn build_lut() -> [u32; 256] {
    let mut lut = [0xff00ffu32; 256]; // magenta = unassigned index
    let mut r = 0;
    while r < RAMPS.len() {
        let mut s = 0;
        while s < RAMP_LEN {
            lut[r * RAMP_LEN + s] = RAMPS[r].1[s];
            s += 1;
        }
        r += 1;
    }
    lut
}

/// 256-entry index -> 0x00RRGGBB lookup table used at present time.
/// Day/night or fog can be applied as a whole-LUT transform.
pub static LUT: [u32; 256] = build_lut();

pub const R_VOID: u8 = 0;
pub const R_STONE: u8 = 1;
pub const R_BONE: u8 = 2;
pub const R_BLOOD: u8 = 3;
pub const R_STEEL: u8 = 4;
pub const R_GOLD: u8 = 5;
pub const R_LEATHER: u8 = 6;
pub const R_DUST: u8 = 7;
pub const R_ARCANE: u8 = 8;
pub const R_WHITE: u8 = 9;
pub const R_HAIR_BROWN: u8 = 15;

pub const C_OUTLINE: u8 = shade(R_VOID, 0);
pub const C_SHADOW: u8 = shade(R_STONE, 0);
pub const C_GOLD: u8 = shade(R_GOLD, 3);
pub const C_DUST: u8 = shade(R_DUST, 3);
pub const C_TRAIL: u8 = shade(R_STEEL, 5);
pub const C_DASH: u8 = shade(R_ARCANE, 4);

pub struct Framebuffer {
    pub w: usize,
    pub h: usize,
    pub color: Vec<u8>,
    pub depth: Vec<f32>,
}

impl Framebuffer {
    pub fn new(w: usize, h: usize) -> Self {
        Framebuffer { w, h, color: vec![0; w * h], depth: vec![f32::INFINITY; w * h] }
    }
    pub fn clear_depth(&mut self) {
        self.depth.fill(f32::INFINITY);
    }
    #[inline]
    pub fn plot(&mut self, x: i32, y: i32, d: f32, c: u8) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let i = y as usize * self.w + x as usize;
        if d <= self.depth[i] {
            self.depth[i] = d;
            self.color[i] = c;
        }
    }
    /// Recolour pixels of colour `from` to `to` inside a disc at `p` of
    /// radius `r`, clipped to the circle (`clip`, `clip_r`). Depth is left
    /// untouched; nothing outside existing `from` pixels changes.
    pub fn recolor_disc(&mut self, p: P2, r: f32, clip: P2, clip_r: f32, from: u8, to: u8) {
        if p.culled() || clip.culled() {
            return;
        }
        let (r, clip_r) = (r * p.s, clip_r * clip.s);
        let (x0, x1) = ((p.x - r).floor() as i32, (p.x + r).ceil() as i32);
        let (y0, y1) = ((p.y - r).floor() as i32, (p.y + r).ceil() as i32);
        for y in y0.max(0)..=y1.min(self.h as i32 - 1) {
            for x in x0.max(0)..=x1.min(self.w as i32 - 1) {
                let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                let (dx, dy) = (cx - p.x, cy - p.y);
                let (ex, ey) = (cx - clip.x, cy - clip.y);
                if dx * dx + dy * dy > r * r || ex * ex + ey * ey > clip_r * clip_r {
                    continue;
                }
                let i = y as usize * self.w + x as usize;
                if self.color[i] == from {
                    self.color[i] = to;
                }
            }
        }
    }
    /// Thick limb: capsule between a and b with radius r (px) and a 1 px outline.
    /// Radii are in iso pixels and scale by the projected `s` (focal / z).
    pub fn capsule(&mut self, a: P2, b: P2, r: f32, c: u8, outline: Option<u8>) {
        if a.culled() || b.culled() {
            return;
        }
        let r = (r * (a.s + b.s) * 0.5).min(MAX_RADIUS);
        let ro = if outline.is_some() { r + 1.0 } else { r };
        let x0 = (a.x.min(b.x) - ro).floor() as i32;
        let x1 = (a.x.max(b.x) + ro).ceil() as i32;
        let y0 = (a.y.min(b.y) - ro).floor() as i32;
        let y1 = (a.y.max(b.y) + ro).ceil() as i32;
        let (abx, aby) = (b.x - a.x, b.y - a.y);
        let l2 = (abx * abx + aby * aby).max(1e-6);
        let (r2, ro2) = (r * r, ro * ro);
        for y in y0.max(0)..=y1.min(self.h as i32 - 1) {
            for x in x0.max(0)..=x1.min(self.w as i32 - 1) {
                let (px, py) = (x as f32 + 0.5 - a.x, y as f32 + 0.5 - a.y);
                let t = ((px * abx + py * aby) / l2).clamp(0.0, 1.0);
                let (dx, dy) = (px - abx * t, py - aby * t);
                let d2 = dx * dx + dy * dy;
                let d = a.d + (b.d - a.d) * t;
                if d2 <= r2 {
                    self.plot(x, y, d, c);
                } else if d2 <= ro2 {
                    if let Some(o) = outline {
                        self.plot(x, y, d + 0.25, o);
                    }
                }
            }
        }
    }
    /// Tapered limb: like `capsule` but the radius goes linearly from `ra`
    /// at `a` to `rb` at `b` (thighs, calves, deltoids).
    pub fn cone(&mut self, a: P2, b: P2, ra: f32, rb: f32, c: u8, outline: Option<u8>) {
        if a.culled() || b.culled() {
            return;
        }
        let (ra, rb) = ((ra * a.s).min(MAX_RADIUS), (rb * b.s).min(MAX_RADIUS));
        let pad = if outline.is_some() { 1.0 } else { 0.0 };
        let rm = ra.max(rb) + pad;
        let x0 = (a.x.min(b.x) - rm).floor() as i32;
        let x1 = (a.x.max(b.x) + rm).ceil() as i32;
        let y0 = (a.y.min(b.y) - rm).floor() as i32;
        let y1 = (a.y.max(b.y) + rm).ceil() as i32;
        let (abx, aby) = (b.x - a.x, b.y - a.y);
        let l2 = (abx * abx + aby * aby).max(1e-6);
        for y in y0.max(0)..=y1.min(self.h as i32 - 1) {
            for x in x0.max(0)..=x1.min(self.w as i32 - 1) {
                let (px, py) = (x as f32 + 0.5 - a.x, y as f32 + 0.5 - a.y);
                let t = ((px * abx + py * aby) / l2).clamp(0.0, 1.0);
                let (dx, dy) = (px - abx * t, py - aby * t);
                let dist = (dx * dx + dy * dy).sqrt();
                let r = ra + (rb - ra) * t;
                let d = a.d + (b.d - a.d) * t;
                if dist <= r {
                    self.plot(x, y, d, c);
                } else if dist <= r + pad {
                    if let Some(o) = outline {
                        self.plot(x, y, d + 0.25, o);
                    }
                }
            }
        }
    }
    pub fn disc(&mut self, p: P2, r: f32, c: u8, outline: Option<u8>) {
        self.capsule(p, p, r, c, outline);
    }
    /// 1 px Bresenham line with interpolated depth.
    pub fn line(&mut self, a: P2, b: P2, c: u8) {
        if a.culled() || b.culled() || (a.x - b.x).abs() + (a.y - b.y).abs() > 4096.0 {
            return;
        }
        let (mut x0, mut y0) = (a.x as i32, a.y as i32);
        let (x1, y1) = (b.x as i32, b.y as i32);
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let n = dx.max(-dy).max(1) as f32;
        let mut err = dx + dy;
        let mut k = 0.0;
        loop {
            self.plot(x0, y0, a.d + (b.d - a.d) * (k / n), c);
            if x0 == x1 && y0 == y1 {
                break;
            }
            k += 1.0;
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }
    /// Filled ellipse written straight to colour (no depth) - used for shadows.
    pub fn shadow_ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: u8) {
        for y in (cy - ry) as i32..=(cy + ry) as i32 {
            for x in (cx - rx) as i32..=(cx + rx) as i32 {
                if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
                    continue;
                }
                let (nx, ny) = ((x as f32 + 0.5 - cx) / rx, (y as f32 + 0.5 - cy) / ry);
                if nx * nx + ny * ny <= 1.0 {
                    self.color[y as usize * self.w + x as usize] = c;
                }
            }
        }
    }
    /// Convert indexed colour to RGBA8 (web ImageData layout).
    pub fn write_rgba(&self, out: &mut [u8]) {
        for (i, &c) in self.color.iter().enumerate() {
            let p = LUT[c as usize];
            out[i * 4] = (p >> 16) as u8;
            out[i * 4 + 1] = (p >> 8) as u8;
            out[i * 4 + 2] = p as u8;
            out[i * 4 + 3] = 255;
        }
    }
}
