// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

use crate::actions::*;
use crate::anim::*;
use crate::body::*;
use crate::camera::Camera;
use crate::character::*;
use crate::color::{ColorSlot, RampMap};
use crate::gait::run_blend;
use crate::game::{Game, Input};
use crate::gear::*;
use crate::math::v3;
use crate::raster::*;
use crate::rig::{self, Joint};

fn setup() -> (AnimLibrary, ActionTable, GearLib) {
    (AnimLibrary::builtin(), ActionTable::builtin(), GearLib::builtin())
}

fn hero(g: &GearLib, t: &ActionTable) -> Character {
    let mut d = CharacterDef::new(Sex::Male);
    d.equip(GearSlot::MainHand, g.index("sword").map(|base| GearRef { base, look_seed: 0 }));
    Character::new(0.0, 0.0, d, g, t)
}

// ---- clip parsing ----

#[test]
fn builtin_clips_parse_and_actions_resolve() {
    let (lib, t, _) = setup();
    for n in ["idle", "dash", "swing", "dodge_roll", "hit_flinch"] {
        assert!(lib.get(n).is_some(), "missing clip {n}");
    }
    for d in &t.defs {
        for c in d.stances.values() {
            assert!(lib.get(c).is_some(), "action {} uses unknown clip {c}", d.id);
        }
    }
}

#[test]
fn clip_schema_v2_fields() {
    let c = Clip::from_ron(r#"(name:"c", duration:1.0, mask: Upper, speed_scale: 1.5, hold: Some((0.2, 0.6)),
        stance: Some("staff"), keys:[(t:0.0, pose:{"r_wpn_pitch":10.0})],
        events:[(t:0.7, name:"release"), (t:0.7, name:"fx_burst")])"#).unwrap();
    assert_eq!(c.meta.mask, MaskKind::Upper);
    assert_eq!(c.meta.hold, Some((0.2, 0.6)));
    assert_eq!(c.meta.speed_scale, 1.5);
    assert_eq!(c.sample(0.0).get(Ch::RWpnPitch), 10.0);
}

#[test]
fn bad_clips_are_errors() {
    assert!(Clip::from_ron(r#"(name:"x", duration:1.0, keys:[(t:0.0, pose:{"tail":1.0})])"#).is_err());
    assert!(Clip::from_ron(r#"(name:"x", duration:1.0, keys:[(t:0.0, pose:{"sword_yaw":1.0})])"#).is_err());
    assert!(Clip::from_ron(r#"(name:"x", duration:1.0, keys:[(t:0.0, pose:{})], events:[(t:0.1, name:"oops")])"#).is_err());
    assert!(Clip::from_ron(r#"(name:"x", duration:1.0, keys:[(t:0.0, pose:{})], events:[(t:0.1, name:"hit_start")])"#).is_err());
}

#[test]
fn sampling_interpolates_and_loops() {
    let c = Clip::from_ron(r#"(name:"x", duration:1.0, looping:true, keys:[(t:0.0, pose:{"lean":0.0}),(t:0.5, pose:{"lean":10.0})])"#).unwrap();
    assert_eq!(c.sample(0.0).get(Ch::Lean), 0.0);
    assert_eq!(c.sample(0.5).get(Ch::Lean), 10.0);
    assert!((c.sample(0.25).get(Ch::Lean) - 5.0).abs() < 1e-4);
    assert!(c.sample(1.0).get(Ch::Lean).abs() < 1e-4); // wraps
}

// ---- events-only loading ----

#[test]
fn events_only_meta_matches_full_clips() {
    let lib = AnimLibrary::builtin();
    for src in BUILTIN_CLIPS {
        let m = ClipMeta::from_ron(src).unwrap();
        assert_eq!(&lib.get(&m.name).unwrap().meta, &m);
    }
    // keys are not needed (and not parsed) for metadata
    let m = ClipMeta::from_ron(r#"(name:"m", duration:0.5, events:[(t:0.1, name:"hit_start"), (t:0.2, name:"hit_end")])"#).unwrap();
    assert!(m.event_between("hit_start", 0.0, 0.15));
    assert!(!m.event_between("hit_end", 0.0, 0.15));
}

// ---- blending ----

#[test]
fn shortest_arc_blend() {
    assert_eq!(wrap_deg(360.0), 0.0);
    assert_eq!(wrap_deg(-180.0), 180.0);
    assert_eq!(wrap_deg(190.0), -170.0);
    let mut a = Pose::default();
    a.set(Ch::RootPitch, 350.0);
    let b = Pose::default();
    // 350 -> 0 goes forward +10, not back -350
    let mid = a.blend(&b, 0.5).get(Ch::RootPitch);
    assert!((wrap_deg(mid) - (-5.0)).abs() < 1e-3, "mid {mid}");
    // bob is linear
    a.set(Ch::Bob, 1.0);
    assert!((a.blend(&b, 0.5).get(Ch::Bob) - 0.5).abs() < 1e-6);
}

#[test]
fn masks_compose_layers() {
    let mut base = Pose::default();
    base.set(Ch::LHip, 30.0);
    base.set(Ch::Lean, 5.0);
    let mut up = Pose::default();
    up.set(Ch::LHip, -10.0);
    up.set(Ch::RShoulder, 90.0);
    up.set(Ch::Lean, 3.0);
    let r = compose(&base, &up, MaskKind::Upper, 1.0);
    assert_eq!(r.get(Ch::LHip), 30.0); // legs keep the base
    assert_eq!(r.get(Ch::RShoulder), 90.0); // arms from the clip
    assert_eq!(r.get(Ch::Lean), 8.0); // shared channels add
    let f = compose(&base, &up, MaskKind::Full, 1.0);
    assert_eq!(f.get(Ch::LHip), -10.0);
    assert_eq!(f.get(Ch::Lean), 3.0);
    let a = compose(&base, &up, MaskKind::Additive, 1.0);
    assert_eq!(a.get(Ch::LHip), 20.0);
}

// ---- stance lookup / fallback ----

#[test]
fn stance_fallback_chain() {
    let t = ActionTable::from_ron(r#"[
        (id:"idle", mode: Idle, stances:{"any":"idle"}),
        (id:"walk", mode: Gait(speed: 2.0)), (id:"run", mode: Gait(speed: 5.0)),
        (id:"slash", mode: OneShot(next:"idle"), stances:{"1h":"slash_1h","2h":"slash_2h","any":"slash_any"}),
        (id:"backstab", mode: OneShot(next:"idle"), stances:{"dual":"backstab"}),
    ]"#).unwrap();
    let s = t.get(t.id("slash").unwrap());
    assert_eq!(s.clip_for("2h"), Some("slash_2h"));
    assert_eq!(s.clip_for("staff"), Some("slash_any"));
    let b = t.get(t.id("backstab").unwrap());
    assert_eq!(b.clip_for("dual"), Some("backstab"));
    assert_eq!(b.clip_for("1h"), None); // unavailable
}

#[test]
fn stance_derives_from_gear() {
    let g = GearLib::builtin();
    let get = |n: &str| g.get(g.index(n).unwrap());
    assert_eq!(derive_stance(None, None), "unarmed");
    assert_eq!(derive_stance(get("sword"), None), "1h");
    assert_eq!(derive_stance(get("sword"), get("shield")), "1h_shield");
    assert_eq!(derive_stance(get("sword"), get("sword")), "dual");
    assert_eq!(derive_stance(get("greatsword"), None), "2h");
}

#[test]
fn bad_action_table_is_error() {
    assert!(ActionTable::from_ron(r#"[(id:"idle", mode: Idle)]"#).is_err()); // missing walk/run
    assert!(ActionTable::from_ron(r#"[(id:"idle", mode: Idle),(id:"walk", mode: Gait(speed:1.0)),
        (id:"run", mode: Gait(speed:2.0)),(id:"x", mode: OneShot(next:"nope"))]"#).is_err());
}

// ---- seed determinism ----

#[test]
fn seed_look_is_deterministic() {
    let g = GearLib::builtin();
    let a = Archetype::from_ron(EXAMPLE_ARCHETYPE).unwrap();
    let d1 = CharacterDef::from_seed(42, &a, &g).unwrap();
    let d2 = CharacterDef::from_seed(42, &a, &g).unwrap();
    assert_eq!(d1, d2);
    let distinct = (0..32u64).map(|s| CharacterDef::from_seed(s, &a, &g).unwrap()).filter(|d| *d != d1).count();
    assert!(distinct >= 30, "seeds should vary looks");
    let h = d1.body.height;
    assert!((1.58..=1.95).contains(&h), "height {h}");
}

// ---- rig ----

#[test]
fn figure_is_about_80px_tall() {
    let lib = AnimLibrary::builtin();
    let pose = lib.get("idle").unwrap().sample(0.0);
    let b = BodyParams::preset(Sex::Male);
    let j = rig::solve(&pose, &b, v3(0.0, 0.0, 0.0), 0.0, 0.0);
    let cam = Camera::new(640, 360);
    let top = cam.project(j.get(Joint::Head)).y - b.head_r * crate::camera::HEIGHT_PX;
    let foot = cam.project(j.get(Joint::LFoot)).y.max(cam.project(j.get(Joint::RFoot)).y);
    let h = foot - top;
    assert!((72.0..=88.0).contains(&h), "height {h}px");
}

#[test]
fn widths_are_full_widths() {
    let b = BodyParams::preset(Sex::Male);
    let j = rig::solve(&Pose::default(), &b, v3(0.0, 0.0, 0.0), 0.0, 0.0);
    let w = (j.get(Joint::RShoulder) - j.get(Joint::LShoulder)).len();
    assert!((w - b.shoulder_w).abs() < 1e-4);
    let f = BodyParams::preset(Sex::Female);
    assert!(f.height < b.height && f.hip_w > b.hip_w);
}

#[test]
fn root_pitch_rolls_whole_body_and_head_stays_up() {
    let b = BodyParams::preset(Sex::Male);
    let mut p = Pose::default();
    p.set(Ch::RootPitch, 180.0);
    let j = rig::solve(&p, &b, v3(0.0, 0.0, 0.0), 0.0, 0.0);
    assert!(j.get(Joint::Head).y < j.get(Joint::LFoot).y, "upside down at 180");
    assert!(j.p.iter().all(|q| q.y >= 0.0199));
    // a full lean flip no longer points the head down
    let mut l = Pose::default();
    l.set(Ch::Lean, 360.0);
    let j = rig::solve(&l, &b, v3(0.0, 0.0, 0.0), 0.0, 0.0);
    assert!(j.get(Joint::Head).y > j.get(Joint::Neck).y);
}

// ---- gait ----

#[test]
fn gait_thresholds() {
    let b = BodyParams::preset(Sex::Male);
    let leg = b.thigh + b.shin;
    assert_eq!(run_blend(2.2, leg), 0.0);
    assert_eq!(run_blend(5.2, leg), 1.0);
    let f = BodyParams::preset(Sex::Female);
    assert_eq!(run_blend(2.2, f.thigh + f.shin), 0.0);
}

// ---- state machine ----

#[test]
fn state_machine_walk_run_dash_swing() {
    let (lib, t, g) = setup();
    let mut c = hero(&g, &t);
    let dt = 1.0 / 60.0;
    let walk = Intent { dir: (1.0, 0.0), ..Default::default() };
    c.step(&lib, &t, &walk, dt);
    assert_eq!(c.action_id(&t), "walk");
    c.step(&lib, &t, &Intent { run: true, ..walk }, dt);
    assert_eq!(c.action_id(&t), "run");
    c.step(&lib, &t, &Intent { request: t.id("dash"), ..walk }, dt);
    assert_eq!(c.action_id(&t), "dash");
    let x = c.pos.x;
    for _ in 0..30 {
        c.step(&lib, &t, &Intent::default(), dt);
    }
    assert!(c.pos.x - x > 1.5, "dash should cover ground");
    assert_eq!(c.action_id(&t), "idle");
    c.step(&lib, &t, &Intent { request: t.id("swing"), ..Default::default() }, dt);
    assert_eq!(c.action_id(&t), "swing");
    let mut saw_hit = false;
    for _ in 0..40 {
        c.step(&lib, &t, &Intent::default(), dt);
        saw_hit |= c.hit_active();
    }
    assert!(saw_hit);
    assert_eq!(c.action_id(&t), "idle");
}

#[test]
fn cancel_window_from_clip_events() {
    let (lib, t, g) = setup();
    let mut c = hero(&g, &t);
    let dt = 1.0 / 60.0;
    let dash = t.id("dash").unwrap();
    c.step(&lib, &t, &Intent { request: t.id("swing"), ..Default::default() }, dt);
    // early in the swing a dash is refused...
    c.step(&lib, &t, &Intent { request: Some(dash), ..Default::default() }, dt);
    assert_eq!(c.action_id(&t), "swing");
    // ...but accepted once cancel_open (t = 0.34) has fired
    for _ in 0..21 {
        c.step(&lib, &t, &Intent::default(), dt);
    }
    assert!(c.windows.cancel);
    c.step(&lib, &t, &Intent { request: Some(dash), ..Default::default() }, dt);
    assert_eq!(c.action_id(&t), "dash");
}

#[test]
fn hit_windows_work_for_any_action_and_iframes() {
    let (lib, t, g) = setup();
    let mut c = hero(&g, &t);
    let dt = 1.0 / 60.0;
    c.step(&lib, &t, &Intent { request: t.id("dodge"), dir: (0.0, 1.0), ..Default::default() }, dt);
    assert_eq!(c.action_id(&t), "dodge");
    let mut saw = false;
    for _ in 0..40 {
        c.step(&lib, &t, &Intent::default(), dt);
        saw |= c.windows.iframes;
    }
    assert!(saw);
    assert!(!c.windows.iframes);
}

#[test]
fn heavy_weapon_swings_slower() {
    let (lib, t, g) = setup();
    let frames = |weapon: &str| {
        let mut c = hero(&g, &t);
        c.def.equip(GearSlot::MainHand, g.index(weapon).map(|base| GearRef { base, look_seed: 0 }));
        c.refresh_gear(&g);
        let dt = 1.0 / 60.0;
        c.step(&lib, &t, &Intent { request: t.id("swing"), ..Default::default() }, dt);
        let mut n = 0;
        while c.action_id(&t) == "swing" && n < 500 {
            c.step(&lib, &t, &Intent::default(), dt);
            n += 1;
        }
        n
    };
    assert!(frames("greatsword") > frames("sword"));
}

#[test]
fn chain_and_hold() {
    let lib = {
        let mut l = AnimLibrary::builtin();
        l.register_ron(r#"(name:"a1", duration:0.4, mask: Upper, keys:[(t:0.0, pose:{})], events:[(t:0.1, name:"cancel_open"), (t:0.4, name:"cancel_close")])"#).unwrap();
        l.register_ron(r#"(name:"a2", duration:0.4, mask: Upper, keys:[(t:0.0, pose:{})])"#).unwrap();
        l.register_ron(r#"(name:"cast", duration:1.0, mask: Upper, hold: Some((0.2, 0.4)), keys:[(t:0.0, pose:{})], events:[(t:0.6, name:"release")])"#).unwrap();
        l
    };
    let t = ActionTable::from_ron(r#"[
        (id:"idle", mode: Idle, stances:{"any":"idle"}),
        (id:"walk", mode: Gait(speed: 2.0)), (id:"run", mode: Gait(speed: 5.0)),
        (id:"atk1", mode: OneShot(next:"idle"), stances:{"any":"a1"}, chain: Some("atk2")),
        (id:"atk2", mode: OneShot(next:"idle"), stances:{"any":"a2"}),
        (id:"cast", mode: Cast(next:"idle"), stances:{"any":"cast"}),
    ]"#).unwrap();
    let g = GearLib::builtin();
    let mut c = Character::new(0.0, 0.0, CharacterDef::new(Sex::Female), &g, &t);
    let dt = 1.0 / 60.0;
    let a1 = t.id("atk1");
    c.step(&lib, &t, &Intent { request: a1, ..Default::default() }, dt);
    for _ in 0..10 {
        c.step(&lib, &t, &Intent::default(), dt);
    }
    c.step(&lib, &t, &Intent { request: a1, ..Default::default() }, dt);
    assert_eq!(c.action_id(&t), "atk2");
    for _ in 0..30 {
        c.step(&lib, &t, &Intent::default(), dt);
    }
    c.step(&lib, &t, &Intent { request: t.id("cast"), ..Default::default() }, dt);
    for _ in 0..200 {
        c.step(&lib, &t, &Intent::default(), dt);
    }
    assert_eq!(c.action_id(&t), "cast", "held until released");
    for _ in 0..80 {
        c.step(&lib, &t, &Intent { release: true, ..Default::default() }, dt);
    }
    assert_eq!(c.action_id(&t), "idle");
}

// ---- colour ----

#[test]
fn ramp_map_team_flash_tint() {
    let m = RampMap::default();
    assert_eq!(m.color(ColorSlot::Body, 4), shade(R_BONE, 4));
    let team = m.team(ramp_id("team_blue").unwrap());
    assert_eq!(team.ramp(ColorSlot::Accent), ramp_id("team_blue").unwrap());
    assert_eq!(team.ramp(ColorSlot::Body), R_BONE);
    let f = m.flashed();
    assert_eq!(f.ramp(ColorSlot::Body), R_WHITE);
    assert_eq!(f.ramp(ColorSlot::Outline), R_VOID);
    let t = m.tint(ramp_id("fire").unwrap());
    assert_eq!(t.ramp(ColorSlot::BodyFar), ramp_id("fire").unwrap());
    assert_eq!(LUT[shade(R_BONE, 4) as usize], 0xe8e2d0);
}

#[test]
fn render_draws_figure_with_outline_toggle() {
    let mut g = Game::new(640, 360);
    let mut i = Input::default();
    g.update(0.1, &mut i);
    g.render();
    let body = shade(R_BONE, 4);
    assert!(g.fb.color.contains(&body));
    assert!(g.fb.color.contains(&shade(R_STEEL, 3)), "sword drawn from gear");
    let outlined = g.fb.color.iter().filter(|&&c| c == C_OUTLINE).count();
    g.opts.outline = false;
    g.render();
    let plain = g.fb.color.iter().filter(|&&c| c == C_OUTLINE).count();
    assert!(outlined > plain);
    let mut rgba = vec![0u8; 640 * 360 * 4];
    g.fb.write_rgba(&mut rgba);
    assert_eq!(rgba[3], 255);
}

#[test]
fn click_to_move_reaches_target() {
    let mut g = Game::new(640, 360);
    let mut i = Input { move_click: Some((400.0, 260.0)), ..Default::default() };
    let goal = g.cam.unproject_ground(400.0, 260.0);
    for _ in 0..600 {
        g.update(1.0 / 60.0, &mut i);
    }
    let (dx, dz) = (g.player.pos.x - goal.0, g.player.pos.z - goal.1);
    assert!(dx * dx + dz * dz < 0.2);
}

#[test]
fn archetype_ignores_game_specific_fields() {
    let a = Archetype::from_ron(r#"(id: "knight", stances: ["2h"], silhouette: ["crest_tall"], idle_clip: "idle_stoic")"#);
    assert!(a.is_ok(), "{a:?}");
}

#[test]
fn hair_options_apply_cycle_and_roundtrip() {
    use crate::body::{HairStyle, ALL_HAIR};
    use crate::color::ColorSlot;
    use crate::custom::HairOptions;
    let mut d = CharacterDef::new(Sex::Male);
    let mut h = HairOptions::default();
    for _ in 0..ALL_HAIR.len() {
        h.cycle_style(1);
    }
    assert_eq!(h.hair, HairStyle::None, "style cycle wraps");
    h.cycle_style(-1);
    assert_eq!(h.hair, HairStyle::Pixie);
    h.cycle_color(1);
    h.apply(&mut d);
    assert_eq!(d.body.hair, HairStyle::Pixie);
    assert_eq!(d.colors.ramp(ColorSlot::Hair), crate::raster::ramp_id("hair_blonde").unwrap());
    let back = HairOptions::from_ron(&h.to_ron()).unwrap();
    assert_eq!(back, h);
    let bad = HairOptions::from_ron("(hair: Bob, hair_color: \"nope\")").unwrap();
    assert_eq!(bad.hair_color, "hair_black");
}

#[test]
fn every_hair_style_draws_pixels_in_the_hair_colour() {
    use crate::body::ALL_HAIR;
    use crate::color::{ColorSlot, RampMap};
    let game = crate::game::Game::new(crate::WIDTH, crate::HEIGHT);
    let j = game.player_joints();
    let map = RampMap::default().with(ColorSlot::Hair, crate::raster::ramp_id("hair_red").unwrap());
    let hc = map.color(ColorSlot::Hair, 3);
    for s in ALL_HAIR {
        let mut b = game.player.def.effective_body(&game.gear);
        b.hair = s;
        let mut fb = crate::raster::Framebuffer::new(crate::WIDTH, crate::HEIGHT);
        crate::render::draw_body(&mut fb, &game.cam, &j, &b, &map, &crate::render::DrawOpts::default());
        let n = fb.color.iter().filter(|&&c| c == hc).count();
        if s == crate::body::HairStyle::None { assert_eq!(n, 0) } else { assert!(n > 0, "{s:?} drew nothing") }
    }
}

#[test]
fn swaying_hair_moves_and_stays_bounded() {
    let mut g = crate::game::Game::new(crate::WIDTH, crate::HEIGHT);
    g.hair.hair = crate::body::HairStyle::Ponytail;
    g.hair.apply(&mut g.player.def);
    let mut i = crate::game::Input { right: true, run: true, ..Default::default() };
    let mut moved = false;
    for _ in 0..120 {
        g.update(1.0 / 60.0, &mut i);
        let s = g.player.hair_sway();
        assert!(s.x.abs() <= 0.036 && s.y.abs() <= 0.036 && s.z.abs() <= 0.036);
        moved |= s.x != 0.0 || s.y != 0.0 || s.z != 0.0;
    }
    assert!(moved);
}

mod perspective {
    use crate::camera::{Camera, View, ISO_SCALE};
    use crate::camrig::{CamConfig, CamMode, CamRig, Subject};
    use crate::game::{Game, Input};
    use crate::math::{v3, Vec3};
    use crate::raster::Framebuffer;

    fn tp(target: Vec3, yaw: f32, dist: f32) -> View {
        View { target, yaw, pitch: 30f32.to_radians(), inv_d: 1.0 / dist, scale: ISO_SCALE, skew: 0.0, ax: 320.0, ay: 198.0 }
    }
    fn pts() -> Vec<Vec3> {
        let mut v = Vec::new();
        for i in -3i32..=3 {
            for k in -3..=3 {
                v.push(v3(i as f32 * 1.7, (i + k).rem_euclid(3) as f32 * 0.6, k as f32 * 1.3 + 0.4));
            }
        }
        v
    }

    #[test]
    fn ortho_view_at_t0_matches_legacy_iso() {
        let legacy = Camera { cx: 1.5, cz: -2.0, ..Camera::new(640, 360) };
        let mut c = legacy;
        c.view = Some(View::iso(1.5, -2.0, 640.0, 360.0));
        let tpv = tp(v3(1.5, 1.1, -2.0), 0.7, 6.0);
        let at0 = View::iso(1.5, -2.0, 640.0, 360.0).lerp(&tpv, 0.0);
        let mut c0 = legacy;
        c0.view = Some(at0);
        for p in pts() {
            let (a, b, e) = (legacy.project(p), c.project(p), c0.project(p));
            for q in [b, e] {
                assert!((a.x - q.x).abs() < 1e-3 && (a.y - q.y).abs() < 1e-3, "{p:?}: {a:?} vs {q:?}");
                assert_eq!(q.s, 1.0);
            }
        }
        // Iso-mode game frames are the legacy frames: the rig leaves `view` unset.
        let mut g = Game::new(640, 360);
        g.update(0.5, &mut Input::default());
        assert!(g.cam.view.is_none());
    }

    #[test]
    fn perspective_projection_math() {
        let v = tp(v3(0.0, 1.0, 0.0), 0.0, 5.0);
        let c = Camera { view: Some(v), ..Camera::new(640, 360) };
        let p = c.project(v.target);
        assert!((p.x - 320.0).abs() < 1e-3 && (p.y - 198.0).abs() < 1e-3 && (p.s - 1.0).abs() < 1e-5);
        // A point 1 m to the screen-right of the target on the focus plane lands `scale` px right.
        let (r, _) = v.basis();
        let q = c.project(v.target + r * (1.0 / ISO_SCALE));
        assert!((q.x - 320.0 - ISO_SCALE).abs() < 1e-2, "{q:?}");
        // Pinhole check: x offset = focal * lateral / eye distance.
        let eye = v.eye().unwrap();
        let f = v.fwd();
        let far = eye + f * 10.0 + r * (2.0 / ISO_SCALE);
        let pf = c.project(far);
        let focal = ISO_SCALE * 5.0;
        assert!((pf.x - 320.0 - focal * 2.0 / 10.0).abs() < 1e-2, "{pf:?}");
        // Depth sorts by view z; behind the eye is culled.
        assert!(c.project(eye + f * 3.0).d < c.project(eye + f * 8.0).d);
        assert!(c.project(eye - f * 1.0).culled());
        assert!(c.project(eye + f * 0.01).culled(), "inside the near plane");
    }

    #[test]
    fn dolly_zoom_keeps_subject_size() {
        let iso = View::iso(0.0, 0.0, 640.0, 360.0);
        let to = tp(v3(0.0, 0.0, 0.0), iso.yaw, 6.0);
        let mut last = None;
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            // Pure dolly (direction fixed): focus-plane size is constant.
            let dv = View { inv_d: t / 6.0, ..to };
            let c = Camera { view: Some(dv), ..Camera::new(640, 360) };
            let (_, dn) = dv.basis();
            let up = dn * (-1.0 / ISO_SCALE);
            let (a, b) = (c.project(v3(0.0, 0.0, 0.0)), c.project(up * 1.8));
            let size = (a.y - b.y).hypot(a.x - b.x);
            if let Some(l) = last {
                assert!((size - l as f32).abs() < 1e-2, "t={t}: {size} vs {l}");
            }
            last = Some(size);
            // Radius scale at the focus stays 1 through the real iso -> 3D blend too.
            let bl = iso.lerp(&to, t);
            let cb = Camera { view: Some(bl), ..Camera::new(640, 360) };
            assert!((cb.project(bl.target).s - 1.0).abs() < 1e-4);
            // The camera moves back as t -> 0 (distance = 1 / inv_d grows).
            assert!(bl.inv_d <= to.inv_d + 1e-6);
        }
    }

    #[test]
    fn radius_scales_with_one_over_z() {
        let v = tp(v3(0.0, 1.0, 0.0), 0.3, 4.0);
        let c = Camera { view: Some(v), ..Camera::new(640, 360) };
        let (eye, f) = (v.eye().unwrap(), v.fwd());
        let (n, fr) = (c.project(eye + f * 4.0), c.project(eye + f * 8.0));
        assert!((n.s / fr.s - 2.0).abs() < 1e-3, "{} {}", n.s, fr.s);
        let ink = |p: crate::camera::P2| {
            let mut fb = Framebuffer::new(640, 360);
            fb.capsule(p, p, 6.0, 7, None);
            fb.depth.iter().filter(|d| d.is_finite()).count() as f32
        };
        let ratio = ink(n) / ink(fr);
        assert!((ratio - 4.0).abs() < 0.6, "area ~ r^2: {ratio}");
        // Behind the camera: nothing drawn.
        let mut fb = Framebuffer::new(640, 360);
        fb.capsule(c.project(eye - f), c.project(eye + f * 3.0), 6.0, 7, None);
        assert!(fb.depth.iter().all(|d| d.is_infinite()));
    }

    #[test]
    fn ground_picking_round_trips_in_every_mode() {
        let s = Subject { pos: v3(1.0, 0.0, 2.0), yaw: 0.4, head: v3(1.0, 1.7, 2.0) };
        let mut rig = CamRig::new(CamConfig::default());
        for mode in [CamMode::Iso, CamMode::ThirdPerson, CamMode::FirstPerson] {
            rig.snap(mode, &s);
            rig.update(0.0, 0.3, &s, 1.0, 2.0, 640.0, 360.0);
            let v = rig.target_view(mode, &s, 1.0, 2.0, 640.0, 360.0);
            let c = Camera { cx: 1.0, cz: 2.0, view: Some(v), ..Camera::new(640, 360) };
            let fh = v3(v.yaw.sin(), 0.0, v.yaw.cos());
            for (dx, dz) in [(0.0, 0.0), (1.5, -0.5), (-2.0, 1.0)] {
                let g = s.pos + fh * 4.0 + v3(dx, 0.0, dz);
                let p = c.project(g);
                assert!(!p.culled());
                let (x, z) = c.pick_ground(p.x, p.y).expect("ground under pixel");
                assert!((x - g.x).abs() < 1e-2 && (z - g.z).abs() < 1e-2, "{mode:?}: {:?} vs {g:?}", (x, z));
            }
            if mode != CamMode::Iso {
                assert!(c.pick_ground(320.0, 0.0).is_none() || mode == CamMode::ThirdPerson, "sky above the FP horizon");
            }
        }
    }

    #[test]
    fn cycle_key_and_camera_relative_movement() {
        let mut g = Game::new(640, 360);
        let mut i = Input { cam_cycle: true, ..Input::default() };
        g.update(1.0 / 60.0 + 1e-4, &mut i);
        assert_eq!(g.rig.mode, CamMode::ThirdPerson);
        for _ in 0..60 {
            g.update(1.0 / 60.0, &mut Input::default()); // blend settles
        }
        assert!(g.cam.view.is_some() && g.rig.t >= 1.0);
        // "Up" walks along the camera's forward on the ground plane.
        let (yaw, p0) = (g.rig.yaw, g.player.pos);
        let mut i = Input { up: true, ..Input::default() };
        for _ in 0..60 {
            g.update(1.0 / 60.0, &mut i);
        }
        let d = g.player.pos - p0;
        let along = d.x * yaw.sin() + d.z * yaw.cos();
        assert!(along > 0.5 && (d.x * yaw.cos() - d.z * yaw.sin()).abs() < 0.3 * along, "{d:?}");
        assert_eq!(g.player.pos.y, 0.0, "still on the ground plane");
        for want in [CamMode::FirstPerson, CamMode::Iso] {
            let mut i = Input { cam_cycle: true, ..Input::default() };
            g.update(1.0 / 60.0 + 1e-4, &mut i);
            assert_eq!(g.rig.mode, want);
            for _ in 0..90 {
                g.update(1.0 / 60.0, &mut Input::default());
            }
        }
        assert!(g.cam.view.is_none(), "back to the legacy iso path");
        g.render();
    }

    /// Player capsules (segment ends + radius, metres) and the head centre.
    fn capsules(g: &Game) -> (Vec<(Vec3, Vec3, f32)>, Vec3) {
        use crate::camera::HEIGHT_PX;
        use crate::rig::{Bone, Joint, ALL_BONES};
        let j = g.player_joints();
        let b = g.player.def.body;
        let mut v = Vec::new();
        for bone in ALL_BONES {
            let (a, z) = bone.ends();
            let r = match bone {
                Bone::Head => b.head_r * b.bone_thickness_mul[Bone::Head as usize],
                Bone::Spine => (b.limb_radius(bone) + 0.6) / HEIGHT_PX,
                _ => b.limb_radius(bone) / HEIGHT_PX,
            };
            if bone == Bone::Head {
                v.push((j.get(Joint::Head), j.get(Joint::Head), r));
                v.push((j.get(a), j.get(Joint::Head), b.limb_radius(Bone::Neck) / HEIGHT_PX));
            } else {
                v.push((j.get(a), j.get(z), r));
            }
        }
        (v, j.get(Joint::Head))
    }

    fn seg_dist(p: Vec3, a: Vec3, b: Vec3) -> f32 {
        let ab = b - a;
        let l2 = ab.x * ab.x + ab.y * ab.y + ab.z * ab.z;
        let t = if l2 < 1e-9 { 0.0 } else { ((p - a).x * ab.x + (p - a).y * ab.y + (p - a).z * ab.z) / l2 };
        let t = t.clamp(0.0, 1.0);
        (p - (a + ab * t)).len()
    }

    /// Sample the eye along every mode transition (including the chained
    /// FP <-> iso move and interrupted blends) and check it never enters a
    /// player capsule. Inside FP_HIDE_RADIUS of the head the body is not
    /// drawn (the first-person endpoint), so those samples are exempt, and
    /// the body must be drawn whenever the eye is outside that zone.
    /// Extra clearance beyond the drawn capsule radius (metres): the drawn
    /// limbs are thin, so the eye must also stay out of the space they span.
    const CLEAR: f32 = 0.15;

    #[test]
    fn transitions_never_pass_through_the_body() {
        use crate::camrig::FP_HIDE_RADIUS;
        let modes = [CamMode::Iso, CamMode::ThirdPerson, CamMode::FirstPerson];
        for pitch in [-1.0f32, 0.12, 1.0] {
            for yaw in [0.0f32, 2.5, -3.1] {
                for from in modes {
                    for to in modes {
                        if from == to {
                            continue;
                        }
                        let mut g = Game::new(640, 360);
                        g.player.yaw = yaw;
                        g.rig.fp_pitch = pitch;
                        let s = g.subject_for_tests();
                        g.rig.snap(from, &s);
                        g.rig.fp_pitch = pitch;
                        g.set_cam_mode(to);
                        let (caps, head) = capsules(&g);
                        let mut n = 0;
                        for _ in 0..400 {
                            g.step_camera_for_tests(1.0 / 240.0);
                            if let Some(e) = g.cam.view.and_then(|v| v.eye()) {
                                if (e - head).len() < FP_HIDE_RADIUS {
                                    continue;
                                }
                                n += 1;
                                for (a, b, r) in &caps {
                                    let d = seg_dist(e, *a, *b);
                                    assert!(d > *r + CLEAR, "{from:?}->{to:?} pitch {pitch} yaw {yaw}: eye {e:?} {d} m from a capsule of radius {r}");
                                }
                            }
                            if g.rig.t >= 1.0 {
                                break;
                            }
                        }
                        assert!(g.rig.t >= 1.0 && g.rig.mode == to);
                        assert!(n > 10, "{from:?}->{to:?}: eye sampled outside the head zone ({n})");
                    }
                }
            }
        }
    }

    /// Regression: the old direct FP -> iso view lerp slid the eye down the
    /// front of the body; the clearance check above must reject that path.
    #[test]
    fn old_direct_fp_to_iso_lerp_is_rejected() {
        let g = Game::new(640, 360);
        let s = g.subject_for_tests();
        let mut rig = CamRig::new(CamConfig::default());
        rig.snap(CamMode::FirstPerson, &s);
        let fp = rig.target_view(CamMode::FirstPerson, &s, 0.0, 0.0, 640.0, 360.0);
        let iso = View::iso(0.0, 0.0, 640.0, 360.0);
        let (caps, head) = capsules(&g);
        let hit = (0..100).filter_map(|k| fp.lerp(&iso, k as f32 / 100.0).eye()).any(|e| {
            (e - head).len() >= crate::camrig::FP_HIDE_RADIUS && caps.iter().any(|(a, b, r)| seg_dist(e, *a, *b) <= r + CLEAR)
        });
        assert!(hit);
    }

    /// FP -> iso is one eased move with no stall: the rail position falls
    /// monotonically and its per-step speed only drops to zero at the ends.
    #[test]
    fn fp_to_iso_is_one_continuous_move() {
        let mut g = Game::new(640, 360);
        let s = g.subject_for_tests();
        g.rig.snap(CamMode::FirstPerson, &s);
        g.set_cam_mode(CamMode::Iso);
        let mut rs = vec![g.rig.r];
        while g.rig.t < 1.0 {
            g.step_camera_for_tests(1.0 / 240.0);
            rs.push(g.rig.r);
        }
        assert_eq!(*rs.last().unwrap(), 0.0);
        assert!(g.cam.view.is_none(), "settles on the legacy iso path");
        let sp: Vec<f32> = rs.windows(2).map(|w| w[0] - w[1]).collect();
        assert!(sp.iter().all(|d| *d >= 0.0));
        let mid = sp.len() / 2;
        let peak = sp.iter().cloned().fold(0.0, f32::max);
        // Crossing TP (r = 1) at full speed, not stopping there.
        let at_tp = rs.iter().position(|r| *r <= 1.0).unwrap();
        assert!(sp[at_tp.min(sp.len() - 1)] > 0.8 * peak, "stall at the third-person midpoint");
        assert!(sp[mid] > 0.8 * peak);
        assert!(sp[0] < 0.1 * peak && sp[sp.len() - 1] < 0.1 * peak, "eased ends");
    }

    #[test]
    fn third_person_yaw_follows_shortest_arc_across_the_wrap() {
        let s = Subject { pos: v3(0.0, 0.0, 0.0), yaw: -3.0, head: v3(0.0, 1.7, 0.0) };
        let mut rig = CamRig::new(CamConfig { mode: CamMode::ThirdPerson, ..CamConfig::default() });
        rig.snap(CamMode::ThirdPerson, &s);
        rig.yaw = 3.0; // 0.28 rad away across +-PI, not 6 rad the long way
        let mut prev = crate::math::angle_diff(rig.yaw, s.yaw).abs();
        for _ in 0..120 {
            rig.update(1.0 / 60.0, 0.0, &s, 0.0, 0.0, 640.0, 360.0);
            let d = crate::math::angle_diff(rig.yaw, s.yaw).abs();
            assert!(d <= prev + 1e-6, "monotone follow");
            assert!(rig.yaw.abs() > 2.8, "stays near +-PI, never spins through 0: {}", rig.yaw);
            prev = d;
        }
        assert!(prev < 1e-3);
        // Smoothed, not snapped: one frame covers only part of a big turn.
        let s2 = Subject { yaw: 0.0, ..s };
        rig.yaw = 1.5;
        rig.update(1.0 / 60.0, 0.0, &s2, 0.0, 0.0, 640.0, 360.0);
        assert!(rig.yaw > 1.0 && rig.yaw < 1.5);
    }

    #[test]
    fn first_person_yaw_is_locked_to_facing() {
        let s = Subject { pos: v3(0.0, 0.0, 0.0), yaw: 3.1, head: v3(0.0, 1.7, 0.0) };
        let mut rig = CamRig::new(CamConfig::default());
        rig.snap(CamMode::FirstPerson, &s);
        for y in [3.1f32, -3.1, 0.5, -1.2] {
            let s = Subject { yaw: y, ..s };
            rig.update(1.0 / 60.0, 1.0, &s, 0.0, 0.0, 640.0, 360.0);
            assert_eq!(rig.yaw, y);
            assert_eq!(rig.view.unwrap().yaw, y);
        }
        assert!(rig.fp_pitch > 0.12, "pitch still adjustable");
    }

    #[test]
    fn turn_keys_turn_the_character_and_the_camera_follows() {
        for mode in [CamMode::ThirdPerson, CamMode::FirstPerson] {
            let mut g = Game::new(640, 360);
            g.set_cam_config(CamConfig { mode, ..CamConfig::default() });
            let y0 = g.player.yaw;
            let mut i = Input { right: true, ..Input::default() };
            for _ in 0..30 {
                g.update(1.0 / 60.0, &mut i);
            }
            let turned = crate::math::angle_diff(y0, g.player.yaw);
            assert!(turned < -0.5, "{mode:?}: D turns right ({turned})");
            assert!(g.player.pos.x.abs() + g.player.pos.z.abs() < 0.05, "turning in place");
            for _ in 0..60 {
                g.update(1.0 / 60.0, &mut Input::default());
            }
            assert!(crate::math::angle_diff(g.rig.yaw, g.player.yaw).abs() < 0.02, "{mode:?}: camera follows");
            // W walks along the facing, S backs up without turning around.
            let (yaw, p0) = (g.player.yaw, g.player.pos);
            let mut i = Input { down: true, ..Input::default() };
            for _ in 0..40 {
                g.update(1.0 / 60.0, &mut i);
            }
            let d = g.player.pos - p0;
            assert!(d.x * yaw.sin() + d.z * yaw.cos() < -0.3, "S backs up");
            assert!(crate::math::angle_diff(yaw, g.player.yaw).abs() < 0.05, "facing kept");
        }
        // Iso: A/D still walk screen-relative and the camera stays iso.
        let mut g = Game::new(640, 360);
        let mut i = Input { right: true, ..Input::default() };
        for _ in 0..30 {
            g.update(1.0 / 60.0, &mut i);
        }
        assert!(g.cam.view.is_none() && g.player.pos.x > 0.2);
    }
}
