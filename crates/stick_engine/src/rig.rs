// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Stick-figure rig: skeleton definition and forward kinematics from a
//! [`Pose`] into 3D joints. Pure maths: no raster, camera or platform code,
//! so it runs headless (tools, tests, servers). Drawing lives in `render`.
use crate::anim::{wrap_deg, Ch, Pose};
use crate::body::BodyParams;
use crate::math::{v3, Vec3};

/// Joint points produced by FK.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Joint {
    Pelvis, Chest, Neck, Head, LHip, RHip, LKnee, RKnee, LFoot, RFoot,
    LShoulder, RShoulder, LElbow, RElbow, LHand, RHand,
}
pub const N_JOINTS: usize = 16;
pub const JOINT_NAMES: [&str; N_JOINTS] = [
    "pelvis", "chest", "neck", "head", "l_hip", "r_hip", "l_knee", "r_knee", "l_foot", "r_foot",
    "l_shoulder", "r_shoulder", "l_elbow", "r_elbow", "l_hand", "r_hand",
];
pub fn joint(name: &str) -> Option<Joint> {
    JOINT_NAMES.iter().position(|n| *n == name).map(|i| ALL_JOINTS[i])
}
const ALL_JOINTS: [Joint; N_JOINTS] = [
    Joint::Pelvis, Joint::Chest, Joint::Neck, Joint::Head, Joint::LHip, Joint::RHip, Joint::LKnee,
    Joint::RKnee, Joint::LFoot, Joint::RFoot, Joint::LShoulder, Joint::RShoulder, Joint::LElbow,
    Joint::RElbow, Joint::LHand, Joint::RHand,
];

/// Drawn bone segments (each has its own thickness multiplier).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Bone {
    Hips, Spine, Neck, Shoulders, Head, LThigh, RThigh, LShin, RShin,
    LUpperArm, RUpperArm, LForearm, RForearm,
}
pub const N_BONES: usize = 13;
pub const BONE_NAMES: [&str; N_BONES] = [
    "hips", "spine", "neck", "shoulders", "head", "l_thigh", "r_thigh", "l_shin", "r_shin",
    "l_upper_arm", "r_upper_arm", "l_forearm", "r_forearm",
];
pub const ALL_BONES: [Bone; N_BONES] = [
    Bone::Hips, Bone::Spine, Bone::Neck, Bone::Shoulders, Bone::Head, Bone::LThigh, Bone::RThigh,
    Bone::LShin, Bone::RShin, Bone::LUpperArm, Bone::RUpperArm, Bone::LForearm, Bone::RForearm,
];
pub fn bone(name: &str) -> Option<Bone> {
    BONE_NAMES.iter().position(|n| *n == name).map(|i| ALL_BONES[i])
}
impl Bone {
    /// (from, to) joints of the segment. `Head` is drawn as a disc at `to`.
    pub fn ends(self) -> (Joint, Joint) {
        use Joint::*;
        match self {
            Bone::Hips => (LHip, RHip),
            Bone::Spine => (Pelvis, Chest),
            Bone::Neck => (Chest, Neck),
            Bone::Shoulders => (LShoulder, RShoulder),
            Bone::Head => (Neck, Head),
            Bone::LThigh => (LHip, LKnee),
            Bone::RThigh => (RHip, RKnee),
            Bone::LShin => (LKnee, LFoot),
            Bone::RShin => (RKnee, RFoot),
            Bone::LUpperArm => (LShoulder, LElbow),
            Bone::RUpperArm => (RShoulder, RElbow),
            Bone::LForearm => (LElbow, LHand),
            Bone::RForearm => (RElbow, RHand),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Joints {
    pub p: [Vec3; N_JOINTS],
    /// Unit weapon directions for the [left, right] hand.
    pub wpn_dir: [Vec3; 2],
    /// Character forward / right (after root pitch: body-local axes).
    pub fwd: Vec3,
    pub right: Vec3,
    /// Hair secondary-motion offset (world m) applied to loose strand tips;
    /// `solve` leaves it 0, the owner of the spring state fills it in.
    pub hair_sway: Vec3,
}

impl Joints {
    #[inline]
    pub fn get(&self, j: Joint) -> Vec3 {
        self.p[j as usize]
    }
    /// Point at distance `len` along the hand's weapon direction (0 = left, 1 = right).
    pub fn weapon_point(&self, hand: usize, len: f32) -> Vec3 {
        let h = if hand == 0 { Joint::LHand } else { Joint::RHand };
        self.get(h) + self.wpn_dir[hand] * len
    }
}

fn basis(yaw: f32) -> (Vec3, Vec3) {
    // forward, right
    (v3(yaw.sin(), 0.0, yaw.cos()), v3(yaw.cos(), 0.0, -yaw.sin()))
}

/// Forward kinematics. `pos` = ground position under the pelvis, `yaw` = facing
/// (rad), `sway` = lateral pelvis offset in metres (gait hip sway).
pub fn solve(pose: &Pose, b: &BodyParams, pos: Vec3, yaw: f32, sway: f32) -> Joints {
    use Joint::*;
    let d = |c: Ch| pose.get(c).to_radians();
    let up = Vec3::UP;
    let (f, r) = basis(yaw);
    let mut pts = [Vec3::default(); N_JOINTS];
    let mut wpn = [Vec3::default(); 2];
    let mut set = |k: Joint, v: Vec3| pts[k as usize] = v;
    let pelvis = pos + up * (b.thigh + b.shin - 0.02 + pose.get(Ch::Bob)) + r * sway;
    set(Pelvis, pelvis);

    // legs: angle swings forward from straight down.
    let hang = |a: f32, f: Vec3| up * -a.cos() + f * a.sin();
    let (hw, sw) = (b.hip_w * 0.5, b.shoulder_w * 0.5);
    let legs = [(-1.0, Ch::LHip, Ch::LKnee, LHip, LKnee, LFoot), (1.0, Ch::RHip, Ch::RKnee, RHip, RKnee, RFoot)];
    for (s, hc, kc, hj, kj, fj) in legs {
        let a = d(hc);
        let hip = pelvis + r * (s * hw);
        let knee = hip + hang(a, f) * b.thigh;
        set(hj, hip);
        set(kj, knee);
        set(fj, knee + hang(a - d(kc), f) * b.shin);
    }

    let lean = d(Ch::Lean);
    let sdir = up * lean.cos() + f * lean.sin();
    let chest = pelvis + sdir * b.torso;
    let neck = chest + sdir * b.neck;
    // The head counter-tilts by half of a *moderate* lean (keeps eyes level),
    // relative to the spine, so large leans (flips) don't invert it.
    let counter = -0.5 * wrap_deg(pose.get(Ch::Lean)).clamp(-90.0, 90.0).to_radians();
    let ha = lean + counter + d(Ch::Head);
    let head = neck + (up * ha.cos() + f * ha.sin()) * b.head_r;
    set(Chest, chest);
    set(Neck, neck);
    set(Head, head);

    let tw = yaw + d(Ch::Twist);
    let (f2, r2) = basis(tw);
    let arms = [
        (0usize, -1.0, Ch::LShoulder, Ch::LElbow, Ch::LArmOut, LShoulder, LElbow, LHand, Ch::LWpnYaw, Ch::LWpnPitch),
        (1, 1.0, Ch::RShoulder, Ch::RElbow, Ch::RArmOut, RShoulder, RElbow, RHand, Ch::RWpnYaw, Ch::RWpnPitch),
    ];
    for (i, s, sh, el, out, sj, ej, hj, wy, wp) in arms {
        let (a, o) = (d(sh), d(out));
        let arm = |a: f32| hang(a, f2) * o.cos() + r2 * (s * o.sin());
        let shoulder = chest - sdir * 0.03 + r2 * (s * sw);
        let elbow = shoulder + arm(a) * b.upper_arm;
        set(sj, shoulder);
        set(ej, elbow);
        set(hj, elbow + arm(a + d(el)) * b.forearm);
        let (fs, _) = basis(tw + d(wy));
        let p = d(wp);
        wpn[i] = fs * p.cos() + up * p.sin();
    }
    let mut j = Joints { p: pts, wpn_dir: wpn, fwd: f, right: r, hair_sway: Vec3::default() };

    // Whole-body rotation (rolls, knockdowns) about the pelvis' right axis.
    let rp = pose.get(Ch::RootPitch);
    if wrap_deg(rp).abs() > 1e-3 {
        let a = rp.to_radians();
        for q in j.p.iter_mut() {
            *q = pelvis + (*q - pelvis).rotate(r, a);
        }
        for w in j.wpn_dir.iter_mut() {
            *w = w.rotate(r, a);
        }
        j.fwd = f.rotate(r, a);
        // keep the rotated body above the ground
        let low = j.p.iter().map(|q| q.y).fold(f32::INFINITY, f32::min);
        if low < 0.02 {
            for q in j.p.iter_mut() {
                q.y += 0.02 - low;
            }
        }
    } else {
        for fj in [LFoot, RFoot] {
            let q = &mut j.p[fj as usize];
            q.y = q.y.max(0.02); // cheap ground clamp (foot IK planned)
        }
    }
    j
}
