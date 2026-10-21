//! Head crops of every hair style at 4 facings + a run frame: `-- out.ppm`
use stick_engine::body::ALL_HAIR;
use stick_engine::custom::HairOptions;
use stick_engine::game::{Game, Input};
use stick_engine::raster::{Framebuffer, LUT};
use stick_engine::render::{draw_character, DrawOpts};

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "hair_crops.ppm".into());
    let (cw, ch) = (48usize, 56usize);
    let yaws = [0.8f32, 2.37, 3.94, 5.5];
    let cols = yaws.len() + 2;
    let (w, h) = (cw * cols, ch * ALL_HAIR.len());
    let mut sheet = vec![0u32; w * h];
    for (row, style) in ALL_HAIR.iter().enumerate() {
        for col in 0..cols {
            let mut g = Game::new(cw, 140);
            g.player.def.equip(stick_engine::gear::GearSlot::MainHand, None);
            g.hair = HairOptions { hair: *style, hair_color: "hair_brown".into() };
            g.hair.apply(&mut g.player.def);
            let mut i = Input::default();
            if col >= yaws.len() { i.right = true; i.run = true; }
            let n = if col == yaws.len() { 20 } else { 33 };
            for _ in 0..n { g.update(1.0 / 60.0, &mut i); }
            if col < yaws.len() { g.player.yaw = yaws[col]; }
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
