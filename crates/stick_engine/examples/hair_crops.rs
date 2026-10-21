// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Head crops of every hair style at 4 facings + a run frame: `-- out.ppm`
use stick_engine::body::ALL_HAIR;
use stick_engine::custom::HairOptions;
use stick_engine::game::{Game, Input};
use stick_engine::raster::{Framebuffer, LUT};
use stick_engine::render::{draw_character, DrawOpts};

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "hair_crops.ppm".into());
    let (cw, ch) = (48usize, 56usize);
    let yaws: Vec<f32> = match std::env::var("YAWS") {
        Ok(v) => v.split(',').map(|x| x.parse().unwrap()).collect(),
        Err(_) => vec![0.8, 2.37, 3.94, 5.5],
    };
    // MODES: r = rest, w = walk, n = run (per yaw)
    let modes: Vec<char> = std::env::var("MODES").unwrap_or_else(|_| "r".into()).chars().collect();
    let cols = yaws.len() * modes.len();
    let (w, h) = (cw * cols, ch * ALL_HAIR.len());
    let mut sheet = vec![0u32; w * h];
    for (row, style) in ALL_HAIR.iter().enumerate() {
        for col in 0..cols {
            let mut g = Game::new(cw, 140);
            g.player.def.equip(stick_engine::gear::GearSlot::MainHand, None);
            g.hair = HairOptions { hair: *style, hair_color: "hair_brown".into() };
            g.hair.apply(&mut g.player.def);
            let (yaw, mode) = (yaws[col / modes.len()], modes[col % modes.len()]);
            let mut i = Input::default();
            // move along the facing so walk/run frames keep the yaw
            let (fx, fz) = (yaw.sin(), yaw.cos());
            let _ = (fx, fz);
            if mode != 'r' { i.run = mode == 'n'; }
            for k in 0..40 {
                if mode != 'r' { g.player.yaw = yaw; }
                let mut t = i;
                if mode != 'r' && k < 39 { t.move_click = Some(g.cam.project(g.player.pos + stick_engine::math::v3(yaw.sin(), 0.0, yaw.cos()) * 3.0)).map(|p| (p.x, p.y)); }
                g.update(1.0 / 60.0, &mut t);
            }
            g.player.yaw = yaw;
            let p = g.player.pos;
            g.cam.cx = p.x - 0.75; g.cam.cz = p.z - 0.75;
            let mut fb = Framebuffer::new(cw, 140);
            fb.color.fill(stick_engine::raster::shade(1, 2));
            let j = g.player_joints();
            draw_character(&mut fb, &g.cam, &j, &g.player.def, &g.gear, &g.player.def.colors, &DrawOpts::default());
            let hy = g.cam.project(j.get(stick_engine::rig::Joint::Head)).y as i32 - ch as i32 / 2 + 6;
            for y in 0..ch { for x in 0..cw {
                let sy = (hy + y as i32).clamp(0, 139) as usize;
                sheet[(row * ch + y) * w + col * cw + x] = LUT[fb.color[sy * cw + x] as usize];
            }}
        }
    }
    let mut buf = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in sheet { buf.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]); }
    std::fs::write(&out, buf).expect("write ppm");
}
