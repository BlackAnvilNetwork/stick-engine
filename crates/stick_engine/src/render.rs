//! Character drawing: projects FK joints through a camera and rasterises
//! limbs, head and gear shapes, resolving colour slots via a `RampMap`.
use crate::body::{BodyParams, CharacterDef};
use crate::camera::{Camera, HEIGHT_PX, P2};
use crate::color::{ColorSlot, RampMap};
use crate::gear::{local, GearLib, Shape, ALL_GEAR_SLOTS};
use crate::raster::Framebuffer;
use crate::rig::{Bone, Joint, Joints};

#[derive(Clone, Copy, Debug)]
pub struct DrawOpts {
    /// 1 px outline ring around every limb/gear primitive (on by default).
    pub outline: bool,
}

impl Default for DrawOpts {
    fn default() -> Self {
        DrawOpts { outline: true }
    }
}

const SHADE_NEAR: u8 = 4;
const SHADE_FAR: u8 = 3;

/// Draw the bare body (no gear).
pub fn draw_body(fb: &mut Framebuffer, cam: &Camera, j: &Joints, b: &BodyParams, map: &RampMap, o: &DrawOpts) {
    let near = map.color(ColorSlot::Body, SHADE_NEAR);
    draw_body_with(fb, cam, j, b, map, o, near);
}

/// Approximate px per metre across the body (iso horizontal ~32-45).
const PX_PER_M: f32 = 40.0;
/// Torso depth bias so limbs on the torso plane win depth ties.
const TORSO_BIAS: f32 = 0.04;

/// Screen-space torso silhouette: `(centre, half_width)` stations from the
/// hips up to the trapezius, plus the strip's unit screen normal. Each
/// station is an ellipse cross-section (width along the body's right axis,
/// depth along its forward axis) projected onto the normal, so the shape
/// narrows to the torso depth in profile. The bust station is pushed
/// forward by `chest` (the side-view curve) and follows `chest_bounce`.
pub fn torso_stations(cam: &Camera, j: &Joints, b: &BodyParams) -> (Vec<(P2, f32)>, (f32, f32)) {
    let pel = j.get(Joint::Pelvis);
    let (ls, rs) = (j.get(Joint::LShoulder), j.get(Joint::RShoulder));
    let top = (ls + rs) * 0.5;
    let axis = top - pel;
    let alen = axis.len().max(1e-4);
    let a_n = axis * (1.0 / alen);
    let sdir = (rs - ls).norm();
    let pp = cam.project(pel);
    let pt = cam.project(top);
    let (sx, sy) = (pt.x - pp.x, pt.y - pp.y);
    let sl = (sx * sx + sy * sy).sqrt().max(1e-3);
    let n = (-sy / sl, sx / sl);
    let spine_mul = 1.0 + (b.bone_thickness_mul[Bone::Spine as usize] - 1.0) * 0.35;
    let arm_r = b.limb_radius(Bone::RUpperArm) * b.arm_thick / PX_PER_M;
    let thigh_r = b.limb_radius(Bone::RThigh) * b.thigh_w / PX_PER_M;
    let sw = b.shoulder_w * 0.5;
    // (t along pelvis->shoulders, half width m, half depth m, forward shift m, bounce)
    let bust_w = (b.waist_w * 0.5 + (sw - b.waist_w * 0.5) * 0.7).max(b.waist_w * 0.5 + b.chest * 1.3);
    let hd = b.torso_depth * 0.5;
    let st = [
        (-0.10f32, b.hip_w * 0.5 + thigh_r * 0.5, hd * 0.95, -b.chest * 0.3, false),
        (0.36, b.waist_w * 0.5, hd * 0.8, 0.0, false),
        (0.74, bust_w, hd * 0.9 + b.chest * 0.5, b.chest * 0.5, true),
        (0.97, sw + arm_r * 0.8, hd * 0.8, 0.0, false),
        (1.12, sw * 0.5, hd * 0.5, 0.0, false),
    ];
    let mut out = Vec::with_capacity(st.len());
    for (t, w, d, fs, bounce) in st {
        let r = (j.right + (sdir - j.right) * t.clamp(0.0, 1.0)).norm();
        let f = r.cross(a_n).norm();
        let mut c = pel + axis * t + f * fs;
        if bounce {
            c = c + crate::math::Vec3::UP * j.chest_bounce;
        }
        let pc = cam.project(c);
        let scr = |v: crate::math::Vec3| {
            let q = cam.project(c + v);
            (q.x - pc.x) * n.0 + (q.y - pc.y) * n.1
        };
        let (rw, fd) = (scr(r * (w * spine_mul)), scr(f * (d * spine_mul)));
        out.push((P2 { d: pc.d + TORSO_BIAS, ..pc }, (rw * rw + fd * fd).sqrt()));
    }
    (out, n)
}

fn draw_body_with(
    fb: &mut Framebuffer, cam: &Camera, j: &Joints, b: &BodyParams, map: &RampMap, o: &DrawOpts, torso_col: u8,
) {
    let p = |k: Joint| cam.project(j.get(k));
    let ol = o.outline.then(|| map.color(ColorSlot::Outline, 0));
    let near = map.color(ColorSlot::Body, SHADE_NEAR);
    let far = map.color(ColorSlot::BodyFar, SHADE_FAR);
    let r = |bone: Bone| b.limb_radius(bone);
    // far side (left) limbs slightly darker for depth readability
    for (bones, c) in [
        ([Bone::LThigh, Bone::LShin, Bone::LUpperArm, Bone::LForearm], far),
        ([Bone::RThigh, Bone::RShin, Bone::RUpperArm, Bone::RForearm], near),
    ] {
        let (h, k) = bones[0].ends();
        let (_, f) = bones[1].ends();
        // thigh: full at the hip, tapering into the knee
        fb.cone(p(h), p(k), r(bones[0]) * b.thigh_w, r(bones[0]) * 0.95, c, ol);
        // calf: slight swell below the knee, slim ankle
        fb.cone(p(k), p(f), r(bones[1]) * 0.95 * b.calf_thick, r(bones[1]) * 0.7 * b.calf_thick, c, ol);
        let (s, e) = bones[2].ends();
        let (_, hn) = bones[3].ends();
        let ar = 0.9 * b.arm_thick;
        fb.cone(p(s), p(e), r(bones[2]) * ar * 1.1, r(bones[2]) * ar * 0.85, c, ol);
        fb.cone(p(e), p(hn), r(bones[3]) * ar * 0.85, r(bones[3]) * ar * 0.7, c, ol);
    }
    let (st, n) = torso_stations(cam, j, b);
    fb.ribbon(&st, n, torso_col, ol);
    let (a, z) = Bone::Neck.ends();
    fb.capsule(p(a), p(z), r(Bone::Neck) * 0.8, near, ol);
    // scarf accent at the neck
    fb.disc(p(Joint::Neck), b.limb_px + 0.4, map.color(ColorSlot::Accent, 3), None);
    let hr = b.head_r * HEIGHT_PX * b.bone_thickness_mul[Bone::Head as usize];
    fb.disc(p(Joint::Head), hr, near, ol);
    draw_hair(fb, cam, j, b, map.color(ColorSlot::Body, SHADE_NEAR - 2), ol);
}

/// Hair silhouette. Placed slightly behind the head centre so depth testing
/// puts the face in front when facing the camera and the hair in front from
/// behind; in profile it shows as a bulge at the back of the head.
fn draw_hair(fb: &mut Framebuffer, cam: &Camera, j: &Joints, b: &BodyParams, c: u8, ol: Option<u8>) {
    use crate::math::Vec3;
    let hm = b.head_r * b.bone_thickness_mul[Bone::Head as usize];
    let h = j.get(Joint::Head);
    let up = (h - j.get(Joint::Neck)).norm();
    let f = j.fwd;
    let px = |m: f32| m * HEIGHT_PX;
    // hair sits behind the face unless the figure faces away from the camera
    let away = j.fwd.x * TO_CAM.0 + j.fwd.z * TO_CAM.1 < -0.3;
    let hd = cam.project(h).d + if away { -0.03 } else { 0.03 };
    let at = |v: Vec3| P2 { d: hd, ..cam.project(v) };
    match b.hair {
        crate::body::HairStyle::None => {}
        crate::body::HairStyle::Bob => {
            // crown cap + a fuller lower shell down to the jaw
            fb.disc(at(h - f * (hm * 0.3) + up * (hm * 0.12)), px(hm * 1.05), c, ol);
            for s in [-1.0f32, 1.0] {
                let q = h - f * (hm * 0.35) - up * (hm * 0.45) + j.right * (s * hm * 0.55);
                fb.disc(at(q), px(hm * 0.6), c, ol);
            }
        }
        crate::body::HairStyle::Ponytail => {
            fb.disc(at(h - f * (hm * 0.3) + up * (hm * 0.15)), px(hm * 1.0), c, ol);
            let a = h - f * (hm * 1.0) + up * (hm * 0.2);
            let z = a - f * (hm * 0.5) - Vec3::UP * (hm * 1.6);
            fb.cone(at(a), at(z), px(hm * 0.38), px(hm * 0.18), c, ol);
        }
    }
}

/// Toward-camera direction on the ground plane (depth decreases along it).
const TO_CAM: (f32, f32) = (std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);

/// Chest silhouette cue: two small discs on the upper torso, offset toward
/// the facing direction. Depth-tested against the body; skipped when the
/// figure faces away so they can never show through the back. Chest armour
/// (if any) recolours them as part of the plate.
pub fn chest_points(j: &Joints, b: &BodyParams) -> Option<[crate::math::Vec3; 2]> {
    if b.chest <= 0.0 {
        return None;
    }
    let facing = j.fwd.x * TO_CAM.0 + j.fwd.z * TO_CAM.1;
    if facing < -0.3 {
        return None;
    }
    let (pel, ch) = (j.get(Joint::Pelvis), j.get(Joint::Chest));
    let base = pel + (ch - pel) * 0.74;
    let spine_r = b.limb_radius(Bone::Spine) / HEIGHT_PX;
    let lat = (b.shoulder_w * 0.16).max(b.chest * 0.9);
    let c = base + j.fwd * (spine_r * 0.9 + b.chest * 0.35) + crate::math::Vec3::UP * j.chest_bounce;
    Some([c - j.right * lat, c + j.right * lat])
}

fn draw_chest(fb: &mut Framebuffer, cam: &Camera, j: &Joints, b: &BodyParams, (lit, shade): (u8, u8), ol: Option<u8>) {
    let Some(pts) = chest_points(j, b) else { return };
    let r = b.chest * HEIGHT_PX * b.thickness.sqrt();
    for p in pts {
        let q = cam.project(p);
        // one-shade-darker underside, then the lit disc nudged up: a soft
        // contour that reads even when drawn over the same-coloured torso
        fb.disc(q, r, shade, ol);
        fb.disc(crate::camera::P2 { y: q.y - 0.8, d: q.d - 0.01, ..q }, r - 0.5, lit, None);
    }
}

/// Draw a full character: body, then every equipped item.
pub fn draw_character(
    fb: &mut Framebuffer, cam: &Camera, j: &Joints, def: &CharacterDef, gear: &GearLib, map: &RampMap, o: &DrawOpts,
) {
    let b = def.effective_body(gear);
    let ol = o.outline.then(|| map.color(ColorSlot::Outline, 0));
    // chest armour recolours the torso silhouette and the chest cue as one plate
    let cols = match def.equipped(crate::gear::GearSlot::Chest, gear) {
        Some(g) => match g.material {
            Some(r) => { let m = map.with(ColorSlot::Material, r); (m.color(ColorSlot::Material, 3), m.color(ColorSlot::Material, 2)) }
            None => (map.color(ColorSlot::Body, SHADE_NEAR), map.color(ColorSlot::Body, SHADE_NEAR - 1)),
        },
        None => (map.color(ColorSlot::Body, SHADE_NEAR), map.color(ColorSlot::Body, SHADE_NEAR - 1)),
    };
    draw_body_with(fb, cam, j, &b, map, o, cols.0);
    draw_chest(fb, cam, j, &b, cols, ol);
    for s in ALL_GEAR_SLOTS {
        let Some(g) = def.equipped(s, gear) else { continue };
        let m = match g.material {
            Some(r) => map.with(ColorSlot::Material, r),
            None => *map,
        };
        let fr = g.frame(j);
        for sh in &g.shapes {
            match sh {
                Shape::Line { from, to, ramp, .. } => {
                    fb.line(cam.project(local(&fr, *from)), cam.project(local(&fr, *to)), m.color(*ramp, 4));
                }
                Shape::Capsule { from, to, ramp, thick } => {
                    let (a, z) = (cam.project(local(&fr, *from)), cam.project(local(&fr, *to)));
                    fb.capsule(a, z, *thick, m.color(*ramp, 3), ol);
                    if *ramp == ColorSlot::Material {
                        fb.line(a, z, m.color(*ramp, 5)); // highlight edge
                    }
                }
                Shape::Disc { at, center, r, ramp } => {
                    let c = match at.as_deref().and_then(crate::rig::joint) {
                        Some(jt) => j.get(jt),
                        None => local(&fr, *center),
                    };
                    fb.disc(cam.project(c), r * HEIGHT_PX, m.color(*ramp, 3), ol);
                }
            }
        }
    }
    // hands drawn over grips
    let near = map.color(ColorSlot::Body, SHADE_NEAR);
    for h in [Joint::LHand, Joint::RHand] {
        fb.disc(cam.project(j.get(h)), b.limb_px * 0.9, near, None);
    }
}
