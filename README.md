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

## License (AGPL-3.0): what developers and agents must do

**Stick Engine, Copyright (C) 2026 Black Anvil Network.** Licensed under the GNU Affero General Public License, version 3 or (at your option) any later version (`AGPL-3.0-or-later`). See [LICENSE](LICENSE).

*This is a plain-language summary, not legal advice. The LICENSE text governs; ask a lawyer for your specific case.*

### If you use the engine in a game or app
- **Linking counts.** A game, tool or app that links to or compiles in Stick Engine (crate dependency, static or wasm build) is a derivative work and, when distributed or offered over a network, must be licensed under AGPL-3.0 (or a compatible license) as a whole.
- **Network use counts.** If you run a modified version (for example a game server or a hosted web build using a modified engine) for users over a network, you must offer those users its complete corresponding source, e.g. with a "Source" link (AGPL §13).
- **Shipping binaries means shipping source.** Native executables and **wasm/JS builds served to browsers** are conveying: provide the complete corresponding source (Rust sources, build scripts, `Cargo.toml`/`Cargo.lock`, wasm-bindgen glue) with them or via a written offer (AGPL §6).

### Always
- **Keep the notices.** Don't remove or alter copyright lines, license notices or warranty disclaimers.
- **Mark modified files** with a prominent notice saying what you changed and when (AGPL §5a).
- **Derivatives stay AGPL.** You can't relicense modified versions as proprietary.
- **No extra restrictions** beyond the license (AGPL §10, §12).
- **Patent grant.** Contributors license any patents needed to use their contributions (AGPL §11).
- **Third-party assets and code** (fonts, sprites, sounds, models, snippets) need licenses compatible with AGPL-3.0 that allow commercial use (e.g. CC0, CC-BY 4.0, MIT, BSD, Apache-2.0). No NC, ND or unknown-license material; record where each came from.

### Checklist for AI agents (and humans)
- [ ] Keep every existing copyright and license header.
- [ ] Don't paste code whose license you can't confirm is AGPL-3.0-compatible.
- [ ] Record the source URL, author and license of every third-party asset or snippet you add.
- [ ] Add a dated "modified" notice when changing files that came from someone else.
- [ ] Keep `license = "AGPL-3.0-or-later"` in every `Cargo.toml`.
- [ ] When deploying a modified build (server, hosted web page, release binary), publish or link the matching source.
