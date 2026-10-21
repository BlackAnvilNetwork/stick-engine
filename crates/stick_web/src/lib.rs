// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

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

/// localStorage key for the engine settings (`input::Settings` text).
const SETTINGS_KEY: &str = "stick-engine.settings";

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    let win = web_sys::window().ok_or("no window")?;
    let doc = win.document().ok_or("no document")?;
    let canvas: HtmlCanvasElement = doc.get_element_by_id("screen").ok_or("no #screen canvas")?.dyn_into()?;
    canvas.set_width(WIDTH as u32);
    canvas.set_height(HEIGHT as u32);
    let ctx: CanvasRenderingContext2d = canvas.get_context("2d")?.ok_or("no 2d ctx")?.dyn_into()?;

    let mut game = Game::new(WIDTH, HEIGHT);
    let store = win.local_storage().ok().flatten();
    if let Some(text) = store.as_ref().and_then(|st| st.get_item(SETTINGS_KEY).ok().flatten()) {
        game.settings = stick_engine::input::Settings::parse(&text);
    }
    let hash = win.location().hash().unwrap_or_default();
    game.demo = hash.contains("demo");
    // `#cam=tp` / `#cam=fp` / `#cam=iso` sets the starting camera mode.
    if let Some(m) = hash.split("cam=").nth(1).and_then(|v| stick_engine::camrig::CamMode::parse(v.split('&').next()?)) {
        game.set_cam_config(stick_engine::camrig::CamConfig { mode: m, ..game.rig.cfg });
    }
    // `#camctl=mouse|numpad` overrides the saved camera control.
    if let Some(c) = hash.split("camctl=").nth(1).and_then(|v| stick_engine::input::CamControl::parse(v.split('&').next()?)) {
        game.settings.cam_control = c;
    }
    // `#demo&t=3.5` fast-forwards the scripted demo (used for screenshots).
    if let Some(t) = hash.split("t=").nth(1).and_then(|v| v.split('&').next()?.parse::<f32>().ok()) {
        let mut i = Input::default();
        for _ in 0..(t * 60.0) as usize {
            game.update(1.0 / 60.0, &mut i);
        }
    }
    let bindings = Rc::new(stick_engine::input::Bindings::default());
    let st = Rc::new(RefCell::new(State { game, input: Input::default(), last: None, rgba: vec![0; WIDTH * HEIGHT * 4] }));

    for down in [true, false] {
        let (s, b) = (st.clone(), bindings.clone());
        let cb = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
            if b.apply(&mut s.borrow_mut().input, &e.code(), down) {
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
                2 => {
                    s.input.move_click = Some(p);
                    s.input.rmb = true;
                }
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
            s.input.rmb = e.buttons() & 2 != 0;
            if e.buttons() & 2 != 0 {
                s.input.move_click = Some(p); // hold right mouse to keep steering
            }
        });
        canvas.add_event_listener_with_callback("mousemove", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }
    {
        let s = st.clone();
        let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
            if e.button() == 2 {
                s.borrow_mut().input.rmb = false;
            }
        });
        win.add_event_listener_with_callback("mouseup", cb.as_ref().unchecked_ref())?;
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
            if game.settings_changed {
                game.settings_changed = false;
                if let Some(Ok(Some(st))) = web_sys::window().map(|w| w.local_storage()) {
                    let _ = st.set_item(SETTINGS_KEY, &game.settings.to_text());
                }
            }
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
