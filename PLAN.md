# Stick Engine — Technical Plan

A super-fast, pixel-based game engine in **Rust → WASM** (browser) and native. Every character is a **stick person**, drawn procedurally into a pixel framebuffer. **No sprites, no 3D models.** It supports Diablo-style ARPG play (isometric) and can blend smoothly into a **3D perspective mode** using the same skeleton data.

---

## 1. Goals / Non-goals

**Goals**
- Everything is drawn procedurally into a CPU framebuffer: bones as thick pixel lines/capsules, heads as pixel circles, gear as parametric shapes.
- 1000+ animated stick characters at 60 fps at a **640×360** internal resolution, integer-upscaled (2× = 720p, 3× = 1080p, 6× = 4K).
- Characters are **~80 px tall** on screen.
- The same build runs in the browser (wasm32) and natively (Windows/Linux/macOS).
- ARPG feature set: click-to-move, pathfinding, crowds of enemies, skills, loot, hit effects, particles.
- Camera modes: isometric 2.5D ↔ full 3D perspective, with a smooth animated transition.
- **Standalone** engine/game: no Godot integration.

**Non-goals (for now)**
- No sprite sheets, texture atlases, or mesh/model importers.
- No general-purpose editor like Godot/Unity. Tools are small and purpose-built.
- No networking/multiplayer, no deterministic replays, no mobile target (all explicitly out of scope).
- No physically-based lighting. The look is palette-driven pixel art.

---

## 2. Workspace / crate layout

```
stick-engine/
  Cargo.toml              (workspace)
  crates/
    se_math/      vec2/3/4, mat, quat, fixed-point (I32F32 optional), iso/persp projections
    se_rig/       skeleton definition, bone hierarchy, character params (proportions, colors, gear)
    se_anim/      keyframe poses, procedural gaits, 2-bone IK, look-at, ragdoll (verlet), blending
    se_raster/    framebuffer, z-buffer, line/thick line/circle/capsule/triangle, palette, dithering
    se_render/    scene → draw list → raster; camera modes + transition; dirty rects; debug overlay
    se_world/     tilemap/heightmap, procedural tile drawing, navgrid
    se_sim/       ECS, fixed timestep, movement, pathfinding, combat, skills, loot, particles
    se_audio/     procedural SFX (simple synth) + playback abstraction
    se_host_web/  wasm-bindgen/web-sys host: canvas, input, rAF loop, WebGL2 blit
    se_host_native/ winit + softbuffer/pixels host
  tools/
    pose_editor/  (egui) pose and rig-parameter editor; exports RON
  assets/
    palettes/     .hex/.gpl palettes (can be authored in Aseprite)
    poses/        *.ron keyframe poses
  examples/       hello_framebuffer, walk_cycle, iso_click_move, arena, mode_switch
```

Rule: `se_sim` never depends on rendering, so the simulation can run headless (tests, tools).

---

> **Prototype status (2026-10-20):** the current code is a 3-crate workspace: `stick_engine` (math, raster, rig, anim, character, game), `stick_web` (wasm host) and `stick_native` (winit + softbuffer host). It will split into the `se_*` crates above as it grows. M0 is done and M1 is done plus extras: run, dash, sword swing, procedural iso floor, WASD + right-click move. The web host uses a 2D-canvas `putImageData` blit; the WebGL2 blit comes in M5.

## 3. Recommended crates

| Need | Crate | Why |
|---|---|---|
| Math | `glam` | Fast, SIMD-backed, wasm-friendly. Used for render-side float math. |
| ECS | `hecs` (start) | Small, archetype storage = cache-friendly SoA-ish iteration, no scheduler magic. Upgrade path: custom SoA for the hot crowd path if profiling demands it. (`bevy_ecs` is heavier than needed.) |
| WASM glue | `wasm-bindgen`, `web-sys`, `js-sys` | Standard, stable. |
| Build | `trunk` or `wasm-pack` | `trunk` for a dev server with hot reload. |
| Native window | `winit` + `softbuffer` | Push a CPU buffer straight to the window with no GPU setup. Alternative: `pixels` (wgpu), which gives scaling shaders for free. |
| GPU blit / later GPU path | `wgpu` | WebGPU/WebGL2/native from one API. Optional at first; the web host can use raw WebGL2 for the blit. |
| Pathfinding | `pathfinding` crate (A*, JPS-ish) + a custom flow field | A* for the player; flow fields for crowds. |
| Spatial queries | custom uniform grid (own code) | Simpler and faster than general trees for uniform ARPG crowds. |
| RNG | `rand_pcg` / `rand_xoshiro` | Seeded, reproducible procedural generation. |
| Serialization | `serde` + `ron` | Human-readable clips/poses/params, hot-reloadable. |
| Audio | `cpal` (native), WebAudio via `web-sys`; maybe `kira` later | Thin abstraction. Procedural SFX keep the "no assets" spirit. |
| Tools UI | `egui` / `eframe` | Fast immediate-mode tooling, also runs in wasm. |
| Profiling | `puffin` (+ `puffin_egui`), `tracy-client` native | Frame-level timing. |
| SIMD | `std::simd` (nightly) or `wide` (stable) | `wide` keeps us on stable Rust; wasm `simd128` target feature on. |
| Threads (native) | `rayon` | Parallel raster in tiles. Web threads (SharedArrayBuffer) are an optional later step. |

---

## 4. Skeleton / Rig (`se_rig`)

- **Standard stick humanoid**: pelvis (root), spine, neck, head, L/R upper arm, forearm, hand, thigh, shin, foot. 15 bones; extra bones (tail, weapon, cape chain) are allowed.
- Bone data is SoA: `parent: [u8]`, `length: [f32]`, `local_rot: [Quat]`, `thickness: [u8]`, `color_idx: [u8]`.
- Joints live in **3D** from day one, even in iso mode. The 2.5D view is just a projection, and that is what makes the 3D mode cheap.
- **CharacterParams** (serializable, seedable):
  - Proportions: height, limb length ratios, head radius, shoulder/hip width, limb thickness.
  - Palette indices: body, outline, accent.
  - Stance/personality: slouch, gait speed, arm swing.
  - **Gear as procedural shapes**: a sword is a line plus a crossguard; a shield is a filled circle/polygon; a helmet is a head-circle overlay with a crest line; a cape is a verlet chain; armor is thicker limbs plus an accent color. Items are described by data (shape primitives + palette + attach bone), so loot can generate visual variety for free.
- Random-character generation from a seed, for crowds of distinct enemies.

---

## 5. Animation (`se_anim`)

Layered, all evaluated per character per tick (or every N ticks for far LOD):

1. **Keyframe poses**: a few hand-authored key poses per action (idle, attack wind-up/strike/recover, cast, hit, death), stored as RON joint rotations and interpolated with slerp plus easing curves.
2. **Procedural locomotion**: walk/run cycles driven by velocity (phase = distance travelled / stride), with sinusoidal hip bob, counter-swinging arms, and lean into acceleration. There is no walk "clip"; it is all parametric, so speed and proportions just work.
3. **2-bone IK**: feet planted on the terrain height and hands on weapon/target; analytic solve (cheap). Look-at for the head.
4. **Additive layers**: hit flinch, breathing, aim offset.
5. **Ragdoll**: on death or knockback, hand the joints to a Verlet particle system with distance constraints and ground collision; blend back to animation on get-up. A few iterations per tick is enough at this scale.
6. **Blending**: per-layer weights, crossfades, and masks (upper body vs lower body, so you can attack while running).

**LOD**: near characters update every tick with IK; mid characters skip IK; far characters update every 2–4 ticks with a cheaper gait.

---

## 6. Renderer (`se_raster` + `se_render`)

### 6.1 Framebuffer
- **Decision: internal 640×360.** At 80 px a figure is ~22% of screen height, close to Diablo II framing, with ~20×11 m of visible floor. 480×270 made 80 px figures fill 30% of the screen (too zoomed in); 960×540 quadruples raster cost per frame for little gain and only scales 2× to 1080p. 640×360 scales cleanly by integers to 720p/1080p/1440p/4K. **Indexed 8-bit color** (`u8` palette index) plus a depth buffer (f32 in the prototype; 16-bit later if bandwidth matters). Small buffers mean the whole frame stays in L2 cache, which is very fast.
- At present time the buffer is converted through the palette to RGBA (a 256-entry LUT) and blitted.

### 6.2 Primitives (all integer, clipped, z-tested)
- **Lines**: Bresenham; **thick lines/limbs**: capsule raster (distance-to-segment over the bbox, or span-based scanline for speed). Each limb is a capsule with depth interpolated along the bone.
- **Circles / discs** (heads, shields, particles): midpoint circle, filled spans.
- **Triangles / polygons**: scanline with z (for 3D mode terrain/props, gear shapes).
- **Outlines**: optional 1-px outline pass with a depth-discontinuity edge detect, which gives the classic crisp pixel look.
- **Point splats**: for particles and voxel-ish effects.

### 6.3 Palette & look
- Palettes are 16–64 colors, loaded from `.gpl`/`.hex`. **Aseprite (CLI installed) can author/export palettes** and reference mockups, never sprites. Example: `aseprite -b palette.aseprite --save-as palette.gpl` (or a Lua script export).
- Shading by **palette ramps** (index + light level → index) and **ordered (Bayer) dithering** for gradients, fog, and fake lighting.
- Palette swaps for hit flash (all white for 2 frames), elemental tints, team colors, and day/night.

### 6.4 Upload / present
- **Web**: one WebGL2 texture `texSubImage2D` per frame plus a fullscreen quad with nearest filtering and integer scaling (letterboxed). Fallback: `putImageData` on a 2D canvas.
- **Native**: `softbuffer` (CPU scaling) or `pixels` (GPU scaling).
- **Dirty rects**: optional for UI/menus. In gameplay the whole small frame is redrawn (cheap), and dirty-rect tracking is used for the static terrain layer cache (see 7).

### 6.5 Draw pipeline
`sim snapshot → interpolate (alpha) → per-character pose → project joints (camera) → build draw list (capsules/circles/polys with depth) → bin into screen tiles (e.g. 32×32) → raster tiles (parallel natively) → post (outline, flash, dither fog) → UI overlay → present`.

Tile binning makes it SIMD/threads-friendly and keeps work local.

### 6.6 Optional GPU path (later)
Same draw list, but instanced capsules/circles via SDF fragment shaders in `wgpu` into a low-res render target. It is only needed if CPU limits are hit (e.g. 3D mode at higher resolutions).

---

## 7. Camera modes & transition

- **Iso 2.5D**: orthographic projection with a classic 2:1 dimetric angle (about 26.565° pitch, 45° yaw). Integer-snapped camera position for stable pixels.
- **Full 3D**: perspective with a free/orbit/chase camera. The same joints and the same capsule raster, with radius scaled by 1/z.
- **Transition**: interpolate the projection matrix `P(t) = lerp(ortho, persp)` with a matched-FOV dolly (dolly-zoom trick: increase the FOV while pulling the camera in, so the framing stays constant), plus pitch/yaw lerp. 0.5–1.0 s, eased. Because everything is already 3D data, nothing pops.
- **World in 3D**: the tilemap heightmap becomes triangles (flat-shaded with palette ramps). Walls are extruded tiles. Optional voxel-column mode for a chunky retro look.
- Gameplay stays on the ground plane (XZ) in both modes, so the sim is unaffected by the camera.

---

## 8. World (`se_world`)

- Tile grid (e.g. 1 m tiles) with height and material ID, generated procedurally (noise + room/corridor dungeon generator, Diablo-style).
- Tiles are drawn procedurally: palette-ramped diamonds with dithered noise patterns, edge outlines, procedural props (rocks = polygons, torches = line + flickering particle, grass = pixel tufts).
- **Static terrain cache**: in iso mode, pre-raster terrain into a larger cached layer (color + depth) and blit the visible window. Re-raster only dirty chunks. In 3D mode, terrain is rasterized each frame (frustum-culled chunks).
- Navgrid derived from tiles (walkable flags, costs).

---

## 9. Simulation (`se_sim`)

- **Fixed timestep**: 60 Hz sim (or 30 Hz with render interpolation; decide in M5), with an accumulator loop. Rendering interpolates between the previous and current state.
- **ECS (hecs)** components: `Transform`, `Velocity`, `Rig(params handle)`, `AnimState`, `Health`, `Faction`, `Ai`, `PathAgent`, `Skills`, `Loot`, `Projectile`, `Particle` (particles may live outside ECS in a pure SoA pool for speed).
- **Input**: platform hosts translate events into an `InputFrame` (mouse world position via inverse projection, buttons, keys). 
- **Click-to-move**: screen → ground-plane raycast (works in both modes) → A* on the navgrid → path smoothing (string-pulling) → steering.
- **Crowds**: flow-field toward the player (recomputed only when the player changes tile), local separation via the uniform spatial grid, and simple avoidance. This scales to thousands.
- **Combat/ARPG**:
  - Melee hit = arc/capsule test against enemy hit capsules (derived from bones, so the hitbox matches the stick body).
  - Skills are data-driven (RON): cooldown, cost, shape (cone, circle, projectile, chain), effects (damage, knockback → ragdoll, status).
  - Hit feedback: palette flash, hit-stop (2–4 frame freeze), screen shake (integer pixels), number pop-ups (pixel font rendered procedurally from a tiny bitmap font defined in code), particles.
  - Loot: seeded drop tables → items with affixes; the item's visual = procedural gear shapes (see Rig). Ground loot drawn as a glinting shape + label.
- **Particles**: SoA pool (pos, vel, life, color ramp), up to about 20k, rendered as 1–2 px points/short streaks with additive palette-ramp stepping. Blood/sparks/magic/dust.
- **Audio**: procedural SFX (noise bursts, pitched blips, swooshes) generated at load time into buffers; event-triggered from the sim via an event queue (decoupled from the sim).

---

## 10. Hosts

- **Web (`se_host_web`)**: `wasm-bindgen` entry, a `requestAnimationFrame` loop, `web-sys` for canvas/WebGL2/pointer/keyboard/WebAudio, and a resize → integer scale calculation. Build with `trunk`, release with `wasm-opt -O3`, and `RUSTFLAGS="-C target-feature=+simd128"`. Target a wasm binary under about 1 MB gzipped.
- **Native (`se_host_native`)**: `winit` event loop + `softbuffer`, with `rayon` tile raster.
---

## 11. Performance budget

Target: **1000+ characters, 60 fps, 640×360**, mid-range laptop, in the browser (single-threaded wasm).

| Stage | Budget (ms) |
|---|---|
| Sim (AI, movement, combat, 1000 agents) | 3.0 |
| Animation (1000 rigs × 15 bones, LOD) | 3.0 |
| Projection + draw list + binning | 1.5 |
| Raster (terrain blit + ~15k capsules + particles) | 5.0 |
| Post + palette convert + upload | 1.5 |
| Headroom | 2.6 |
| **Total** | **16.6** |

Techniques:
- SoA everywhere on hot paths; `u8` indexed color halves or quarters memory bandwidth vs RGBA.
- Integer raster with span fills, early bbox cull, and depth-sorted coarse rejection.
- **Pixel-size LOD**: at 80 px, near characters use full capsules; when zoomed out or in 3D distance a character ~20 px tall renders limbs as 1-px lines (Bresenham) instead of capsules; very far ones become a 3–5 px "glyph" drawn from a cached mini-pose.
- Skip animation updates for off-screen characters (sim still runs).
- `simd128` for palette conversion, clears, and batch transforms.
- Zero allocations per frame (arena / reused Vecs); verified with a counting allocator in debug.
- Native: `rayon` across tiles. Web: optional threads later (needs COOP/COEP headers).
- Benchmarks: `criterion` microbenches for raster primitives, plus an in-engine stress scene with a frame-time graph.

---

## 12. Tooling

- **Debug overlay** (toggle F1): FPS, frame-time graph, per-stage timings (puffin), entity counts, bone/joint gizmos, navgrid/path/flow-field visualization, hit capsules, z-buffer view, dirty-chunk view.
- **Pose editor** (`tools/pose_editor`, egui, runs native and in the browser): drag joints (IK-assisted), timeline of key poses, easing curves, live preview of proportions/params, export RON. Hot-reload poses into the running game.
- **Character lab**: sliders for CharacterParams + seed randomizer; export presets.
- **Palette pipeline**: author in Aseprite → export `.gpl` via CLI script → engine hot-reloads.

---

## 13. Milestone roadmap

### M0 — Workspace + wasm hello framebuffer
- Cargo workspace, crates stubbed, CI (fmt, clippy, test, wasm build).
- Web host: 640×360 indexed buffer, palette, animated test pattern, integer upscale. Native host the same.
- **Accept**: `trunk serve` shows the pattern in Chrome/Firefox at 60 fps; `cargo run -p se_host_native` shows the same; frame time under 1 ms for clear + convert + present; CI green.

### M1 — Stick figure rig + walk cycle
- `se_rig` humanoid, `se_anim` procedural walk/run/idle, thick-line/capsule/circle raster with z-buffer, outline pass.
- **Accept**: one figure walks in place with adjustable speed and proportions (sliders in the debug UI); 3 random-seeded figures look distinct; no pixel shimmer when standing still; raster microbenchmarks recorded.

### M2 — Iso world + click-to-move
- Procedural tile dungeon, terrain cache, iso camera with follow, screen→ground picking, A* + smoothing, foot IK on height.
- **Accept**: clicking anywhere walkable moves the player along a sensible path around walls; feet don't slide (stride matches speed); 200 wandering NPCs at 60 fps in the browser.

### M3 — Combat (ARPG core)
- Enemies with flow-field chase + separation, melee + 2 skills (cone, projectile), hit capsules, health, hit-stop/flash/shake, particles, death ragdoll, loot drops with procedural gear that changes the player's look, procedural SFX.
- **Accept**: a 5-minute "arena" loop is fun to play; 500 enemies chasing at 60 fps (web); a picked-up sword visibly appears on the player.

### M4 — 3D mode
- Perspective camera, terrain-to-triangles, depth-scaled capsules, iso↔3D transition (hotkey), picking in 3D.
- **Accept**: pressing Tab transitions over about 0.75 s with no popping; gameplay (click-to-move, combat) works in both modes; 300 characters at 60 fps in 3D (web).

### M5 — Performance & polish
- LOD tiers, SIMD, zero-alloc audit, native threading, palette effects (day/night, fog dithering), pose editor v1.
- **Accept**: **1000+ characters at 60 fps at 640×360 in the browser** in the stress scene (iso), with a documented frame breakdown; wasm under 1 MB gzipped; pose editor can author and hot-reload an attack.

Later: GPU SDF path, more body plans (quadruped sticks), level editor.

---

## 14. Risks

| Risk | Mitigation |
|---|---|
| Single-threaded wasm CPU raster misses the 1000-character target | Pixel-size LOD, glyph impostors, 30 Hz anim for distant characters; GPU SDF fallback path is designed in from the start. |
| Stick figures read poorly (thin limbs) | Outline pass, team-color accents, exaggerated poses, test readability in M1. |
| Iso↔3D transition looks odd (pixel sizes change, swimming) | Dolly-zoom matching, pixel-snapped camera, stick to ground-plane gameplay. |
| Procedural animation feels floaty | Key poses for actions, plus IK foot planting and hit-stop. Budget polish time in M3. |
| Scope creep toward a general engine | Purpose-built; each milestone ships a playable example. |
| OneDrive syncing `target/` (huge, slow) | Put the build dir outside OneDrive (`CARGO_TARGET_DIR=C:\Users\raxac\cargo-target\stick-engine`) or exclude `target/` from sync. |

---

## 15. Decisions (answered 2026-10-20)

1. **Standalone.** No Godot integration.
2. **No deterministic mode / replays.** Plain f32 sim with a fixed timestep.
3. **Characters ~80 px tall**, so the internal resolution is 640×360 (see 6.1).
4. **Mobile and multiplayer are out of scope.**
5. 3D mode design is kept (section 7) and scheduled for M4.

## 16. Animation system (implemented in M1 prototype)

- **Pose** = 16 named channels (`lean, twist, bob, l_hip, l_knee, r_hip, r_knee, l_shoulder, l_elbow, l_arm_out, r_shoulder, r_elbow, r_arm_out, sword_yaw, sword_pitch, head`), in degrees except `bob` (metres).
- **Clip** = RON file in `crates/stick_engine/assets/clips/`: `name`, `duration`, `looping`, optional `base` pose, `keys: [(t, pose:{channel: value})]`, optional `events: [(t, name)]` (e.g. `hit_start`/`hit_end` active frames). Missing channels inherit from `base`. Keys are smoothstep-interpolated; looping clips wrap.
- **AnimLibrary** registers clips (built-ins via `include_str!`, or `register_ron` at runtime for hot-reload).
- **Animator** plays one clip with cross-fade from the previous output pose.
- **State machine** (`character.rs`): `Action` enum + `ACTIONS` table mapping each action to clip, blend time and mode (`Loop{ref_speed}` scales playback rate by ground speed; `OneShot{next}` is uninterruptible and returns to `next`).
- Adding a move = clip file + `BUILTIN_CLIPS` line + `Action` variant + `ACTIONS` row + trigger in `Character::step`. See README.

## 17. Open questions

1. Art direction: palette mood (current: dark gothic stone + bone-white figures), outlines on/off.
2. Should 3D mode be fully playable or a cinematic/alternate view?
3. Which moves next (block, combo chain, bow, spells, dodge-roll)?
4. License / open source?
