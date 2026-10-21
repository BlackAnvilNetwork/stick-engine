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

/// Body is hidden (first-person drawing) while the eye is within this
/// distance of the head centre (metres).
pub const FP_HIDE_RADIUS: f32 = 0.4;

/// Exponential follow rate of the third-person yaw (1/s).
pub const TP_YAW_FOLLOW: f32 = 8.0;

/// Position of a mode on the camera rail: iso 0, third-person 1, first-person 2.
/// Every view is a point on the rail, so any transition that involves
/// first-person travels along the third-person boom (1..2) and never cuts
/// through the body, and FP <-> iso is one continuous eased move.
pub fn rail(mode: CamMode) -> f32 {
    match mode {
        CamMode::Iso => 0.0,
        CamMode::ThirdPerson => 1.0,
        CamMode::FirstPerson => 2.0,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CamRig {
    pub cfg: CamConfig,
    pub mode: CamMode,
    /// Camera yaw for the 3D modes (same convention as character yaw).
    /// Follows the character's facing: locked in first-person, smoothed in
    /// third-person.
    pub yaw: f32,
    /// First-person pitch (radians, positive looks down).
    pub fp_pitch: f32,
    /// Current rail position (see `rail`).
    pub r: f32,
    r_from: f32,
    /// Blend progress 0..1 (1 = settled).
    pub t: f32,
    focus: Vec3,
    /// The view currently in effect (None = legacy iso path).
    pub view: Option<View>,
}

impl CamRig {
    pub fn new(cfg: CamConfig) -> CamRig {
        let r = rail(cfg.mode);
        CamRig { cfg, mode: cfg.mode, yaw: 0.0, fp_pitch: 0.12, r, r_from: r, t: 1.0, focus: v3(0.0, 1.1, 0.0), view: None }
    }

    /// Focal length (px) shared by the 3D modes.
    pub fn focal(&self) -> f32 {
        ISO_SCALE * self.cfg.tp_distance
    }

    fn tp_view(&self, w: f32, h: f32) -> View {
        View {
            target: self.focus,
            yaw: self.yaw,
            pitch: self.cfg.tp_pitch_deg.to_radians(),
            inv_d: 1.0 / self.cfg.tp_distance,
            scale: ISO_SCALE,
            skew: 0.0,
            ax: w * 0.5,
            ay: h * 0.55,
        }
    }

    /// First-person eye position for the subject.
    pub fn fp_eye(&self, s: &Subject) -> Vec3 {
        let fh = v3(self.yaw.sin(), 0.0, self.yaw.cos());
        s.head + fh * 0.14 + v3(0.0, 0.04, 0.0)
    }

    /// View at rail position `r` (0 iso, 1 third-person, 2 first-person).
    /// 0..1 is the ortho -> perspective dolly zoom; 1..2 is the boom from
    /// the third-person position in to the eye at constant focal length.
    #[allow(clippy::too_many_arguments)]
    pub fn view_at(&self, r: f32, s: &Subject, cx: f32, cz: f32, w: f32, h: f32) -> View {
        let r = r.clamp(0.0, 2.0);
        let tp = self.tp_view(w, h);
        if r <= 1.0 {
            return View::iso(cx, cz, w, h).lerp(&tp, r);
        }
        let b = r - 1.0;
        let l = |a: f32, z: f32, t: f32| a + (z - a) * t;
        let d = l(self.cfg.tp_distance, 0.1, b);
        // Pitch reaches the first-person pitch only at the very end of the
        // boom, so the boom keeps its third-person (from-above) direction
        // and never swings through the torso when looking up.
        let pitch = l(tp.pitch, self.fp_pitch, b.powi(4));
        let mut fp = View { target: self.fp_eye(s), yaw: self.yaw, pitch: self.fp_pitch, inv_d: 10.0, scale: 0.0, skew: 0.0, ax: w * 0.5, ay: h * 0.5 };
        fp.target = fp.target + fp.fwd() * 0.1;
        View {
            target: tp.target.lerp(fp.target, b),
            yaw: self.yaw,
            pitch,
            inv_d: 1.0 / d,
            scale: self.focal() / d,
            skew: 0.0,
            ax: w * 0.5,
            ay: l(tp.ay, fp.ay, b),
        }
    }

    /// Settled view for `mode` (iso returns the iso view at `cx, cz`).
    pub fn target_view(&self, mode: CamMode, s: &Subject, cx: f32, cz: f32, w: f32, h: f32) -> View {
        self.view_at(rail(mode), s, cx, cz, w, h)
    }

    /// Switch mode, blending along the rail from where the camera is now.
    pub fn set_mode(&mut self, mode: CamMode, s: &Subject, _current: View) {
        if mode == self.mode {
            return;
        }
        if self.r <= 1e-4 {
            // Entering 3D from iso: start behind the character.
            self.yaw = s.yaw;
            self.focus = s.pos + v3(0.0, 1.1, 0.0);
        }
        self.mode = mode;
        self.r_from = self.r;
        self.t = 0.0;
    }

    /// Jump to a mode without a blend (config at start).
    pub fn snap(&mut self, mode: CamMode, s: &Subject) {
        self.yaw = s.yaw;
        self.focus = s.pos + v3(0.0, 1.1, 0.0);
        self.mode = mode;
        self.r = rail(mode);
        self.r_from = self.r;
        self.t = 1.0;
    }

    /// Blend duration for the current move: one eased timeline, longer
    /// for the chained FP <-> iso move.
    fn duration(&self) -> f32 {
        let span = (rail(self.mode) - self.r_from).abs().max(0.25);
        self.cfg.blend_s.max(1e-3) * (0.5 + 0.5 * span)
    }

    /// Follow the character's facing: locked in first-person, exponential
    /// shortest-arc follow in third-person (no 360-degree spins).
    pub fn follow_yaw(&mut self, dt: f32, subject_yaw: f32) {
        match self.mode {
            CamMode::FirstPerson => self.yaw = subject_yaw,
            _ => {
                let k = 1.0 - (-dt * TP_YAW_FOLLOW).exp();
                self.yaw = crate::math::wrap_angle(self.yaw + crate::math::angle_diff(self.yaw, subject_yaw) * k);
            }
        }
    }

    /// Advance one step: pitch input (-1..1), yaw follow, smoothing, blend.
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, dt: f32, look_pitch: f32, s: &Subject, cx: f32, cz: f32, w: f32, h: f32) {
        if self.mode != CamMode::Iso {
            let lim = self.cfg.fp_pitch_limit_deg.to_radians();
            self.fp_pitch = (self.fp_pitch + look_pitch * self.cfg.look_speed * dt).clamp(-lim, lim);
        }
        self.follow_yaw(dt, s.yaw);
        let want = s.pos + v3(0.0, 1.1, 0.0);
        self.focus = self.focus.lerp(want, 1.0 - (-dt * 8.0).exp());
        if self.t < 1.0 {
            self.t = (self.t + dt / self.duration()).min(1.0);
        }
        let to = rail(self.mode);
        self.r = self.r_from + (to - self.r_from) * ease(self.t);
        self.view = if self.r <= 1e-5 { None } else { Some(self.view_at(self.r, s, cx, cz, w, h)) };
    }
}
