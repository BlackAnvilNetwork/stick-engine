// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Stick Engine core: math, CPU pixel raster with ramp palettes, headless
//! stick rig (FK), character definitions, gear, data-driven animation and
//! actions, procedural gait, drawing, and a tiny demo game loop. Platform
//! hosts live in `stick_web` (wasm) and `stick_native` (winit + softbuffer).
//!
//! Headless-capable modules (no drawing or camera; palette constants only):
//! `anim`, `actions`,
//! `body`, `gait`, `gear`, `rig`, `character`, `math`.

pub mod actions;
pub mod anim;
pub mod body;
pub mod camera;
pub mod camrig;
pub mod input;
pub mod character;
pub mod color;
pub mod custom;
pub mod gait;
pub mod game;
pub mod gear;
pub mod math;
pub mod raster;
pub mod render;
pub mod rig;

/// Internal framebuffer resolution. 640x360 keeps an ~80 px stick figure at
/// roughly 22% of screen height (Diablo-like framing) and upscales 2x/3x/6x
/// cleanly to 720p/1080p/4K.
pub const WIDTH: usize = 640;
pub const HEIGHT: usize = 360;

#[cfg(test)]
mod tests;
