// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Input actions and bindings. Hosts translate physical keys (W3C
//! `KeyboardEvent.code` names, e.g. `"KeyW"`, `"Numpad4"`) into `Action`s
//! through a `Bindings` table, so a gamepad host can drive the same actions
//! (`set_action`) without touching game code.
use crate::game::Input;

/// Logical game actions (held buttons).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    Run,
    Dash,
    Attack,
    HairStyle,
    HairColor,
    /// Cycle iso -> third-person -> first-person.
    CamCycle,
    /// 3D modes: turn the character (the camera follows its facing).
    CamTurnLeft,
    CamTurnRight,
    /// 3D modes: camera pitch.
    CamPitchUp,
    CamPitchDown,
    /// Switch camera control between numpad and mouse.
    CamControlToggle,
}

/// How the 3D camera is turned and pitched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CamControl {
    /// Numpad 4/6 turn, 8/2 pitch (default).
    #[default]
    Numpad,
    /// Right-drag turns (x) and pitches (y); a right click without a drag
    /// still moves to the clicked spot.
    Mouse,
}

impl CamControl {
    pub fn as_str(self) -> &'static str {
        match self {
            CamControl::Numpad => "numpad",
            CamControl::Mouse => "mouse",
        }
    }
    pub fn parse(s: &str) -> Option<CamControl> {
        match s.trim().to_ascii_lowercase().as_str() {
            "numpad" | "keys" | "keyboard" => Some(CamControl::Numpad),
            "mouse" => Some(CamControl::Mouse),
            _ => None,
        }
    }
    pub fn toggled(self) -> CamControl {
        match self {
            CamControl::Numpad => CamControl::Mouse,
            CamControl::Mouse => CamControl::Numpad,
        }
    }
}

/// Persisted engine settings (`key=value` lines; hosts store the string in
/// localStorage or a file).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Settings {
    pub cam_control: CamControl,
}

impl Settings {
    pub fn to_text(&self) -> String {
        format!("cam_control={}\n", self.cam_control.as_str())
    }
    /// Parse settings text; unknown keys and bad values keep the defaults.
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == "cam_control" {
                    if let Some(c) = CamControl::parse(v) {
                        s.cam_control = c;
                    }
                }
            }
        }
        s
    }
}

/// Key code -> action table.
#[derive(Clone, Debug)]
pub struct Bindings {
    pub keys: Vec<(&'static str, Action)>,
}

impl Default for Bindings {
    fn default() -> Self {
        use Action::*;
        Bindings {
            keys: vec![
                ("KeyW", MoveUp), ("ArrowUp", MoveUp),
                ("KeyS", MoveDown), ("ArrowDown", MoveDown),
                ("KeyA", MoveLeft), ("ArrowLeft", MoveLeft),
                ("KeyD", MoveRight), ("ArrowRight", MoveRight),
                ("ShiftLeft", Run), ("ShiftRight", Run),
                ("Space", Dash),
                ("KeyJ", Attack),
                ("KeyH", HairStyle),
                ("KeyK", HairColor),
                // Camera on the numpad.
                ("Numpad5", CamCycle), ("KeyV", CamCycle),
                ("Numpad4", CamTurnLeft), ("Numpad6", CamTurnRight),
                ("Numpad8", CamPitchUp), ("Numpad2", CamPitchDown),
                ("NumpadDecimal", CamControlToggle),
            ],
        }
    }
}

impl Bindings {
    pub fn action(&self, code: &str) -> Option<Action> {
        self.keys.iter().find(|(k, _)| *k == code).map(|(_, a)| *a)
    }
    /// Apply a key event; returns true if the key is bound.
    pub fn apply(&self, i: &mut Input, code: &str, down: bool) -> bool {
        match self.action(code) {
            Some(a) => {
                set_action(i, a, down);
                true
            }
            None => false,
        }
    }
}

/// Set one action's held state on the input.
pub fn set_action(i: &mut Input, a: Action, down: bool) {
    use Action::*;
    match a {
        MoveUp => i.up = down,
        MoveDown => i.down = down,
        MoveLeft => i.left = down,
        MoveRight => i.right = down,
        Run => i.run = down,
        Dash => i.dash = down,
        Attack => i.attack = down,
        HairStyle => i.hair_style = down,
        HairColor => i.hair_color = down,
        CamCycle => i.cam_cycle = down,
        CamTurnLeft => i.look_left = down,
        CamTurnRight => i.look_right = down,
        CamPitchUp => i.look_up = down,
        CamPitchDown => i.look_down = down,
        CamControlToggle => i.cam_control_toggle = down,
    }
}
