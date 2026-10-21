# Stick Engine

A tiny, fast, CPU pixel game engine in Rust. It runs in the browser via WASM and natively.
Characters are **procedurally drawn stick people** with no sprites and no 3D models. Skeleton joints are 3D, projected isometrically into a 640×360 indexed-colour framebuffer and upscaled with crisp pixels. See `PLAN.md` for the full design and roadmap. Planned: IK, crowds, combat helpers and an optional 3D camera mode. Game-specific integration (e.g. Black Anvil) lives in the game repos.

## Controls

| Action | Keys |
|---|---|
| Walk | Iso: WASD / arrow keys (screen-relative). 3D modes: **W/S** forward/back along the facing, **A/D** (or **Q/E**) turn the character. Or **right-click** (hold to steer) to move to a spot in any mode |
| Run | hold **Shift** while moving |
| Dash | **Space** (dashes in the move direction, or facing; 0.45 s cooldown) |
| Sword swing | **Left click** (aims at cursor) or **J** |
| Cycle camera (iso → third-person → first-person) | **V** |
| Camera pitch (3D modes) | **R / F** |
| Quit (native) | Esc |

### Camera modes

- **Iso** (default): the original 2:1 dimetric view, byte-identical to earlier output.
- **Third-person**: behind and above the character, pitched 30° down, `tp_distance` 6 m. The boom follows the character's facing with an exponential, shortest-arc yaw follow.
- **First-person**: eye at the head, yaw locked to the character's facing; head and body are hidden only while the eye is within 0.4 m of the head, forearms and held gear are drawn; pitch clamped to ±60°.

All modes sit on one camera rail (iso → TP dolly-zoom → TP → FP boom), so every move that involves first-person travels along the third-person boom and never through the body; FP ↔ iso is a single eased move through TP (no stall), blending the facing yaw/pitch to the iso angles (`blend_s` 0.7 s for one step, 1.05 s for FP ↔ iso). Turning turns the character and the camera follows; click-to-move/aim pick the ground in every mode and the character turns toward the target. Iso keeps its fixed camera. Configure via `CamConfig` (`Game::set_cam_config`), `--cam iso|tp|fp` or `STICK_CAM` natively, or `#cam=tp` on the web. `cargo run --release -p stick_engine --example camshots -- out/` renders each mode and prints per-mode frame cost.

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

## License (AGPL-3.0): what developers and agents must do

**Stick Engine, Copyright (C) 2026 Black Anvil Network.** Licensed under the GNU Affero General Public License, version 3 or (at your option) any later version (`AGPL-3.0-or-later`). See [LICENSE](LICENSE).

*This is a plain-language summary, not legal advice. The LICENSE text governs; ask a lawyer for your specific case.*

### If you use the engine in a game or app
- **Linking counts.** A game, tool or app that links to or compiles in Stick Engine (crate dependency, static or wasm build) is a derivative work and, when distributed or offered over a network, must be licensed under AGPL-3.0 (or a compatible license) as a whole.
- **Network use counts.** If you run a modified version (for example a game server or a hosted web build using a modified engine) for users over a network, you must offer those users its complete corresponding source, e.g. with a "Source" link (AGPL §13).
- **Shipping binaries means shipping source.** Native executables and **wasm/JS builds served to browsers** are conveying: provide the complete corresponding source (Rust sources, build scripts, `Cargo.toml`/`Cargo.lock`, wasm-bindgen glue) with them or via a written offer (AGPL §6).

### Always
- **Keep the notices (AGPL §5).** Don't remove or alter copyright lines, license notices or warranty disclaimers. Every source file starts with a `Copyright (c) 2026 Black Anvil Network` / `SPDX-License-Identifier: AGPL-3.0-or-later` header; keep it (and any third-party notice) and add it to new files with `node scripts/add-license-headers.mjs` (CI checks with `--check`).
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

## Trademarks

Stick Engine™ and its logo are trademarks of Black Anvil Network. The code license does not grant rights to use them. Forks and modified versions must use their own name and logo. See [TRADEMARK.md](TRADEMARK.md).
