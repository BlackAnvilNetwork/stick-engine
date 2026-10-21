# Stick Engine

A tiny, fast, CPU pixel game engine in Rust. It runs in the browser via WASM and natively.
Characters are **procedurally drawn stick people** with no sprites and no 3D models. Skeleton joints are 3D, projected isometrically into a 640×360 indexed-colour framebuffer and upscaled with crisp pixels. See `PLAN.md` for the full design and roadmap. Planned: IK, crowds, combat helpers and an optional 3D camera mode. Game-specific integration (e.g. Black Anvil) lives in the game repos.

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
crates/stick_engine/src/
  math.rs       vectors, angle helpers, Pcg32 (deterministic RNG)
  anim.rs       19-channel Pose, masks, clips, ClipMeta (events-only), layered Animator   [headless]
  rig.rs        Joint/Bone enums, forward kinematics                                      [headless]
  body.rs       CharacterDef, sex presets, Archetype + from_seed                          [headless]
  gear.rs       gear shapes attached to joints, stance derivation                         [headless]
  gait.rs       procedural walk/run                                                       [headless]
  actions.rs    data-driven action table (actions.ron)                                    [headless]
  character.rs  generic action state machine                                              [headless]
  color.rs      ColorSlot + RampMap (team colour, tint, hit flash)
  raster.rs     framebuffer, capsules/discs/lines, ramp palette + 256-entry LUT
  render.rs     draws body + gear (outline on by default)
  camera.rs     iso projection;  game.rs  demo scene
crates/stick_engine/assets/   actions.ron, clips/*.ron, gear/*.ron, archetypes/*.ron (examples)
crates/stick_web/             wasm-bindgen host (canvas, input, rAF loop)
crates/stick_native/          winit + softbuffer host
index.html, Trunk.toml        web shell
```
"Headless" modules don't draw or touch the camera, so tools, tests and servers can use them.

## How the animation system works
- A **pose** has 19 channels, addressed by `anim::Ch` (degrees, `bob` in metres): `lean twist bob l_hip l_knee r_hip r_knee l_shoulder l_elbow l_arm_out r_shoulder r_elbow r_arm_out r_wpn_yaw r_wpn_pitch l_wpn_yaw l_wpn_pitch head root_pitch`.
  - `r_wpn_*`/`l_wpn_*` aim the item in each hand.
  - `root_pitch` rotates the whole body (rolls, knockdowns), while `lean` only bends the spine.
  - Cross-fades take the shortest arc.
- **Layers:** the base layer is the procedural gait (`gait.rs`, there are no walk/run clips), blended with the stance's idle clip. The action clip goes on top according to its `mask` (`Upper`, `Lower`, `Full` or `Additive`), then any additive clips (e.g. `hit_flinch`).
- A **clip** is a RON file with keyed poses, a `mask`, an optional `speed_scale`, an optional `hold: Some((t0, t1))` for casts, and named events. Events drive gameplay windows for any action: `hit_start`/`hit_end`, `iframes_start`/`iframes_end`, `cancel_open`/`cancel_close`, plus `release`, `commit`, `footstep`, `fx_*` and `sfx_*`.
- `ClipMeta::from_ron` reads only a clip's timing (no keys) for headless use.
- **Actions** are data in `assets/actions.ron`: mode (`Idle`, `Gait`, `OneShot`, `Cast`, `Additive`), blend time, per-stance clips, `move_mult`, `turn_rate`, `chain`, `cooldown`, `face`, `root_motion` and `weapon`. A clip is chosen by stance (`stances[stance]` → `stances["any"]` → unavailable). The stance is derived from the equipped hands (`unarmed`, `1h`, `1h_shield`, `dual`, `2h`, …).
- `rig::solve` turns a pose into 3D joints, and `render::draw_character` draws the body and gear. Colours are slots resolved by a `RampMap`.

## Adding an animation (e.g. a "kick")
1. **Clip file:** `crates/stick_engine/assets/clips/kick.ron`:
   ```ron
   (
       name: "kick",
       duration: 0.4,
       looping: false,
       mask: Full,
       base: { "lean": -5.0 },
       keys: [
           (t: 0.0,  pose: { "r_hip": 0.0,  "r_knee": 10.0 }),
           (t: 0.15, pose: { "r_hip": 40.0, "r_knee": 90.0 }),
           (t: 0.22, pose: { "r_hip": 85.0, "r_knee": 0.0, "lean": -15.0 }),
           (t: 0.4,  pose: { "r_hip": 0.0,  "r_knee": 10.0 }),
       ],
       events: [ (t: 0.2, name: "hit_start"), (t: 0.28, name: "hit_end") ],
   )
   ```
   Unknown channels or events and unpaired start/end events are load errors.
2. **Action row** in `assets/actions.ron`:
   `(id: "kick", mode: OneShot(next: "idle"), blend: 0.05, stances: { "any": "kick" }, move_mult: 0.0, face: Aim),`
3. **Built-in clips** are embedded: add `include_str!("../assets/clips/kick.ron"),` to `BUILTIN_CLIPS` in `anim.rs`. Alternatively, load it at runtime with `AnimLibrary::register_ron` and `ActionTable::from_ron`, with no Rust changes.
4. **Trigger** it with `Intent { request: actions.id("kick"), .. }`. The demo binds keys in `game.rs` and the hosts.
5. Run `cargo test` (it checks every action's clips exist), then `trunk serve`.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
