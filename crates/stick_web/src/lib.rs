//! Browser host: canvas 2D + putImageData, keyboard/mouse input, rAF loop.
use std::cell::RefCell;
use std::rc::Rc;
use stick_engine::game::{Game, Input};
use stick_engine::{HEIGHT, WIDTH};
use wasm_bindgen::prelude::*;
use wasm_bindgen::{Clamped, JsCast};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData, KeyboardEvent, MouseEvent};

struct State {
    game: Game,
    input: Input,
    last: Option<f64>,
    rgba: Vec<u8>,
}

fn set_key(i: &mut Input, code: &str, down: bool) -> bool {
    match code {
        "KeyW" | "ArrowUp" => i.up = down,
        "KeyS" | "ArrowDown" => i.down = down,
        "KeyA" | "ArrowLeft" => i.left = down,
        "KeyD" | "ArrowRight" => i.right = down,
        "ShiftLeft" | "ShiftRight" => i.run = down,
        "Space" => i.dash = down,
        "KeyJ" => i.attack = down,
        _ => return false,
    }
    true
}

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    let win = web_sys::window().ok_or("no window")?;
    let doc = win.document().ok_or("no document")?;
    let canvas: HtmlCanvasElement = doc.get_element_by_id("screen").ok_or("no #screen canvas")?.dyn_into()?;
    canvas.set_width(WIDTH as u32);
    canvas.set_height(HEIGHT as u32);
    let ctx: CanvasRenderingContext2d = canvas.get_context("2d")?.ok_or("no 2d ctx")?.dyn_into()?;

    let mut game = Game::new(WIDTH, HEIGHT);
    let hash = win.location().hash().unwrap_or_default();
    game.demo = hash.contains("demo");
    // `#demo&t=3.5` fast-forwards the scripted demo (used for screenshots).
    if let Some(t) = hash.split("t=").nth(1).and_then(|v| v.split('&').next()?.parse::<f32>().ok()) {
        let mut i = Input::default();
        for _ in 0..(t * 60.0) as usize {
            game.update(1.0 / 60.0, &mut i);
        }
    }
    let st = Rc::new(RefCell::new(State { game, input: Input::default(), last: None, rgba: vec![0; WIDTH * HEIGHT * 4] }));

    for down in [true, false] {
        let s = st.clone();
        let cb = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
            if set_key(&mut s.borrow_mut().input, &e.code(), down) {
                e.prevent_default();
            }
        });
        win.add_event_listener_with_callback(if down { "keydown" } else { "keyup" }, cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    let to_px = {
        let c = canvas.clone();
        move |e: &MouseEvent| {
            let r = c.get_bounding_client_rect();
            (
                ((e.client_x() as f64 - r.left()) / r.width() * WIDTH as f64) as f32,
                ((e.client_y() as f64 - r.top()) / r.height() * HEIGHT as f64) as f32,
            )
        }
    };
    {
        let (s, to_px) = (st.clone(), to_px.clone());
        let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
            let p = to_px(&e);
            let mut s = s.borrow_mut();
            s.input.mouse = Some(p);
            match e.button() {
                0 => s.input.attack_click = Some(p),
                2 => s.input.move_click = Some(p),
                _ => {}
            }
            e.prevent_default();
        });
        canvas.add_event_listener_with_callback("mousedown", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }
    {
        let (s, to_px) = (st.clone(), to_px.clone());
        let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
            let p = to_px(&e);
            let mut s = s.borrow_mut();
            s.input.mouse = Some(p);
            if e.buttons() & 2 != 0 {
                s.input.move_click = Some(p); // hold right mouse to keep steering
            }
        });
        canvas.add_event_listener_with_callback("mousemove", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }
    {
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(|e: web_sys::Event| e.prevent_default());
        canvas.add_event_listener_with_callback("contextmenu", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }

    type Frame = Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>;
    let f: Frame = Rc::new(RefCell::new(None));
    let g = f.clone();
    *g.borrow_mut() = Some(Closure::new(move |ts: f64| {
        {
            let mut s = st.borrow_mut();
            let dt = s.last.map(|l| ((ts - l) / 1000.0) as f32).unwrap_or(1.0 / 60.0);
            s.last = Some(ts);
            let State { game, input, rgba, .. } = &mut *s;
            game.update(dt, input);
            game.render();
            game.fb.write_rgba(rgba);
            if let Ok(img) = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&rgba[..]), WIDTH as u32, HEIGHT as u32) {
                let _ = ctx.put_image_data(&img, 0.0, 0.0);
            }
        }
        let w = web_sys::window().unwrap();
        let _ = w.request_animation_frame(f.borrow().as_ref().unwrap().as_ref().unchecked_ref());
    }));
    win.request_animation_frame(g.borrow().as_ref().unwrap().as_ref().unchecked_ref())?;
    Ok(())
}
