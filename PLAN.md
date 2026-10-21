# Stick Engine — Technical Plan

A super-fast, pixel-based game engine in **Rust → WASM** (browser) and native. Every character is a **stick person**, drawn procedurally into a pixel framebuffer. **No sprites, no 3D models.** It supports isometric ARPG play and can later blend into a **3D perspective mode** using the same skeleton data.

This file covers **engine-generic** design only. Game-specific content (classes, item rosters, loot-look grammar, networking and server integration) belongs to the games that use the engine. Black Anvil's integration plan lives in BA-Stick as [`docs/STICK-ENGINE-INTEGRATION.md`](https://github.com/BlackAnvilNetwork/BA-Stick/blob/main/docs/STICK-ENGINE-INTEGRATION.md). The engine ships only a small example archetype and example items (sword, greatsword, shield, cuirass) for demos and tests.

License: **GPL-3.0-or-later**.

---

## 1. Goals / Non-goals

**Goals**
- Everything is drawn procedurally into a CPU framebuffer: bones as thick pixel capsules, heads as discs, gear as shape primitives attached to joints.
- 1000+ animated stick characters at 60 fps at a **640×360** internal resolution, integer-upscaled.
- Characters are **~80 px tall** on screen.
- The same build runs in the browser (wasm32) and natively (Windows/Linux/macOS).
- ARPG feature set: click-to-move, pathfinding, crowds, skills, loot visuals, hit effects, particles.
- **Data-driven content**: actions, clips, gear and archetypes are RON files; adding a move needs no Rust change.
- **Headless-capable core**: the rig (FK), clip metadata, actions, character definitions and the state machine have no drawing or platform dependencies, so tools, tests and game servers can use them.
- **Pure-function cosmetics**: a character's look is a function of `(seed, archetype)` or an explicit `CharacterDef`, and its animation is a function of the actions it plays. A game can replicate seeds and action events instead of poses.
- Camera modes: isometric 2.5D now, 3D perspective later.

**Non-goals (for now)**
- No sprite sheets, texture atlases, or mesh/model importers.
- No general-purpose editor. Tools are small and purpose-built.
- **No networking code in the engine.** Multiplayer is a game-layer concern; the engine provides the deterministic/headless building blocks (§9).
- No bit-exact simulation determinism. Plain f32; discrete look generation uses integer RNG.
- No mobile target. No physically based lighting.

---

## 2. Code layout

Today the engine is one core crate with modules, plus two hosts. The modules are kept separable so they can be split into crates when needed.

```
crates/stick_engine/
  src/
    math.rs       Vec3, rotate/cross, angle helpers, Pcg32 (integer RNG)
    anim.rs       Ch-indexed Pose, masks, compose, Clip, ClipMeta, AnimLibrary, Animator      [headless]
    rig.rs        Joint/Bone enums, Joints, FK solve                                         [headless]
    body.rs       Sex presets, BodyParams, Personality, CharacterDef, Archetype, from_seed   [headless]
    gear.rs       GearSlot, GearDef shapes, frames, stance derivation, GearLib               [headless]
    gait.rs       procedural walk/run generator                                               [headless]
    actions.rs    ActionTable from actions.ron                                                [headless]
    character.rs  generic action state machine + windows from clip events                    [headless]
    color.rs      ColorSlot, RampMap                                                          [palette data]
    raster.rs     framebuffer, capsule/disc/line, ramp palette + 256 LUT
    render.rs     draw_body / draw_character (body + gear), DrawOpts (outline)
    camera.rs     iso projection + ground picking
    game.rs       demo scene (walk, run, dash, swing, click-to-move)
  assets/
    actions.ron   action table
    clips/*.ron   animation clips
    gear/*.ron    example gear
    archetypes/*.ron  example archetype
crates/stick_web/     wasm-bindgen host (canvas, input, rAF loop)
crates/stick_native/  winit + softbuffer host
```

"Headless" modules never call raster drawing or camera code. They only use palette *constants* (ramp ids) from `raster`. The planned split is `se_math`, `se_data` (actions/clip meta/gear/archetype loaders), `se_rig`, `se_anim`, `se_raster`, `se_render`, `se_world`, `se_sim`, `se_audio` and hosts, with the same rule: nothing headless depends on rendering.

---

## 3. Recommended crates

| Need | Crate | Why |
|---|---|---|
| Math | `glam` (later) | SIMD, wasm-friendly. The prototype uses a tiny own `Vec3`. |
| ECS | `hecs` (start) | Small, cache-friendly, no scheduler magic. |
| WASM glue | `wasm-bindgen`, `web-sys`, `js-sys` | Standard, stable. |
| Build | `trunk` | Dev server with reload. |
| Native window | `winit` + `softbuffer` | CPU buffer straight to the window. |
| GPU blit / later GPU path | `wgpu` | Optional. |
| Pathfinding | `pathfinding` (A*) + custom flow field | A* for agents; flow fields for crowds. |
| Spatial queries | custom uniform grid | Simple and fast for uniform crowds. |
| RNG | own `Pcg32` (in `math`) | Integer state, identical on wasm and native, for look generation. |
| Serialization | `serde` + `ron` | Human-readable data, hot-reloadable. |
| Audio | `cpal`, WebAudio | Thin abstraction; procedural SFX. |
| Tools UI | `egui` / `eframe` | Also runs in wasm. |
| Profiling | `puffin`, `tracy-client` | Frame-level timing. |
| SIMD | `wide` + wasm `simd128` | Stable Rust. |
| Threads (native) | `rayon` | Parallel tile raster. |

---

## 4. Character system

### 4.1 Principles
- **One skeleton.** Sex, archetype and gear change parameters, never the rig topology.
- Players, NPCs and crowd enemies all use `CharacterDef` and the same anim/render path.
- **Colour is always a slot** (`ColorSlot`), resolved to a ramp by a per-draw `RampMap` (§6.3).

### 4.2 Skeleton
- **16 joints** (`enum Joint`): pelvis, chest, neck, head, l/r_hip, l/r_knee, l/r_foot, l/r_shoulder, l/r_elbow, l/r_hand. `Joints` stores `[Vec3; 16]` indexed by joint, plus the two weapon directions and the body forward/right axes.
- **13 drawn bones** (`enum Bone`, each with its own thickness multiplier): hips, spine, neck, shoulders, head, l/r_thigh, l/r_shin, l/r_upper_arm, l/r_forearm.
- Joints are **3D**; the iso view is just a projection.
- Joint and bone names in RON resolve at load time; unknown names are load errors.
- Extra bones (tail, cape chain) are planned, not implemented.

### 4.3 CharacterDef & BodyParams
```rust
pub struct CharacterDef {
    pub seed: u64,
    pub sex: Sex,                         // Male | Female
    pub body: BodyParams,
    pub colors: RampMap,                  // base slot -> ramp map
    pub personality: Personality,         // slouch (deg), gait_speed, arm_swing
    pub gear: [Option<GearRef>; 9],       // GearRef { base: u16, look_seed: u32 }
}
pub struct BodyParams {
    pub height, thigh, shin, upper_arm, forearm, torso, neck, head_r: f32,
    pub shoulder_w, hip_w: f32,           // FULL widths (the rig uses half)
    pub limb_px, thickness: f32,
    pub bone_thickness_mul: [f32; N_BONES],
}
```
- `effective_body(&GearLib)` multiplies the gear's per-bone `thickness_mul` into the body.
- `stance(&GearLib)` derives the stance from the hands (§5.5).
- `swing_weight(&GearLib)` is the main-hand weapon's clip-duration multiplier.

### 4.4 Sex presets (starting values)
| | Male | Female |
|---|---|---|
| height (m) | 1.76 (segment sum) | ≈1.64 (scaled ×1.68/1.80) |
| shoulder_w (full) | 0.40 | 0.34 |
| hip_w (full) | 0.22 | 0.26 |
| thickness | 1.00 | 0.92 |
| head_r | 0.130 | 0.125 |
| gait: twist mul / arm mul | 0.8 / 1.0 | 1.2 / 0.9 |
| gait: hip sway (m) | 0.010 | 0.022 |

The widths are tuned to the prototype's stylised look (big head, narrow hips), not to realistic anatomy. **Readability:** at ~44 px/m, small width differences are 2–3 px, so the sexes must differ mainly through **motion** (twist, hip sway, arm carriage) and silhouette gear. Exaggerate proportions if needed.

### 4.5 Gear
Gear = shape primitives attached to a joint, in a joint-local frame (metres). Hands use the weapon direction (`r_wpn_*` / `l_wpn_*`) as +y; other joints use the spine axis.
```ron
// assets/gear/greatsword.ron
( id: "greatsword", slot: MainHand, stance: Some("2h"), reach: 1.35, swing_weight: 1.4,
  material: Some("steel"),
  shapes: [
    Capsule(from: (0.0, -0.18, 0.0), to: (0.0, 0.0, 0.0), ramp: Trim, thick: 1.4),
    Capsule(from: (-0.14, 0.0, 0.0), to: (0.14, 0.0, 0.0), ramp: Trim, thick: 1.2),
    Capsule(from: (0.0, 0.0, 0.0), to: (0.0, 1.30, 0.0), ramp: Material, thick: 1.6),
  ], )
// assets/gear/cuirass.ron
( id: "cuirass", slot: Chest, material: Some("steel"),
  thickness_mul: { "spine": 1.45, "l_upper_arm": 1.15, "r_upper_arm": 1.15, "shoulders": 1.2 },
  shapes: [ Disc(at: Some("l_shoulder"), r: 0.075, ramp: Material), Disc(at: Some("r_shoulder"), r: 0.075, ramp: Material) ], )
```
- Primitives: `Line`, `Capsule`, `Disc` (at a joint, or at a frame-local centre). `Poly` and `Arc` are planned.
- Slots: Head, Shoulders, Chest, Hands, Legs, Feet, Back, MainHand, OffHand.
- `reach` (length of the striking part) and `swing_weight` (clip duration multiplier) are the only timing-relevant fields. Stats belong to the game.
- `material` overrides the `Material` slot while that item is drawn.
- **Loot-look grammars** (mods, rarity → shapes from a look seed) are game content. The engine carries `GearRef::look_seed` for them.

### 4.6 Archetypes & seeds
`CharacterDef::from_seed(seed, &Archetype, &GearLib)`:
- The archetype (RON) gives the ranges: sex (fixed or random), height per sex, shoulder/thickness multipliers, personality, accent ramps, and gear choices per slot (`""` = empty).
- `Pcg32` picks the values. Floats are quantised to 1/1024 steps and slots are visited in a fixed (`BTreeMap`) order, so the same `(seed, archetype)` gives an identical `CharacterDef` on every platform. A tested invariant.
- A game can therefore replicate a crowd enemy as `(seed, archetype id)`.
- `assets/archetypes/example_warrior.ron` is the demo/test archetype.

---

## 5. Animation

### 5.1 Pose: 19 indexed channels
`lean, twist, bob, l_hip, l_knee, r_hip, r_knee, l_shoulder, l_elbow, l_arm_out, r_shoulder, r_elbow, r_arm_out, r_wpn_yaw, r_wpn_pitch, l_wpn_yaw, l_wpn_pitch, head, root_pitch`

These are degrees, except `bob` (metres).
- `Pose` is `[f32; 19]` addressed by `Ch` enum index. Names are resolved only when a clip loads.
- `r_wpn_*` (formerly `sword_*`) and `l_wpn_*` aim the right and left hand items, for dual-wield and off-hand items.
- `root_pitch` rotates the **whole body** about the pelvis's right axis (rolls, knockdowns, get-ups), and the body is lifted to stay above ground. `lean` only bends the spine.
- Inside a clip, keys interpolate linearly, so a clip may sweep 0→360. **Cross-fades use the shortest arc**, and the cross-fade source is wrapped to (−180, 180], so leaving a roll never spins back.

### 5.2 Layers and masks
Masks are `u32` channel bitsets with presets:
- `LOWER` = bob, hips, knees
- `UPPER` = twist, shoulders, elbows, arm_out, all weapon channels
- `SHARED` = lean, head. Layers *add* on these.
- `FULL` = everything

A clip declares `mask: Upper | Lower | Full | Additive` (default `Upper`).

| Layer | Source |
|---|---|
| 1. Base | gait generator while moving, blended with the stance's idle clip when slow |
| 2. Action | the current action's clip, composed by its mask (`Full` replaces the base) |
| 3. Additives | fire-and-forget additive clips (flinch, …) added on top |
| 4. IK / ragdoll | planned |

Every action change cross-fades from the last output pose over the action's `blend` time.

### 5.3 Gait generator
Walk/run are procedural (`gait.rs`); there are no walk/run clips.
```
leg = thigh + shin;  Fr = v² / (g·leg);  w = smoothstep(0.6, 1.5, Fr)   // walk 0 .. run 1
stride = leg · lerp(1.3, 2.2, w);  phase += dist · gait_speed / stride
hip = lerp(25, 50, w) · sin(2πφ) (right +π);  knee = lerp(8, 70, w) · max(0, cos(2πφ + 0.6)) + 5
shoulder = −0.7 · hip · arm_swing · sex.arm_mul;  elbow = 15 + 60w
bob = lerp(0.015, 0.05, w) · cos(4πφ) − 0.02;  twist = (4 + 6w) · sex.twist_mul · sin(2πφ)
lean = 2 + 10w + 4 · clamp(accel/10, −1, 1) + slouch;  hip sway = sex.hip_sway · sin(2πφ)
```
The thresholds (0.6, 1.5) keep 2.2 m/s a walk (Fr ≈ 0.54) and 5.2 m/s a full run (Fr ≈ 3.0) for every preset leg length. The patch's (0.35, 0.65) would have turned the walk into a jog. Planned: sprint/sneak/strafe variants, foot-plant IK, breathing additive.

### 5.4 Clip schema v2
```
name, duration, looping, base, keys, events        // as before
mask: Upper | Lower | Full | Additive              (default Upper)
speed_scale: f32                                   (default 1.0)
hold: Some((t0, t1))                               loop inside until released
stance: Some("2h")                                 metadata
events: hit_start/hit_end, iframes_start/iframes_end, cancel_open/cancel_close,
        release, commit, footstep, fx_*, sfx_*
```
- Unknown channels, unknown events (except `fx_`/`sfx_`) and unpaired start/end events are load errors.
- **Events are the source of truth for gameplay timing.** The state machine opens and closes `hit`, `iframes` and `cancel` windows from them, for any action.
- **`ClipMeta::from_ron`** parses only the timing fields (keys and base are skipped). It's meant for headless consumers (tools, validators, a game server that needs hit timings). It is protocol-agnostic.

### 5.5 Stances and lookup
- The stance is derived from the hands: none → `unarmed`; 1h + shield → `1h_shield`; 1h + 1h → `dual`; otherwise the main-hand stance (`1h`, `2h`, `staff`, …).
- Clip lookup for `(action, stance)`: `stances[stance]` → `stances["any"]` → *unavailable* (the action can't start).

### 5.6 Built-in clips (demo)
`idle` (Full), `dash` (Full), `swing` (Full, hit + cancel windows), `dodge_roll` (Full, root_pitch 0→360, i-frames), `hit_flinch` (Additive).

---

## 6. Renderer

### 6.1 Framebuffer
640×360, `u8` palette index + f32 depth. Present converts through a **256-entry LUT** (`raster::LUT`).

### 6.2 Primitives
Capsules (distance-to-segment, span version planned), discs, Bresenham lines, shadow ellipses. Triangles/polys are planned (gear `Poly`, 3D terrain).

### 6.3 Ramps, slots and RampMap
- Palette = ramps of `RAMP_LEN = 6` shades; index = `ramp × 6 + shade`.
- There are 14 built-in ramps: void, stone, bone, blood, steel, gold, leather, dust, arcane, white, fire, ice, team_blue and team_green. The LUT has room for 42.
- `.gpl` loading from Aseprite is planned.
- Bodies and gear reference `ColorSlot`s: Body, BodyFar, Outline, Accent, Material, Trim, Glow.
- `RampMap { ramps: [u8; 7], flash }` resolves a slot to a ramp per draw:
  - `team(r)` overrides Accent.
  - `tint(r)` overrides Body and BodyFar (elemental tint).
  - `flashed()` sends every slot except Outline to `white` (hit flash).
- Planned: Bayer dither shading, day/night and fog as LUT transforms.

### 6.4 Outline
**On by default** (`DrawOpts::outline`). Every capsule/disc writes a 1 px ring in the Outline slot at a slightly deeper depth, which separates overlapping limbs at no extra cost. A depth-edge post pass is optional later (props/3D).

### 6.5 Upload / present
- Web: `putImageData` today, WebGL2 blit planned.
- Native: softbuffer with integer scaling.

### 6.6 Draw pipeline
`sim → per-character layered pose → rig::solve (headless) → render::draw_character (project + body + gear via RampMap) → particles/trails → present`. Tile binning and parallel raster are planned.

---

## 7. Camera modes
- **Iso 2.5D**: 2:1 dimetric, ground picking (`unproject_ground`).
- **3D perspective** (planned): the same joints and capsules, radius scaled by 1/z, with an ortho→persp dolly-zoom transition. Gameplay stays on the ground plane.

## 8. World
1 m tile grid with height and material, procedurally generated and drawn (today: procedural iso floor). Planned: dungeon generator, static terrain cache, navgrid.

## 9. Simulation (generic)
- Fixed 60 Hz timestep in the demo, with an accumulator.
- `Character::step(lib, actions, intent, dt)` is a generic state machine. `Intent { dir, run, request: Option<action id>, release, aim }`.
  - Busy actions (OneShot/Cast) block other requests outside their **cancel window**.
  - Requesting the same action inside the window starts its `chain`.
  - `Cast` holds inside the clip's `hold` range until `release`.
  - Per-action `cooldown`, `move_mult`, `turn_rate`, `face` (Keep/Move/Aim), and `root_motion: Dash{speed}`.
  - Weapon actions scale their duration by `swing_weight`.
- **Building blocks for multiplayer games** (no protocol in the engine):
  - headless FK
  - `ClipMeta` timings
  - deterministic `from_seed`
  - cosmetics as a pure function of `(CharacterDef, action, start time)`
- Planned: ECS, pathfinding, crowds (flow fields), combat helpers (e.g. hit sectors baked from clip sweeps), particles pool, procedural audio.

## 10. Hosts
- Web: wasm-bindgen + rAF + canvas, built with `trunk`.
- Native: winit + softbuffer.
- Both consume `Game`, `Input` and the LUT.

## 11. Performance budget
Target: 1000+ characters at 60 fps at 640×360 in single-threaded wasm.

| Stage | ms |
|---|---|
| Sim | 3.0 |
| Animation (layered, LOD) | 3.0 |
| FK + projection + draw list | 2.0 |
| Raster | 5.5 |
| Post + LUT + upload | 1.5 |
| Headroom | 1.6 |

Techniques: index-addressed channels/joints (no strings on hot paths), SoA, span fills, pixel-size LOD, skipping off-screen animation, `simd128`, zero per-frame allocation.

## 12. Tooling
- Debug overlay (planned).
- Pose editor: drag joints, timeline, event markers, mask picker, RON export, hot-reload via `AnimLibrary::register_ron`.
- Character lab: CharacterDef sliders, seed randomiser, gear try-on, RampMap preview.
- Data validator: every action's clips exist (a test today).

## 13. Milestones
- **M0** workspace + wasm framebuffer ✅
- **M1** rig + walk ✅ (plus run, dash, swing, iso floor, WASD/right-click)
- **M2** character & animation core ✅ on branch `engine-upgrade-character-anim`: 19-channel indexed pose, shortest-arc blending, masks/layers, clip schema v2, ClipMeta, headless rig, root_pitch, CharacterDef/presets/archetypes, gear + stances, actions.ron, gait, ramp palette + RampMap, outline toggle. *Remaining:* M/F readability A/B, breathing additive, Poly/Arc shapes, extra bones/capes, `.gpl` loading, crate split.
- **M3** iso world + click-to-move: dungeon gen, terrain cache, A*, foot IK. Accept: 200 NPCs at 60 fps (web).
- **M4** combat helpers: flow-field crowds, hit tests from clip windows + weapon reach, hit-stop/flash/shake, particles, ragdoll. Accept: 500 chasing enemies at 60 fps (web).
- **M5** performance & polish: LOD, span raster, SIMD, WebGL2 blit, dither/fog, pose editor v1. Accept: 1000+ characters at 60 fps (web).
- **M6** 3D mode (optional view).

## 14. Risks
| Risk | Mitigation |
|---|---|
| wasm CPU raster misses 1000 chars | LOD, glyph impostors, GPU SDF fallback |
| Thin limbs read poorly | Outline on by default, accent ramps, big heads |
| M/F unreadable at 80 px | Motion cues (twist, hip sway), silhouette gear, exaggeration |
| Clip budget explodes with stances | `any` fallbacks first, procedural locomotion |
| Data typos | Load-time validation of channels, events, joints, bones, ramps, action refs |
| Procedural motion feels floaty | Key-posed actions, IK, hit-stop |
| OneDrive syncing `target/` | `CARGO_TARGET_DIR` outside OneDrive |

## 15. Decisions (2026-10-21)
1. Standalone engine, no Godot.
2. Plain f32 sim; integer RNG for look generation; no bit-exact replays.
3. ~80 px characters at 640×360.
4. No networking inside the engine; it stays protocol-agnostic. Mobile is out of scope.
5. One skeleton; 19 indexed channels including `root_pitch`; shortest-arc cross-fades.
6. Animation keyed by (action, stance) with `stance → any → unavailable`.
7. Actions are data (`actions.ron`).
8. Colour is always a slot → ramp via `RampMap`; outline on by default.
9. Rig maths is headless; drawing lives in `render`.
10. License GPL-3.0-or-later.

## 16. Animation system: data-driven actions
`assets/actions.ron` replaces `enum Action` + `ACTIONS`:
```ron
[
    (id: "idle", mode: Idle, blend: 0.15, stances: { "any": "idle" }),
    (id: "walk", mode: Gait(speed: 2.2), blend: 0.15),
    (id: "run", mode: Gait(speed: 5.2), blend: 0.12),
    (id: "dash", mode: OneShot(next: "idle"), blend: 0.04, stances: { "any": "dash" },
        move_mult: 0.0, root_motion: Some(Dash(speed: 13.0)), cooldown: 0.45, face: Move),
    (id: "swing", mode: OneShot(next: "idle"), blend: 0.05, stances: { "any": "swing" },
        move_mult: 0.2, face: Aim, weapon: true),
    (id: "dodge", mode: OneShot(next: "idle"), blend: 0.03, stances: { "any": "dodge_roll" },
        move_mult: 0.0, root_motion: Some(Dash(speed: 7.0)), cooldown: 0.6, face: Move),
    (id: "flinch", mode: Additive, blend: 0.0, stances: { "any": "hit_flinch" }),
]
```
- Modes: `Idle`, `Gait{speed}`, `OneShot{next}`, `Cast{next}`, `Additive`.
- `idle`, `walk` and `run` are required. References (`next`, `chain`) are validated.
- **Adding a move = one clip file + one row.** Built-in clips are currently embedded with `include_str!`; runtime loading uses `AnimLibrary::register_ron` / `ActionTable::from_ron`.

## 17. Open questions
1. Art direction: dark gothic stone + bone-white figures, outline on (decided).
2. 3D mode: optional view (decided).
3. Strafe/aim: free-facing upper body (twist follows aim, ±70°) recommended; not implemented yet.
4. Dual-wield: done at channel level (`l_wpn_*`); dual clips still to author.
5. Directory-based asset discovery vs embedded bundle for web: open.

## 18. Implementation status vs earlier findings
| Finding | Status |
|---|---|
| String channel lookup on hot paths | ✅ `Ch` indices |
| Linear degree cross-fades (360→0 spin) | ✅ shortest arc + wrapped source |
| Rolls via `lean` (legs ignore it, head inverts) | ✅ `root_pitch`; head relative to spine |
| Half-width naming | ✅ full widths in `BodyParams` |
| 16-colour palette, `& 15` | ✅ ramp model + 256 LUT |
| FK mixed with drawing | ✅ `rig` (headless) / `render` |
| Single-clip animator | ✅ base + action + additive layers with masks |
| Uninterruptible OneShots | ✅ cancel windows from clip events |
| `hit_active` tied to Swing | ✅ windows from events for any action |
| Hard-coded walk/run clips | ✅ gait generator |
| Speeds as consts | ✅ `Gait{speed}` rows in actions.ron |

---

## Reconciliation notes

The 2026-10-21 Character/Gear/Animation patch was split between this file and BA-Stick's integration doc.

| Patch item | Where | Outcome |
|---|---|---|
| One skeleton, params only | engine §4.1 | Adopted |
| (Action, stance) → clip with fallback | engine §5.5 | Adopted (`stance → any → unavailable`) |
| actions.ron replaces enum Action | engine §16 | Adopted and implemented |
| Cosmetics as pure functions of seeds/events | engine §1, §9 | Adopted as a generic property; replication is game-side |
| Outline on by default | engine §6.4 | Improved: per-primitive ring (cheap, separates limbs) |
| Colour = ramp slot | engine §6.3 | Adopted |
| RampMap([u8;16]) | engine §6.3 | Improved: a 7-slot map is smaller and expresses team/tint/flash directly |
| CharacterDef / BodyParams / Slot | engine §4.3 | Adopted; explicit full widths; `GearRef{base, look_seed}` |
| Sex presets | engine §4.4 | Improved: widths tuned to the prototype look; head 0.130/0.125 |
| Gear primitives on bones | engine §4.5 | Adopted (Line/Capsule/Disc now; Poly/Arc planned) |
| reach / swing_weight | engine §4.5 | Adopted; swing_weight scales weapon clip duration |
| Loot grammar ItemLook | BA-Stick doc | Moved (game content) |
| 7 classes, class rows, class clips | BA-Stick doc | Moved; the engine keeps an example archetype |
| from_seed(seed, archetype) | engine §4.6 | Adopted and implemented, deterministic |
| Masks / layers | engine §5.2 | Adopted; bitsets plus a Lower mask |
| Gait generator | engine §5.3 | Improved: thresholds (0.6, 1.5) |
| Clip schema v2, events as truth | engine §5.4 | Adopted plus validation |
| Server loads events-only clips | engine §5.4 | Kept as generic `ClipMeta`, not tied to a protocol |
| Rolls past ±180 | engine §5.1 | Improved: `root_pitch` instead of `lean` |
| Dual-wield 18 channels | engine §5.1 | Improved: 19 with `root_pitch` |
| Strafe/aim | engine §17 | Recommended, not yet implemented |
| MMO / server authority / ActionEvent / tick rates | BA-Stick doc | Moved; deferred there (BA-Stick keeps bichael's protocol) |
| Milestones M1.5/M3a-c/M4+ | engine §13 + BA-Stick doc | Engine parts folded into M2/M4; game parts in BA-Stick |
