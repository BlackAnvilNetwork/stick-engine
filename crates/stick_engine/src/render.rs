// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

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
    /// First-person view of this character: draw only arms, hands and
    /// hand-held gear (the head and torso would sit on the camera).
    pub first_person: bool,
}

impl Default for DrawOpts {
    fn default() -> Self {
        DrawOpts { outline: true, first_person: false }
    }
}

const SHADE_NEAR: u8 = 4;
const SHADE_FAR: u8 = 3;

/// Draw the bare body (no gear).
pub fn draw_body(fb: &mut Framebuffer, cam: &Camera, j: &Joints, b: &BodyParams, map: &RampMap, o: &DrawOpts) {
    let p = |k: Joint| cam.project(j.get(k));
    let ol = o.outline.then(|| map.color(ColorSlot::Outline, 0));
    let near = map.color(ColorSlot::Body, SHADE_NEAR);
    let far = map.color(ColorSlot::BodyFar, SHADE_FAR);
    let seg = |fb: &mut Framebuffer, bone: Bone, c: u8, mul: f32| {
        let (a, z) = bone.ends();
        fb.capsule(p(a), p(z), b.limb_radius(bone) * mul, c, ol);
    };
    if o.first_person {
        for (bone, c) in [(Bone::LForearm, far), (Bone::RForearm, near)] {
            seg(fb, bone, c, 0.9);
        }
        return;
    }
    // far side (left) limbs slightly darker for depth readability
    for (bones, c) in [
        ([Bone::LThigh, Bone::LShin, Bone::LUpperArm, Bone::LForearm], far),
        ([Bone::RThigh, Bone::RShin, Bone::RUpperArm, Bone::RForearm], near),
    ] {
        seg(fb, bones[0], c, 1.0);
        seg(fb, bones[1], c, 1.0);
        seg(fb, bones[2], c, 0.9);
        seg(fb, bones[3], c, 0.9);
    }
    seg(fb, Bone::Hips, near, 1.0);
    let (a, z) = Bone::Spine.ends();
    fb.capsule(p(a), p(z), b.limb_radius(Bone::Spine) + 0.6, near, ol);
    seg(fb, Bone::Shoulders, near, 1.0);
    seg(fb, Bone::Neck, near, 0.8);
    // scarf accent at the neck
    fb.disc(p(Joint::Neck), b.limb_px + 0.4, map.color(ColorSlot::Accent, 3), None);
    fb.disc(p(Joint::Head), b.head_r * HEIGHT_PX * b.bone_thickness_mul[Bone::Head as usize], near, ol);
    draw_hair(fb, cam, j, b, map.color(ColorSlot::Hair, 3), near, ol);
}

/// Hair silhouette in the `Hair` slot. Each part is depth-tested at its own
/// 3D position, biased behind the face unless the figure faces away, so the
/// face wins from the front, the hair wins from behind and in profile the
/// parts show past the head outline. Loose strand tips follow `hair_sway`.
fn draw_hair(fb: &mut Framebuffer, cam: &Camera, j: &Joints, b: &BodyParams, c: u8, skin: u8, ol: Option<u8>) {
    use crate::body::HairStyle as H;
    use crate::math::Vec3;
    let hm = b.head_r * b.bone_thickness_mul[Bone::Head as usize];
    let h = j.get(Joint::Head);
    let up = (h - j.get(Joint::Neck)).norm();
    let (f, r) = (j.fwd, j.right);
    let px = |m: f32| m * HEIGHT_PX;
    let tc = cam.to_eye(h);
    let tl = (tc.x * tc.x + tc.z * tc.z).sqrt().max(1e-4);
    let away = (j.fwd.x * tc.x + j.fwd.z * tc.z) / tl < -0.3;
    let bias = if away { -0.03 } else { 0.03 };
    // head-local point: (up, fwd, right) in head radii
    let q = |u: f32, fw: f32, rt: f32| h + up * (u * hm) + f * (fw * hm) + r * (rt * hm);
    let at = |v: Vec3, extra: f32| {
        let p = cam.project(v);
        P2 { d: p.d + bias + extra, ..p }
    };
    let sway = j.hair_sway * 1.5;
    let disc = |fb: &mut Framebuffer, v: Vec3, rad: f32| fb.disc(at(v, 0.0), px(hm * rad), c, ol);
    let spike = |fb: &mut Framebuffer, a: Vec3, z: Vec3, ra: f32, rz: f32| {
        fb.cone(at(a, 0.0), at(z, 0.0), px(hm * ra), px(hm * rz), c, ol)
    };
    let strand = |fb: &mut Framebuffer, a: Vec3, z: Vec3, ra: f32, rz: f32| {
        // long strands hang beside the neck: in front of the torso's centre plane
        let e = if away { 0.0 } else { -0.03 };
        fb.cone(at(a, e), at(z, e), px(hm * ra), px(hm * rz), c, ol)
    };
    match b.hair {
        H::None => {}
        H::Buzz => disc(fb, q(0.32, -0.15, 0.0), 0.95),
        H::Spiky => {
            disc(fb, q(0.3, -0.15, 0.0), 0.97);
            for s in [-0.6f32, -0.2, 0.2, 0.6] {
                spike(fb, q(0.7, -0.1, s * 0.8), q(1.3, -0.1, s * 1.5), 0.3, 0.06);
            }
            spike(fb, q(0.5, -0.6, 0.0), q(0.8, -1.2, 0.0), 0.3, 0.06);
        }
        H::SlickedBack => {
            disc(fb, q(0.28, -0.35, 0.0), 1.0);
            disc(fb, q(-0.25, -0.75, 0.0), 0.5);
        }
        H::Mohawk => {
            for k in 0..8 {
                let a = (30.0 - k as f32 * 21.0).to_radians();
                disc(fb, h + (up * a.cos() + f * a.sin()) * (hm * 1.08), 0.27);
            }
        }
        H::Topknot => {
            disc(fb, q(0.3, -0.15, 0.0), 0.96);
            let bun = q(1.05, -0.25, 0.0);
            disc(fb, bun, 0.42);
            spike(fb, bun, bun - f * (hm * 0.45) - up * (hm * 0.7) + sway, 0.2, 0.08);
        }
        H::Bob => {
            disc(fb, q(0.12, -0.3, 0.0), 1.05);
            for s in [-1.0f32, 1.0] {
                disc(fb, q(-0.4, -0.55, s * 0.55), 0.6);
            }
        }
        H::Ponytail => {
            disc(fb, q(0.15, -0.3, 0.0), 1.0);
            let a = q(0.2, -1.0, 0.0);
            let z = a - f * (hm * 0.5) - Vec3::UP * (hm * 1.6) + sway;
            strand(fb, a, z, 0.38, 0.18);
        }
        H::LongStraight => {
            disc(fb, q(0.12, -0.25, 0.0), 1.06);
            for s in [-1.0f32, 1.0] {
                let a = q(-0.1, -0.25, s * 0.7);
                strand(fb, a, a - up * (hm * 2.3) + r * (s * hm * 0.15) - f * (hm * 0.1) + sway, 0.5, 0.4);
            }
            let a = q(0.0, -0.55, 0.0);
            strand(fb, a, a - up * (hm * 2.5) - f * (hm * 0.2) + sway, 0.75, 0.6);
        }
        H::Pigtails => {
            disc(fb, q(0.12, -0.2, 0.0), 1.03);
            for s in [-1.0f32, 1.0] {
                disc(fb, q(0.7, -0.6, s * 0.85), 0.42);
            }
        }
        H::Pixie => {
            disc(fb, q(0.15, -0.2, 0.0), 1.04);
            disc(fb, q(-0.35, -0.6, 0.0), 0.45);
            // side-swept fringe drawn over the forehead
            let fr = q(0.78, 0.35, 0.3);
            fb.disc(at(fr, if away { 0.0 } else { -0.08 }), px(hm * 0.42), c, ol);
        }
    }
    if b.hair == H::None {
        return;
    }
    // Hairline over the forehead, depth-biased in front of the face so the
    // style still reads when the figure faces the camera (the cap alone is
    // hidden behind the face disc from the front).
    let (fu, fr_r) = match b.hair {
        H::Mohawk => (0.0, 0.0),
        H::SlickedBack => (0.95, 0.22),
        H::Buzz => (0.9, 0.2),
        H::Bob | H::LongStraight | H::Pixie => (0.68, 0.34),
        _ => (0.82, 0.26),
    };
    if fr_r > 0.0 && !away {
        let fd = -0.09;
        fb.cone(at(q(fu, 0.42, -0.72), fd), at(q(fu + 0.08, 0.5, 0.0), fd), px(hm * fr_r), px(hm * fr_r), c, None);
        fb.cone(at(q(fu + 0.08, 0.5, 0.0), fd), at(q(fu, 0.42, 0.72), fd), px(hm * fr_r), px(hm * fr_r), c, None);
    }
    // Form-fitted base hair: recolour only head-skin pixels over the scalp
    // (crown, top, temples above the ears, back of the head). It never adds
    // pixels, so the head outline and every style's shapes stay exactly as
    // drawn; only bare scalp skin turns hair-coloured. Samples on the near
    // side of the head sphere (in front of the flat head disc) recolour;
    // far-side samples would sit over the face and are skipped by depth.
    if !matches!(b.hair, H::None | H::Mohawk) {
        let hc = cam.project(h);
        let hr = px(hm);
        for ei in 0..10 {
            let el = (-25.0 + ei as f32 * 12.0).to_radians(); // -25..83 deg
            for ai in 0..24 {
                let az = ai as f32 * std::f32::consts::TAU / 24.0;
                let (u, fw, rt) = (el.sin(), el.cos() * az.cos(), el.cos() * az.sin());
                let min_u = if fw > 0.55 { 0.55 } else if fw > 0.0 { 0.25 } else if fw > -0.5 { 0.05 } else { -0.4 };
                if u < min_u {
                    continue;
                }
                let p = cam.project(q(u, fw, rt));
                if p.d > hc.d {
                    continue;
                }
                fb.recolor_disc(p, px(hm * 0.22), hc, hr, skin, c);
            }
        }
        let _ = ol;
    }
}


/// Draw a full character: body, then every equipped item.
pub fn draw_character(
    fb: &mut Framebuffer, cam: &Camera, j: &Joints, def: &CharacterDef, gear: &GearLib, map: &RampMap, o: &DrawOpts,
) {
    let b = def.effective_body(gear);
    draw_body(fb, cam, j, &b, map, o);
    let ol = o.outline.then(|| map.color(ColorSlot::Outline, 0));
    for s in ALL_GEAR_SLOTS {
        if o.first_person && !matches!(s, crate::gear::GearSlot::MainHand | crate::gear::GearSlot::OffHand) {
            continue;
        }
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
