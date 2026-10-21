//! Character definition: body proportions, sex presets, personality, colour
//! slots and equipped gear, plus deterministic generation from
//! `(seed, archetype)`. Pure data: no rendering.
use crate::color::RampMap;
use crate::gear::{derive_stance, GearLib, GearSlot, ALL_GEAR_SLOTS, N_GEAR_SLOTS};
use crate::math::Pcg32;
use crate::raster::ramp_id;
use crate::rig::N_BONES;
use serde::Deserialize;
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

/// Cosmetic hair silhouette drawn on the head (body ramp, one shade darker).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HairStyle {
    #[default]
    None,
    Bob,
    Ponytail,
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
    /// Chest silhouette cue: radius in metres of each of the two small chest
    /// discs (0 = none). A body parameter, not a rig bone.
    pub chest: f32,
    /// Torso width at the waist (m, full width). Torso is drawn as a filled
    /// tapered silhouette: hips -> waist -> bust -> shoulders.
    pub waist_w: f32,
    /// Torso front-to-back depth (m); what the side view shows.
    pub torso_depth: f32,
    /// Thigh radius multiplier at the hip (tapers to the knee).
    pub thigh_w: f32,
    /// Arm radius multiplier.
    pub arm_thick: f32,
    /// Shin/calf radius multiplier.
    pub calf_thick: f32,
    pub hair: HairStyle,
}

impl BodyParams {
    /// Reference proportions for a sex. Starting values, tuned to the
    /// prototype's look (stylised: big head, narrow hips).
    pub fn preset(sex: Sex) -> BodyParams {
        let m = BodyParams {
            height: 0.0, thigh: 0.46, shin: 0.46, upper_arm: 0.30, forearm: 0.28, torso: 0.52,
            neck: 0.08, head_r: 0.13, shoulder_w: 0.40, hip_w: 0.22, limb_px: 2.2, thickness: 1.0,
            bone_thickness_mul: [1.0; N_BONES], chest: 0.0,
            // V-taper: broad shoulders, narrow waist and hips, heavier limbs
            waist_w: 0.22, torso_depth: 0.21, thigh_w: 1.2, arm_thick: 1.15, calf_thick: 1.05,
            hair: HairStyle::None,
        };
        let m = BodyParams { shoulder_w: 0.46, ..m };
        let b = match sex {
            Sex::Male => m,
            Sex::Female => BodyParams {
                head_r: 0.125, shoulder_w: 0.32, hip_w: 0.27, thickness: 0.92, chest: 0.062,
                // hourglass: defined waist, wide hips, full tapering thighs, slim arms/calves
                waist_w: 0.18, torso_depth: 0.17, thigh_w: 1.7, arm_thick: 0.78, calf_thick: 0.85,
                hair: HairStyle::Bob, ..m
            }
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
            &mut b.head_r, &mut b.shoulder_w, &mut b.hip_w, &mut b.chest, &mut b.waist_w, &mut b.torso_depth] {
            *v *= k;
        }
        b.height = b.computed_height();
        b
    }
    /// Scale so the standing height equals `h`.
    pub fn with_height(&self, h: f32) -> BodyParams {
        self.scaled(h / self.computed_height())
    }
    /// Blend the shape fields (widths, depth, limb multipliers, chest) toward
    /// `o` by `t` (0 = self). Lengths/height stay; hair switches at t >= 0.5.
    /// Lets archetypes sit anywhere between the male and female builds.
    pub fn blend_shape(&self, o: &BodyParams, t: f32) -> BodyParams {
        let t = t.clamp(0.0, 1.0);
        let k = self.height / o.height.max(1e-3); // compare at equal height
        let l = |a: f32, b: f32| a + (b - a) * t;
        BodyParams {
            shoulder_w: l(self.shoulder_w, o.shoulder_w * k), waist_w: l(self.waist_w, o.waist_w * k),
            hip_w: l(self.hip_w, o.hip_w * k), torso_depth: l(self.torso_depth, o.torso_depth * k),
            chest: l(self.chest, o.chest * k), thigh_w: l(self.thigh_w, o.thigh_w),
            arm_thick: l(self.arm_thick, o.arm_thick), calf_thick: l(self.calf_thick, o.calf_thick),
            hair: if t >= 0.5 { o.hair } else { self.hair },
            ..*self
        }
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
        // drawn last so archetypes without the range keep their old results
        if let Some((lo, hi)) = a.chest_mul {
            body.chest *= r.range(lo, hi);
        }
        if let Some((lo, hi)) = a.shape_mix {
            let other = BodyParams::preset(if sex == Sex::Male { Sex::Female } else { Sex::Male });
            body = body.blend_shape(&other, r.range(lo, hi));
        }
        if !a.hair.is_empty() {
            body.hair = a.hair[r.below(a.hair.len() as u32) as usize];
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
    /// Optional multiplier range for `BodyParams::chest` (e.g. (0.85, 1.15)).
    #[serde(default)]
    pub chest_mul: Option<(f32, f32)>,
    /// Optional range blending the body shape toward the opposite sex's
    /// preset (0 = pure preset). Cosmetic only.
    #[serde(default)]
    pub shape_mix: Option<(f32, f32)>,
    /// Hair styles to pick from (empty = keep the preset's).
    #[serde(default)]
    pub hair: Vec<HairStyle>,
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
