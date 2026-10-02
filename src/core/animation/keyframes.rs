//! Keyframes retain playback identity independently of style and track caches.
//! Finished/paused entries stay attached without requesting presentation frames.
use super::{PROPERTIES, Value, read, write};
use crate::style::{
    animation::{
        AnimationDirection as Direction, AnimationFillMode as Fill, AnimationPlayState as Play,
        Timing,
    },
    computed::ComputedStyle,
    css::{Stylesheet, parser::Keyframes},
    declaration::{Property, PropertyMask},
    style::Style,
    transition::Easing,
};
use std::{
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Debug)]
struct Stop {
    offset: f64,
    value: Option<Value>,
    easing: Option<Easing>,
}
#[derive(Debug)]
struct Track {
    property: Property,
    stops: Vec<Stop>,
}
#[derive(Debug)]
struct Playback {
    name: String,
    frames: Rc<Keyframes>,
    timing: Timing,
    elapsed: f64,
    sampled_at: Instant,
    tracks: Vec<Track>,
    finished: bool,
}
impl Playback {
    fn elapsed_at(&self, now: Instant) -> f64 {
        self.elapsed
            + if self.timing.state == Play::Paused {
                0.0
            } else {
                now.saturating_duration_since(self.sampled_at).as_secs_f64()
            }
    }
    fn active_duration(&self) -> f64 {
        // Zero duration has an end even when iteration-count is infinite.
        if self.timing.duration == 0.0 {
            0.0
        } else {
            self.timing.duration * self.timing.iterations
        }
    }
    fn reverse(&self, iteration: f64) -> bool {
        match self.timing.direction {
            Direction::Normal => false,
            Direction::Reverse => true,
            Direction::Alternate => iteration % 2.0 >= 1.0,
            Direction::AlternateReverse => iteration % 2.0 < 1.0,
        }
    }
    fn progress(&self, now: Instant) -> Option<(f64, bool)> {
        let active = self.elapsed_at(now) - self.timing.delay;
        let duration = self.active_duration();
        let before = active < 0.0;
        let after = !before && active >= duration;
        if before && !matches!(self.timing.fill, Fill::Backwards | Fill::Both)
            || after && !matches!(self.timing.fill, Fill::Forwards | Fill::Both)
        {
            return None;
        }
        let overall = if before {
            0.0
        } else if after {
            if self.timing.iterations.is_finite() {
                self.timing.iterations
            } else {
                1.0
            }
        } else {
            active / self.timing.duration
        };
        let mut iteration = overall.floor();
        let mut progress = overall.fract();
        // The end of an integral count is the last iteration's 100%, not the
        // next iteration's 0%. Fractional counts finish inside their iteration.
        if after && overall > 0.0 && progress == 0.0 {
            iteration -= 1.0;
            progress = 1.0;
        }
        let reverse = self.reverse(iteration);
        if reverse {
            progress = 1.0 - progress;
        }
        Some((progress, if reverse { after } else { before }))
    }
    fn next_frame(&self, now: Instant) -> Option<Instant> {
        if self.timing.state == Play::Paused || self.finished || self.tracks.is_empty() {
            return None;
        }
        let elapsed = self.elapsed_at(now);
        if elapsed < self.timing.delay {
            return Duration::try_from_secs_f64(self.timing.delay - elapsed)
                .ok()
                .and_then(|d| now.checked_add(d));
        }
        Some(now)
    }
    fn sample(
        &mut self,
        target: &mut ComputedStyle,
        important: PropertyMask,
        size: [f32; 2],
        now: Instant,
        applied: &mut PropertyMask,
    ) {
        // Completion is acknowledged by sampling, not by a deadline query. A
        // frame that presents across the end boundary must still paint its end.
        self.finished = self.elapsed_at(now) >= self.timing.delay + self.active_duration();
        let Some((progress, before)) = self.progress(now) else {
            return;
        };
        for track in &self.tracks {
            if important.contains(track.property) {
                continue;
            }
            let hi = track
                .stops
                .partition_point(|stop| stop.offset <= progress)
                .clamp(1, track.stops.len() - 1);
            let (a, b) = (&track.stops[hi - 1], &track.stops[hi]);
            let underlying = read(target, track.property, size);
            let (Some(mut from), Some(mut to)) = (
                a.value.clone().or_else(|| underlying.clone()),
                b.value.clone().or(underlying),
            ) else {
                continue;
            };
            for value in [&mut from, &mut to] {
                if let Value::Transform(_, basis) = value {
                    *basis = size;
                }
            }
            let easing = a.easing.as_ref().unwrap_or(&self.timing.easing);
            let t = easing.evaluate((progress - a.offset) / (b.offset - a.offset), before) as f32;
            // Incompatible values use CSS's discrete midpoint behavior.
            let value = from
                .interpolate(&to, t)
                .unwrap_or_else(|| if t < 0.5 { from } else { to });
            write(target, track.property, value);
            applied.insert(track.property);
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Animations {
    running: Vec<Playback>,
    base: Option<ComputedStyle>,
    applied: PropertyMask,
}
impl Animations {
    /// Match repeated names from right to left, preserving elapsed time during
    /// reorder, timing edits and stylesheet reload. Removing a name cancels it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update(
        state: &mut Option<Box<Self>>,
        source: &Style,
        parent: &ComputedStyle,
        target: &mut ComputedStyle,
        sheets: &[Stylesheet],
        now: Instant,
        visible: bool,
        restyle: bool,
        top_layer: bool,
        size: [f32; 2],
    ) -> PropertyMask {
        let previous = state.as_ref().map(|s| s.applied).unwrap_or_default();
        if !visible || target.animation.names.iter().all(|name| name.0.is_none()) {
            *state = None;
            return previous;
        }
        let state = state.get_or_insert_with(Default::default);
        let rebuild = restyle || state.base.as_ref() != Some(target);
        // Stable frames only sample cached tracks. Selector matching, declaration
        // resolution and list reconciliation stay on the style-change path.
        if rebuild {
            let mut old = std::mem::take(&mut state.running);
            for (index, name) in target.animation.names.iter().enumerate().rev() {
                let Some(name) = &name.0 else {
                    continue;
                };
                let Some(frames) = sheets.iter().rev().find_map(|s| s.keyframes(name)) else {
                    continue;
                };
                let timing = target.animation.timing(index);
                let existing = old
                    .iter()
                    .rposition(|p| &p.name == name)
                    .map(|i| old.remove(i));
                let fresh = existing.is_none();
                let mut playback = existing.unwrap_or_else(|| Playback {
                    name: name.clone(),
                    frames: frames.clone(),
                    timing: timing.clone(),
                    elapsed: 0.0,
                    sampled_at: now,
                    tracks: Vec::new(),
                    finished: false,
                });
                playback.elapsed = playback.elapsed_at(now);
                playback.sampled_at = now;
                playback.timing = timing;
                if fresh || rebuild || !Rc::ptr_eq(&playback.frames, frames) {
                    playback.frames = frames.clone();
                    playback.tracks = tracks(frames, source, parent, target, top_layer, size);
                }
                state.running.push(playback);
            }
            state.running.reverse();
            state.base = Some(target.clone());
        }
        state.applied = Default::default();
        for playback in &mut state.running {
            playback.sample(target, source.important, size, now, &mut state.applied);
        }
        // Transitions must not be synthesized by a keyframe entering/leaving its
        // effect interval. Already-running transitions keep their higher priority.
        previous.union(state.applied)
    }
    pub(crate) fn next_frame(&self, now: Instant) -> Option<Instant> {
        self.running.iter().filter_map(|p| p.next_frame(now)).min()
    }
}

fn tracks(
    frames: &Keyframes,
    source: &Style,
    parent: &ComputedStyle,
    base: &ComputedStyle,
    top_layer: bool,
    size: [f32; 2],
) -> Vec<Track> {
    let mut tracks: Vec<Track> = Vec::new();
    let mut unsupported = PropertyMask::default();
    let mut initial_easing = None;
    for frame in &frames.0 {
        let mut specified = source.clone();
        for declaration in &frame.declarations {
            declaration.apply(&mut specified);
        }
        let values = crate::style::css::values::resolve(
            &specified,
            parent,
            Some(base.root_font_size),
            base.viewport_size,
        );
        let easing = frame
            .declarations
            .iter()
            .any(|d| d.property() == Property::AnimationTimingFunction)
            .then(|| {
                values
                    .style
                    .resolve_animations(&parent.animation)
                    .easing
                    .first()
                    .cloned()
                    .unwrap_or_default()
            });
        if frame.offset == 0.0 {
            initial_easing = easing.clone();
        }
        let computed = ComputedStyle::resolve_values(values, parent, top_layer, base.viewport_size);
        for &(property, _) in PROPERTIES {
            if !frame.declarations.iter().any(|d| d.property() == property) {
                continue;
            }
            let Some(value) = read(&computed, property, size) else {
                // Do not silently turn an authored auto/calc endpoint into an
                // omitted endpoint. This registry cannot represent that track.
                unsupported.insert(property);
                continue;
            };
            let index = tracks
                .iter()
                .position(|t| t.property == property)
                .unwrap_or_else(|| {
                    tracks.push(Track {
                        property,
                        stops: Vec::new(),
                    });
                    tracks.len() - 1
                });
            tracks[index].stops.push(Stop {
                offset: frame.offset,
                value: Some(value),
                easing: easing.clone(),
            });
        }
    }
    tracks.retain(|track| !unsupported.contains(track.property));
    for track in &mut tracks {
        if track.stops[0].offset != 0.0 {
            track.stops.insert(
                0,
                Stop {
                    offset: 0.0,
                    value: None,
                    easing: initial_easing.clone(),
                },
            );
        }
        if track.stops.last().unwrap().offset != 1.0 {
            track.stops.push(Stop {
                offset: 1.0,
                value: None,
                easing: None,
            });
        }
    }
    tracks
}
