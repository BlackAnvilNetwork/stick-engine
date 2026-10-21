// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Screenshots and frame cost for each camera mode (headless).
//! `cargo run --release -p stick_engine --example camshots -- <out_dir>`
//! writes iso / tp / fp / transition frames as PPM at the internal resolution.
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
    }
    g.set_cam_mode(CamMode::Iso);
    settle(&mut g, 1.0);
    g.set_cam_mode(CamMode::ThirdPerson);
    let half = g.rig.cfg.blend_s * 0.5;
    settle(&mut g, half);
    g.render();
    save(&g, &format!("{out}/persp-transition.ppm"));
    println!("transition t={:.2}", g.rig.t);
}
