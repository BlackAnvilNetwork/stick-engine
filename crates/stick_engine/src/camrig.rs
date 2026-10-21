// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Camera modes on top of `camera::View`: isometric (default), third-person
//! and first-person 3D, with eased, time-based blends between them
//! (dolly zoom between iso and 3D, boom move between first and third person).
//! Only the view changes: gameplay stays on the ground plane.
use crate::camera::{View, ISO_SCALE};
use crate::math::{v3, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CamMode {
    #[default]
    Iso,
    ThirdPerson,
    FirstPerson,
}

impl CamMode {
    /// Cycle order used by the camera key: iso -> third -> first -> iso.
    pub fn next(self) -> CamMode {
        match self {
            CamMode::Iso => CamMode::ThirdPerson,
            CamMode::ThirdPerson => CamMode::FirstPerson,
            CamMode::FirstPerson => CamMode::Iso,
        }
    }
    /// Parse a config value: `iso`, `tp`/`third`, `fp`/`first`.
    pub fn parse(s: &str) -> Option<CamMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "iso" | "isometric" => Some(CamMode::Iso),
            "tp" | "third" | "third-person" => Some(CamMode::ThirdPerson),
            "fp" | "first" | "first-person" => Some(CamMode::FirstPerson),
            _ => None,
        }
    }
}

/// Camera configuration.
#[derive(Clone, Copy, Debug)]
pub struct CamConfig {
    /// Mode at start.
    pub mode: CamMode,
    /// Third-person boom length (metres). Also sets the 3D focal length
    /// (`ISO_SCALE * tp_distance` px) so the iso <-> 3D dolly keeps the
    /// subject's size: 6 m gives a ~67 degree vertical FOV at 360 px.
    pub tp_distance: f32,
    /// Third-person pitch below the horizon (degrees).
    pub tp_pitch_deg: f32,
    /// Blend duration between modes (seconds).
    pub blend_s: f32,
    /// Key look speed (radians per second).
    pub look_speed: f32,
    /// First-person pitch limit (degrees, both ways).
    pub fp_pitch_limit_deg: f32,
}

impl Default for CamConfig {
    fn default() -> Self {
        CamConfig { mode: CamMode::Iso, tp_distance: 6.0, tp_pitch_deg: 30.0, blend_s: 0.7, look_speed: 2.0, fp_pitch_limit_deg: 60.0 }
    }
}

/// Smoothstep easing.
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// What the rig needs from the followed character each tick.
#[derive(Clone, Copy, Debug)]
pub struct Subject {
    pub pos: Vec3,
    pub yaw: f32,
    pub head: Vec3,
}

#[derive(Clone, Copy, Debug)]
pub struct CamRig {
    pub cfg: CamConfig,
    pub mode: CamMode,
    /// Look yaw for the 3D modes (same convention as character yaw).
    pub yaw: f32,
    /// First-person pitch (radians, positive looks down).
    pub fp_pitch: f32,
    from: Option<View>,
    /// Blend progress 0..1 (1 = settled).
    pub t: f32,
    focus: Vec3,
    /// The view currently in effect (None = legacy iso path).
    pub view: Option<View>,
}

impl CamRig {
    pub fn new(cfg: CamConfig) -> CamRig {
        CamRig { cfg, mode: cfg.mode, yaw: 0.0, fp_pitch: 0.12, from: None, t: 1.0, focus: v3(0.0, 1.1, 0.0), view: None }
    }

    /// Focal length (px) shared by the 3D modes.
    pub fn focal(&self) -> f32 {
        ISO_SCALE * self.cfg.tp_distance
    }

    /// Settled view for `mode` (iso returns the iso view at `cx, cz`).
    pub fn target_view(&self, mode: CamMode, s: &Subject, cx: f32, cz: f32, w: f32, h: f32) -> View {
        match mode {
            CamMode::Iso => View::iso(cx, cz, w, h),
            CamMode::ThirdPerson => View {
                target: self.focus,
                yaw: self.yaw,
                pitch: self.cfg.tp_pitch_deg.to_radians(),
                inv_d: 1.0 / self.cfg.tp_distance,
                scale: ISO_SCALE,
                skew: 0.0,
                ax: w * 0.5,
                ay: h * 0.55,
            },
            CamMode::FirstPerson => {
                let d = 0.1;
                let fh = v3(self.yaw.sin(), 0.0, self.yaw.cos());
                let eye = s.head + fh * 0.14 + v3(0.0, 0.04, 0.0);
                let mut v = View { target: eye, yaw: self.yaw, pitch: self.fp_pitch, inv_d: 1.0 / d, scale: 0.0, skew: 0.0, ax: w * 0.5, ay: h * 0.5 };
                v.scale = self.focal() * v.inv_d;
                v.target = eye + v.fwd() * d;
                v
            }
        }
    }

    /// Switch mode, blending from the view currently on screen.
    pub fn set_mode(&mut self, mode: CamMode, s: &Subject, current: View) {
        if mode == self.mode {
            return;
        }
        if self.mode == CamMode::Iso {
            // Entering 3D: start behind the character.
            self.yaw = s.yaw;
            self.focus = s.pos + v3(0.0, 1.1, 0.0);
        }
        self.mode = mode;
        self.from = Some(current);
        self.t = 0.0;
    }

    /// Jump to a mode without a blend (config at start).
    pub fn snap(&mut self, mode: CamMode, s: &Subject) {
        self.yaw = s.yaw;
        self.focus = s.pos + v3(0.0, 1.1, 0.0);
        self.mode = mode;
        self.from = None;
        self.t = 1.0;
    }

    /// Advance one step: look input (yaw, pitch in -1..1), smoothing, blend.
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, dt: f32, look_yaw: f32, look_pitch: f32, s: &Subject, cx: f32, cz: f32, w: f32, h: f32) {
        if self.mode != CamMode::Iso {
            self.yaw -= look_yaw * self.cfg.look_speed * dt;
            let lim = self.cfg.fp_pitch_limit_deg.to_radians();
            self.fp_pitch = (self.fp_pitch + look_pitch * self.cfg.look_speed * dt).clamp(-lim, lim);
        }
        let want = s.pos + v3(0.0, 1.1, 0.0);
        self.focus = self.focus.lerp(want, 1.0 - (-dt * 8.0).exp());
        let to = self.target_view(self.mode, s, cx, cz, w, h);
        if self.t < 1.0 {
            self.t = (self.t + dt / self.cfg.blend_s.max(1e-3)).min(1.0);
        }
        self.view = match (self.from, self.t < 1.0) {
            (Some(f), true) => Some(f.lerp(&to, ease(self.t))),
            _ if self.mode == CamMode::Iso => None,
            _ => Some(to),
        };
        if self.t >= 1.0 {
            self.from = None;
        }
    }

    /// Ground-plane movement direction for screen-aligned keys (sx right,
    /// sy down) in the 3D modes: forward follows the look yaw.
    pub fn move_dir(&self, sx: f32, sy: f32) -> (f32, f32) {
        let (sn, cs) = self.yaw.sin_cos();
        (sn * -sy - cs * sx, cs * -sy + sn * sx)
    }
}
