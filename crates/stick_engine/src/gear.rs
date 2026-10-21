// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Gear: shape primitives attached to a joint, in joint-local metres.
//! Looks only (plus animation timing: `reach`, `swing_weight`); stats are a
//! game concern.
use crate::color::ColorSlot;
use crate::rig::{bone, joint, Joint, Joints, N_BONES};
use crate::math::Vec3;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GearSlot {
    Head, Shoulders, Chest, Hands, Legs, Feet, Back, MainHand, OffHand,
}
pub const N_GEAR_SLOTS: usize = 9;
pub const ALL_GEAR_SLOTS: [GearSlot; N_GEAR_SLOTS] = [
    GearSlot::Head, GearSlot::Shoulders, GearSlot::Chest, GearSlot::Hands, GearSlot::Legs,
    GearSlot::Feet, GearSlot::Back, GearSlot::MainHand, GearSlot::OffHand,
];

type P3 = (f32, f32, f32);

#[derive(Deserialize, Clone, Debug)]
pub enum Shape {
    Line { from: P3, to: P3, ramp: ColorSlot, #[serde(default = "one")] thick: f32 },
    Capsule { from: P3, to: P3, ramp: ColorSlot, #[serde(default = "one")] thick: f32 },
    /// Disc of radius `r` metres, at `center` in the attach frame, or at joint `at`.
    Disc { #[serde(default)] at: Option<String>, #[serde(default)] center: P3, r: f32, ramp: ColorSlot },
}
fn one() -> f32 {
    1.0
}

#[derive(Deserialize, Clone, Debug)]
struct GearRon {
    id: String,
    slot: GearSlot,
    #[serde(default)]
    stance: Option<String>,
    #[serde(default)]
    attach: Option<String>,
    #[serde(default)]
    reach: f32,
    #[serde(default = "one")]
    swing_weight: f32,
    #[serde(default)]
    material: Option<String>,
    #[serde(default)]
    thickness_mul: HashMap<String, f32>,
    #[serde(default)]
    shapes: Vec<Shape>,
}

#[derive(Clone, Debug)]
pub struct GearDef {
    pub id: String,
    pub slot: GearSlot,
    /// Main hand: "1h", "2h", "staff", ...; off hand: "shield" or a weapon stance.
    pub stance: Option<String>,
    pub attach: Joint,
    /// Length of the striking part along the weapon axis (m).
    pub reach: f32,
    /// Multiplies action clip duration (heavier = slower).
    pub swing_weight: f32,
    /// Ramp for the `Material` slot when this item is drawn.
    pub material: Option<u8>,
    pub thickness_mul: [f32; N_BONES],
    pub shapes: Vec<Shape>,
}

impl GearDef {
    pub fn from_ron(src: &str) -> Result<GearDef, String> {
        let g: GearRon = ron::from_str(src).map_err(|e| e.to_string())?;
        let default_attach = match g.slot {
            GearSlot::MainHand => "r_hand",
            GearSlot::OffHand => "l_hand",
            GearSlot::Head => "head",
            GearSlot::Legs | GearSlot::Feet => "pelvis",
            _ => "chest",
        };
        let an = g.attach.as_deref().unwrap_or(default_attach);
        let attach = joint(an).ok_or_else(|| format!("gear '{}': unknown joint '{an}'", g.id))?;
        let mut tm = [1.0; N_BONES];
        for (k, v) in &g.thickness_mul {
            let b = bone(k).ok_or_else(|| format!("gear '{}': unknown bone '{k}'", g.id))?;
            tm[b as usize] = *v;
        }
        for s in &g.shapes {
            if let Shape::Disc { at: Some(j), .. } = s {
                joint(j).ok_or_else(|| format!("gear '{}': unknown joint '{j}'", g.id))?;
            }
        }
        let material = match &g.material {
            Some(m) => Some(crate::raster::ramp_id(m).ok_or_else(|| format!("gear '{}': unknown ramp '{m}'", g.id))?),
            None => None,
        };
        Ok(GearDef {
            id: g.id, slot: g.slot, stance: g.stance, attach, reach: g.reach, swing_weight: g.swing_weight,
            material, thickness_mul: tm, shapes: g.shapes,
        })
    }

    /// Joint-local frame (origin, x, y, z) for this item's attach point.
    /// Hands use the weapon direction as +y.
    pub fn frame(&self, j: &Joints) -> (Vec3, Vec3, Vec3, Vec3) {
        let o = j.get(self.attach);
        let y = match self.attach {
            Joint::RHand => j.wpn_dir[1],
            Joint::LHand => j.wpn_dir[0],
            _ => (j.get(Joint::Chest) - j.get(Joint::Pelvis)).norm(),
        };
        let mut x = y.cross(Vec3::UP).norm();
        if x.len() < 0.5 {
            x = j.right;
        }
        let z = x.cross(y).norm();
        (o, x, y, z)
    }
}

/// Map a joint-local point into world space.
pub fn local(frame: &(Vec3, Vec3, Vec3, Vec3), p: P3) -> Vec3 {
    let (o, x, y, z) = *frame;
    o + x * p.0 + y * p.1 + z * p.2
}

/// Stance from equipped hands: no weapon = "unarmed"; 1h + shield =
/// "1h_shield"; 1h + 1h = "dual"; otherwise the main-hand stance.
pub fn derive_stance(main: Option<&GearDef>, off: Option<&GearDef>) -> String {
    let m = main.and_then(|g| g.stance.as_deref());
    let o = off.and_then(|g| g.stance.as_deref());
    match (m, o) {
        (None, _) => "unarmed".into(),
        (Some("1h"), Some("shield")) => "1h_shield".into(),
        (Some("1h"), Some("1h")) => "dual".into(),
        (Some(s), _) => s.into(),
    }
}

/// Built-in example gear (demo + tests). Games ship their own gear files.
pub const BUILTIN_GEAR: &[&str] = &[
    include_str!("../assets/gear/sword.ron"),
    include_str!("../assets/gear/greatsword.ron"),
    include_str!("../assets/gear/shield.ron"),
    include_str!("../assets/gear/cuirass.ron"),
];

#[derive(Default, Clone, Debug)]
pub struct GearLib {
    pub items: Vec<GearDef>,
}

impl GearLib {
    pub fn builtin() -> GearLib {
        let mut l = GearLib::default();
        for s in BUILTIN_GEAR {
            l.register_ron(s).expect("builtin gear failed to parse");
        }
        l
    }
    pub fn register_ron(&mut self, src: &str) -> Result<u16, String> {
        let g = GearDef::from_ron(src)?;
        if let Some(i) = self.index(&g.id) {
            self.items[i as usize] = g;
            return Ok(i);
        }
        self.items.push(g);
        Ok((self.items.len() - 1) as u16)
    }
    pub fn index(&self, id: &str) -> Option<u16> {
        self.items.iter().position(|g| g.id == id).map(|i| i as u16)
    }
    pub fn get(&self, i: u16) -> Option<&GearDef> {
        self.items.get(i as usize)
    }
}

