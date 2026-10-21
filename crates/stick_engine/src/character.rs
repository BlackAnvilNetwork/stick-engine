// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Character controller: a generic state machine over the data-driven
//! [`ActionTable`]. Locomotion is procedural ([`Gait`]); actions play on the
//! animator's action layer; gameplay windows (hit, i-frames, cancel) come
//! from clip events, for any action.
use crate::actions::{ActionTable, Face, Mode, RootMotion};
use crate::anim::{AnimLibrary, Animator, Pose};
use crate::body::CharacterDef;
use crate::gait::Gait;
use crate::gear::GearLib;
use crate::math::{angle_diff, v3, Vec3};

/// Per-tick intent produced from raw input by the game.
#[derive(Clone, Copy, Debug, Default)]
pub struct Intent {
    /// Desired move direction on the ground (x, z), unnormalised; zero = none.
    pub dir: (f32, f32),
    pub run: bool,
    /// Action to start this tick (index into the `ActionTable`).
    pub request: Option<usize>,
    /// Release a held (Cast) action.
    pub release: bool,
    /// Where to face for `Face::Aim` actions (world x, z), if known.
    pub aim: Option<(f32, f32)>,
    /// Keep the current facing while moving (3D camera modes: W/S walk
    /// forward/back along the facing and the game turns the character).
    pub keep_facing: bool,
}

/// Gameplay windows opened/closed by clip events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Windows {
    pub hit: bool,
    pub iframes: bool,
    pub cancel: bool,
}

#[derive(Clone, Debug)]
pub struct Character {
    pub pos: Vec3,
    pub yaw: f32,
    pub vel: (f32, f32),
    pub action: usize,
    pub def: CharacterDef,
    pub stance: String,
    pub swing_weight: f32,
    pub anim: Animator,
    pub gait: Gait,
    pub windows: Windows,
    pub dash_dir: (f32, f32),
    cooldowns: Vec<f32>,
    idle_t: f32,
    /// Hair sway springs on world x, y, z (only for styles that sway).
    pub hair_spring: [HairSpring; 3],
}

/// Damped spring for hair secondary motion: the soft mass lags the anchor's
/// acceleration. `x` = displacement (m) along one world axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HairSpring {
    pub x: f32,
    pub v: f32,
    prev_y: f32,
    prev_vy: f32,
    primed: u8,
}

impl HairSpring {
    /// Natural frequency (Hz); damping ratio. Settles in ~0.25 s.
    pub const FREQ: f32 = 5.0;
    pub const ZETA: f32 = 0.45;
    /// How strongly the anchor's acceleration drives the spring.
    pub const GAIN: f32 = 1.5;
    /// Hard bound on the displacement (m).
    pub const MAX: f32 = 0.035;

    pub fn step(&mut self, y: f32, dt: f32, gain: f32) {
        if dt <= 0.0 {
            return;
        }
        let vy = if self.primed > 0 { (y - self.prev_y) / dt } else { 0.0 };
        let ay = if self.primed > 1 { (vy - self.prev_vy) / dt } else { 0.0 };
        self.prev_y = y;
        self.prev_vy = vy;
        self.primed = (self.primed + 1).min(2);
        let w = std::f32::consts::TAU * Self::FREQ;
        let drive = -(ay * Self::GAIN * gain).clamp(-60.0, 60.0);
        let a = drive - w * w * self.x - 2.0 * Self::ZETA * w * self.v;
        self.v += a * dt;
        self.x += self.v * dt;
        if self.x.abs() > Self::MAX {
            self.x = self.x.clamp(-Self::MAX, Self::MAX);
            self.v = 0.0;
        }
    }
}

impl Character {
    pub fn new(x: f32, z: f32, def: CharacterDef, gear: &GearLib, actions: &ActionTable) -> Self {
        let mut c = Character {
            pos: v3(x, 0.0, z), yaw: 0.8, vel: (0.0, 0.0), action: actions.idle, stance: String::new(),
            swing_weight: 1.0, def, anim: Animator::new(), gait: Gait::default(), windows: Windows::default(),
            dash_dir: (1.0, 0.0), cooldowns: vec![0.0; actions.defs.len()], idle_t: 0.0,
            hair_spring: [HairSpring::default(); 3],
        };
        c.refresh_gear(gear);
        c
    }

    /// Advance hair secondary motion from freshly solved joints (call once
    /// per tick after `step`).
    pub fn update_secondary(&mut self, j: &crate::rig::Joints, dt: f32) {
        if self.def.body.hair.sways() {
            let h = j.get(crate::rig::Joint::Head);
            for (s, v) in self.hair_spring.iter_mut().zip([h.x, h.y, h.z]) {
                s.step(v, dt, Self::HAIR_GAIN);
            }
        } else {
            self.hair_spring = [HairSpring::default(); 3];
        }
    }
    pub const HAIR_GAIN: f32 = 1.6;
    /// Current hair sway offset (world metres) for `Joints::hair_sway`.
    pub fn hair_sway(&self) -> crate::math::Vec3 {
        let [x, y, z] = self.hair_spring;
        crate::math::v3(x.x, y.x, z.x)
    }

    /// Recompute stance and swing weight after the gear changed.
    pub fn refresh_gear(&mut self, gear: &GearLib) {
        self.stance = self.def.stance(gear);
        self.swing_weight = self.def.swing_weight(gear);
    }

    pub fn speed(&self) -> f32 {
        (self.vel.0 * self.vel.0 + self.vel.1 * self.vel.1).sqrt()
    }
    pub fn hit_active(&self) -> bool {
        self.windows.hit
    }
    pub fn action_id<'a>(&self, t: &'a ActionTable) -> &'a str {
        &t.get(self.action).id
    }

    /// Can `a` start now (cooldown, clip available for the stance, not
    /// blocked by a busy action outside its cancel window)?
    pub fn can_start(&self, t: &ActionTable, a: usize) -> bool {
        let d = t.get(a);
        if self.cooldowns.get(a).copied().unwrap_or(0.0) > 0.0 {
            return false;
        }
        if !matches!(d.mode, Mode::Gait { .. }) && d.clip_for(&self.stance).is_none() {
            return false;
        }
        d.mode == Mode::Additive || !t.get(self.action).is_busy() || self.windows.cancel
    }

    fn enter(&mut self, t: &ActionTable, a: usize) {
        let d = t.get(a);
        self.action = a;
        self.windows = Windows::default();
        if let Some(c) = self.cooldowns.get_mut(a) {
            *c = d.cooldown;
        }
        match d.mode {
            Mode::Idle | Mode::Gait { .. } => self.anim.play(None, d.blend, 1.0, false),
            Mode::OneShot { .. } | Mode::Cast { .. } => {
                let rate = if d.weapon { 1.0 / self.swing_weight } else { 1.0 };
                self.anim.play(d.clip_for(&self.stance), d.blend, rate, true);
            }
            Mode::Additive => {}
        }
    }

    pub fn step(&mut self, lib: &AnimLibrary, t: &ActionTable, it: &Intent, dt: f32) {
        for c in &mut self.cooldowns {
            *c = (*c - dt).max(0.0);
        }
        let len = (it.dir.0 * it.dir.0 + it.dir.1 * it.dir.1).sqrt();
        let dir = if len > 1e-4 { Some((it.dir.0 / len, it.dir.1 / len)) } else { None };

        // finished one-shots return to their `next`
        if t.get(self.action).is_busy() && self.anim.action_finished(lib) {
            let next = t.get(self.action).next().and_then(|n| t.id(n)).unwrap_or(t.idle);
            self.enter(t, next);
        }
        if it.release {
            if let Some(a) = &mut self.anim.action {
                a.holding = false;
            }
        }

        let mut started = false;
        if let Some(mut a) = it.request {
            if a == self.action {
                if let Some(c) = t.get(a).chain.as_deref().and_then(|c| t.id(c)) {
                    a = c;
                }
            }
            if self.can_start(t, a) {
                let d = t.get(a);
                if d.mode == Mode::Additive {
                    if let Some(c) = d.clip_for(&self.stance) {
                        self.anim.play_additive(c);
                    }
                } else {
                    match d.face {
                        Face::Aim => {
                            let aimed = it.aim.map(|(ax, az)| (ax - self.pos.x, az - self.pos.z))
                                .filter(|(dx, dz)| dx * dx + dz * dz > 1e-4);
                            if let Some((dx, dz)) = aimed.or(dir) {
                                self.yaw = dx.atan2(dz);
                            }
                        }
                        Face::Move => {
                            self.dash_dir = dir.unwrap_or((self.yaw.sin(), self.yaw.cos()));
                            self.yaw = self.dash_dir.0.atan2(self.dash_dir.1);
                        }
                        Face::Keep => {}
                    }
                    self.enter(t, a);
                    started = true;
                }
            }
        }
        if !started && !t.get(self.action).is_busy() {
            let next = match dir {
                Some(_) if it.run => t.run,
                Some(_) => t.walk,
                None => t.idle,
            };
            if next != self.action {
                self.enter(t, next);
            }
        }

        // movement
        let d = t.get(self.action);
        let walk = match t.get(t.walk).mode {
            Mode::Gait { speed } => speed,
            _ => 2.2,
        };
        let mv = |s: f32| dir.map(|d| (d.0 * s, d.1 * s)).unwrap_or((0.0, 0.0));
        let (target_vel, snap) = match (&d.mode, d.root_motion) {
            (_, Some(RootMotion::Dash { speed })) => ((self.dash_dir.0 * speed, self.dash_dir.1 * speed), true),
            (Mode::Gait { speed }, _) => (mv(*speed), false),
            (Mode::Idle, _) => ((0.0, 0.0), false),
            _ => (mv(walk * d.move_mult), false),
        };
        let k = if snap { 1.0 } else { (dt * 14.0).min(1.0) };
        self.vel.0 += (target_vel.0 - self.vel.0) * k;
        self.vel.1 += (target_vel.1 - self.vel.1) * k;
        self.pos.x += self.vel.0 * dt;
        self.pos.z += self.vel.1 * dt;

        if matches!(d.mode, Mode::Gait { .. }) && self.speed() > 0.1 && !it.keep_facing {
            let want = self.vel.0.atan2(self.vel.1);
            self.yaw += angle_diff(self.yaw, want) * (dt * 12.0).min(1.0);
        } else if d.is_busy() && d.turn_rate > 0.0 {
            if let Some((ax, az)) = it.aim {
                let want = (ax - self.pos.x).atan2(az - self.pos.z);
                let max = d.turn_rate.to_radians() * dt;
                self.yaw += angle_diff(self.yaw, want).clamp(-max, max);
            }
        }

        // base layer: idle clip blended into the gait by ground speed
        let speed = self.speed();
        let body = self.def.body;
        let sx = self.def.sex.gait();
        let gait_pose = self.gait.update(dt, speed, &body, &self.def.personality, &sx);
        self.idle_t += dt;
        let idle_clip = t.get(t.idle).clip_for(&self.stance).and_then(|c| lib.get(c));
        let idle_pose = idle_clip.map(|c| c.sample(self.idle_t)).unwrap_or_default();
        let m = (speed / 0.8).clamp(0.0, 1.0);
        let base: Pose = idle_pose.blend(&gait_pose, m * m * (3.0 - 2.0 * m));
        self.anim.update(lib, dt, &base);

        // clip events drive the gameplay windows
        if let Some(a) = &self.anim.action {
            if let Some(c) = lib.get(&a.clip) {
                for e in c.meta.events_between(a.prev_time, a.time) {
                    match e.name.as_str() {
                        "hit_start" => self.windows.hit = true,
                        "hit_end" => self.windows.hit = false,
                        "iframes_start" => self.windows.iframes = true,
                        "iframes_end" => self.windows.iframes = false,
                        "cancel_open" => self.windows.cancel = true,
                        "cancel_close" => self.windows.cancel = false,
                        _ => {}
                    }
                }
            }
        }
    }
}
