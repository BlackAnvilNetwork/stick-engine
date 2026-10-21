// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Screenshots and frame cost for each camera mode (headless).
//! `cargo run --release -p stick_engine --example camshots -- <out_dir>`
//! writes iso / tp / fp / transition frames, plus mid-path frames of the
//! chained first-person -> iso move (`persp-fp2iso-<t>.ppm`), as PPM at the internal resolution.
use std::time::Instant;
use stick_engine::camrig::CamMode;
use stick_engine::game::{Game, Input};
use stick_engine::{HEIGHT, WIDTH};

fn save(g: &Game, path: &str) {
    let mut rgba = vec![0u8; WIDTH * HEIGHT * 4];
    g.fb.write_rgba(&mut rgba);
    let mut ppm = format!("P6\n{WIDTH} {HEIGHT}\n255\n").into_bytes();
    for px in rgba.chunks_exact(4) {
        ppm.extend_from_slice(&px[..3]);
    }
    std::fs::write(path, ppm).expect("write ppm");
}

fn settle(g: &mut Game, s: f32) {
    let mut i = Input::default();
    for _ in 0..(s * 60.0) as usize {
        g.update(1.0 / 60.0, &mut i);
    }
}

fn cost(g: &mut Game) -> f64 {
    let n = 400;
    let mut i = Input::default();
    let t = Instant::now();
    for _ in 0..n {
        g.update(1.0 / 60.0, &mut i);
        g.render();
    }
    t.elapsed().as_secs_f64() * 1000.0 / n as f64
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let mut g = Game::new(WIDTH, HEIGHT);
    g.demo = true;
    settle(&mut g, 1.2);
    for (name, mode) in [("iso", CamMode::Iso), ("tp", CamMode::ThirdPerson), ("fp", CamMode::FirstPerson)] {
        g.set_cam_mode(mode);
        settle(&mut g, 1.0);
        g.render();
        save(&g, &format!("{out}/persp-{name}.ppm"));
        println!("{name}: {:.3} ms/frame (update + render)", cost(&mut g));
        if mode != CamMode::Iso {
            g.sky_fog = false;
            let off = cost(&mut g);
            g.sky_fog = true;
            let on = cost(&mut g);
            println!("{name}: sky+fog {on:.3} ms vs off {off:.3} ms ({:+.3} ms)", on - off);
        }
    }
    g.set_cam_mode(CamMode::Iso);
    settle(&mut g, 1.0);
    g.set_cam_mode(CamMode::ThirdPerson);
    let half = g.rig.cfg.blend_s * 0.5;
    settle(&mut g, half);
    g.render();
    save(&g, &format!("{out}/persp-transition.ppm"));
    println!("transition t={:.2}", g.rig.t);
    // First-person -> iso: boom pull-back then dolly zoom, one eased move.
    g.set_cam_mode(CamMode::FirstPerson);
    settle(&mut g, 1.5);
    g.set_cam_mode(CamMode::Iso);
    let mut i = Input::default();
    for want in [0.15f32, 0.3, 0.45, 0.6, 0.8] {
        while g.rig.t < want {
            g.update(1.0 / 240.0, &mut i);
        }
        g.render();
        let tag = (want * 100.0) as u32;
        save(&g, &format!("{out}/persp-fp2iso-{tag:02}.ppm"));
        println!("fp2iso t={:.2} rail={:.2}", g.rig.t, g.rig.r);
    }
}
