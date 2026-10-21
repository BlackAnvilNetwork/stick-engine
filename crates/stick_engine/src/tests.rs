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
