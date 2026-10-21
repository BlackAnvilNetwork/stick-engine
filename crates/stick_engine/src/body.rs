// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Character definition: body proportions, sex presets, personality, colour
//! slots and equipped gear, plus deterministic generation from
//! `(seed, archetype)`. Pure data: no rendering.
use crate::color::RampMap;
use crate::gear::{derive_stance, GearLib, GearSlot, ALL_GEAR_SLOTS, N_GEAR_SLOTS};
use crate::math::Pcg32;
use crate::raster::ramp_id;
use crate::rig::N_BONES;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sex {
    Male,
    Female,
}

/// Motion cues that make sexes read differently at 80 px (proportions alone
/// differ by only ~3 px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SexGait {
    pub twist_mul: f32,
    pub arm_mul: f32,
    /// Lateral pelvis sway amplitude (m).
    pub hip_sway: f32,
    /// Idle stance width multiplier.
    pub stance_w: f32,
}

impl Sex {
    pub fn gait(self) -> SexGait {
        match self {
            Sex::Male => SexGait { twist_mul: 0.8, arm_mul: 1.0, hip_sway: 0.010, stance_w: 1.10 },
            Sex::Female => SexGait { twist_mul: 1.2, arm_mul: 0.9, hip_sway: 0.022, stance_w: 0.90 },
        }
    }
}

/// Cosmetic hair silhouette drawn on the head in the `Hair` colour slot.
/// `None` = bald. Every style works on every body.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HairStyle {
    #[default]
    None,
    Buzz,
    Spiky,
    SlickedBack,
    Mohawk,
    Topknot,
    Bob,
    Ponytail,
    LongStraight,
    Pigtails,
    Pixie,
}

/// Every style in menu order (bald first).
pub const ALL_HAIR: [HairStyle; 11] = [
    HairStyle::None, HairStyle::Buzz, HairStyle::Spiky, HairStyle::SlickedBack, HairStyle::Mohawk,
    HairStyle::Topknot, HairStyle::Bob, HairStyle::Ponytail, HairStyle::LongStraight, HairStyle::Pigtails,
    HairStyle::Pixie,
];

impl HairStyle {
    /// Styles with loose strands that get spring sway.
    pub fn sways(self) -> bool {
        matches!(self, HairStyle::Ponytail | HairStyle::LongStraight | HairStyle::Topknot)
    }
    pub fn name(self) -> &'static str {
        match self {
            HairStyle::None => "BALD",
            HairStyle::Buzz => "BUZZ",
            HairStyle::Spiky => "SPIKY",
            HairStyle::SlickedBack => "SLICKED BACK",
            HairStyle::Mohawk => "MOHAWK",
            HairStyle::Topknot => "TOPKNOT",
            HairStyle::Bob => "BOB",
            HairStyle::Ponytail => "PONYTAIL",
            HairStyle::LongStraight => "LONG STRAIGHT",
            HairStyle::Pigtails => "TWIN BUNS",
            HairStyle::Pixie => "PIXIE",
        }
    }
}

/// Body proportions in metres. Widths are FULL widths (the rig uses half).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyParams {
    pub height: f32,
    pub thigh: f32,
    pub shin: f32,
    pub upper_arm: f32,
    pub forearm: f32,
    pub torso: f32,
    pub neck: f32,
    pub head_r: f32,
    pub shoulder_w: f32,
    pub hip_w: f32,
    /// Base limb radius in pixels at 80 px figure height.
    pub limb_px: f32,
    /// Global limb thickness multiplier.
    pub thickness: f32,
    /// Per-bone thickness multipliers (armour writes here).
    pub bone_thickness_mul: [f32; N_BONES],
    /// Cosmetic hair style (does not affect the rig).
    pub hair: HairStyle,
}

impl BodyParams {
    /// Reference proportions for a sex. Starting values, tuned to the
    /// prototype's look (stylised: big head, narrow hips).
    pub fn preset(sex: Sex) -> BodyParams {
        let m = BodyParams {
            height: 0.0, thigh: 0.46, shin: 0.46, upper_arm: 0.30, forearm: 0.28, torso: 0.52,
            neck: 0.08, head_r: 0.13, shoulder_w: 0.40, hip_w: 0.22, limb_px: 2.2, thickness: 1.0,
            bone_thickness_mul: [1.0; N_BONES], hair: HairStyle::None,
        };
        let b = match sex {
            Sex::Male => m,
            Sex::Female => BodyParams { head_r: 0.125, shoulder_w: 0.34, hip_w: 0.26, thickness: 0.92, ..m }
                .scaled(1.68 / 1.80),
        };
        BodyParams { height: b.computed_height(), ..b }
    }
    /// Standing height implied by the segment lengths.
    pub fn computed_height(&self) -> f32 {
        self.thigh + self.shin - 0.02 + self.torso + self.neck + 2.0 * self.head_r
    }
    /// Uniformly scale all lengths (not thickness or head size ratio).
    pub fn scaled(&self, k: f32) -> BodyParams {
        let mut b = *self;
        for v in [&mut b.thigh, &mut b.shin, &mut b.upper_arm, &mut b.forearm, &mut b.torso, &mut b.neck,
            &mut b.head_r, &mut b.shoulder_w, &mut b.hip_w] {
            *v *= k;
        }
        b.height = b.computed_height();
        b
    }
    /// Scale so the standing height equals `h`.
    pub fn with_height(&self, h: f32) -> BodyParams {
        self.scaled(h / self.computed_height())
    }
    /// Limb radius in px for a bone.
    pub fn limb_radius(&self, b: crate::rig::Bone) -> f32 {
        self.limb_px * self.thickness * self.bone_thickness_mul[b as usize]
    }
}

impl Default for BodyParams {
    fn default() -> Self {
        BodyParams::preset(Sex::Male)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Personality {
    /// Extra forward lean in degrees (negative = upright/proud).
    pub slouch: f32,
    /// Cosmetic cadence multiplier.
    pub gait_speed: f32,
    pub arm_swing: f32,
}

impl Default for Personality {
    fn default() -> Self {
        Personality { slouch: 0.0, gait_speed: 1.0, arm_swing: 1.0 }
    }
}

/// An equipped item: gear base index + per-item look seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GearRef {
    pub base: u16,
    pub look_seed: u32,
}

/// Everything needed to draw and animate one character.
#[derive(Clone, Debug, PartialEq)]
pub struct CharacterDef {
    pub seed: u64,
    pub sex: Sex,
    pub body: BodyParams,
    pub colors: RampMap,
    pub personality: Personality,
    pub gear: [Option<GearRef>; N_GEAR_SLOTS],
}

impl CharacterDef {
    pub fn new(sex: Sex) -> Self {
        CharacterDef {
            seed: 0, sex, body: BodyParams::preset(sex), colors: RampMap::default(),
            personality: Personality::default(), gear: [None; N_GEAR_SLOTS],
        }
    }
    pub fn equip(&mut self, slot: GearSlot, gear: Option<GearRef>) {
        self.gear[slot as usize] = gear;
    }
    pub fn equipped<'a>(&self, slot: GearSlot, lib: &'a GearLib) -> Option<&'a crate::gear::GearDef> {
        self.gear[slot as usize].and_then(|g| lib.get(g.base))
    }
    /// Body with armour thickness multipliers applied.
    pub fn effective_body(&self, lib: &GearLib) -> BodyParams {
        let mut b = self.body;
        for s in ALL_GEAR_SLOTS {
            if let Some(g) = self.equipped(s, lib) {
                for (m, k) in b.bone_thickness_mul.iter_mut().zip(g.thickness_mul) {
                    *m *= k;
                }
            }
        }
        b
    }
    /// Stance derived from the equipped hands.
    pub fn stance(&self, lib: &GearLib) -> String {
        derive_stance(self.equipped(GearSlot::MainHand, lib), self.equipped(GearSlot::OffHand, lib))
    }
    /// Swing weight of the main-hand item (1.0 if none).
    pub fn swing_weight(&self, lib: &GearLib) -> f32 {
        self.equipped(GearSlot::MainHand, lib).map(|g| g.swing_weight).unwrap_or(1.0)
    }

    /// Deterministic generation: the archetype gives ranges and choices, the
    /// seed picks. Integer RNG with quantised floats, so the same
    /// `(seed, archetype)` produces the same look on every platform.
    pub fn from_seed(seed: u64, a: &Archetype, lib: &GearLib) -> Result<CharacterDef, String> {
        let mut r = Pcg32::new(seed, 0x5713_c4e6);
        let sex = match a.sex {
            Some(s) => s,
            None => if r.below(2) == 0 { Sex::Male } else { Sex::Female },
        };
        let preset = BodyParams::preset(sex);
        let h = match sex {
            Sex::Male => a.height_male,
            Sex::Female => a.height_female,
        };
        let mut body = preset.with_height(r.range(h.0, h.1));
        body.shoulder_w *= r.range(a.shoulder_mul.0, a.shoulder_mul.1);
        body.thickness *= r.range(a.thickness.0, a.thickness.1);
        let personality = Personality {
            slouch: r.range(a.slouch.0, a.slouch.1),
            gait_speed: r.range(a.gait_speed.0, a.gait_speed.1),
            arm_swing: r.range(a.arm_swing.0, a.arm_swing.1),
        };
        let mut colors = RampMap::default();
        if !a.accent.is_empty() {
            let n = &a.accent[r.below(a.accent.len() as u32) as usize];
            colors = colors.team(ramp_id(n).ok_or_else(|| format!("archetype '{}': unknown ramp '{n}'", a.id))?);
        }
        let mut gear = [None; N_GEAR_SLOTS];
        for (slot, choices) in &a.gear {
            if choices.is_empty() {
                continue;
            }
            let pick = &choices[r.below(choices.len() as u32) as usize];
            let look_seed = r.next_u32();
            if pick.is_empty() {
                continue;
            }
            let base = lib.index(pick).ok_or_else(|| format!("archetype '{}': unknown gear '{pick}'", a.id))?;
            gear[*slot as usize] = Some(GearRef { base, look_seed });
        }
        Ok(CharacterDef { seed, sex, body, colors, personality, gear })
    }
}

/// Generation ranges for a family of characters (enemy type, NPC group,
/// or a game's class). Loaded from RON.
#[derive(Deserialize, Clone, Debug)]
pub struct Archetype {
    pub id: String,
    /// Fixed sex, or `None` to pick from the seed.
    #[serde(default)]
    pub sex: Option<Sex>,
    #[serde(default = "h_m")]
    pub height_male: (f32, f32),
    #[serde(default = "h_f")]
    pub height_female: (f32, f32),
    #[serde(default = "unit")]
    pub shoulder_mul: (f32, f32),
    #[serde(default = "unit")]
    pub thickness: (f32, f32),
    #[serde(default)]
    pub slouch: (f32, f32),
    #[serde(default = "unit")]
    pub gait_speed: (f32, f32),
    #[serde(default = "unit")]
    pub arm_swing: (f32, f32),
    /// Accent ramp names to pick from.
    #[serde(default)]
    pub accent: Vec<String>,
    /// Gear ids to pick from per slot ("" = leave empty). BTreeMap keeps the
    /// pick order (and so the result) deterministic.
    #[serde(default)]
    pub gear: BTreeMap<GearSlot, Vec<String>>,
    /// Stances this archetype is expected to use (validation/tools).
    #[serde(default)]
    pub stances: Vec<String>,
}
fn h_m() -> (f32, f32) {
    (1.70, 1.95)
}
fn h_f() -> (f32, f32) {
    (1.58, 1.82)
}
fn unit() -> (f32, f32) {
    (1.0, 1.0)
}

impl Archetype {
    pub fn from_ron(src: &str) -> Result<Archetype, String> {
        ron::from_str(src).map_err(|e| e.to_string())
    }
}

/// Example archetype used by the demo and tests.
pub const EXAMPLE_ARCHETYPE: &str = include_str!("../assets/archetypes/example_warrior.ron");
