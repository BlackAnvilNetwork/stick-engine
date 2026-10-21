//! Character drawing: projects FK joints through a camera and rasterises
//! limbs, head and gear shapes, resolving colour slots via a `RampMap`.
use crate::body::{BodyParams, CharacterDef};
use crate::camera::{Camera, HEIGHT_PX};
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
    let p = |k: Joint| cam.project(j.get(k));
    let ol = o.outline.then(|| map.color(ColorSlot::Outline, 0));
    let near = map.color(ColorSlot::Body, SHADE_NEAR);
    let far = map.color(ColorSlot::BodyFar, SHADE_FAR);
    let seg = |fb: &mut Framebuffer, bone: Bone, c: u8, mul: f32| {
        let (a, z) = bone.ends();
        fb.capsule(p(a), p(z), b.limb_radius(bone) * mul, c, ol);
    };
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
}

/// Draw a full character: body, then every equipped item.
pub fn draw_character(
    fb: &mut Framebuffer, cam: &Camera, j: &Joints, def: &CharacterDef, gear: &GearLib, map: &RampMap, o: &DrawOpts,
) {
    let b = def.effective_body(gear);
    draw_body(fb, cam, j, &b, map, o);
    let ol = o.outline.then(|| map.color(ColorSlot::Outline, 0));
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
