//! Player-facing character customization: a small serializable record
//! (RON) of preset, sliders, hair and colours that is applied onto a
//! [`CharacterDef`]. Purely cosmetic: it only touches `BodyParams` shape
//! fields and the colour map, never gameplay stats.
use crate::body::{BodyParams, CharacterDef, HairStyle, Sex, ALL_HAIR, FEMALE_HAIR, MALE_HAIR};
use crate::color::ColorSlot;
use crate::math::Pcg32;
use crate::raster::{ramp_id, HAIR_RAMPS, OUTFIT_RAMPS, SKIN_RAMPS};
use serde::{Deserialize, Serialize};

/// Slider ranges `(min, max)`; `clamped` enforces them.
pub const BLEND: (f32, f32) = (0.0, 1.0);
/// Height multiplier on the preset's standing height.
pub const HEIGHT: (f32, f32) = (0.88, 1.12);
pub const SHOULDER: (f32, f32) = (0.8, 1.2);
pub const WAIST: (f32, f32) = (0.8, 1.25);
pub const HIP: (f32, f32) = (0.8, 1.25);
pub const CHEST: (f32, f32) = (0.0, 1.6);
pub const THIGH: (f32, f32) = (0.75, 1.3);
pub const LIMB: (f32, f32) = (0.8, 1.25);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct CharacterCustomization {
    pub sex: Sex,
    /// 0 = pure preset, 1 = the other sex's build (`BodyParams::blend_shape`).
    pub blend: f32,
    pub height: f32,
    pub shoulder: f32,
    pub waist: f32,
    pub hip: f32,
    pub chest: f32,
    pub thigh: f32,
    /// Overall limb thickness multiplier.
    pub limb: f32,
    pub hair: HairStyle,
    /// Ramp names (see `raster::RAMPS`).
    pub hair_color: String,
    pub skin: String,
    pub outfit: String,
}

impl Default for CharacterCustomization {
    fn default() -> Self {
        Self::for_sex(Sex::Male)
    }
}

impl CharacterCustomization {
    /// Neutral sliders for a sex (applies to exactly that sex's preset).
    pub fn for_sex(sex: Sex) -> Self {
        CharacterCustomization {
            sex, blend: 0.0, height: 1.0, shoulder: 1.0, waist: 1.0, hip: 1.0, chest: 1.0, thigh: 1.0, limb: 1.0,
            hair: BodyParams::preset(sex).hair, hair_color: "hair_brown".into(), skin: "bone".into(),
            outfit: "bone".into(),
        }
    }
    /// Copy with every slider inside its range and unknown ramps replaced.
    pub fn clamped(&self) -> Self {
        let c = |v: f32, (lo, hi): (f32, f32)| if v.is_finite() { v.clamp(lo, hi) } else { 1.0f32.clamp(lo, hi) };
        let ramp = |n: &str, list: &[&str]| if list.contains(&n) { n.to_string() } else { list[0].to_string() };
        CharacterCustomization {
            sex: self.sex, blend: c(self.blend, BLEND), height: c(self.height, HEIGHT),
            shoulder: c(self.shoulder, SHOULDER), waist: c(self.waist, WAIST), hip: c(self.hip, HIP),
            chest: c(self.chest, CHEST), thigh: c(self.thigh, THIGH), limb: c(self.limb, LIMB), hair: self.hair,
            hair_color: ramp(&self.hair_color, &HAIR_RAMPS), skin: ramp(&self.skin, &SKIN_RAMPS),
            outfit: ramp(&self.outfit, &OUTFIT_RAMPS),
        }
    }
    /// Body for these settings (clamped first).
    pub fn body(&self) -> BodyParams {
        let s = self.clamped();
        let mut b = BodyParams::preset(s.sex);
        if s.height != 1.0 {
            b = b.with_height(b.height * s.height);
        }
        if s.blend > 0.0 {
            let other = BodyParams::preset(if s.sex == Sex::Male { Sex::Female } else { Sex::Male });
            b = b.blend_shape(&other, s.blend);
        }
        b.shoulder_w *= s.shoulder;
        b.waist_w *= s.waist;
        b.hip_w *= s.hip;
        b.chest *= s.chest;
        b.thigh_w *= s.thigh;
        b.thickness *= s.limb;
        b.hair = s.hair;
        b
    }
    /// Apply onto a definition: sex, body and the skin/outfit/hair slots.
    /// Seed, gear, personality and other colour slots are kept.
    pub fn apply(&self, def: &mut CharacterDef) {
        let s = self.clamped();
        def.sex = s.sex;
        def.body = s.body();
        let r = |n: &str| ramp_id(n).unwrap_or(0);
        def.colors = def.colors.skin(r(&s.skin)).with(ColorSlot::Outfit, r(&s.outfit)).with(ColorSlot::Hair, r(&s.hair_color));
    }
    /// Deterministic random look from a seed (sliders kept near neutral so
    /// results stay tasteful).
    pub fn random(seed: u64) -> Self {
        let mut g = Pcg32::new(seed, 0xc057_0a11);
        let sex = if g.below(2) == 0 { Sex::Male } else { Sex::Female };
        let near = |g: &mut Pcg32, (lo, hi): (f32, f32)| {
            let (a, b) = (1.0 - (1.0 - lo) * 0.6, 1.0 + (hi - 1.0) * 0.6);
            (g.range(a, b) * 20.0).round() / 20.0
        };
        let pick = |g: &mut Pcg32, l: &[&str]| l[g.below(l.len() as u32) as usize].to_string();
        let menu = if sex == Sex::Male { MALE_HAIR } else { FEMALE_HAIR };
        let hair = if g.below(12) == 0 { HairStyle::None } else { menu[g.below(5) as usize] };
        CharacterCustomization {
            sex, blend: (g.range(0.0, 0.25) * 20.0).round() / 20.0, height: near(&mut g, HEIGHT),
            shoulder: near(&mut g, SHOULDER), waist: near(&mut g, WAIST), hip: near(&mut g, HIP),
            chest: near(&mut g, (0.6, 1.4)), thigh: near(&mut g, THIGH), limb: near(&mut g, LIMB), hair,
            hair_color: pick(&mut g, &HAIR_RAMPS), skin: pick(&mut g, &SKIN_RAMPS), outfit: pick(&mut g, &OUTFIT_RAMPS),
        }
    }
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("serialize customization")
    }
    /// Parse and clamp (hand-edited files can't push sliders out of range).
    pub fn from_ron(src: &str) -> Result<Self, String> {
        ron::from_str::<Self>(src).map(|c| c.clamped()).map_err(|e| e.to_string())
    }
}

/// Rows of the customization screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Body, Blend, Height, Shoulder, Waist, Hip, Chest, Thigh, Limb, Hair, HairColor, Skin, Outfit,
    Randomize, Reset, Save, Load, Close,
}
pub const ROWS: [Row; 18] = [
    Row::Body, Row::Blend, Row::Height, Row::Shoulder, Row::Waist, Row::Hip, Row::Chest, Row::Thigh, Row::Limb,
    Row::Hair, Row::HairColor, Row::Skin, Row::Outfit, Row::Randomize, Row::Reset, Row::Save, Row::Load,
    Row::Close,
];

fn cycle<T: PartialEq + Clone>(list: &[T], cur: &T, d: i32) -> T {
    let i = list.iter().position(|x| x == cur).unwrap_or(0) as i32;
    list[(i + d).rem_euclid(list.len() as i32) as usize].clone()
}
fn step(v: &mut f32, d: i32, inc: f32, r: (f32, f32)) {
    *v = ((*v + d as f32 * inc).clamp(r.0, r.1) / inc).round() * inc;
}

impl CharacterCustomization {
    /// Flip male/female; a hair style that was the old preset's default
    /// follows to the new preset's default.
    pub fn switch_sex(&mut self) {
        let old = self.sex;
        self.sex = if old == Sex::Male { Sex::Female } else { Sex::Male };
        if self.hair == BodyParams::preset(old).hair {
            self.hair = BodyParams::preset(self.sex).hair;
        }
    }
    /// Change a value row by `d` (-1 / +1). Action rows are ignored here.
    pub fn adjust(&mut self, row: Row, d: i32) {
        match row {
            Row::Body => self.switch_sex(),
            Row::Blend => step(&mut self.blend, d, 0.1, BLEND),
            Row::Height => step(&mut self.height, d, 0.02, HEIGHT),
            Row::Shoulder => step(&mut self.shoulder, d, 0.05, SHOULDER),
            Row::Waist => step(&mut self.waist, d, 0.05, WAIST),
            Row::Hip => step(&mut self.hip, d, 0.05, HIP),
            Row::Chest => step(&mut self.chest, d, 0.1, CHEST),
            Row::Thigh => step(&mut self.thigh, d, 0.05, THIGH),
            Row::Limb => step(&mut self.limb, d, 0.05, LIMB),
            Row::Hair => self.hair = cycle(&ALL_HAIR, &self.hair, d),
            Row::HairColor => self.hair_color = cycle(&HAIR_RAMPS, &self.hair_color.as_str(), d).to_string(),
            Row::Skin => self.skin = cycle(&SKIN_RAMPS, &self.skin.as_str(), d).to_string(),
            Row::Outfit => self.outfit = cycle(&OUTFIT_RAMPS, &self.outfit.as_str(), d).to_string(),
            _ => {}
        }
    }
    /// `(label, value)` text for a row.
    pub fn row_text(&self, row: Row) -> (&'static str, String) {
        let pct = |v: f32| format!("{:.0}", v * 100.0);
        let up = |s: &str| s.replace('_', " ").to_uppercase();
        match row {
            Row::Body => ("BODY", if self.sex == Sex::Male { "MALE".into() } else { "FEMALE".into() }),
            Row::Blend => ("BLEND", pct(self.blend)),
            Row::Height => ("HEIGHT", format!("{:.2}M", self.body().height)),
            Row::Shoulder => ("SHOULDERS", pct(self.shoulder)),
            Row::Waist => ("WAIST", pct(self.waist)),
            Row::Hip => ("HIPS", pct(self.hip)),
            Row::Chest => ("CHEST", pct(self.chest)),
            Row::Thigh => ("THIGHS", pct(self.thigh)),
            Row::Limb => ("LIMBS", pct(self.limb)),
            Row::Hair => ("HAIR", self.hair.name().into()),
            Row::HairColor => ("HAIR COLOUR", up(self.hair_color.trim_start_matches("hair_"))),
            Row::Skin => ("SKIN", up(self.skin.trim_start_matches("skin_"))),
            Row::Outfit => ("OUTFIT", up(&self.outfit)),
            Row::Randomize => ("RANDOMIZE  [R]", String::new()),
            Row::Reset => ("RESET  [X]", String::new()),
            Row::Save => ("SAVE  [F5]", String::new()),
            Row::Load => ("LOAD  [F9]", String::new()),
            Row::Close => ("CLOSE  [C]", String::new()),
        }
    }
}
