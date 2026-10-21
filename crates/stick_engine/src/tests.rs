use crate::anim::*;
use crate::camera::Camera;
use crate::character::*;
use crate::game::{Game, Input};
use crate::math::v3;
use crate::rig;

#[test]
fn builtin_clips_parse() {
    let lib = AnimLibrary::builtin();
    for n in ["idle", "walk", "run", "dash", "swing"] {
        assert!(lib.get(n).is_some(), "missing clip {n}");
    }
    for d in ACTIONS {
        assert!(lib.get(d.clip).is_some(), "action {:?} uses unknown clip", d.action);
    }
}

#[test]
fn unknown_channel_is_error() {
    let e = Clip::from_ron(r#"(name:"x", duration:1.0, keys:[(t:0.0, pose:{"tail":1.0})])"#);
    assert!(e.is_err());
}

#[test]
fn sampling_interpolates_and_loops() {
    let c = Clip::from_ron(r#"(name:"x", duration:1.0, looping:true, keys:[(t:0.0, pose:{"lean":0.0}),(t:0.5, pose:{"lean":10.0})])"#).unwrap();
    assert_eq!(c.sample(0.0).get("lean"), 0.0);
    assert_eq!(c.sample(0.5).get("lean"), 10.0);
    assert!((c.sample(0.25).get("lean") - 5.0).abs() < 1e-4);
    assert!((c.sample(1.0).get("lean")).abs() < 1e-4); // wraps
}

#[test]
fn figure_is_about_80px_tall() {
    let lib = AnimLibrary::builtin();
    let pose = lib.get("idle").unwrap().sample(0.0);
    let pr = rig::Proportions::default();
    let j = rig::solve(&pose, &pr, v3(0.0, 0.0, 0.0), 0.0);
    let cam = Camera::new(640, 360);
    let top = cam.project(j.head).y - pr.head_r * crate::camera::HEIGHT_PX;
    let foot = cam.project(j.foot[0]).y.max(cam.project(j.foot[1]).y);
    let h = foot - top;
    assert!((72.0..=88.0).contains(&h), "height {h}px");
}

#[test]
fn state_machine_walk_run_dash_swing() {
    let lib = AnimLibrary::builtin();
    let mut c = Character::new(0.0, 0.0);
    let dt = 1.0 / 60.0;
    let walk = Intent { dir: (1.0, 0.0), ..Default::default() };
    c.step(&lib, &walk, dt);
    assert_eq!(c.action, Action::Walk);
    c.step(&lib, &Intent { run: true, ..walk }, dt);
    assert_eq!(c.action, Action::Run);
    c.step(&lib, &Intent { dash: true, ..walk }, dt);
    assert_eq!(c.action, Action::Dash);
    let x = c.pos.x;
    for _ in 0..30 { c.step(&lib, &Intent::default(), dt); }
    assert!(c.pos.x - x > 1.5, "dash should cover ground");
    assert_eq!(c.action, Action::Idle);
    c.step(&lib, &Intent { swing: true, ..Default::default() }, dt);
    assert_eq!(c.action, Action::Swing);
    let mut saw_hit = false;
    for _ in 0..40 { c.step(&lib, &Intent::default(), dt); saw_hit |= c.hit_active; }
    assert!(saw_hit);
    assert_eq!(c.action, Action::Idle);
}

#[test]
fn render_draws_figure() {
    let mut g = Game::new(640, 360);
    let mut i = Input::default();
    g.update(0.1, &mut i);
    g.render();
    assert!(g.fb.color.iter().any(|&c| c == crate::raster::C_BODY));
    let mut rgba = vec![0u8; 640 * 360 * 4];
    g.fb.write_rgba(&mut rgba);
    assert_eq!(rgba[3], 255);
}

#[test]
fn click_to_move_reaches_target() {
    let mut g = Game::new(640, 360);
    let mut i = Input { move_click: Some((400.0, 260.0)), ..Default::default() };
    let goal = g.cam.unproject_ground(400.0, 260.0);
    for _ in 0..600 { g.update(1.0 / 60.0, &mut i); }
    let (dx, dz) = (g.player.pos.x - goal.0, g.player.pos.z - goal.1);
    assert!(dx * dx + dz * dz < 0.2);
}
