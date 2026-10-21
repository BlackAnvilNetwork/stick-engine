//! Data-driven animation: a `Pose` is a fixed set of named joint channels
//! (degrees / metres); a `Clip` is a list of keyed poses loaded from RON.
//! `AnimLibrary` holds all registered clips; `Animator` plays one clip and
//! cross-fades from the previous pose when the clip changes.
use serde::Deserialize;
use std::collections::HashMap;

/// Joint channel names. Angles are degrees, `bob` is metres.
/// Leg/arm "hip"/"shoulder" = swing forward(+)/back(-); knee = bend;
/// elbow = bend forward; *_out = arm raised sideways; twist = upper body yaw;
/// sword_yaw/pitch = blade direction relative to the upper body.
pub const CHANNELS: [&str; 16] = [
    "lean", "twist", "bob", "l_hip", "l_knee", "r_hip", "r_knee", "l_shoulder", "l_elbow",
    "l_arm_out", "r_shoulder", "r_elbow", "r_arm_out", "sword_yaw", "sword_pitch", "head",
];
pub const N: usize = CHANNELS.len();

pub fn channel(name: &str) -> Option<usize> {
    CHANNELS.iter().position(|c| *c == name)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose(pub [f32; N]);

impl Pose {
    pub fn get(&self, name: &str) -> f32 {
        channel(name).map(|i| self.0[i]).unwrap_or(0.0)
    }
    pub fn lerp(&self, o: &Pose, t: f32) -> Pose {
        let mut p = *self;
        for i in 0..N {
            p.0[i] += (o.0[i] - p.0[i]) * t;
        }
        p
    }
}

#[derive(Deserialize, Debug, Clone)]
struct KeyDef {
    t: f32,
    pose: HashMap<String, f32>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct EventDef {
    pub t: f32,
    pub name: String,
}

#[derive(Deserialize, Debug, Clone)]
struct ClipDef {
    name: String,
    duration: f32,
    #[serde(default)]
    looping: bool,
    /// Channels not mentioned in a key inherit from this base pose.
    #[serde(default)]
    base: HashMap<String, f32>,
    keys: Vec<KeyDef>,
    #[serde(default)]
    events: Vec<EventDef>,
}

#[derive(Debug, Clone)]
pub struct Clip {
    pub name: String,
    pub duration: f32,
    pub looping: bool,
    pub keys: Vec<(f32, Pose)>,
    pub events: Vec<EventDef>,
}

impl Clip {
    pub fn from_ron(src: &str) -> Result<Clip, String> {
        let def: ClipDef = ron::from_str(src).map_err(|e| e.to_string())?;
        if def.keys.is_empty() || def.duration <= 0.0 {
            return Err(format!("clip '{}' needs keys and duration > 0", def.name));
        }
        let to_pose = |m: &HashMap<String, f32>, base: Pose| -> Result<Pose, String> {
            let mut p = base;
            for (k, v) in m {
                let i = channel(k).ok_or_else(|| format!("clip '{}': unknown channel '{k}'", def.name))?;
                p.0[i] = *v;
            }
            Ok(p)
        };
        let base = to_pose(&def.base, Pose::default())?;
        let mut keys = Vec::new();
        for k in &def.keys {
            keys.push((k.t, to_pose(&k.pose, base)?));
        }
        keys.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(Clip { name: def.name, duration: def.duration, looping: def.looping, keys, events: def.events })
    }

    /// Sample at time t (seconds). Looping clips wrap the last key to the first;
    /// one-shots hold the last key. Interpolation is smoothstep-eased.
    pub fn sample(&self, t: f32) -> Pose {
        let t = if self.looping { t.rem_euclid(self.duration) } else { t.clamp(0.0, self.duration) };
        let n = self.keys.len();
        if n == 1 {
            return self.keys[0].1;
        }
        let i = self.keys.iter().rposition(|k| k.0 <= t).unwrap_or(0);
        let (t0, p0) = self.keys[i];
        let (t1, p1) = if i + 1 < n {
            self.keys[i + 1]
        } else if self.looping {
            (self.duration + self.keys[0].0, self.keys[0].1)
        } else {
            return p0;
        };
        let u = ((t - t0) / (t1 - t0).max(1e-6)).clamp(0.0, 1.0);
        p0.lerp(&p1, u * u * (3.0 - 2.0 * u))
    }

    /// True if event `name` fires in (prev, now].
    pub fn event_between(&self, name: &str, prev: f32, now: f32) -> bool {
        self.events.iter().any(|e| e.name == name && e.t > prev && e.t <= now)
    }
}

/// Built-in clips compiled into the binary. To add an animation, drop a RON
/// file into `assets/clips/` and add a line here (see README "Adding an animation").
pub const BUILTIN_CLIPS: &[&str] = &[
    include_str!("../assets/clips/idle.ron"),
    include_str!("../assets/clips/walk.ron"),
    include_str!("../assets/clips/run.ron"),
    include_str!("../assets/clips/dash.ron"),
    include_str!("../assets/clips/swing.ron"),
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
        if let Some(&i) = self.by_name.get(&clip.name) {
            self.clips[i] = clip;
            return i;
        }
        self.by_name.insert(clip.name.clone(), self.clips.len());
        self.clips.push(clip);
        self.clips.len() - 1
    }
    pub fn get(&self, name: &str) -> Option<&Clip> {
        self.by_name.get(name).map(|&i| &self.clips[i])
    }
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.clips.iter().map(|c| c.name.as_str())
    }
}

/// Plays one clip at a time with a cross-fade from the last output pose.
#[derive(Clone, Debug)]
pub struct Animator {
    pub clip: String,
    pub time: f32,
    pub prev_time: f32,
    from: Pose,
    fade: f32,
    fade_len: f32,
    pub pose: Pose,
}

impl Animator {
    pub fn new(clip: &str) -> Self {
        Animator { clip: clip.into(), time: 0.0, prev_time: 0.0, from: Pose::default(), fade: 1.0, fade_len: 0.0, pose: Pose::default() }
    }
    /// Switch clip (no-op if already playing). `blend` = cross-fade seconds.
    pub fn play(&mut self, clip: &str, blend: f32, restart: bool) {
        if self.clip == clip && !restart {
            return;
        }
        self.from = self.pose;
        self.clip = clip.into();
        self.time = 0.0;
        self.prev_time = 0.0;
        self.fade = 0.0;
        self.fade_len = blend;
    }
    /// Advance by dt * rate and recompute the pose.
    pub fn update(&mut self, lib: &AnimLibrary, dt: f32, rate: f32) {
        self.prev_time = self.time;
        self.time += dt * rate;
        if self.fade_len > 0.0 {
            self.fade = (self.fade + dt / self.fade_len).min(1.0);
        } else {
            self.fade = 1.0;
        }
        let target = lib.get(&self.clip).map(|c| c.sample(self.time)).unwrap_or_default();
        self.pose = if self.fade < 1.0 { self.from.lerp(&target, self.fade) } else { target };
    }
    pub fn finished(&self, lib: &AnimLibrary) -> bool {
        lib.get(&self.clip).map(|c| !c.looping && self.time >= c.duration).unwrap_or(true)
    }
}
