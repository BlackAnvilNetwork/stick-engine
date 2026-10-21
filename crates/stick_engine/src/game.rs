//! Minimal playable scene: one stick hero on a procedural tile floor.
use crate::actions::ActionTable;
use crate::anim::AnimLibrary;
use crate::body::{CharacterDef, GearRef, Sex};
use crate::camera::Camera;
use crate::character::{Character, Intent};
use crate::color::RampMap;
use crate::gear::{GearLib, GearSlot};
use crate::math::{v3, Vec3};
use crate::raster::*;
use crate::render::{self, DrawOpts};
use crate::actions::Mode;
use crate::rig;
use crate::ui::{Panel, UiCmd};
use crate::custom::{CharacterCustomization, Row, ROWS};

pub const TICK: f32 = 1.0 / 60.0;

/// Raw host input, in internal framebuffer pixels for mouse coords.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub run: bool,
    pub dash: bool,
    pub attack: bool,
    pub mouse: Option<(f32, f32)>,
    /// Move-to click (set by host, consumed by the game).
    pub move_click: Option<(f32, f32)>,
    /// Attack click position (left mouse); attack aims here.
    pub attack_click: Option<(f32, f32)>,
    /// Hotkeys pressed since the last update (bitmask of `HK_*`; set by
    /// the host on key-down, consumed by the game).
    pub hotkeys: u32,
}

/// Hotkey bit for number key `n` (1..=9): triggers the n-th panel action.
pub const fn hk_digit(n: u32) -> u32 {
    1 << (n - 1)
}
pub const HK_SEX: u32 = 1 << 9;
pub const HK_GEAR: u32 = 1 << 10;
pub const HK_UI: u32 = 1 << 11;
/// Customization screen keys (C, R, X, F5, F9, arrows/WASD edges, Enter).
pub const HK_CUSTOM: u32 = 1 << 12;
pub const HK_RANDOM: u32 = 1 << 13;
pub const HK_RESET: u32 = 1 << 14;
pub const HK_SAVE: u32 = 1 << 15;
pub const HK_LOAD: u32 = 1 << 16;
pub const HK_NAV_UP: u32 = 1 << 17;
pub const HK_NAV_DOWN: u32 = 1 << 18;
pub const HK_NAV_LEFT: u32 = 1 << 19;
pub const HK_NAV_RIGHT: u32 = 1 << 20;
pub const HK_ENTER: u32 = 1 << 21;

/// Help lines shown under the debug panel.
pub const HELP: [&str; 4] = [
    "WASD MOVE  SHIFT RUN  SPACE DASH  J SWING",
    "1-4 ACTIONS  G SEX  T GEAR  F1 PANEL",
    "C CUSTOMIZE  R RANDOM  X RESET",
    "F5 SAVE  F9 LOAD  CHARACTER.RON",
];

/// Customization screen state.
#[derive(Clone, Debug, Default)]
pub struct CustomizeUi {
    pub sel: usize,
}

/// Gear presets cycled by the T button: (label, main hand, off hand).
pub const GEAR_PRESETS: [(&str, Option<&str>, Option<&str>); 4] = [
    ("SWORD", Some("sword"), None),
    ("GREATSWORD", Some("greatsword"), None),
    ("SWORD+SHIELD", Some("sword"), Some("shield")),
    ("UNARMED", None, None),
];

struct Particle {
    p: Vec3,
    v: Vec3,
    life: f32,
    c: u8,
}

pub struct Game {
    pub lib: AnimLibrary,
    pub actions: ActionTable,
    pub gear: GearLib,
    pub player: Character,
    pub cam: Camera,
    pub fb: Framebuffer,
    pub opts: DrawOpts,
    act_dash: usize,
    act_swing: usize,
    /// Debug button panel (F1 / "UI" button toggles it).
    pub panel: Panel,
    /// Action queued by a panel button or hotkey for the next tick.
    pending: Option<usize>,
    /// Index into `GEAR_PRESETS`.
    pub gear_preset: usize,
    target: Option<(f32, f32)>,
    prev: Input,
    accum: f32,
    particles: Vec<Particle>,
    trail: Vec<(Vec3, Vec3, f32)>,
    ghosts: Vec<(rig::Joints, f32)>,
    flash: f32,
    pub time: f32,
    pub demo: bool,
    rng: u32,
    /// Cosmetic look applied to the player (body never affects gameplay).
    pub custom: CharacterCustomization,
    /// Open customization screen, if any.
    pub customize: Option<CustomizeUi>,
    /// RON text the host should write to its save file (taken by the host).
    pub save_request: Option<String>,
    /// Set when the host should read its save file and call `load_customization`.
    pub load_request: bool,
    /// Short status message and seconds left.
    pub status: Option<(String, f32)>,
    random_n: u64,
}

const PILLARS: [(f32, f32); 4] = [(4.0, -3.0), (-5.0, 2.0), (6.0, 6.0), (-3.0, -7.0)];

impl Game {
    pub fn new(w: usize, h: usize) -> Self {
        let actions = ActionTable::builtin();
        let gear = GearLib::builtin();
        let mut def = CharacterDef::new(Sex::Male);
        let sword = gear.index("sword").map(|base| GearRef { base, look_seed: 0 });
        def.equip(GearSlot::MainHand, sword);
        let player = Character::new(0.0, 0.0, def, &gear, &actions);
        let mut rows = Vec::new();
        let mut n = 1;
        for (i, d) in actions.defs.iter().enumerate() {
            if matches!(d.mode, Mode::OneShot { .. } | Mode::Cast { .. } | Mode::Additive) && n <= 9 {
                rows.push((format!("{n} {}", d.id.to_uppercase()), UiCmd::Action(i)));
                n += 1;
            }
        }
        rows.push((String::new(), UiCmd::ToggleSex));
        rows.push((String::new(), UiCmd::CycleGear));
        rows.push(("C CUSTOMIZE".into(), UiCmd::Customize));
        let mut panel = Panel::new(rows);
        panel.relabel(UiCmd::ToggleSex, "G MALE".into());
        panel.relabel(UiCmd::CycleGear, format!("T {}", GEAR_PRESETS[0].0));
        Game {
            panel,
            pending: None,
            gear_preset: 0,
            lib: AnimLibrary::builtin(),
            act_dash: actions.id("dash").expect("dash action"),
            act_swing: actions.id("swing").expect("swing action"),
            actions,
            gear,
            player,
            cam: Camera::new(w, h),
            fb: Framebuffer::new(w, h),
            opts: DrawOpts::default(),
            target: None,
            prev: Input::default(),
            accum: 0.0,
            particles: Vec::new(),
            trail: Vec::new(),
            ghosts: Vec::new(),
            flash: 0.0,
            time: 0.0,
            demo: false,
            rng: 0x1234_5678,
            custom: CharacterCustomization::default(),
            customize: None,
            save_request: None,
            load_request: false,
            status: None,
            random_n: 0,
        }
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// Scripted input for `#demo` / screenshots.
    fn demo_input(&self) -> Input {
        let t = self.time % 8.0;
        let mut i = Input::default();
        match t {
            t if t < 2.0 => i.right = true,
            t if t < 3.6 => { i.down = true; i.run = true; }
            t if t < 3.7 => { i.left = true; i.dash = true; }
            t if t < 4.5 => i.left = true,
            t if t < 4.6 => i.attack = true,
            t if t < 5.4 => {}
            t if t < 7.0 => { i.up = true; i.run = true; i.left = t < 6.0; }
            _ => {}
        }
        i
    }

    /// Advance by real elapsed seconds using a fixed 60 Hz timestep.
    pub fn update(&mut self, dt: f32, input: &mut Input) {
        self.handle_ui(input);
        self.accum = (self.accum + dt.min(0.25)).min(0.25);
        while self.accum >= TICK {
            self.accum -= TICK;
            let inp = if self.demo {
                self.demo_input()
            } else if self.customize.is_some() {
                Input::default() // screen open: the figure idles
            } else {
                *input
            };
            self.tick(&inp);
            input.move_click = None;
            input.attack_click = None;
        }
    }

    /// Route panel clicks and hotkeys. Clicks on the panel are consumed so
    /// they never also move or attack in the world.
    pub fn handle_ui(&mut self, input: &mut Input) {
        let mut cmds = Vec::new();
        if let Some((x, y)) = input.attack_click {
            if let Some(c) = self.panel.hit(x, y) {
                cmds.push(c);
                input.attack_click = None;
            }
        }
        if let Some((x, y)) = input.move_click {
            if self.panel.hit(x, y).is_some() {
                input.move_click = None;
            }
        }
        let hk = std::mem::take(&mut input.hotkeys);
        let actions: Vec<UiCmd> =
            self.panel.buttons.iter().map(|b| b.cmd).filter(|c| matches!(c, UiCmd::Action(_))).collect();
        for (i, c) in actions.into_iter().enumerate() {
            if hk & hk_digit(i as u32 + 1) != 0 {
                cmds.push(c);
            }
        }
        for (bit, c) in [
            (HK_SEX, UiCmd::ToggleSex), (HK_GEAR, UiCmd::CycleGear), (HK_UI, UiCmd::TogglePanel),
            (HK_CUSTOM, UiCmd::Customize),
        ] {
            if hk & bit != 0 {
                cmds.push(c);
            }
        }
        for c in cmds {
            self.run_cmd(c);
        }
        if hk & HK_RANDOM != 0 { self.custom_row(Row::Randomize); }
        if hk & HK_RESET != 0 { self.custom_row(Row::Reset); }
        if hk & HK_SAVE != 0 { self.custom_row(Row::Save); }
        if hk & HK_LOAD != 0 { self.custom_row(Row::Load); }
        if let Some(ui) = self.customize.as_mut() {
            let n = ROWS.len();
            if hk & HK_NAV_UP != 0 { ui.sel = (ui.sel + n - 1) % n; }
            if hk & HK_NAV_DOWN != 0 { ui.sel = (ui.sel + 1) % n; }
            let row = ROWS[ui.sel];
            let d = (hk & HK_NAV_RIGHT != 0) as i32 - (hk & HK_NAV_LEFT != 0) as i32;
            let act = matches!(row, Row::Randomize | Row::Reset | Row::Save | Row::Load | Row::Close);
            if act && (d != 0 || hk & HK_ENTER != 0) {
                self.custom_row(row);
            } else if d != 0 {
                if row == Row::Body {
                    self.custom.switch_sex();
                } else {
                    self.custom.adjust(row, d);
                }
                self.apply_custom();
            }
        }
    }

    /// Re-apply `custom` onto the player.
    pub fn apply_custom(&mut self) {
        self.custom = self.custom.clamped();
        self.custom.apply(&mut self.player.def);
        self.player.refresh_gear(&self.gear);
        let label = format!("G {}", if self.custom.sex == Sex::Male { "MALE" } else { "FEMALE" });
        self.panel.relabel(UiCmd::ToggleSex, label);
    }

    /// Load a saved customization (RON); clamps and applies it.
    pub fn load_customization(&mut self, src: &str) -> Result<(), String> {
        let c = CharacterCustomization::from_ron(src)?;
        self.custom = c;
        self.apply_custom();
        Ok(())
    }

    pub fn set_status(&mut self, s: &str) {
        self.status = Some((s.to_uppercase(), 2.0));
    }

    /// Run a customization action row (also bound to R / X / F5 / F9 / C).
    pub fn custom_row(&mut self, row: Row) {
        match row {
            Row::Randomize => {
                self.random_n += 1;
                self.custom = CharacterCustomization::random(self.player.def.seed ^ self.random_n.wrapping_mul(0x9e37_79b9));
                self.apply_custom();
            }
            Row::Reset => {
                self.custom = CharacterCustomization::for_sex(self.custom.sex);
                self.apply_custom();
            }
            Row::Save => {
                self.save_request = Some(self.custom.to_ron());
            }
            Row::Load => self.load_request = true,
            Row::Close => self.customize = None,
            _ => {}
        }
    }

    /// Execute a panel command (also usable from tests/tools).
    pub fn run_cmd(&mut self, c: UiCmd) {
        self.panel.press(c);
        match c {
            UiCmd::Action(a) => self.pending = Some(a),
            UiCmd::TogglePanel => self.panel.visible = !self.panel.visible,
            UiCmd::ToggleSex => {
                // same seed, gear and personality; other sex, sliders kept
                self.custom.switch_sex();
                self.apply_custom();
            }
            UiCmd::Customize => {
                self.customize = match self.customize {
                    Some(_) => None,
                    None => Some(CustomizeUi::default()),
                };
            }
            UiCmd::CycleGear => {
                self.gear_preset = (self.gear_preset + 1) % GEAR_PRESETS.len();
                let (label, main, off) = GEAR_PRESETS[self.gear_preset];
                let r = |n: Option<&str>| n.and_then(|n| self.gear.index(n)).map(|base| GearRef { base, look_seed: 0 });
                let (m, o) = (r(main), r(off));
                self.player.def.equip(GearSlot::MainHand, m);
                self.player.def.equip(GearSlot::OffHand, o);
                self.player.refresh_gear(&self.gear);
                self.panel.relabel(UiCmd::CycleGear, format!("T {label}"));
            }
        }
    }

    fn tick(&mut self, inp: &Input) {
        self.time += TICK;
        // screen-aligned WASD -> iso world directions
        let (mut sx, mut sy) = (0.0f32, 0.0f32);
        if inp.up { sy -= 1.0; }
        if inp.down { sy += 1.0; }
        if inp.left { sx -= 1.0; }
        if inp.right { sx += 1.0; }
        let mut dir = (sx + sy, sy - sx);
        if sx != 0.0 || sy != 0.0 {
            self.target = None;
        }
        if let Some((mx, my)) = inp.move_click {
            self.target = Some(self.cam.unproject_ground(mx, my));
        }
        if let Some((tx, tz)) = self.target {
            let (dx, dz) = (tx - self.player.pos.x, tz - self.player.pos.z);
            if dx * dx + dz * dz < 0.05 {
                self.target = None;
            } else {
                dir = (dx, dz);
            }
        }
        let attack_edge = (inp.attack && !self.prev.attack) || inp.attack_click.is_some();
        let aim_px = inp.attack_click.or(inp.mouse.filter(|_| inp.attack));
        let dash = inp.dash && !self.prev.dash;
        let request = if let Some(a) = self.pending.take() {
            Some(a)
        } else if attack_edge {
            Some(self.act_swing)
        } else if dash {
            Some(self.act_dash)
        } else {
            None
        };
        let it = Intent {
            dir,
            run: inp.run,
            request,
            release: false,
            aim: aim_px.map(|(x, y)| self.cam.unproject_ground(x, y)),
        };
        if request.is_some() {
            self.target = None;
        }
        self.prev = *inp;
        let was = self.player.action;
        let was_hit = self.player.hit_active();
        self.player.step(&self.lib, &self.actions, &it, TICK);
        let j = self.player_joints();
        self.player.update_secondary(&j, &self.gear, TICK);

        // effects
        let p = self.player.pos;
        let act = self.player.action;
        if act == self.act_dash {
            let j = self.player_joints();
            self.ghosts.push((j, 0.25));
            if was != self.act_dash {
                for _ in 0..14 { self.spawn_dust(p, 2.5); }
            }
        }
        if act == self.actions.run && (self.time * 10.0) as i32 % 2 == 0 && self.rand() > 0.3 {
            self.spawn_dust(p, 0.8);
        }
        if act == self.act_swing {
            let j = self.player_joints();
            let reach = self.player.def.equipped(GearSlot::MainHand, &self.gear).map(|g| g.reach).unwrap_or(0.9);
            let tip = j.weapon_point(1, reach);
            self.trail.push((j.weapon_point(1, 0.2), tip, 0.12));
            if self.player.hit_active() && self.rand() > 0.4 {
                let v = v3(self.rand(), self.rand().abs() + 0.5, self.rand()) * 2.0;
                self.particles.push(Particle { p: tip, v, life: 0.3, c: C_GOLD });
            }
            if self.player.hit_active() && !was_hit {
                self.flash = 2.0 * TICK; // demo of the hit-flash RampMap
            }
        }
        self.flash = (self.flash - TICK).max(0.0);
        if let Some((_, t)) = self.status.as_mut() {
            *t -= TICK;
        }
        if self.status.as_ref().is_some_and(|s| s.1 <= 0.0) {
            self.status = None;
        }
        for g in &mut self.ghosts { g.1 -= TICK; }
        self.ghosts.retain(|g| g.1 > 0.0);
        for t in &mut self.trail { t.2 -= TICK; }
        self.trail.retain(|t| t.2 > 0.0);
        for q in &mut self.particles {
            q.life -= TICK;
            q.v.y -= 6.0 * TICK;
            q.p = q.p + q.v * TICK;
            if q.p.y < 0.0 { q.p.y = 0.0; q.v = q.v * 0.3; }
        }
        self.particles.retain(|q| q.life > 0.0);

        // camera follow
        self.cam.cx += (p.x - self.cam.cx) * 0.08;
        self.cam.cz += (p.z - self.cam.cz) * 0.08;
    }

    /// FK joints of the player for the current pose.
    pub fn player_joints(&self) -> rig::Joints {
        let c = &self.player;
        let b = c.def.effective_body(&self.gear);
        let mut j = rig::solve(&c.anim.pose, &b, c.pos, c.yaw, c.gait.sway);
        j.chest_bounce = c.chest_spring.x;
        j.hair_sway = c.hair_sway();
        j
    }

    fn spawn_dust(&mut self, p: Vec3, s: f32) {
        let v = v3(self.rand() * s, self.rand().abs() * s * 0.6, self.rand() * s);
        let life = 0.35 + self.rand().abs() * 0.3;
        self.particles.push(Particle { p: p + v3(0.0, 0.05, 0.0), v, life, c: C_DUST });
    }

    pub fn render(&mut self) {
        self.draw_ground();
        self.fb.clear_depth();
        let cam = self.cam;
        let pp = cam.project(self.player.pos);
        self.fb.shadow_ellipse(pp.x, pp.y, 13.0, 6.0, C_SHADOW);
        if let Some((tx, tz)) = self.target {
            let t = cam.project(v3(tx, 0.0, tz));
            let a = |dx: f32, dy: f32| crate::camera::P2 { x: t.x + dx, y: t.y + dy, d: 1e6 };
            self.fb.line(a(-5.0, 0.0), a(0.0, -3.0), C_GOLD);
            self.fb.line(a(0.0, -3.0), a(5.0, 0.0), C_GOLD);
            self.fb.line(a(5.0, 0.0), a(0.0, 3.0), C_GOLD);
            self.fb.line(a(0.0, 3.0), a(-5.0, 0.0), C_GOLD);
        }
        let ghosts: Vec<_> = self.ghosts.iter().map(|g| g.0).collect();
        let body = self.player.def.effective_body(&self.gear);
        let ghost = RampMap::default().tint(R_ARCANE).team(R_ARCANE);
        for j in ghosts.iter().step_by(3) {
            render::draw_body(&mut self.fb, &cam, j, &body, &ghost, &self.opts);
            // push ghosts behind the live figure
            for d in self.fb.depth.iter_mut() { if *d < f32::INFINITY { *d = f32::INFINITY; } }
        }
        for (x, z) in PILLARS {
            let b = cam.project(v3(x, 0.0, z));
            self.fb.shadow_ellipse(b.x, b.y, 16.0, 7.0, C_SHADOW);
            self.fb.capsule(b, cam.project(v3(x, 2.2, z)), 9.0, shade(R_STONE, 5), Some(C_OUTLINE));
        }
        let j = self.player_joints();
        let mut map = self.player.def.colors;
        if self.flash > 0.0 {
            map = map.flashed();
        }
        render::draw_character(&mut self.fb, &cam, &j, &self.player.def, &self.gear, &map, &self.opts);
        for (a, b, _) in &self.trail {
            let (pa, pb) = (cam.project(*a), cam.project(*b));
            self.fb.line(pa, pb, C_TRAIL);
        }
        for q in &self.particles {
            let s = cam.project(q.p);
            self.fb.plot(s.x as i32, s.y as i32, s.d, q.c);
            if q.life > 0.2 { self.fb.plot(s.x as i32 + 1, s.y as i32, s.d, q.c); }
        }
        self.panel.draw(&mut self.fb);
        if self.panel.visible {
            let y = self.panel.buttons.last().map(|b| b.y + b.h + 3).unwrap_or(0);
            let st = format!("STANCE: {}", self.player.stance);
            crate::ui::text(&mut self.fb, 5, y, &st, shade(R_BONE, 3));
            let act = format!("ACTION: {}", self.player.action_id(&self.actions));
            crate::ui::text(&mut self.fb, 5, y + 7, &act, shade(R_BONE, 3));
            for (k, l) in HELP.iter().enumerate() {
                crate::ui::text(&mut self.fb, 5, y + 18 + k as i32 * 7, l, shade(R_BONE, 2));
            }
        }
        if self.customize.is_some() {
            self.draw_customize();
        }
        if let Some((s, _)) = &self.status {
            let (x, y) = (self.fb.w as i32 / 2 - s.len() as i32 * 2, self.fb.h as i32 - 12);
            crate::ui::text(&mut self.fb, x, y, s, shade(R_GOLD, 5));
        }
    }

    /// Customization screen: row list on the left, a large live preview
    /// (2x, slowly turning through every facing) on the right.
    fn draw_customize(&mut self) {
        use crate::ui::{rect, text};
        let (w, h) = (self.fb.w as i32, self.fb.h as i32);
        rect(&mut self.fb, 0, 0, w, h, shade(R_STONE, 0), shade(R_STONE, 3));
        let (ink, dim, hi) = (shade(R_BONE, 5), shade(R_BONE, 2), shade(R_GOLD, 4));
        text(&mut self.fb, 24, 14, "CUSTOMIZE CHARACTER", hi);
        let sel = self.customize.as_ref().map(|u| u.sel).unwrap_or(0);
        for (i, row) in ROWS.iter().enumerate() {
            let y = 30 + i as i32 * 11 + if i >= 13 { 5 } else { 0 };
            let (label, val) = self.custom.row_text(*row);
            let on = i == sel;
            if on {
                rect(&mut self.fb, 20, y - 3, 210, 11, shade(R_GOLD, 1), hi);
            }
            text(&mut self.fb, 26, y, label, if on { hi } else { ink });
            if !val.is_empty() {
                let v = if on { format!("< {val} >") } else { val };
                text(&mut self.fb, 120, y, &v, if on { hi } else { ink });
            }
        }
        text(&mut self.fb, 24, h - 26, "UP/DOWN SELECT   LEFT/RIGHT CHANGE   ENTER ACTIVATE", dim);
        text(&mut self.fb, 24, h - 18, "R RANDOM  X RESET  F5 SAVE  F9 LOAD  C CLOSE", dim);
        // live preview
        let (pw, ph, s) = (120usize, 168usize, 2usize);
        let mut fb = Framebuffer::new(pw, ph);
        fb.color.fill(shade(R_STONE, 2));
        let cam = Camera::new(pw, ph);
        let cam = Camera { cx: 0.0, cz: 0.0, ..cam };
        let c = &self.player;
        let body = c.def.effective_body(&self.gear);
        let yaw = std::f32::consts::FRAC_PI_4 + self.time * 0.6;
        let mut j = rig::solve(&c.anim.pose, &body, v3(0.0, 0.0, 0.0), yaw, c.gait.sway);
        j.chest_bounce = c.chest_spring.x;
        j.hair_sway = c.hair_sway();
        let g = cam.project(v3(0.0, 0.0, 0.0));
        fb.shadow_ellipse(g.x, g.y, 13.0, 6.0, C_SHADOW);
        render::draw_character(&mut fb, &cam, &j, &c.def, &self.gear, &c.def.colors, &self.opts);
        let (ox, oy) = (w as usize - pw * s - 40, 12usize);
        rect(&mut self.fb, ox as i32 - 2, oy as i32 - 2, (pw * s) as i32 + 4, (ph * s) as i32 + 4, 0, shade(R_STONE, 5));
        for y in 0..ph * s {
            for x in 0..pw * s {
                let (dx, dy) = (ox + x, oy + y);
                if dx < self.fb.w && dy < self.fb.h {
                    self.fb.color[dy * self.fb.w + dx] = fb.color[(y / s) * pw + x / s];
                }
            }
        }
    }

    fn draw_ground(&mut self) {
        let cam = self.cam;
        let (w, h) = (self.fb.w, self.fb.h);
        // incremental unprojection: ground coords are affine in screen x/y
        let (x0, z0) = cam.unproject_ground(0.5, 0.5);
        let (x1, z1) = cam.unproject_ground(1.5, 0.5);
        let (x2, z2) = cam.unproject_ground(0.5, 1.5);
        let (dxx, dzx, dxy, dzy) = (x1 - x0, z1 - z0, x2 - x0, z2 - z0);
        for y in 0..h {
            let (mut gx, mut gz) = (x0 + dxy * y as f32, z0 + dzy * y as f32);
            let row = &mut self.fb.color[y * w..(y + 1) * w];
            for px in row.iter_mut() {
                let (tx, tz) = (gx.floor(), gz.floor());
                let (fx, fz) = (gx - tx, gz - tz);
                let hsh = ((tx as i32).wrapping_mul(73856093) ^ (tz as i32).wrapping_mul(19349663)) as u32;
                let base = if (tx as i32 + tz as i32) & 1 == 0 { shade(R_STONE, 2) } else { shade(R_STONE, 4) };
                let base = if hsh.is_multiple_of(11) { shade(R_STONE, 1) } else { base };
                *px = if fx < 0.03 || fz < 0.03 {
                    shade(R_STONE, 1)
                } else if (hsh >> 8).is_multiple_of(7) && ((fx * 7.0) as i32 + (fz * 5.0) as i32) % 5 == 0 {
                    shade(R_STONE, 5) // pebbles
                } else {
                    base
                };
                gx += dxx;
                gz += dzx;
            }
        }
    }
}
