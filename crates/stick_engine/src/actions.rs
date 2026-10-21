// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Data-driven action table (`assets/actions.ron`), replacing the old
//! `enum Action` + `ACTIONS` const table.
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub enum Mode {
    /// Standing still: plays the stance's idle clip as the base layer.
    Idle,
    /// Procedural locomotion at `speed` m/s (no clip).
    Gait { speed: f32 },
    /// Plays once, then returns to `next`. Interruptible only inside the
    /// clip's `cancel_open`..`cancel_close` window.
    OneShot { next: String },
    /// Like OneShot, but the clip's `hold` range loops until released.
    Cast { next: String },
    /// Plays on the additive layer; never changes the current action.
    Additive,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum RootMotion {
    /// Move at a fixed speed along the dash direction for the whole action.
    Dash { speed: f32 },
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Face {
    /// Keep the current facing.
    #[default]
    Keep,
    /// Face the move direction (or keep facing if not moving).
    Move,
    /// Face the aim point (or the move direction).
    Aim,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ActionDef {
    pub id: String,
    pub mode: Mode,
    #[serde(default)]
    pub blend: f32,
    #[serde(default)]
    pub stances: HashMap<String, String>,
    /// Fraction of walk speed allowed while the action plays.
    #[serde(default = "one")]
    pub move_mult: f32,
    /// Max facing change while playing, deg/s (0 = locked).
    #[serde(default)]
    pub turn_rate: f32,
    /// Requesting the same action inside its cancel window starts this one.
    #[serde(default)]
    pub chain: Option<String>,
    #[serde(default)]
    pub root_motion: Option<RootMotion>,
    #[serde(default)]
    pub cooldown: f32,
    #[serde(default)]
    pub face: Face,
    /// Weapon action: clip duration scales with the main-hand swing_weight.
    #[serde(default)]
    pub weapon: bool,
}
fn one() -> f32 {
    1.0
}

impl ActionDef {
    /// Plays once and blocks other actions outside its cancel window.
    pub fn is_busy(&self) -> bool {
        matches!(self.mode, Mode::OneShot { .. } | Mode::Cast { .. })
    }
    pub fn next(&self) -> Option<&str> {
        match &self.mode {
            Mode::OneShot { next } | Mode::Cast { next } => Some(next),
            _ => None,
        }
    }
    /// Clip for a stance: exact stance -> "any" -> none (unavailable).
    pub fn clip_for(&self, stance: &str) -> Option<&str> {
        self.stances.get(stance).or_else(|| self.stances.get("any")).map(|s| s.as_str())
    }
}

pub const BUILTIN_ACTIONS: &str = include_str!("../assets/actions.ron");

#[derive(Clone, Debug)]
pub struct ActionTable {
    pub defs: Vec<ActionDef>,
    by_id: HashMap<String, usize>,
    pub idle: usize,
    pub walk: usize,
    pub run: usize,
}

impl ActionTable {
    pub fn from_ron(src: &str) -> Result<ActionTable, String> {
        let defs: Vec<ActionDef> = ron::from_str(src).map_err(|e| e.to_string())?;
        let mut by_id = HashMap::new();
        for (i, d) in defs.iter().enumerate() {
            if by_id.insert(d.id.clone(), i).is_some() {
                return Err(format!("duplicate action '{}'", d.id));
            }
        }
        for d in &defs {
            for r in d.next().into_iter().chain(d.chain.as_deref()) {
                if !by_id.contains_key(r) {
                    return Err(format!("action '{}' refers to unknown action '{r}'", d.id));
                }
            }
        }
        let need = |n: &str| by_id.get(n).copied().ok_or_else(|| format!("action table needs '{n}'"));
        let (idle, walk, run) = (need("idle")?, need("walk")?, need("run")?);
        Ok(ActionTable { defs, by_id, idle, walk, run })
    }
    pub fn builtin() -> ActionTable {
        ActionTable::from_ron(BUILTIN_ACTIONS).expect("builtin actions.ron failed to parse")
    }
    pub fn id(&self, name: &str) -> Option<usize> {
        self.by_id.get(name).copied()
    }
    pub fn get(&self, i: usize) -> &ActionDef {
        &self.defs[i]
    }
}
