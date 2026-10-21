# Stick Engine

A tiny, fast, CPU pixel game engine in Rust. It runs in the browser via WASM and natively.
Characters are **procedurally drawn stick people** with no sprites and no 3D models. Skeleton joints are 3D, projected isometrically into a 640×360 indexed-colour framebuffer and upscaled with crisp pixels. See `PLAN.md` for the full design and roadmap, including the planned 3D camera mode.

## Controls

| Action | Keys |
|---|---|
| Walk | WASD / arrow keys (screen-relative), or **right-click** (hold to steer) to move to a spot |
| Run | hold **Shift** while moving |
| Dash | **Space** (dashes in the move direction, or facing; 0.45 s cooldown) |
| Sword swing | **Left click** (aims at cursor) or **J** |
| Quit (native) | Esc |

Add `#demo` to the web URL (or `--demo` natively) for a scripted autoplay. `#demo&t=4.7` fast-forwards to a moment.

## Run it

Prerequisites: Rust stable (`rustup`), plus for the web build `rustup target add wasm32-unknown-unknown` and `cargo install trunk --locked`.

**Web (dev server with reload):**
```
trunk serve --open
```
**Web (static build):** `trunk build --release` writes `dist/`. Serve that folder with any static server, e.g. `python -m http.server 8080 -d dist`, and open http://localhost:8080. It won't work from `file://`, because wasm needs http.

**Native:**
```
cargo run -p stick_native --release
```
**Tests:** `cargo test`

### Build output outside OneDrive (Windows)
This folder lives in OneDrive, and syncing `target/` (GBs) is slow. `.cargo/config.toml` on this PC sets
`target-dir = "C:/Users/raxac/cargo-target/stick-engine"`. Elsewhere, delete that file or set `CARGO_TARGET_DIR`.

## Layout
```
crates/stick_engine/   core: math, raster (capsules, discs, lines, z-buffer, palette), camera (iso),
                       rig (FK + drawing), anim (poses/clips/animator), character (state machine), game
crates/stick_engine/assets/clips/*.ron   animation clips (data)
crates/stick_web/      wasm-bindgen host (canvas, input, rAF loop)
crates/stick_native/   winit + softbuffer host
index.html, Trunk.toml web shell
```

## How the animation system works
- A **pose** is 16 named channels, angles in degrees: `lean twist bob l_hip l_knee r_hip r_knee l_shoulder l_elbow l_arm_out r_shoulder r_elbow r_arm_out sword_yaw sword_pitch head` (`bob` is metres of pelvis drop or lift).
  Hips and shoulders swing forward (+) or back (−) from hanging straight down, knees and elbows bend, `*_arm_out` raises the arm sideways, `twist` yaws the upper body, and `sword_yaw`/`sword_pitch` aim the blade relative to the upper body.
- A **clip** is a RON file: keyed poses over time, smoothstep-interpolated, looping or one-shot, with optional named events.
- `rig::solve` turns a pose into 3D joints (forward kinematics), and `rig::draw` rasterizes them as capsule limbs, a circle head and a line sword.
- `character.rs` holds the **state machine**. Each `Action` has an `ActionDef { clip, blend, mode }` in the `ACTIONS` table. `Loop { ref_speed }` scales playback by ground speed (so feet match movement). `OneShot { next }` plays once and can't be interrupted.

## Adding an animation (e.g. a "kick")
1. **Clip file:** create `crates/stick_engine/assets/clips/kick.ron`:
   ```ron
   (
       name: "kick",
       duration: 0.4,
       looping: false,
       base: { "lean": -5.0 },                       // shared by all keys
       keys: [
           (t: 0.0,  pose: { "r_hip": 0.0,  "r_knee": 10.0 }),
           (t: 0.15, pose: { "r_hip": 40.0, "r_knee": 90.0 }),   // chamber
           (t: 0.22, pose: { "r_hip": 85.0, "r_knee": 0.0, "lean": -15.0 }), // extend
           (t: 0.4,  pose: { "r_hip": 0.0,  "r_knee": 10.0 }),
       ],
       events: [ (t: 0.2, name: "hit_start"), (t: 0.28, name: "hit_end") ],
   )
   ```
   Unknown channel names are a load error, so typos are caught by `cargo test`.
2. **Register the clip:** add `include_str!("../assets/clips/kick.ron"),` to `BUILTIN_CLIPS` in `anim.rs`.
3. **Register the action:** add `Kick` to `enum Action`, and add a row to `ACTIONS` in `character.rs`:
   `ActionDef { action: Action::Kick, clip: "kick", blend: 0.05, mode: Mode::OneShot { next: Action::Idle } },`
   Also add `Action::Kick => (0.0, 0.0),` to the movement `match` in `Character::step`.
4. **Trigger it:** add a `kick: bool` to `Intent`, then start it next to the swing/dash triggers in `Character::step` (`self.enter(Action::Kick)`). Bind a key in `game::Input` and both hosts (e.g. `KeyK`).
5. `cargo test` (the `builtin_clips_parse` test checks every action's clip exists), then `trunk serve`.

For a new **locomotion** style (e.g. sneak), use `Mode::Loop { ref_speed: <m/s> }` and choose it where Walk/Run are chosen.
To tweak an existing move, just edit its `.ron` and rebuild. `AnimLibrary::register_ron` also accepts clips at runtime, ready for hot-reload or a future pose editor.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
