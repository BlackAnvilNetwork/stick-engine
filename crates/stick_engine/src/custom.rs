// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Player-facing hair options: a small serializable record (RON) of hair
//! style and hair colour applied onto a [`CharacterDef`]. Purely cosmetic.
use crate::body::{CharacterDef, HairStyle, ALL_HAIR};
use crate::color::ColorSlot;
use crate::math::Pcg32;
use crate::raster::{ramp_id, HAIR_RAMPS};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct HairOptions {
    pub hair: HairStyle,
    /// Ramp name from [`HAIR_RAMPS`].
    pub hair_color: String,
}

impl Default for HairOptions {
    fn default() -> Self {
        HairOptions { hair: HairStyle::None, hair_color: "hair_brown".into() }
    }
}

fn cycle<T: PartialEq + Clone>(list: &[T], cur: &T, d: i32) -> T {
    let i = list.iter().position(|x| x == cur).unwrap_or(0) as i32;
    list[(i + d).rem_euclid(list.len() as i32) as usize].clone()
}

impl HairOptions {
    /// Copy with an unknown colour replaced by the first hair ramp.
    pub fn clamped(&self) -> Self {
        let c = if HAIR_RAMPS.contains(&self.hair_color.as_str()) { self.hair_color.clone() } else { HAIR_RAMPS[0].into() };
        HairOptions { hair: self.hair, hair_color: c }
    }
    /// Apply onto a definition: body hair style + `Hair` colour slot only.
    pub fn apply(&self, def: &mut CharacterDef) {
        let s = self.clamped();
        def.body.hair = s.hair;
        def.colors = def.colors.with(ColorSlot::Hair, ramp_id(&s.hair_color).unwrap_or(0));
    }
    /// Next/previous style (`d` = +1 / -1), bald included.
    pub fn cycle_style(&mut self, d: i32) {
        self.hair = cycle(&ALL_HAIR, &self.hair, d);
    }
    /// Next/previous hair colour.
    pub fn cycle_color(&mut self, d: i32) {
        self.hair_color = cycle(&HAIR_RAMPS, &self.hair_color.as_str(), d).to_string();
    }
    /// Deterministic random look from a seed.
    pub fn random(seed: u64) -> Self {
        let mut g = Pcg32::new(seed, 0xc057_0a11);
        let hair = ALL_HAIR[g.below(ALL_HAIR.len() as u32) as usize];
        let hair_color = HAIR_RAMPS[g.below(HAIR_RAMPS.len() as u32) as usize].to_string();
        HairOptions { hair, hair_color }
    }
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("serialize hair options")
    }
    pub fn from_ron(src: &str) -> Result<Self, String> {
        ron::from_str::<Self>(src).map(|c| c.clamped()).map_err(|e| e.to_string())
    }
}
