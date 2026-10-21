// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Colour slots. Bodies and gear never store palette indices: they name a
//! [`ColorSlot`], and a per-draw [`RampMap`] resolves slot -> ramp. Team
//! colours, elemental tints and hit flashes are just different maps.
use crate::raster::{shade, R_ARCANE, R_BLOOD, R_BONE, R_GOLD, R_STEEL, R_VOID, R_WHITE};
use crate::raster::R_HAIR_BROWN;
use serde::Deserialize;

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ColorSlot {
    Body, BodyFar, Outline, Accent, Material, Trim, Glow,
    Hair,
}
pub const N_SLOTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RampMap {
    pub ramps: [u8; N_SLOTS],
    /// Hit flash: every slot except the outline draws from the white ramp.
    pub flash: bool,
}

impl Default for RampMap {
    fn default() -> Self {
        RampMap { ramps: [R_BONE, R_BONE, R_VOID, R_BLOOD, R_STEEL, R_GOLD, R_ARCANE, R_HAIR_BROWN], flash: false }
    }
}

impl RampMap {
    #[inline]
    pub fn ramp(&self, s: ColorSlot) -> u8 {
        if self.flash && s != ColorSlot::Outline { R_WHITE } else { self.ramps[s as usize] }
    }
    /// Palette index for slot `s` at shade `sh` (0 dark .. 5 light).
    #[inline]
    pub fn color(&self, s: ColorSlot, sh: u8) -> u8 {
        shade(self.ramp(s), sh)
    }
    pub fn with(mut self, s: ColorSlot, ramp: u8) -> Self {
        self.ramps[s as usize] = ramp;
        self
    }
    /// Team colour = accent override.
    pub fn team(self, ramp: u8) -> Self {
        self.with(ColorSlot::Accent, ramp)
    }
    /// Elemental tint = body override.
    pub fn tint(self, ramp: u8) -> Self {
        self.with(ColorSlot::Body, ramp).with(ColorSlot::BodyFar, ramp).with(ColorSlot::Hair, ramp)
    }
    pub fn flashed(mut self) -> Self {
        self.flash = true;
        self
    }
}
