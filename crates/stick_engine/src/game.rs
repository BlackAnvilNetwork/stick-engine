// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Minimal playable scene: one stick hero on a procedural tile floor.
use crate::actions::ActionTable;
use crate::anim::AnimLibrary;
use crate::custom::HairOptions;
use crate::body::{CharacterDef, GearRef, Sex};
use crate::camera::{Camera, GroundMap};
use crate::camrig::{CamConfig, CamMode, CamRig, Subject};
use crate::character::{Character, Intent};
use crate::color::RampMap;
use crate::gear::{GearLib, GearSlot};
use crate::math::{v3, Vec3};
use crate::raster::*;
use crate::render::{self, DrawOpts};
use crate::rig;

pub const TICK: f32 = 1.0 / 60.0;

/// Raw host input, in internal framebuffer pixels for mouse coords.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub run: bool,
    pub dash: bool,
    pub attack: bool,
    pub mouse: Option<(f32, f32)>,
    /// Move-to click (set by host, consumed by the game).
    pub move_click: Option<(f32, f32)>,
    /// Attack click position (left mouse); attack aims here.
    pub attack_click: Option<(f32, f32)>,
    /// Held: cycle hair style on press (H).
    pub hair_style: bool,
    /// Held: cycle hair colour on press (K).
    pub hair_color: bool,
    /// Held: cycle camera mode iso -> third -> first person on press (V).
    pub cam_cycle: bool,
    /// Held (`CamTurnLeft/Right`, `CamPitchUp/Down`, numpad 4/6/8/2 by
    /// default): in the 3D camera modes turn the character (like A / D) and
    /// pitch the camera. Ignored in mouse camera control.
    pub look_left: bool,
    pub look_right: bool,
    pub look_up: bool,
    pub look_down: bool,
    /// Held: switch numpad / mouse camera control on press (numpad `.`).
    pub cam_control_toggle: bool,
    /// Right mouse button held (mouse camera control: drag to turn / pitch).
    pub rmb: bool,
}

/// Mouse camera control: radians per framebuffer pixel of right-drag.
pub const MOUSE_TURN: f32 = 0.012;
pub const MOUSE_PITCH: f32 = 0.008;
/// Right-drag distance (px) before it counts as a turn instead of a click.
pub const DRAG_SLOP: f32 = 3.0;

/// Character turn rate in the 3D camera modes (radians per second).
pub const TURN_SPEED: f32 = 2.6;

struct Particle {
    p: Vec3,
    v: Vec3,
    life: f32,
    c: u8,
}

pub struct Game {
    pub lib: AnimLibrary,
    pub actions: ActionTable,
    pub gear: GearLib,
    pub player: Character,
    pub cam: Camera,
    pub fb: Framebuffer,
    pub opts: DrawOpts,
    act_dash: usize,
    act_swing: usize,
    target: Option<(f32, f32)>,
    prev: Input,
    accum: f32,
    particles: Vec<Particle>,
    trail: Vec<(Vec3, Vec3, f32)>,
    ghosts: Vec<(rig::Joints, f32)>,
    flash: f32,
    pub time: f32,
    pub demo: bool,
    /// Player hair options (style + colour), applied on change.
    pub hair: HairOptions,
    /// Camera mode, blends and look state (config in `rig.cfg`).
    pub rig: CamRig,
    /// Persisted engine settings (camera control); hosts save them when
    /// `settings_changed` is set and clear the flag.
    pub settings: crate::input::Settings,
    pub settings_changed: bool,
    /// 3D sky gradient + distance fog (on by default; off = flat void, no fog).
    pub sky_fog: bool,
    drag: Option<((f32, f32), f32, bool)>,
    rng: u32,
}

const PILLARS: [(f32, f32); 4] = [(4.0, -3.0), (-5.0, 2.0), (6.0, 6.0), (-3.0, -7.0)];

impl Game {
    pub fn new(w: usize, h: usize) -> Self {
        let actions = ActionTable::builtin();
        let gear = GearLib::builtin();
        let mut def = CharacterDef::new(Sex::Male);
        let sword = gear.index("sword").map(|base| GearRef { base, look_seed: 0 });
        def.equip(GearSlot::MainHand, sword);
        let hair = HairOptions { hair: crate::body::HairStyle::Buzz, ..HairOptions::default() };
        hair.apply(&mut def);
        let player = Character::new(0.0, 0.0, def, &gear, &actions);
        Game {
            lib: AnimLibrary::builtin(),
            act_dash: actions.id("dash").expect("dash action"),
            act_swing: actions.id("swing").expect("swing action"),
            actions,
            gear,
            player,
            cam: Camera::new(w, h),
            fb: Framebuffer::new(w, h),
            opts: DrawOpts::default(),
            target: None,
            prev: Input::default(),
            accum: 0.0,
            particles: Vec::new(),
            trail: Vec::new(),
            ghosts: Vec::new(),
            flash: 0.0,
            time: 0.0,
            demo: false,
            hair,
            rig: CamRig::new(CamConfig::default()),
            settings: crate::input::Settings::default(),
            settings_changed: false,
            sky_fog: true,
            drag: None,
            rng: 0x1234_5678,
        }
    }

    /// Apply a camera config; its `mode` takes effect immediately (no blend).
    pub fn set_cam_config(&mut self, cfg: CamConfig) {
        self.rig.cfg = cfg;
        let s = self.subject();
        self.rig.snap(cfg.mode, &s);
        self.update_camera(0.0, &Input::default());
    }

    /// Start a blended switch to `mode`.
    pub fn set_cam_mode(&mut self, mode: CamMode) {
        let s = self.subject();
        let cur = self.cam.effective_view();
        self.rig.set_mode(mode, &s, cur);
    }

    fn subject(&self) -> Subject {
        let j = self.player_joints();
        Subject { pos: self.player.pos, yaw: self.player.yaw, head: j.get(rig::Joint::Head) }
    }

    #[cfg(test)]
    pub(crate) fn subject_for_tests(&self) -> Subject {
        self.subject()
    }

    /// Advance only the camera (tests).
    #[cfg(test)]
    pub(crate) fn step_camera_for_tests(&mut self, dt: f32) {
        self.update_camera(dt, &Input::default());
    }

    fn update_camera(&mut self, dt: f32, inp: &Input) {
        let s = self.subject();
        let numpad = self.settings.cam_control == crate::input::CamControl::Numpad;
        let lp = if numpad { inp.look_down as i32 as f32 - inp.look_up as i32 as f32 } else { 0.0 };
        let (w, h) = (self.cam.w, self.cam.h);
        self.rig.update(dt, lp, &s, self.cam.cx, self.cam.cz, w, h);
        self.cam.view = self.rig.view;
    }

    /// Mouse camera control: right-drag turns the character (x) and pitches
    /// the camera (y). Returns true while the drag is a turn (past
    /// `DRAG_SLOP`), so the right click is not also a move order.
    fn mouse_drag(&mut self, inp: &Input) -> bool {
        let (Some(m), true) = (inp.mouse, inp.rmb) else {
            self.drag = None;
            return false;
        };
        let (last, travelled, turning) = self.drag.unwrap_or((m, 0.0, false));
        let (dx, dy) = (m.0 - last.0, m.1 - last.1);
        let travelled = travelled + dx.abs() + dy.abs();
        let turning = turning || travelled > DRAG_SLOP;
        if turning {
            self.player.yaw = crate::math::wrap_angle(self.player.yaw - dx * MOUSE_TURN);
            let lim = self.rig.cfg.fp_pitch_limit_deg.to_radians();
            self.rig.fp_pitch = (self.rig.fp_pitch + dy * MOUSE_PITCH).clamp(-lim, lim);
        }
        self.drag = Some((m, travelled, turning));
        turning
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// Scripted input for `#demo` / screenshots.
    fn demo_input(&self) -> Input {
        let t = self.time % 8.0;
        let mut i = Input::default();
        match t {
            t if t < 2.0 => i.right = true,
            t if t < 3.6 => { i.down = true; i.run = true; }
            t if t < 3.7 => { i.left = true; i.dash = true; }
            t if t < 4.5 => i.left = true,
            t if t < 4.6 => i.attack = true,
            t if t < 5.4 => {}
            t if t < 7.0 => { i.up = true; i.run = true; i.left = t < 6.0; }
            _ => {}
        }
        i
    }

    /// Advance by real elapsed seconds using a fixed 60 Hz timestep.
    pub fn update(&mut self, dt: f32, input: &mut Input) {
        self.accum = (self.accum + dt.min(0.25)).min(0.25);
        while self.accum >= TICK {
            self.accum -= TICK;
            let inp = if self.demo { self.demo_input() } else { *input };
            self.tick(&inp);
            input.move_click = None;
            input.attack_click = None;
        }
    }

    fn tick(&mut self, inp: &Input) {
        self.time += TICK;
        // screen-aligned WASD -> iso world directions
        let (mut sx, mut sy) = (0.0f32, 0.0f32);
        if inp.up { sy -= 1.0; }
        if inp.down { sy += 1.0; }
        if inp.left { sx -= 1.0; }
        if inp.right { sx += 1.0; }
        let iso = self.rig.mode == CamMode::Iso;
        let mut keep_facing = false;
        let mut dir = if iso {
            (sx + sy, sy - sx)
        } else {
            // 3D: A/D and Q/E turn the character (the camera follows its
            // facing); W/S walk forward/back along the facing.
            let numpad = self.settings.cam_control == crate::input::CamControl::Numpad;
            let keys = if numpad { inp.look_right as i32 as f32 - inp.look_left as i32 as f32 } else { 0.0 };
            let turn = sx + keys;
            self.player.yaw = crate::math::wrap_angle(self.player.yaw - turn.clamp(-1.0, 1.0) * TURN_SPEED * TICK);
            keep_facing = true;
            sx = 0.0;
            let (sn, cs) = self.player.yaw.sin_cos();
            (sn * -sy, cs * -sy)
        };
        let mut move_click = inp.move_click;
        if !iso && self.settings.cam_control == crate::input::CamControl::Mouse {
            if self.mouse_drag(inp) {
                move_click = None;
                self.target = None;
            }
        } else {
            self.drag = None;
        }
        if !iso && (inp.look_left || inp.look_right || inp.left || inp.right) {
            self.target = None;
        }
        if sx != 0.0 || sy != 0.0 {
            self.target = None;
        }
        if let Some((mx, my)) = move_click {
            if let Some(t) = self.cam.pick_ground(mx, my) {
                self.target = Some(t);
            }
        }
        if let Some((tx, tz)) = self.target {
            let (dx, dz) = (tx - self.player.pos.x, tz - self.player.pos.z);
            if dx * dx + dz * dz < 0.05 {
                self.target = None;
            } else {
                dir = (dx, dz);
                keep_facing = false; // turn toward the click target
            }
        }
        let attack_edge = (inp.attack && !self.prev.attack) || inp.attack_click.is_some();
        let aim_px = inp.attack_click.or(inp.mouse.filter(|_| inp.attack));
        let dash = inp.dash && !self.prev.dash;
        let request = if attack_edge {
            Some(self.act_swing)
        } else if dash {
            Some(self.act_dash)
        } else {
            None
        };
        let it = Intent {
            dir,
            run: inp.run,
            request,
            release: false,
            aim: aim_px.and_then(|(x, y)| self.cam.pick_ground(x, y)),
            keep_facing,
        };
        if request.is_some() {
            self.target = None;
        }
        if (inp.hair_style && !self.prev.hair_style) || (inp.hair_color && !self.prev.hair_color) {
            if inp.hair_style && !self.prev.hair_style { self.hair.cycle_style(1); }
            if inp.hair_color && !self.prev.hair_color { self.hair.cycle_color(1); }
            self.hair.apply(&mut self.player.def);
        }
        if inp.cam_control_toggle && !self.prev.cam_control_toggle {
            self.settings.cam_control = self.settings.cam_control.toggled();
            self.settings_changed = true;
        }
        if inp.cam_cycle && !self.prev.cam_cycle {
            self.set_cam_mode(self.rig.mode.next());
        }
        self.prev = *inp;
        let was = self.player.action;
        let was_hit = self.player.hit_active();
        self.player.step(&self.lib, &self.actions, &it, TICK);
        let jn = self.player_joints();
        self.player.update_secondary(&jn, TICK);

        // effects
        let p = self.player.pos;
        let act = self.player.action;
        if act == self.act_dash {
            let j = self.player_joints();
            self.ghosts.push((j, 0.25));
            if was != self.act_dash {
                for _ in 0..14 { self.spawn_dust(p, 2.5); }
            }
        }
        if act == self.actions.run && (self.time * 10.0) as i32 % 2 == 0 && self.rand() > 0.3 {
            self.spawn_dust(p, 0.8);
        }
        if act == self.act_swing {
            let j = self.player_joints();
            let reach = self.player.def.equipped(GearSlot::MainHand, &self.gear).map(|g| g.reach).unwrap_or(0.9);
            let tip = j.weapon_point(1, reach);
            self.trail.push((j.weapon_point(1, 0.2), tip, 0.12));
            if self.player.hit_active() && self.rand() > 0.4 {
                let v = v3(self.rand(), self.rand().abs() + 0.5, self.rand()) * 2.0;
                self.particles.push(Particle { p: tip, v, life: 0.3, c: C_GOLD });
            }
            if self.player.hit_active() && !was_hit {
                self.flash = 2.0 * TICK; // demo of the hit-flash RampMap
            }
        }
        self.flash = (self.flash - TICK).max(0.0);
        for g in &mut self.ghosts { g.1 -= TICK; }
        self.ghosts.retain(|g| g.1 > 0.0);
        for t in &mut self.trail { t.2 -= TICK; }
        self.trail.retain(|t| t.2 > 0.0);
        for q in &mut self.particles {
            q.life -= TICK;
            q.v.y -= 6.0 * TICK;
            q.p = q.p + q.v * TICK;
            if q.p.y < 0.0 { q.p.y = 0.0; q.v = q.v * 0.3; }
        }
        self.particles.retain(|q| q.life > 0.0);

        // camera follow
        self.cam.cx += (p.x - self.cam.cx) * 0.08;
        self.cam.cz += (p.z - self.cam.cz) * 0.08;
        self.update_camera(TICK, inp);
    }

    /// FK joints of the player for the current pose.
    pub fn player_joints(&self) -> rig::Joints {
        let c = &self.player;
        let b = c.def.effective_body(&self.gear);
        let mut j = rig::solve(&c.anim.pose, &b, c.pos, c.yaw, c.gait.sway);
        j.hair_sway = c.hair_sway();
        j
    }

    fn spawn_dust(&mut self, p: Vec3, s: f32) {
        let v = v3(self.rand() * s, self.rand().abs() * s * 0.6, self.rand() * s);
        let life = 0.35 + self.rand().abs() * 0.3;
        self.particles.push(Particle { p: p + v3(0.0, 0.05, 0.0), v, life, c: C_DUST });
    }

    pub fn render(&mut self) {
        self.draw_ground();
        self.fb.clear_depth();
        let cam = self.cam;
        let pp = cam.project(self.player.pos);
        let head = self.player_joints().get(rig::Joint::Head);
        // First person once the eye reaches the head: hide head and torso.
        let fp = cam.view.and_then(|v| v.eye()).is_some_and(|e| (e - head).len() < crate::camrig::FP_HIDE_RADIUS);
        if !pp.culled() && !fp {
            self.fb.shadow_ellipse(pp.x, pp.y, 13.0 * pp.s, 6.0 * pp.s, C_SHADOW);
        }
        if let Some((tx, tz)) = self.target {
            let t = cam.project(v3(tx, 0.0, tz));
            let a = |dx: f32, dy: f32| crate::camera::P2 { x: t.x + dx * t.s, y: t.y + dy * t.s, d: 1e6, s: t.s };
            self.fb.line(a(-5.0, 0.0), a(0.0, -3.0), C_GOLD);
            self.fb.line(a(0.0, -3.0), a(5.0, 0.0), C_GOLD);
            self.fb.line(a(5.0, 0.0), a(0.0, 3.0), C_GOLD);
            self.fb.line(a(0.0, 3.0), a(-5.0, 0.0), C_GOLD);
        }
        let ghosts: Vec<_> = self.ghosts.iter().map(|g| g.0).collect();
        let body = self.player.def.effective_body(&self.gear);
        let ghost = RampMap::default().tint(R_ARCANE).team(R_ARCANE);
        for j in ghosts.iter().step_by(3) {
            render::draw_body(&mut self.fb, &cam, j, &body, &ghost, &self.opts);
            // push ghosts behind the live figure
            for d in self.fb.depth.iter_mut() { if *d < f32::INFINITY { *d = f32::INFINITY; } }
        }
        for (x, z) in PILLARS {
            let b = cam.project(v3(x, 0.0, z));
            if !b.culled() {
                self.fb.shadow_ellipse(b.x, b.y, 16.0 * b.s, 7.0 * b.s, C_SHADOW);
            }
            self.fb.capsule(b, cam.project(v3(x, 2.2, z)), 9.0, shade(R_STONE, 5), Some(C_OUTLINE));
        }
        let j = self.player_joints();
        let mut map = self.player.def.colors;
        if self.flash > 0.0 {
            map = map.flashed();
        }
        let opts = DrawOpts { first_person: fp, ..self.opts };
        render::draw_character(&mut self.fb, &cam, &j, &self.player.def, &self.gear, &map, &opts);
        for (a, b, _) in &self.trail {
            let (pa, pb) = (cam.project(*a), cam.project(*b));
            self.fb.line(pa, pb, C_TRAIL);
        }
        for q in &self.particles {
            let s = cam.project(q.p);
            if s.culled() {
                continue;
            }
            self.fb.plot(s.x as i32, s.y as i32, s.d, q.c);
            if q.life > 0.2 { self.fb.plot(s.x as i32 + 1, s.y as i32, s.d, q.c); }
        }
    }

    fn draw_ground(&mut self) {
        if let Some(v) = self.cam.view {
            return self.draw_ground_view(&GroundMap::new(&v), &v);
        }
        let cam = self.cam;
        let (w, h) = (self.fb.w, self.fb.h);
        // incremental unprojection: ground coords are affine in screen x/y
        let (x0, z0) = cam.unproject_ground(0.5, 0.5);
        let (x1, z1) = cam.unproject_ground(1.5, 0.5);
        let (x2, z2) = cam.unproject_ground(0.5, 1.5);
        let (dxx, dzx, dxy, dzy) = (x1 - x0, z1 - z0, x2 - x0, z2 - z0);
        for y in 0..h {
            let (mut gx, mut gz) = (x0 + dxy * y as f32, z0 + dzy * y as f32);
            let row = &mut self.fb.color[y * w..(y + 1) * w];
            for px in row.iter_mut() {
                let (tx, tz) = (gx.floor(), gz.floor());
                let (fx, fz) = (gx - tx, gz - tz);
                let hsh = ((tx as i32).wrapping_mul(73856093) ^ (tz as i32).wrapping_mul(19349663)) as u32;
                let base = if (tx as i32 + tz as i32) & 1 == 0 { shade(R_STONE, 2) } else { shade(R_STONE, 4) };
                let base = if hsh.is_multiple_of(11) { shade(R_STONE, 1) } else { base };
                *px = if fx < 0.03 || fz < 0.03 {
                    shade(R_STONE, 1)
                } else if (hsh >> 8).is_multiple_of(7) && ((fx * 7.0) as i32 + (fz * 5.0) as i32) % 5 == 0 {
                    shade(R_STONE, 5) // pebbles
                } else {
                    base
                };
                gx += dxx;
                gz += dzx;
            }
        }
    }

    /// Ground through the screen->ground homography (one divide per pixel);
    /// pixels above the horizon or past `FAR` get the sky.
    /// 3D floor plus sky: a dithered dusk gradient above the horizon and
    /// ordered-dither distance fog on the ground that reaches the horizon
    /// colour at `FAR`, so the floor never ends in a hard edge.
    fn draw_ground_view(&mut self, g: &GroundMap, v: &crate::camera::View) {
        let (w, h) = (self.fb.w, self.fb.h);
        let st = g.step_x();
        let sky = SkyFog::new(v, h as f32);
        let on = self.sky_fog;
        for y in 0..h {
            let mut n = g.raw(0.5, y as f32 + 0.5);
            let row = &mut self.fb.color[y * w..(y + 1) * w];
            let sky_px = sky.sky_row(y);
            for (x, px) in row.iter_mut().enumerate() {
                let th = BAYER4[(y & 3) * 4 + (x & 3)];
                *px = if !on {
                    match g.resolve(n) {
                        None => shade(R_VOID, 0),
                        Some((gx, gz)) => ground_texel(gx, gz),
                    }
                } else { match g.resolve_dist(n) {
                    Some((gx, gz, d)) if d <= FAR || v.inv_d <= 0.0 => {
                        if sky.fog(d) > th { FOG_COLOR } else { ground_texel(gx, gz) }
                    }
                    Some(_) => FOG_COLOR,
                    None if y as f32 >= sky.horizon => FOG_COLOR,
                    None => sky_px.pick(th),
                } };
                n = [n[0] + st[0], n[1] + st[1], n[2] + st[2]];
            }
        }
    }
}

use crate::camera::FAR;

/// 4x4 ordered-dither thresholds in (0, 1).
const BAYER4: [f32; 16] = {
    let m = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
    let mut o = [0.0f32; 16];
    let mut i = 0;
    while i < 16 {
        o[i] = (m[i] as f32 + 0.5) / 16.0;
        i += 1;
    }
    o
};

/// Fog and horizon colour (dust ramp, matches the stone floor's hue).
pub const FOG_COLOR: u8 = shade(R_DUST, 1);
/// Fog starts at this eye distance (metres) and is full at `FAR`.
pub const FOG_START: f32 = 18.0;
/// Sky bands, horizon -> zenith.
const SKY_BANDS: [u8; 6] = [shade(R_DUST, 1), shade(R_DUST, 0), shade(R_STONE, 4), shade(R_STONE, 2), shade(R_STONE, 0), shade(R_VOID, 0)];

#[derive(Clone, Copy)]
struct SkyPx {
    a: u8,
    b: u8,
    t: f32,
}
impl SkyPx {
    #[inline]
    fn pick(self, th: f32) -> u8 {
        if self.t > th { self.b } else { self.a }
    }
}

/// Per-frame sky / fog parameters for one view.
struct SkyFog {
    horizon: f32,
    span: f32,
}
impl SkyFog {
    fn new(v: &crate::camera::View, h: f32) -> SkyFog {
        let f = v.fwd();
        let fh = v3(f.x, 0.0, f.z);
        let l = (fh.x * fh.x + fh.z * fh.z).sqrt().max(1e-4);
        let eye = v.eye().unwrap_or(v.target);
        let p = Camera::project_view(v, eye + fh * (1.0e4 / l));
        let horizon = if p.culled() { -1.0 } else { p.y };
        SkyFog { horizon, span: (h * 0.3).max(1.0) }
    }
    /// Gradient position for a sky row, as two bands and a dither mix.
    fn sky_row(&self, y: usize) -> SkyPx {
        let u = ((self.horizon - y as f32) / self.span).clamp(0.0, 0.999) * (SKY_BANDS.len() - 1) as f32;
        let i = u as usize;
        SkyPx { a: SKY_BANDS[i], b: SKY_BANDS[(i + 1).min(SKY_BANDS.len() - 1)], t: u - i as f32 }
    }
    /// Fog amount (0..1) at eye distance `d`.
    #[inline]
    fn fog(&self, d: f32) -> f32 {
        ((d - FOG_START) / (FAR - FOG_START)).clamp(0.0, 1.0)
    }
}

/// Floor colour at a ground point (shared by the iso and 3D ground passes).
#[inline]
fn ground_texel(gx: f32, gz: f32) -> u8 {
    let (tx, tz) = (gx.floor(), gz.floor());
    let (fx, fz) = (gx - tx, gz - tz);
    let hsh = ((tx as i32).wrapping_mul(73856093) ^ (tz as i32).wrapping_mul(19349663)) as u32;
    let base = if (tx as i32 + tz as i32) & 1 == 0 { shade(R_STONE, 2) } else { shade(R_STONE, 4) };
    let base = if hsh.is_multiple_of(11) { shade(R_STONE, 1) } else { base };
    if fx < 0.03 || fz < 0.03 {
        shade(R_STONE, 1)
    } else if (hsh >> 8).is_multiple_of(7) && ((fx * 7.0) as i32 + (fz * 5.0) as i32) % 5 == 0 {
        shade(R_STONE, 5) // pebbles
    } else {
        base
    }
}
