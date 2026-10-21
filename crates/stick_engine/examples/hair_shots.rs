// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Renders every hair style in two colours into a PPM contact sheet:
//! `cargo run -p stick_engine --example hair_shots -- out.ppm`
use stick_engine::body::ALL_HAIR;
use stick_engine::custom::HairOptions;
use stick_engine::game::Game;
use stick_engine::raster::{Framebuffer, LUT};
use stick_engine::render::{draw_character, DrawOpts};

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "hair_shots.ppm".into());
    let (cw, ch) = (96usize, 120usize);
    let cols = ALL_HAIR.len();
    let colours = ["hair_brown", "hair_blonde", "hair_red"];
    let (w, h) = (cw * cols, ch * colours.len());
    let mut sheet = vec![0u32; w * h];
    for (row, colour) in colours.iter().enumerate() {
        for (col, style) in ALL_HAIR.iter().enumerate() {
            let mut g = Game::new(cw, ch);
            g.player.yaw = 0.8 + col as f32 * 0.55;
            g.hair = HairOptions { hair: *style, hair_color: colour.to_string() };
            g.hair.apply(&mut g.player.def);
            g.player.def.equip(stick_engine::gear::GearSlot::MainHand, None);
            let mut i = stick_engine::game::Input::default();
            for _ in 0..30 {
                g.update(1.0 / 60.0, &mut i);
            }
            g.cam.cx = g.player.pos.x - 0.75;
            g.cam.cz = g.player.pos.z - 0.75;
            let mut fb = Framebuffer::new(cw, ch);
            fb.color.fill(stick_engine::raster::shade(1, 2));
            let j = g.player_joints();
            draw_character(&mut fb, &g.cam, &j, &g.player.def, &g.gear, &g.player.def.colors, &DrawOpts::default());
            for y in 0..ch {
                for x in 0..cw {
                    sheet[(row * ch + y) * w + col * cw + x] = LUT[fb.color[y * cw + x] as usize];
                }
            }
        }
    }
    let mut buf = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in sheet {
        buf.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
    }
    std::fs::write(&out, buf).expect("write ppm");
}
