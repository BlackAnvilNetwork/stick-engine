// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Native host: winit window + softbuffer, nearest-neighbour integer upscale.
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;
use stick_engine::game::{Game, Input};
use stick_engine::raster::LUT;
use stick_engine::{HEIGHT, WIDTH};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

struct Gfx {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
}

struct App {
    gfx: Option<Gfx>,
    game: Game,
    input: Input,
    last: Instant,
    cursor: (f64, f64),
    view: (usize, usize, usize), // scale, offset x, offset y
    right_down: bool,
    bindings: stick_engine::input::Bindings,
}

/// Settings file: `$XDG_CONFIG_HOME/stick-engine/settings.txt` (or `~/.config/...`).
fn settings_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| {
        std::env::var_os("HOME").or_else(|| std::env::var_os("APPDATA")).map(|h| std::path::PathBuf::from(h).join(".config"))
    })?;
    Some(base.join("stick-engine").join("settings.txt"))
}

fn save_settings(s: &stick_engine::input::Settings) {
    if let Some(p) = settings_path() {
        let _ = p.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(p, s.to_text());
    }
}

impl App {
    fn to_px(&self) -> (f32, f32) {
        let (s, ox, oy) = self.view;
        (((self.cursor.0 - ox as f64) / s as f64) as f32, ((self.cursor.1 - oy as f64) / s as f64) as f32)
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Stick Engine")
            .with_inner_size(LogicalSize::new((WIDTH * 2) as f64, (HEIGHT * 2) as f64));
        let window = Rc::new(el.create_window(attrs).expect("window"));
        let ctx = softbuffer::Context::new(window.clone()).expect("softbuffer ctx");
        let surface = softbuffer::Surface::new(&ctx, window.clone()).expect("surface");
        self.gfx = Some(Gfx { window, surface });
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, ev: WindowEvent) {
        match ev {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::KeyboardInput { event, .. } => {
                let down = event.state == ElementState::Pressed;
                if let PhysicalKey::Code(code) = event.physical_key {
                    if code == KeyCode::Escape {
                        el.exit();
                    }
                    // winit's KeyCode names match W3C `KeyboardEvent.code` (KeyW, Numpad4, ...).
                    self.bindings.apply(&mut self.input, &format!("{code:?}"), down);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                let p = self.to_px();
                self.input.mouse = Some(p);
                if self.right_down {
                    self.input.move_click = Some(p);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let p = self.to_px();
                let down = state == ElementState::Pressed;
                match button {
                    MouseButton::Left if down => self.input.attack_click = Some(p),
                    MouseButton::Right => {
                        self.right_down = down;
                        self.input.rmb = down;
                        if down {
                            self.input.move_click = Some(p);
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = (now - self.last).as_secs_f32();
                self.last = now;
                self.game.update(dt, &mut self.input);
                if self.game.settings_changed {
                    self.game.settings_changed = false;
                    save_settings(&self.game.settings);
                }
                self.game.render();
                let Some(gfx) = self.gfx.as_mut() else { return };
                let size = gfx.window.inner_size();
                let (ww, wh) = (size.width.max(1) as usize, size.height.max(1) as usize);
                let s = (ww / WIDTH).min(wh / HEIGHT).max(1);
                let (ox, oy) = (ww.saturating_sub(WIDTH * s) / 2, wh.saturating_sub(HEIGHT * s) / 2);
                self.view = (s, ox, oy);
                gfx.surface
                    .resize(NonZeroU32::new(ww as u32).unwrap(), NonZeroU32::new(wh as u32).unwrap())
                    .expect("resize");
                let mut buf = gfx.surface.buffer_mut().expect("buffer");
                buf.fill(0);
                let src = &self.game.fb.color;
                for y in 0..(HEIGHT * s).min(wh - oy) {
                    let row = &src[(y / s) * WIDTH..(y / s + 1) * WIDTH];
                    let dst = &mut buf[(y + oy) * ww + ox..];
                    for x in 0..(WIDTH * s).min(ww - ox) {
                        dst[x] = LUT[row[x / s] as usize];
                    }
                }
                buf.present().expect("present");
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }
}

fn main() {
    let el = EventLoop::new().expect("event loop");
    let mut game = Game::new(WIDTH, HEIGHT);
    let args: Vec<String> = std::env::args().collect();
    game.demo = args.iter().any(|a| a == "--demo");
    // `--cam tp|fp|iso` (or STICK_CAM) sets the starting camera mode.
    let cam = args.iter().position(|a| a == "--cam").and_then(|i| args.get(i + 1).cloned()).or_else(|| std::env::var("STICK_CAM").ok());
    if let Some(m) = cam.as_deref().and_then(stick_engine::camrig::CamMode::parse) {
        game.set_cam_config(stick_engine::camrig::CamConfig { mode: m, ..game.rig.cfg });
    }
    if let Some(text) = settings_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        game.settings = stick_engine::input::Settings::parse(&text);
    }
    // `--cam-control mouse|numpad` (or STICK_CAM_CONTROL) overrides the saved setting.
    let ctl = args.iter().position(|a| a == "--cam-control").and_then(|i| args.get(i + 1).cloned()).or_else(|| std::env::var("STICK_CAM_CONTROL").ok());
    if let Some(c) = ctl.as_deref().and_then(stick_engine::input::CamControl::parse) {
        game.settings.cam_control = c;
    }
    let mut app = App { gfx: None, game, input: Input::default(), last: Instant::now(), cursor: (0.0, 0.0), view: (2, 0, 0), right_down: false, bindings: Default::default() };
    el.run_app(&mut app).expect("run");
}
