//! Minimal playable scene: one stick hero on a procedural tile floor.
use crate::actions::ActionTable;
use crate::anim::AnimLibrary;
use crate::body::{CharacterDef, GearRef, Sex};
use crate::camera::Camera;
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
}

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
            rng: 0x1234_5678,
        }
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
        let mut dir = (sx + sy, sy - sx);
        if sx != 0.0 || sy != 0.0 {
            self.target = None;
        }
        if let Some((mx, my)) = inp.move_click {
            self.target = Some(self.cam.unproject_ground(mx, my));
        }
        if let Some((tx, tz)) = self.target {
            let (dx, dz) = (tx - self.player.pos.x, tz - self.player.pos.z);
            if dx * dx + dz * dz < 0.05 {
                self.target = None;
            } else {
                dir = (dx, dz);
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
            aim: aim_px.map(|(x, y)| self.cam.unproject_ground(x, y)),
        };
        if request.is_some() {
            self.target = None;
        }
        self.prev = *inp;
        let was = self.player.action;
        let was_hit = self.player.hit_active();
        self.player.step(&self.lib, &self.actions, &it, TICK);

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
    }

    /// FK joints of the player for the current pose.
    pub fn player_joints(&self) -> rig::Joints {
        let c = &self.player;
        let b = c.def.effective_body(&self.gear);
        rig::solve(&c.anim.pose, &b, c.pos, c.yaw, c.gait.sway)
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
        self.fb.shadow_ellipse(pp.x, pp.y, 13.0, 6.0, C_SHADOW);
        if let Some((tx, tz)) = self.target {
            let t = cam.project(v3(tx, 0.0, tz));
            let a = |dx: f32, dy: f32| crate::camera::P2 { x: t.x + dx, y: t.y + dy, d: 1e6 };
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
            self.fb.shadow_ellipse(b.x, b.y, 16.0, 7.0, C_SHADOW);
            self.fb.capsule(b, cam.project(v3(x, 2.2, z)), 9.0, shade(R_STONE, 5), Some(C_OUTLINE));
        }
        let j = self.player_joints();
        let mut map = self.player.def.colors;
        if self.flash > 0.0 {
            map = map.flashed();
        }
        render::draw_character(&mut self.fb, &cam, &j, &self.player.def, &self.gear, &map, &self.opts);
        for (a, b, _) in &self.trail {
            let (pa, pb) = (cam.project(*a), cam.project(*b));
            self.fb.line(pa, pb, C_TRAIL);
        }
        for q in &self.particles {
            let s = cam.project(q.p);
            self.fb.plot(s.x as i32, s.y as i32, s.d, q.c);
            if q.life > 0.2 { self.fb.plot(s.x as i32 + 1, s.y as i32, s.d, q.c); }
        }
    }

    fn draw_ground(&mut self) {
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
}
