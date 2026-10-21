//! Tiny in-framebuffer debug UI: a 3x5 procedural pixel font and a column of
//! clickable buttons. No extra crates; works in every host because it only
//! writes palette indices into the framebuffer.
use crate::raster::{shade, Framebuffer, R_BONE, R_GOLD, R_STONE};

/// 3x5 glyphs, rows top->bottom, '#' = pixel.
fn glyph(c: char) -> [&'static str; 5] {
    match c.to_ascii_uppercase() {
        'A' => [".#.", "#.#", "###", "#.#", "#.#"],
        'B' => ["##.", "#.#", "##.", "#.#", "##."],
        'C' => [".##", "#..", "#..", "#..", ".##"],
        'D' => ["##.", "#.#", "#.#", "#.#", "##."],
        'E' => ["###", "#..", "##.", "#..", "###"],
        'F' => ["###", "#..", "##.", "#..", "#.."],
        'G' => [".##", "#..", "#.#", "#.#", ".##"],
        'H' => ["#.#", "#.#", "###", "#.#", "#.#"],
        'I' => ["###", ".#.", ".#.", ".#.", "###"],
        'J' => ["..#", "..#", "..#", "#.#", ".#."],
        'K' => ["#.#", "#.#", "##.", "#.#", "#.#"],
        'L' => ["#..", "#..", "#..", "#..", "###"],
        'M' => ["#.#", "###", "###", "#.#", "#.#"],
        'N' => ["##.", "#.#", "#.#", "#.#", "#.#"],
        'O' => [".#.", "#.#", "#.#", "#.#", ".#."],
        'P' => ["##.", "#.#", "##.", "#..", "#.."],
        'Q' => [".#.", "#.#", "#.#", "##.", ".##"],
        'R' => ["##.", "#.#", "##.", "#.#", "#.#"],
        'S' => [".##", "#..", ".#.", "..#", "##."],
        'T' => ["###", ".#.", ".#.", ".#.", ".#."],
        'U' => ["#.#", "#.#", "#.#", "#.#", "###"],
        'V' => ["#.#", "#.#", "#.#", "#.#", ".#."],
        'W' => ["#.#", "#.#", "###", "###", "#.#"],
        'X' => ["#.#", "#.#", ".#.", "#.#", "#.#"],
        'Y' => ["#.#", "#.#", ".#.", ".#.", ".#."],
        'Z' => ["###", "..#", ".#.", "#..", "###"],
        '0' => ["###", "#.#", "#.#", "#.#", "###"],
        '1' => [".#.", "##.", ".#.", ".#.", "###"],
        '2' => ["##.", "..#", ".#.", "#..", "###"],
        '3' => ["##.", "..#", ".#.", "..#", "##."],
        '4' => ["#.#", "#.#", "###", "..#", "..#"],
        '5' => ["###", "#..", "##.", "..#", "##."],
        '6' => [".##", "#..", "###", "#.#", "###"],
        '7' => ["###", "..#", ".#.", ".#.", ".#."],
        '8' => ["###", "#.#", "###", "#.#", "###"],
        '9' => ["###", "#.#", "###", "..#", "##."],
        '+' => ["...", ".#.", "###", ".#.", "..."],
        '-' => ["...", "...", "###", "...", "..."],
        '_' => ["...", "...", "...", "...", "###"],
        ':' => ["...", ".#.", "...", ".#.", "..."],
        '/' => ["..#", "..#", ".#.", "#..", "#.."],
        '[' => ["##.", "#..", "#..", "#..", "##."],
        ']' => [".##", "..#", "..#", "..#", ".##"],
        _ => ["...", "...", "...", "...", "..."],
    }
}

pub const CHAR_W: i32 = 4;
pub const CHAR_H: i32 = 5;

/// Draw `s` with its top-left at (x, y) in palette colour `c` (no depth test).
pub fn text(fb: &mut Framebuffer, x: i32, y: i32, s: &str, c: u8) {
    for (i, ch) in s.chars().enumerate() {
        let g = glyph(ch);
        for (r, row) in g.iter().enumerate() {
            for (k, b) in row.bytes().enumerate() {
                if b == b'#' {
                    put(fb, x + i as i32 * CHAR_W + k as i32, y + r as i32, c);
                }
            }
        }
    }
}

#[inline]
fn put(fb: &mut Framebuffer, x: i32, y: i32, c: u8) {
    if x >= 0 && y >= 0 && (x as usize) < fb.w && (y as usize) < fb.h {
        fb.color[y as usize * fb.w + x as usize] = c;
    }
}

fn rect(fb: &mut Framebuffer, x: i32, y: i32, w: i32, h: i32, fill: u8, border: u8) {
    for yy in y..y + h {
        for xx in x..x + w {
            let edge = yy == y || yy == y + h - 1 || xx == x || xx == x + w - 1;
            put(fb, xx, yy, if edge { border } else { fill });
        }
    }
}

/// What a button does when clicked or its hotkey is pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiCmd {
    /// Trigger action index in the `ActionTable`.
    Action(usize),
    ToggleSex,
    CycleGear,
    TogglePanel,
}

#[derive(Clone, Debug)]
pub struct Button {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub label: String,
    pub cmd: UiCmd,
}

impl Button {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x as f32 && py >= self.y as f32 && px < (self.x + self.w) as f32 && py < (self.y + self.h) as f32
    }
}

pub const BTN_H: i32 = 11;
pub const BTN_W: i32 = 92;
const PAD: i32 = 4;

/// Debug panel: a column of buttons plus an always-visible UI toggle.
#[derive(Clone, Debug)]
pub struct Panel {
    pub visible: bool,
    pub buttons: Vec<Button>,
    /// The small toggle button (always visible, even when hidden).
    pub toggle: Button,
    /// Button highlighted after a press (index, frames left).
    pub flash: Option<(usize, u8)>,
}

impl Panel {
    /// Build a panel from `(label, command)` rows, top-left aligned.
    pub fn new(rows: Vec<(String, UiCmd)>) -> Panel {
        let toggle = Button { x: PAD, y: PAD, w: 32, h: BTN_H, label: "F1 UI".into(), cmd: UiCmd::TogglePanel };
        let buttons = rows
            .into_iter()
            .enumerate()
            .map(|(i, (label, cmd))| Button {
                x: PAD, y: PAD + (i as i32 + 1) * (BTN_H + 2), w: BTN_W, h: BTN_H, label, cmd,
            })
            .collect();
        Panel { visible: true, buttons, toggle, flash: None }
    }

    /// Button under a framebuffer pixel, if any (respects visibility).
    pub fn hit(&self, px: f32, py: f32) -> Option<UiCmd> {
        if self.toggle.contains(px, py) {
            return Some(UiCmd::TogglePanel);
        }
        if !self.visible {
            return None;
        }
        self.buttons.iter().find(|b| b.contains(px, py)).map(|b| b.cmd)
    }

    pub fn press(&mut self, cmd: UiCmd) {
        if let Some(i) = self.buttons.iter().position(|b| b.cmd == cmd) {
            self.flash = Some((i, 8));
        }
    }

    pub fn draw(&mut self, fb: &mut Framebuffer) {
        let (fill, border, ink, hi) = (shade(R_STONE, 0), shade(R_STONE, 5), shade(R_BONE, 5), shade(R_GOLD, 4));
        let draw_btn = |fb: &mut Framebuffer, b: &Button, lit: bool| {
            rect(fb, b.x, b.y, b.w, b.h, if lit { shade(R_GOLD, 1) } else { fill }, if lit { hi } else { border });
            text(fb, b.x + 3, b.y + 3, &b.label, if lit { hi } else { ink });
        };
        draw_btn(fb, &self.toggle, false);
        if self.visible {
            for (i, b) in self.buttons.iter().enumerate() {
                draw_btn(fb, b, self.flash.map(|f| f.0) == Some(i));
            }
        }
        if let Some((i, n)) = self.flash {
            self.flash = if n > 1 { Some((i, n - 1)) } else { None };
        }
    }

    /// Set a button's label (e.g. to show the current sex or gear).
    pub fn relabel(&mut self, cmd: UiCmd, label: String) {
        if let Some(b) = self.buttons.iter_mut().find(|b| b.cmd == cmd) {
            b.label = label;
        }
    }
}
