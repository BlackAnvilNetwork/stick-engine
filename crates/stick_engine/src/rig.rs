//! Stick-figure rig: procedural forward kinematics from a `Pose` into 3D
//! joints, then drawing those joints through a `Camera`.
use crate::anim::Pose;
use crate::camera::Camera;
use crate::math::{v3, Vec3};
use crate::raster::*;

/// Body proportions in metres (total height ~1.8 m => ~80 px on screen).
#[derive(Clone, Copy, Debug)]
pub struct Proportions {
    pub thigh: f32,
    pub shin: f32,
    pub spine: f32,
    pub neck: f32,
    pub head_r: f32,
    pub upper_arm: f32,
    pub forearm: f32,
    pub shoulder_w: f32,
    pub hip_w: f32,
    pub sword: f32,
    pub limb_px: f32,
}

impl Default for Proportions {
    fn default() -> Self {
        Proportions {
            thigh: 0.46, shin: 0.46, spine: 0.52, neck: 0.08, head_r: 0.14,
            upper_arm: 0.3, forearm: 0.28, shoulder_w: 0.19, hip_w: 0.1,
            sword: 0.9, limb_px: 2.2,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Joints {
    pub pelvis: Vec3,
    pub chest: Vec3,
    pub neck: Vec3,
    pub head: Vec3,
    pub hip: [Vec3; 2],
    pub knee: [Vec3; 2],
    pub foot: [Vec3; 2],
    pub shoulder: [Vec3; 2],
    pub elbow: [Vec3; 2],
    pub hand: [Vec3; 2],
    pub sword_tip: Vec3,
    pub sword_dir: Vec3,
}

fn basis(yaw: f32) -> (Vec3, Vec3) {
    // forward, right
    (v3(yaw.sin(), 0.0, yaw.cos()), v3(yaw.cos(), 0.0, -yaw.sin()))
}

/// Forward kinematics. `pos` = ground position under the pelvis, `yaw` = facing (rad).
pub fn solve(pose: &Pose, pr: &Proportions, pos: Vec3, yaw: f32) -> Joints {
    let d = |n: &str| pose.get(n).to_radians();
    let up = Vec3::UP;
    let (f, r) = basis(yaw);
    let mut j = Joints::default();
    j.pelvis = pos + up * (pr.thigh + pr.shin - 0.02 + pose.get("bob"));

    // legs: 0 = left, 1 = right. Angle a swings forward from straight down.
    let hang = |a: f32, f: Vec3| up * -a.cos() + f * a.sin();
    for (i, s, hip_n, knee_n) in [(0, -1.0, "l_hip", "l_knee"), (1, 1.0, "r_hip", "r_knee")] {
        let a = d(hip_n);
        j.hip[i] = j.pelvis + r * (s * pr.hip_w);
        j.knee[i] = j.hip[i] + hang(a, f) * pr.thigh;
        let mut foot = j.knee[i] + hang(a - d(knee_n), f) * pr.shin;
        foot.y = foot.y.max(0.02); // cheap ground clamp (real IK planned)
        j.foot[i] = foot;
    }

    let lean = d("lean");
    let sdir = up * lean.cos() + f * lean.sin();
    j.chest = j.pelvis + sdir * pr.spine;
    j.neck = j.chest + sdir * pr.neck;
    let head_dir = up * (lean * 0.5 + d("head")).cos() + f * (lean * 0.5 + d("head")).sin();
    j.head = j.neck + head_dir * pr.head_r;

    let tw = yaw + d("twist");
    let (f2, r2) = basis(tw);
    for (i, s, sh, el, out) in [(0, -1.0, "l_shoulder", "l_elbow", "l_arm_out"), (1, 1.0, "r_shoulder", "r_elbow", "r_arm_out")] {
        let (a, o) = (d(sh), d(out));
        let arm = |a: f32| hang(a, f2) * o.cos() + r2 * (s * o.sin());
        j.shoulder[i] = j.chest - sdir * 0.03 + r2 * (s * pr.shoulder_w);
        j.elbow[i] = j.shoulder[i] + arm(a) * pr.upper_arm;
        j.hand[i] = j.elbow[i] + arm(a + d(el)) * pr.forearm;
    }
    let (fs, _) = basis(tw + d("sword_yaw"));
    let p = d("sword_pitch");
    j.sword_dir = fs * p.cos() + up * p.sin();
    j.sword_tip = j.hand[1] + j.sword_dir * pr.sword;
    j
}

/// Colours for one character.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub body: u8,
    pub far: u8,
    pub accent: u8,
}

impl Default for Look {
    fn default() -> Self {
        Look { body: C_BODY, far: C_BODY_FAR, accent: C_ACCENT }
    }
}

pub fn draw(fb: &mut Framebuffer, cam: &Camera, j: &Joints, pr: &Proportions, look: &Look) {
    let p = |v: Vec3| cam.project(v);
    let o = Some(C_OUTLINE);
    let t = pr.limb_px;
    // far side (left) limbs slightly darker for depth readability
    for (i, c) in [(0usize, look.far), (1, look.body)] {
        fb.capsule(p(j.hip[i]), p(j.knee[i]), t, c, o);
        fb.capsule(p(j.knee[i]), p(j.foot[i]), t, c, o);
        fb.capsule(p(j.shoulder[i]), p(j.elbow[i]), t * 0.9, c, o);
        fb.capsule(p(j.elbow[i]), p(j.hand[i]), t * 0.9, c, o);
    }
    fb.capsule(p(j.hip[0]), p(j.hip[1]), t, look.body, o);
    fb.capsule(p(j.pelvis), p(j.chest), t + 0.6, look.body, o);
    fb.capsule(p(j.shoulder[0]), p(j.shoulder[1]), t, look.body, o);
    fb.capsule(p(j.chest), p(j.neck), t * 0.8, look.body, o);
    // scarf accent at the neck
    fb.disc(p(j.neck), t + 0.4, look.accent, None);
    let head_px = pr.head_r * crate::camera::HEIGHT_PX;
    fb.disc(p(j.head), head_px, look.body, o);
    // sword: hilt, crossguard, blade
    let hand = j.hand[1];
    let pommel = hand - j.sword_dir * 0.1;
    let guard_c = hand + j.sword_dir * 0.06;
    let side = v3(j.sword_dir.z, 0.0, -j.sword_dir.x);
    let side = if side.len() > 1e-3 { side * (0.11 / side.len()) } else { v3(0.11, 0.0, 0.0) };
    fb.capsule(p(pommel), p(guard_c), 1.2, C_HILT, None);
    fb.capsule(p(guard_c - side), p(guard_c + side), 1.0, C_GOLD, None);
    fb.capsule(p(guard_c), p(j.sword_tip), 1.0, C_STEEL, o);
    fb.line(p(guard_c), p(j.sword_tip), C_TRAIL);
    fb.disc(p(j.hand[1]), t * 0.9, look.body, None);
}
