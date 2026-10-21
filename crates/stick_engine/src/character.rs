//! Character controller + action state machine.
//!
//! Each `Action` maps to an `ActionDef` (clip name, blend time, how it plays).
//! Adding a move = clip RON file + `Action` variant + `ACTIONS` entry +
//! a trigger in `Character::step` (see README).
use crate::anim::{AnimLibrary, Animator};
use crate::math::{angle_diff, v3, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Idle,
    Walk,
    Run,
    Dash,
    Swing,
}

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    /// Looping locomotion; playback rate = speed / ref_speed.
    Loop { ref_speed: f32 },
    /// Plays once (clip duration), cannot be interrupted, then returns to `next`.
    OneShot { next: Action },
}

#[derive(Clone, Copy, Debug)]
pub struct ActionDef {
    pub action: Action,
    pub clip: &'static str,
    pub blend: f32,
    pub mode: Mode,
}

/// Registration table: action -> clip + playback rules.
pub const ACTIONS: &[ActionDef] = &[
    ActionDef { action: Action::Idle, clip: "idle", blend: 0.15, mode: Mode::Loop { ref_speed: 0.0 } },
    ActionDef { action: Action::Walk, clip: "walk", blend: 0.15, mode: Mode::Loop { ref_speed: WALK_SPEED } },
    ActionDef { action: Action::Run, clip: "run", blend: 0.12, mode: Mode::Loop { ref_speed: RUN_SPEED } },
    ActionDef { action: Action::Dash, clip: "dash", blend: 0.04, mode: Mode::OneShot { next: Action::Idle } },
    ActionDef { action: Action::Swing, clip: "swing", blend: 0.05, mode: Mode::OneShot { next: Action::Idle } },
];

pub fn action_def(a: Action) -> &'static ActionDef {
    ACTIONS.iter().find(|d| d.action == a).expect("action not registered in ACTIONS")
}

pub const WALK_SPEED: f32 = 2.2;
pub const RUN_SPEED: f32 = 5.2;
pub const DASH_SPEED: f32 = 13.0;
pub const DASH_COOLDOWN: f32 = 0.45;

/// Per-tick intent produced from raw input by the game.
#[derive(Clone, Copy, Debug, Default)]
pub struct Intent {
    /// Desired move direction on the ground (x, z), unnormalised; zero = none.
    pub dir: (f32, f32),
    pub run: bool,
    pub dash: bool,
    pub swing: bool,
    /// Where to face when swinging (world x, z), if known.
    pub aim: Option<(f32, f32)>,
}

#[derive(Clone, Debug)]
pub struct Character {
    pub pos: Vec3,
    pub yaw: f32,
    pub vel: (f32, f32),
    pub action: Action,
    pub dash_dir: (f32, f32),
    pub dash_cd: f32,
    pub anim: Animator,
    pub hit_active: bool,
}

impl Character {
    pub fn new(x: f32, z: f32) -> Self {
        Character {
            pos: v3(x, 0.0, z), yaw: 0.8, vel: (0.0, 0.0), action: Action::Idle,
            dash_dir: (1.0, 0.0), dash_cd: 0.0, anim: Animator::new("idle"), hit_active: false,
        }
    }

    pub fn speed(&self) -> f32 {
        (self.vel.0 * self.vel.0 + self.vel.1 * self.vel.1).sqrt()
    }

    fn enter(&mut self, a: Action) {
        let def = action_def(a);
        let restart = matches!(def.mode, Mode::OneShot { .. });
        self.action = a;
        self.anim.play(def.clip, def.blend, restart);
    }

    pub fn step(&mut self, lib: &AnimLibrary, it: &Intent, dt: f32) {
        self.dash_cd = (self.dash_cd - dt).max(0.0);
        let len = (it.dir.0 * it.dir.0 + it.dir.1 * it.dir.1).sqrt();
        let dir = if len > 1e-4 { Some((it.dir.0 / len, it.dir.1 / len)) } else { None };
        let busy = matches!(action_def(self.action).mode, Mode::OneShot { .. });

        if busy {
            if self.anim.finished(lib) {
                if let Mode::OneShot { next } = action_def(self.action).mode {
                    self.enter(next);
                }
            }
        }
        if !matches!(action_def(self.action).mode, Mode::OneShot { .. }) {
            if it.swing {
                if let Some((ax, az)) = it.aim {
                    let (dx, dz) = (ax - self.pos.x, az - self.pos.z);
                    if dx * dx + dz * dz > 1e-4 {
                        self.yaw = dx.atan2(dz);
                    }
                } else if let Some(d) = dir {
                    self.yaw = d.0.atan2(d.1);
                }
                self.enter(Action::Swing);
            } else if it.dash && self.dash_cd <= 0.0 {
                self.dash_dir = dir.unwrap_or((self.yaw.sin(), self.yaw.cos()));
                self.yaw = self.dash_dir.0.atan2(self.dash_dir.1);
                self.dash_cd = DASH_COOLDOWN;
                self.enter(Action::Dash);
            } else {
                let next = match dir {
                    Some(_) if it.run => Action::Run,
                    Some(_) => Action::Walk,
                    None => Action::Idle,
                };
                if next != self.action {
                    self.enter(next);
                }
            }
        }

        // movement per action
        let target_vel = match self.action {
            Action::Walk => dir.map(|d| (d.0 * WALK_SPEED, d.1 * WALK_SPEED)).unwrap_or((0.0, 0.0)),
            Action::Run => dir.map(|d| (d.0 * RUN_SPEED, d.1 * RUN_SPEED)).unwrap_or((0.0, 0.0)),
            Action::Dash => (self.dash_dir.0 * DASH_SPEED, self.dash_dir.1 * DASH_SPEED),
            Action::Swing => (self.vel.0 * 0.2, self.vel.1 * 0.2),
            Action::Idle => (0.0, 0.0),
        };
        let k = if self.action == Action::Dash { 1.0 } else { (dt * 14.0).min(1.0) };
        self.vel.0 += (target_vel.0 - self.vel.0) * k;
        self.vel.1 += (target_vel.1 - self.vel.1) * k;
        self.pos.x += self.vel.0 * dt;
        self.pos.z += self.vel.1 * dt;

        if matches!(self.action, Action::Walk | Action::Run) && self.speed() > 0.1 {
            let want = self.vel.0.atan2(self.vel.1);
            self.yaw += angle_diff(self.yaw, want) * (dt * 12.0).min(1.0);
        }

        let rate = match action_def(self.action).mode {
            Mode::Loop { ref_speed } if ref_speed > 0.0 => (self.speed() / ref_speed).clamp(0.4, 1.6),
            _ => 1.0,
        };
        self.anim.update(lib, dt, rate);
        if let Some(c) = lib.get(&self.anim.clip) {
            if c.event_between("hit_start", self.anim.prev_time, self.anim.time) {
                self.hit_active = true;
            }
            if c.event_between("hit_end", self.anim.prev_time, self.anim.time) || self.action != Action::Swing {
                self.hit_active = false;
            }
        }
    }
}
