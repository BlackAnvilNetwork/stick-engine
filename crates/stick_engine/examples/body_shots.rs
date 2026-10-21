//! Headless body-shape comparison shots. Writes `<out>/<sex>-<view>.ppm`
//! crops (raw framebuffer pixels) for idle, mid-run, side, back and
//! armoured views. Usage: `cargo run --example body_shots -- <out_dir>`.
use stick_engine::body::{GearRef, Sex};
use stick_engine::game::{Game, Input};
use stick_engine::gear::GearSlot;
use stick_engine::ui::UiCmd;

const CROP: (usize, usize) = (72, 104);

fn shot(g: &mut Game, path: &str) {
    g.render();
    let p = g.cam.project(g.player.pos);
    let (cw, ch) = CROP;
    let x0 = (p.x as i32 - cw as i32 / 2).max(0) as usize;
    let y0 = (p.y as i32 - ch as i32 + 14).max(0) as usize;
    let mut rgba = vec![0u8; g.fb.w * g.fb.h * 4];
    g.fb.write_rgba(&mut rgba);
    let mut out = format!("P6 {cw} {ch} 255\n").into_bytes();
    for y in y0..y0 + ch {
        for x in x0..x0 + cw {
            let i = (y * g.fb.w + x) * 4;
            out.extend_from_slice(&rgba[i..i + 3]);
        }
    }
    std::fs::write(path, out).unwrap();
}

fn setup(sex: Sex, armour: bool) -> Game {
    let mut g = Game::new(640, 360);
    for _ in 0..3 {
        g.run_cmd(UiCmd::CycleGear); // -> unarmed
    }
    g.run_cmd(UiCmd::TogglePanel);
    if sex == Sex::Female {
        g.run_cmd(UiCmd::ToggleSex);
    }
    if armour {
        let base = g.gear.index("cuirass").expect("cuirass");
        g.player.def.equip(GearSlot::Chest, Some(GearRef { base, look_seed: 1 }));
        g.player.refresh_gear(&g.gear);
    }
    g
}

fn settle(g: &mut Game, yaw: f32) {
    let mut i = Input::default();
    for _ in 0..120 {
        g.player.yaw = yaw;
        g.update(1.0 / 60.0, &mut i);
    }
    g.player.yaw = yaw;
    g.cam.cx = g.player.pos.x;
    g.cam.cz = g.player.pos.z;
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let front = std::f32::consts::FRAC_PI_4;
    for (sex, name) in [(Sex::Male, "male"), (Sex::Female, "female")] {
        for (view, yaw, armour) in [("idle", front, false), ("side", front + std::f32::consts::FRAC_PI_2, false),
            ("back", front + std::f32::consts::PI, false), ("armour", front, true)] {
            let mut g = setup(sex, armour);
            settle(&mut g, yaw);
            shot(&mut g, &format!("{dir}/{name}-{view}.ppm"));
        }
        // mid-run: run down-right (toward the camera's right) and grab a stride
        let mut g = setup(sex, false);
        let mut i = Input { down: true, right: true, run: true, ..Default::default() };
        for _ in 0..47 {
            g.update(1.0 / 60.0, &mut i);
        }
        g.cam.cx = g.player.pos.x;
        g.cam.cz = g.player.pos.z;
        shot(&mut g, &format!("{dir}/{name}-run.ppm"));
    }
}
