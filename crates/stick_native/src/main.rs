//! Native host: winit window + softbuffer, nearest-neighbour integer upscale.
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;
use stick_engine::game::*;
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
                    let i = &mut self.input;
                    if down {
                        // edge bits for the customization screen
                        i.hotkeys |= match code {
                            KeyCode::KeyW | KeyCode::ArrowUp => HK_NAV_UP,
                            KeyCode::KeyS | KeyCode::ArrowDown => HK_NAV_DOWN,
                            KeyCode::KeyA | KeyCode::ArrowLeft => HK_NAV_LEFT,
                            KeyCode::KeyD | KeyCode::ArrowRight => HK_NAV_RIGHT,
                            KeyCode::Enter | KeyCode::NumpadEnter => HK_ENTER,
                            KeyCode::KeyC => HK_CUSTOM,
                            KeyCode::KeyR => HK_RANDOM,
                            KeyCode::KeyX => HK_RESET,
                            KeyCode::F5 => HK_SAVE,
                            KeyCode::F9 => HK_LOAD,
                            _ => 0,
                        };
                    }
                    match code {
                        KeyCode::KeyW | KeyCode::ArrowUp => i.up = down,
                        KeyCode::KeyS | KeyCode::ArrowDown => i.down = down,
                        KeyCode::KeyA | KeyCode::ArrowLeft => i.left = down,
                        KeyCode::KeyD | KeyCode::ArrowRight => i.right = down,
                        KeyCode::ShiftLeft | KeyCode::ShiftRight => i.run = down,
                        KeyCode::Space => i.dash = down,
                        KeyCode::KeyJ => i.attack = down,
                        KeyCode::Escape => el.exit(),
                        KeyCode::KeyG if down => i.hotkeys |= HK_SEX,
                        KeyCode::KeyT if down => i.hotkeys |= HK_GEAR,
                        KeyCode::F1 if down => i.hotkeys |= HK_UI,
                        KeyCode::Digit1 if down => i.hotkeys |= hk_digit(1),
                        KeyCode::Digit2 if down => i.hotkeys |= hk_digit(2),
                        KeyCode::Digit3 if down => i.hotkeys |= hk_digit(3),
                        KeyCode::Digit4 if down => i.hotkeys |= hk_digit(4),
                        KeyCode::Digit5 if down => i.hotkeys |= hk_digit(5),
                        KeyCode::Digit6 if down => i.hotkeys |= hk_digit(6),
                        KeyCode::Digit7 if down => i.hotkeys |= hk_digit(7),
                        KeyCode::Digit8 if down => i.hotkeys |= hk_digit(8),
                        KeyCode::Digit9 if down => i.hotkeys |= hk_digit(9),
                        _ => {}
                    }
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
                save_load(&mut self.game);
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

/// Customization save file, next to the executable.
fn save_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| ".".into())
        .join("character.ron")
}

fn save_load(g: &mut Game) {
    if let Some(text) = g.save_request.take() {
        match std::fs::write(save_path(), text) {
            Ok(()) => g.set_status("saved character.ron"),
            Err(e) => {
                eprintln!("save failed: {e}");
                g.set_status("save failed");
            }
        }
    }
    if std::mem::take(&mut g.load_request) {
        match std::fs::read_to_string(save_path()).map_err(|e| e.to_string()).and_then(|s| g.load_customization(&s)) {
            Ok(()) => g.set_status("loaded character.ron"),
            Err(e) => {
                eprintln!("load failed: {e}");
                g.set_status("load failed");
            }
        }
    }
}

fn main() {
    let el = EventLoop::new().expect("event loop");
    let mut game = Game::new(WIDTH, HEIGHT);
    game.demo = std::env::args().any(|a| a == "--demo");
    if let Ok(s) = std::fs::read_to_string(save_path()) {
        if let Err(e) = game.load_customization(&s) {
            eprintln!("ignoring {}: {e}", save_path().display());
        }
    }
    let mut app = App { gfx: None, game, input: Input::default(), last: Instant::now(), cursor: (0.0, 0.0), view: (2, 0, 0), right_down: false };
    el.run_app(&mut app).expect("run");
}
