// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.

//! Data-driven animation.
//!
//! * A [`Pose`] is a fixed array of 19 joint channels, addressed by [`Ch`]
//!   index (strings are only resolved when a clip is loaded).
//! * A [`Clip`] is a list of keyed poses loaded from RON, with a channel
//!   [`Mask`], optional hold section and named events.
//! * [`ClipMeta`] is the events-only view of a clip (no keys), usable by a
//!   headless process that needs timing but never poses.
//! * [`Animator`] layers an action clip and additive clips over a base pose
//!   (gait or idle) using masks, and cross-fades on every change with
//!   shortest-arc blending.
use serde::Deserialize;
use std::collections::HashMap;

/// Pose channels. Angles are degrees, `Bob` is metres.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Ch {
    Lean, Twist, Bob, LHip, LKnee, RHip, RKnee, LShoulder, LElbow, LArmOut,
    RShoulder, RElbow, RArmOut, RWpnYaw, RWpnPitch, LWpnYaw, LWpnPitch, Head, RootPitch,
}

/// Channel names as used in RON, in [`Ch`] order.
pub const CHANNELS: [&str; 19] = [
    "lean", "twist", "bob", "l_hip", "l_knee", "r_hip", "r_knee", "l_shoulder", "l_elbow",
    "l_arm_out", "r_shoulder", "r_elbow", "r_arm_out", "r_wpn_yaw", "r_wpn_pitch",
    "l_wpn_yaw", "l_wpn_pitch", "head", "root_pitch",
];
pub const N: usize = CHANNELS.len();

/// Channel index by RON name.
pub fn channel(name: &str) -> Option<usize> {
    CHANNELS.iter().position(|c| *c == name)
}

/// Wrap degrees to (-180, 180].
pub fn wrap_deg(a: f32) -> f32 {
    let w = (a + 180.0).rem_euclid(360.0) - 180.0;
    if w <= -180.0 { w + 360.0 } else { w }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose(pub [f32; N]);

impl Pose {
    #[inline]
    pub fn get(&self, c: Ch) -> f32 {
        self.0[c as usize]
    }
    #[inline]
    pub fn set(&mut self, c: Ch, v: f32) {
        self.0[c as usize] = v;
    }
    /// Plain per-channel lerp. Used between keys inside one clip, so a clip
    /// may deliberately sweep past ±180 (e.g. a roll keyed 0 -> 360).
    pub fn lerp(&self, o: &Pose, t: f32) -> Pose {
        let mut p = *self;
        for i in 0..N {
            p.0[i] += (o.0[i] - p.0[i]) * t;
        }
        p
    }
    /// Cross-fade blend: angle channels take the shortest arc, `bob` is linear.
    pub fn blend(&self, o: &Pose, t: f32) -> Pose {
        let mut p = *self;
        for i in 0..N {
            let d = if i == Ch::Bob as usize { o.0[i] - p.0[i] } else { wrap_deg(o.0[i] - p.0[i]) };
            p.0[i] += d * t;
        }
        p
    }
    /// All angle channels wrapped to (-180, 180].
    pub fn wrapped(&self) -> Pose {
        let mut p = *self;
        for i in 0..N {
            if i != Ch::Bob as usize {
                p.0[i] = wrap_deg(p.0[i]);
            }
        }
        p
    }
}

/// Channel bitset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mask(pub u32);

const fn bits(chs: &[Ch]) -> u32 {
    let mut m = 0;
    let mut i = 0;
    while i < chs.len() {
        m |= 1 << chs[i] as u32;
        i += 1;
    }
    m
}

impl Mask {
    pub const LOWER: Mask = Mask(bits(&[Ch::Bob, Ch::LHip, Ch::LKnee, Ch::RHip, Ch::RKnee]));
    pub const UPPER: Mask = Mask(bits(&[
        Ch::Twist, Ch::LShoulder, Ch::LElbow, Ch::LArmOut, Ch::RShoulder, Ch::RElbow, Ch::RArmOut,
        Ch::RWpnYaw, Ch::RWpnPitch, Ch::LWpnYaw, Ch::LWpnPitch,
    ]));
    /// Channels that layers add together (gait lean + clip lean).
    pub const SHARED: Mask = Mask(bits(&[Ch::Lean, Ch::Head]));
    pub const FULL: Mask = Mask((1 << N) - 1);
    #[inline]
    pub fn has(self, i: usize) -> bool {
        self.0 & (1 << i) != 0
    }
}

/// How a clip combines with the layers below it.
#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaskKind {
    /// Owns the upper-body channels; adds to lean/head; legs keep the base.
    #[default]
    Upper,
    /// Owns the leg channels; adds to lean/head.
    Lower,
    /// Owns every channel (rolls, knockdowns): base is ignored.
    Full,
    /// Adds its channels as deltas on top (flinch, breathing, aim offset).
    Additive,
}

/// Overlay `layer` on `base` with weight `w` according to `kind`.
pub fn compose(base: &Pose, layer: &Pose, kind: MaskKind, w: f32) -> Pose {
    let mut p = *base;
    let own = match kind {
        MaskKind::Upper => Mask::UPPER,
        MaskKind::Lower => Mask::LOWER,
        MaskKind::Full => Mask::FULL,
        MaskKind::Additive => Mask(0),
    };
    for i in 0..N {
        if kind == MaskKind::Additive || (kind != MaskKind::Full && Mask::SHARED.has(i)) {
            p.0[i] += layer.0[i] * w;
        } else if own.has(i) {
            let d = if i == Ch::Bob as usize { layer.0[i] - p.0[i] } else { wrap_deg(layer.0[i] - p.0[i]) };
            p.0[i] += d * w;
        }
    }
    p
}

#[derive(Deserialize, Debug, Clone)]
struct KeyDef {
    t: f32,
    pose: HashMap<String, f32>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct EventDef {
    pub t: f32,
    pub name: String,
}

/// Event names with engine meaning. `fx_*` / `sfx_*` are free-form.
pub const KNOWN_EVENTS: &[&str] = &[
    "hit_start", "hit_end", "iframes_start", "iframes_end", "cancel_open", "cancel_close",
    "release", "commit", "footstep",
];

/// Fields shared by the full clip and the events-only view.
#[derive(Deserialize, Debug, Clone)]
struct MetaDef {
    name: String,
    duration: f32,
    #[serde(default)]
    looping: bool,
    #[serde(default)]
    stance: Option<String>,
    #[serde(default)]
    mask: MaskKind,
    #[serde(default = "one")]
    speed_scale: f32,
    #[serde(default)]
    hold: Option<(f32, f32)>,
    #[serde(default)]
    events: Vec<EventDef>,
}
fn one() -> f32 {
    1.0
}

#[derive(Deserialize, Debug, Clone)]
struct ClipDef {
    name: String,
    duration: f32,
    #[serde(default)]
    looping: bool,
    #[serde(default)]
    stance: Option<String>,
    #[serde(default)]
    mask: MaskKind,
    #[serde(default = "one")]
    speed_scale: f32,
    #[serde(default)]
    hold: Option<(f32, f32)>,
    #[serde(default)]
    events: Vec<EventDef>,
    #[serde(default)]
    base: HashMap<String, f32>,
    keys: Vec<KeyDef>,
}

/// Timing-only view of a clip: everything except poses.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipMeta {
    pub name: String,
    pub duration: f32,
    pub looping: bool,
    pub stance: Option<String>,
    pub mask: MaskKind,
    pub speed_scale: f32,
    pub hold: Option<(f32, f32)>,
    pub events: Vec<EventDef>,
}

impl ClipMeta {
    fn from_def(d: MetaDef) -> Result<ClipMeta, String> {
        if d.duration <= 0.0 {
            return Err(format!("clip '{}' needs duration > 0", d.name));
        }
        if d.speed_scale <= 0.0 {
            return Err(format!("clip '{}': speed_scale must be > 0", d.name));
        }
        if let Some((a, b)) = d.hold {
            if !(0.0 <= a && a < b && b <= d.duration) {
                return Err(format!("clip '{}': bad hold range", d.name));
            }
        }
        let mut events = d.events;
        for e in &events {
            let free = e.name.starts_with("fx_") || e.name.starts_with("sfx_");
            if !free && !KNOWN_EVENTS.contains(&e.name.as_str()) {
                return Err(format!("clip '{}': unknown event '{}'", d.name, e.name));
            }
        }
        for (a, b) in [("hit_start", "hit_end"), ("iframes_start", "iframes_end"), ("cancel_open", "cancel_close")] {
            let na = events.iter().filter(|e| e.name == a).count();
            let nb = events.iter().filter(|e| e.name == b).count();
            if na != nb {
                return Err(format!("clip '{}': '{a}' and '{b}' must pair up", d.name));
            }
        }
        events.sort_by(|a, b| a.t.total_cmp(&b.t));
        Ok(ClipMeta {
            name: d.name, duration: d.duration, looping: d.looping, stance: d.stance, mask: d.mask,
            speed_scale: d.speed_scale, hold: d.hold, events,
        })
    }

    /// Parse only the timing data of a clip RON file. Keys and base poses are
    /// skipped, so this needs no pose/rig code (headless use).
    pub fn from_ron(src: &str) -> Result<ClipMeta, String> {
        let d: MetaDef = ron::from_str(src).map_err(|e| e.to_string())?;
        Self::from_def(d)
    }

    /// True if event `name` fires in (prev, now] (clip seconds).
    pub fn event_between(&self, name: &str, prev: f32, now: f32) -> bool {
        self.events.iter().any(|e| e.name == name && e.t > prev && e.t <= now)
    }

    /// Events firing in (prev, now], in time order.
    pub fn events_between(&self, prev: f32, now: f32) -> impl Iterator<Item = &EventDef> {
        self.events.iter().filter(move |e| e.t > prev && e.t <= now)
    }
}

#[derive(Debug, Clone)]
pub struct Clip {
    pub meta: ClipMeta,
    pub keys: Vec<(f32, Pose)>,
}

impl Clip {
    pub fn from_ron(src: &str) -> Result<Clip, String> {
        let def: ClipDef = ron::from_str(src).map_err(|e| e.to_string())?;
        let name = def.name.clone();
        if def.keys.is_empty() {
            return Err(format!("clip '{name}' needs keys"));
        }
        let to_pose = |m: &HashMap<String, f32>, base: Pose| -> Result<Pose, String> {
            let mut p = base;
            for (k, v) in m {
                let i = channel(k).ok_or_else(|| format!("clip '{name}': unknown channel '{k}'"))?;
                p.0[i] = *v;
            }
            Ok(p)
        };
        // Additive clips are deltas: unkeyed channels stay 0.
        let base = to_pose(&def.base, Pose::default())?;
        let mut keys = Vec::new();
        for k in &def.keys {
            keys.push((k.t, to_pose(&k.pose, base)?));
        }
        keys.sort_by(|a, b| a.0.total_cmp(&b.0));
        let meta = ClipMeta::from_def(MetaDef {
            name: def.name, duration: def.duration, looping: def.looping, stance: def.stance,
            mask: def.mask, speed_scale: def.speed_scale, hold: def.hold, events: def.events,
        })?;
        Ok(Clip { meta, keys })
    }

    pub fn name(&self) -> &str {
        &self.meta.name
    }

    /// Sample at time t (seconds). Looping clips wrap the last key to the first;
    /// one-shots hold the last key. Interpolation is smoothstep-eased.
    pub fn sample(&self, t: f32) -> Pose {
        let dur = self.meta.duration;
        let t = if self.meta.looping { t.rem_euclid(dur) } else { t.clamp(0.0, dur) };
        let n = self.keys.len();
        if n == 1 {
            return self.keys[0].1;
        }
        let i = self.keys.iter().rposition(|k| k.0 <= t).unwrap_or(0);
        let (t0, p0) = self.keys[i];
        let (t1, p1) = if i + 1 < n {
            self.keys[i + 1]
        } else if self.meta.looping {
            (dur + self.keys[0].0, self.keys[0].1)
        } else {
            return p0;
        };
        let u = ((t - t0) / (t1 - t0).max(1e-6)).clamp(0.0, 1.0);
        p0.lerp(&p1, u * u * (3.0 - 2.0 * u))
    }

    pub fn event_between(&self, name: &str, prev: f32, now: f32) -> bool {
        self.meta.event_between(name, prev, now)
    }
}

/// Built-in clips compiled into the binary.
pub const BUILTIN_CLIPS: &[&str] = &[
    include_str!("../assets/clips/idle.ron"),
    include_str!("../assets/clips/dash.ron"),
    include_str!("../assets/clips/swing.ron"),
    include_str!("../assets/clips/dodge_roll.ron"),
    include_str!("../assets/clips/hit_flinch.ron"),
];

#[derive(Default)]
pub struct AnimLibrary {
    clips: Vec<Clip>,
    by_name: HashMap<String, usize>,
}

impl AnimLibrary {
    pub fn builtin() -> AnimLibrary {
        let mut lib = AnimLibrary::default();
        for src in BUILTIN_CLIPS {
            lib.register_ron(src).expect("builtin clip failed to parse");
        }
        lib
    }
    /// Register (or replace) a clip from RON source. Usable at runtime for hot-reload.
    pub fn register_ron(&mut self, src: &str) -> Result<usize, String> {
        let clip = Clip::from_ron(src)?;
        Ok(self.register(clip))
    }
    pub fn register(&mut self, clip: Clip) -> usize {
        if let Some(&i) = self.by_name.get(clip.name()) {
            self.clips[i] = clip;
            return i;
        }
        self.by_name.insert(clip.name().to_string(), self.clips.len());
        self.clips.push(clip);
        self.clips.len() - 1
    }
    pub fn get(&self, name: &str) -> Option<&Clip> {
        self.by_name.get(name).map(|&i| &self.clips[i])
    }
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.clips.iter().map(|c| c.name())
    }
}

/// One playing clip.
#[derive(Clone, Debug)]
pub struct ClipPlayer {
    pub clip: String,
    pub time: f32,
    pub prev_time: f32,
    pub rate: f32,
    /// While true, a clip with a `hold` range loops inside it.
    pub holding: bool,
}

impl ClipPlayer {
    pub fn new(clip: &str, rate: f32) -> Self {
        ClipPlayer { clip: clip.into(), time: 0.0, prev_time: 0.0, rate, holding: true }
    }
    fn advance(&mut self, lib: &AnimLibrary, dt: f32) {
        self.prev_time = self.time;
        self.time += dt * self.rate;
        if let Some(c) = lib.get(&self.clip) {
            if let (true, Some((a, b))) = (self.holding, c.meta.hold) {
                if self.time > b {
                    self.time = a + (self.time - b) % (b - a);
                    self.prev_time = self.prev_time.min(self.time);
                }
            }
        }
    }
    pub fn finished(&self, lib: &AnimLibrary) -> bool {
        lib.get(&self.clip).map(|c| !c.meta.looping && self.time >= c.meta.duration).unwrap_or(true)
    }
}

/// Layered animator: base pose (supplied each update) + optional action clip
/// + additive clips. Any change of action cross-fades from the last output.
#[derive(Clone, Debug, Default)]
pub struct Animator {
    pub action: Option<ClipPlayer>,
    pub additives: Vec<ClipPlayer>,
    from: Pose,
    fade: f32,
    fade_len: f32,
    pub pose: Pose,
}

impl Animator {
    pub fn new() -> Self {
        Animator { fade: 1.0, ..Default::default() }
    }
    /// Start (or stop, with `None`) the action clip. `blend` = cross-fade seconds.
    /// `rate` scales playback (e.g. 1 / (speed_scale * swing_weight)).
    pub fn play(&mut self, clip: Option<&str>, blend: f32, rate: f32, restart: bool) {
        let same = self.action.as_ref().map(|a| a.clip.as_str()) == clip;
        if same && !restart {
            return;
        }
        self.from = self.pose.wrapped();
        self.action = clip.map(|c| ClipPlayer::new(c, rate));
        self.fade = 0.0;
        self.fade_len = blend;
    }
    /// Fire-and-forget additive clip (removed when it finishes).
    pub fn play_additive(&mut self, clip: &str) {
        self.additives.push(ClipPlayer::new(clip, 1.0));
    }
    /// Advance by dt and recompute `pose` over `base`.
    pub fn update(&mut self, lib: &AnimLibrary, dt: f32, base: &Pose) {
        if let Some(a) = &mut self.action {
            a.advance(lib, dt);
        }
        for a in &mut self.additives {
            a.advance(lib, dt);
        }
        self.additives.retain(|a| !a.finished(lib));
        self.fade = if self.fade_len > 0.0 { (self.fade + dt / self.fade_len).min(1.0) } else { 1.0 };

        let mut target = *base;
        if let Some(a) = &self.action {
            if let Some(c) = lib.get(&a.clip) {
                target = compose(&target, &c.sample(a.time), c.meta.mask, 1.0);
            }
        }
        for a in &self.additives {
            if let Some(c) = lib.get(&a.clip) {
                target = compose(&target, &c.sample(a.time), MaskKind::Additive, 1.0);
            }
        }
        self.pose = if self.fade < 1.0 { self.from.blend(&target, self.fade) } else { target };
    }
    pub fn action_finished(&self, lib: &AnimLibrary) -> bool {
        self.action.as_ref().map(|a| a.finished(lib)).unwrap_or(true)
    }
}
