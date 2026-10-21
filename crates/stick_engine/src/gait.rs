// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Procedural locomotion: walk/run cycles generated from ground speed and
//! leg length (no walk/run clips). Output is an ordinary `Pose` used as the
//! animator's base layer.
use crate::anim::{Ch, Pose};
use crate::body::{BodyParams, Personality, SexGait};
use std::f32::consts::{PI, TAU};

pub const G: f32 = 9.81;
/// Froude-number band for the walk -> run blend. With legs ~0.92 m this
/// keeps 2.2 m/s (Fr ~0.54) a walk and 5.2 m/s (Fr ~3.0) a full run.
pub const FR_WALK: f32 = 0.6;
pub const FR_RUN: f32 = 1.5;

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Gait {
    /// Cycle phase in [0, 1).
    pub phase: f32,
    /// Walk (0) .. run (1) blend from the last update.
    pub w: f32,
    /// Lateral hip sway (m) for `rig::solve`.
    pub sway: f32,
    prev_speed: f32,
}

/// Walk/run blend for a speed and leg length.
pub fn run_blend(speed: f32, leg_len: f32) -> f32 {
    smoothstep(FR_WALK, FR_RUN, speed * speed / (G * leg_len))
}

impl Gait {
    /// Advance by `dt` at ground `speed` (m/s) and return the gait pose.
    pub fn update(&mut self, dt: f32, speed: f32, b: &BodyParams, p: &Personality, sx: &SexGait) -> Pose {
        let leg = b.thigh + b.shin;
        let w = run_blend(speed, leg);
        let stride = leg * lerp(1.3, 2.2, w);
        self.phase = (self.phase + speed * dt * p.gait_speed / stride).rem_euclid(1.0);
        let accel = if dt > 0.0 { (speed - self.prev_speed) / dt } else { 0.0 };
        self.prev_speed = speed;
        self.w = w;
        let ph = TAU * self.phase;

        let mut q = Pose::default();
        let a_hip = lerp(25.0, 50.0, w);
        let l_hip = a_hip * ph.sin();
        let r_hip = a_hip * (ph + PI).sin();
        let k = lerp(8.0, 70.0, w);
        let delta = 0.6;
        q.set(Ch::LHip, l_hip);
        q.set(Ch::RHip, r_hip);
        q.set(Ch::LKnee, k * (ph + delta).cos().max(0.0) + 5.0);
        q.set(Ch::RKnee, k * (ph + PI + delta).cos().max(0.0) + 5.0);
        let arm = -0.7 * p.arm_swing * sx.arm_mul;
        q.set(Ch::LShoulder, arm * l_hip);
        q.set(Ch::RShoulder, arm * r_hip);
        q.set(Ch::LElbow, 15.0 + 60.0 * w);
        q.set(Ch::RElbow, 15.0 + 60.0 * w);
        q.set(Ch::LArmOut, 10.0);
        q.set(Ch::RArmOut, 12.0);
        q.set(Ch::RWpnYaw, lerp(20.0, 10.0, w));
        q.set(Ch::RWpnPitch, lerp(-50.0, -30.0, w));
        q.set(Ch::Bob, lerp(0.015, 0.05, w) * (2.0 * ph).cos() - 0.02);
        q.set(Ch::Twist, (4.0 + 6.0 * w) * sx.twist_mul * ph.sin());
        q.set(Ch::Lean, (2.0 + 10.0 * w) + 4.0 * (accel / 10.0).clamp(-1.0, 1.0) + p.slouch);
        self.sway = sx.hip_sway * ph.sin();
        q
    }
}
