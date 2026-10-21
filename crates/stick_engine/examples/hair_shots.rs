//! Headless hair/customization shots: `<out>/<sex>-<i>-<front|side|back>.ppm`
//! crops for every menu hair style, and `<out>/customize.ppm` (full frame).
//! Usage: `cargo run --example hair_shots -- <out_dir>`.
use stick_engine::body::{Sex, FEMALE_HAIR, MALE_HAIR};
use stick_engine::game::{Game, Input, HK_CUSTOM, HK_NAV_DOWN};
use stick_engine::ui::UiCmd;

fn ppm(g: &Game, path: &str, x0: usize, y0: usize, cw: usize, ch: usize) {
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

fn base(sex: Sex) -> Game {
    let mut g = Game::new(640, 360);
    for _ in 0..3 {
        g.run_cmd(UiCmd::CycleGear); // unarmed
    }
    g.run_cmd(UiCmd::TogglePanel);
    g.custom = stick_engine::custom::CharacterCustomization::for_sex(sex);
    g
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let front = std::f32::consts::FRAC_PI_4;
    for (sex, name, menu, color) in
        [(Sex::Male, "male", MALE_HAIR, "hair_black"), (Sex::Female, "female", FEMALE_HAIR, "hair_blonde")]
    {
        for (k, hair) in menu.iter().enumerate() {
            for (view, yaw) in [("front", front), ("side", front + 1.5708), ("back", front + 3.1416)] {
                let mut g = base(sex);
                g.custom.hair = *hair;
                g.custom.hair_color = color.into();
                g.apply_custom();
                // walk a little then stop so loose hair is mid-sway
                let mut i = Input { right: true, ..Default::default() };
                for t in 0..70 {
                    if t == 60 {
                        i = Input::default();
                    }
                    g.update(1.0 / 60.0, &mut i);
                }
                g.player.yaw = yaw;
                g.cam.cx = g.player.pos.x;
                g.cam.cz = g.player.pos.z;
                g.render();
                let p = g.cam.project(g.player.pos);
                ppm(&g, &format!("{dir}/{name}-{k}-{view}.ppm"), p.x as usize - 24, p.y as usize - 96, 48, 48);
            }
        }
    }
    let mut g = base(Sex::Female);
    g.custom.hair = stick_engine::body::HairStyle::LongStraight;
    g.custom.skin = "skin_tan".into();
    g.custom.outfit = "team_blue".into();
    g.apply_custom();
    let mut i = Input { hotkeys: HK_CUSTOM, ..Default::default() };
    g.update(1.0 / 60.0, &mut i);
    for _ in 0..9 {
        let mut i = Input { hotkeys: HK_NAV_DOWN, ..Default::default() };
        g.update(1.0 / 60.0, &mut i);
    }
    let mut i = Input::default();
    for _ in 0..40 {
        g.update(1.0 / 60.0, &mut i);
    }
    g.render();
    ppm(&g, &format!("{dir}/customize.ppm"), 0, 0, 640, 360);
}
