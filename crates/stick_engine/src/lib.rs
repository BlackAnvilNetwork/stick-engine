//! Stick Engine core: math, CPU pixel raster, stick rig, data-driven animation,
//! character state machine and a tiny game loop. Platform hosts live in
//! `stick_web` (wasm) and `stick_native` (winit + softbuffer).

pub mod anim;
pub mod camera;
pub mod character;
pub mod game;
pub mod math;
pub mod raster;
pub mod rig;

/// Internal framebuffer resolution. 640x360 keeps an ~80 px stick figure at
/// roughly 22% of screen height (Diablo-like framing) and upscales 2x/3x/6x
/// cleanly to 720p/1080p/4K.
pub const WIDTH: usize = 640;
pub const HEIGHT: usize = 360;

#[cfg(test)]
mod tests;
