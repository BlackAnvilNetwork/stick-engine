//! CPU pixel rasterizer: 8-bit indexed colour + f32 depth buffer.
use crate::camera::P2;

/// 16-colour palette (0x00RRGGBB). Index constants below.
pub const PALETTE: [u32; 16] = [
    0x0d0b14, // 0 void / outline
    0x2a2633, // 1 ground dark
    0x34303f, // 2 ground light
    0x221f2a, // 3 grid line
    0x18161f, // 4 shadow
    0xe8e2d0, // 5 bone white (body)
    0xc9b98f, // 6 body far side
    0xb33939, // 7 accent red (scarf/head band)
    0x9aa6b8, // 8 steel
    0xf4f7ff, // 9 steel highlight / trail
    0x6c5a42, // 10 hilt brown
    0xd9a441, // 11 gold
    0x7d7486, // 12 dust
    0x4a4458, // 13 stone
    0x5fc4e8, // 14 dash cyan
    0x3b3546, // 15 ground mid
];
pub const C_OUTLINE: u8 = 0;
pub const C_SHADOW: u8 = 4;
pub const C_BODY: u8 = 5;
pub const C_BODY_FAR: u8 = 6;
pub const C_ACCENT: u8 = 7;
pub const C_STEEL: u8 = 8;
pub const C_TRAIL: u8 = 9;
pub const C_HILT: u8 = 10;
pub const C_GOLD: u8 = 11;
pub const C_DUST: u8 = 12;
pub const C_DASH: u8 = 14;

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
    /// Thick limb: capsule between a and b with radius r (px) and a 1 px outline.
    pub fn capsule(&mut self, a: P2, b: P2, r: f32, c: u8, outline: Option<u8>) {
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
    pub fn disc(&mut self, p: P2, r: f32, c: u8, outline: Option<u8>) {
        self.capsule(p, p, r, c, outline);
    }
    /// 1 px Bresenham line with interpolated depth.
    pub fn line(&mut self, a: P2, b: P2, c: u8) {
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
            let p = PALETTE[c as usize & 15];
            out[i * 4] = (p >> 16) as u8;
            out[i * 4 + 1] = (p >> 8) as u8;
            out[i * 4 + 2] = p as u8;
            out[i * 4 + 3] = 255;
        }
    }
}
