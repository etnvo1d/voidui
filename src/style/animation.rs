//! CSS keyframe playback controls. Empty longhand lists use CSS initial values.
use super::{list::StyleList, transition::Easing};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AnimationDirection {
    #[default]
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AnimationFillMode {
    #[default]
    None,
    Forwards,
    Backwards,
    Both,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AnimationPlayState {
    #[default]
    Running,
    Paused,
}

/// `None` disables a list entry; names are case-sensitive, including quoted names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnimationName(pub Option<String>);
impl From<&str> for AnimationName {
    fn from(name: &str) -> Self {
        Self(Some(name.into()))
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnimationStyle {
    pub names: StyleList<AnimationName>,
    pub durations: StyleList<f64>,
    pub delays: StyleList<f64>,
    pub easing: StyleList<Easing>,
    /// Nonnegative counts, including fractions; positive infinity means infinite.
    pub iterations: StyleList<f64>,
    pub directions: StyleList<AnimationDirection>,
    pub fill_modes: StyleList<AnimationFillMode>,
    pub play_states: StyleList<AnimationPlayState>,
}

/// One list entry after shorter longhand lists have been repeated to match names.
#[derive(Debug, Clone)]
pub(crate) struct Timing {
    pub duration: f64,
    pub delay: f64,
    pub easing: Easing,
    pub iterations: f64,
    pub direction: AnimationDirection,
    pub fill: AnimationFillMode,
    pub state: AnimationPlayState,
}
impl AnimationStyle {
    pub(crate) fn timing(&self, index: usize) -> Timing {
        fn at<T: Clone>(list: &[T], index: usize, initial: T) -> T {
            list.get(index % list.len().max(1))
                .cloned()
                .unwrap_or(initial)
        }
        Timing {
            duration: at(&self.durations, index, 0.0),
            delay: at(&self.delays, index, 0.0),
            easing: at(&self.easing, index, Easing::default()),
            iterations: at(&self.iterations, index, 1.0),
            direction: at(&self.directions, index, Default::default()),
            fill: at(&self.fill_modes, index, Default::default()),
            state: at(&self.play_states, index, Default::default()),
        }
    }
}
