//! Deterministic CSS transition state. The clock is supplied by the caller, so
//! interruption, reversal, delay and completion are testable without sleeping.
use crate::{
    core::layout::*,
    style::{
        color::{Color, ColorSpace, HueDirection},
        computed::ComputedStyle,
        declaration::Property,
        properties::LayoutProperty,
        shadow::BoxShadows,
        text::LineHeight,
        transition::{Easing, TransitionStyle},
    },
};
use std::time::{Duration, Instant};
use taffy::Dimension;

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Transform(crate::Transform, [f32; 2]),
    TransformOrigin(crate::TransformOrigin),
    Number(f32),
    ZIndex(i32),
    Visibility(crate::style::layer::Visibility),
    Length(f32, bool),
    LineHeight(f32, bool),
    Color(Color),
    Shadows(BoxShadows),
}
impl Value {
    fn same_target(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Transform(a, _), Self::Transform(b, _)) => a == b,
            _ => self == other,
        }
    }

    fn interpolate(&self, other: &Self, t: f32) -> Option<Self> {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        match (self, other) {
            (Self::Transform(a, size), Self::Transform(b, _)) => a
                .interpolate(b, t, *size)
                .map(|v| Self::Transform(v, *size)),
            (Self::TransformOrigin(a), Self::TransformOrigin(b)) => {
                Some(Self::TransformOrigin(crate::TransformOrigin {
                    x: a.x.interpolate(&b.x, t),
                    y: a.y.interpolate(&b.y, t),
                }))
            }
            (Self::ZIndex(a), Self::ZIndex(b)) => Some(Self::ZIndex(
                (f64::from(*a) + (f64::from(*b) - f64::from(*a)) * f64::from(t) + 0.5)
                    .floor()
                    .clamp(i32::MIN as f64, i32::MAX as f64) as i32,
            )),
            (Self::Visibility(a), Self::Visibility(b)) => Some(Self::Visibility(if t <= 0.0 {
                *a
            } else if t >= 1.0 {
                *b
            } else {
                crate::style::layer::Visibility::Visible
            })),
            (Self::Number(a), Self::Number(b)) => Some(Self::Number(mix(*a, *b))),
            (Self::Length(a, p), Self::Length(b, q)) if p == q => {
                Some(Self::Length(mix(*a, *b), *p))
            }
            (Self::LineHeight(a, relative), Self::LineHeight(b, other)) if relative == other => {
                Some(Self::LineHeight(mix(*a, *b), *relative))
            }
            (Self::Color(a), Self::Color(b)) => Some(Self::Color(a.interpolate(
                *b,
                t,
                ColorSpace::Oklab,
                HueDirection::Shorter,
            ))),
            (Self::Shadows(a), Self::Shadows(b)) => {
                crate::style::shadow::interpolate(a, b, t).map(Self::Shadows)
            }
            _ => None,
        }
    }
}
trait AnimatedLength: Sized {
    fn value(self) -> Option<Value>;
    fn from_value(value: &Value, signed: bool) -> Option<Self>;
}
macro_rules! length_type {
    ($ty:ty,$expanded:ident) => {
        impl AnimatedLength for $ty {
            fn value(self) -> Option<Value> {
                match self.expand() {
                    $expanded::Length(v) => Some(Value::Length(v, false)),
                    $expanded::Percent(v) => Some(Value::Length(v, true)),
                    #[allow(unreachable_patterns)]
                    _ => None,
                }
            }
            fn from_value(v: &Value, signed: bool) -> Option<Self> {
                if let Value::Length(n, p) = v {
                    let n = if signed { *n } else { n.max(0.0) };
                    Some(if *p {
                        Self::percent(n)
                    } else {
                        Self::length(n)
                    })
                } else {
                    None
                }
            }
        }
    };
}
length_type!(Dimension, ExpandedDimension);
length_type!(LengthPercentage, ExpandedLengthPercentage);
length_type!(LengthPercentageAuto, ExpandedLengthPercentageAuto);

macro_rules! animation_properties {
    (layout_setters{$($(#[$pd:meta])* $plain:ident=>$pn:ident($pt:ty)=>$($pf:ident).+;)*}
     optional_setters{$($(#[$od:meta])* $opt:ident=>$on:ident($ot:ty)=>$of:ident;)*}
     length_setters{$($(#[$ld:meta])* $len:ident=>$ln:ident($lt:ty),$signed:literal=>$($lf:ident).+;)*})=>{
        const PROPERTIES:&[(Property,&str)]=&[
            (Property::Transform,"transform"),(Property::TransformOrigin,"transform-origin"),
            (Property::ZIndex,"z-index"),(Property::Visibility,"visibility"),
            (Property::Color,"color"),(Property::Background,"background-color"),(Property::BorderColor,"border-color"),
            (Property::BorderRadius,"border-radius"),(Property::BoxShadow,"box-shadow"),
            (Property::FontSize,"font-size"),(Property::FontWeight,"font-weight"),(Property::LineHeight,"line-height"),
            (Property::Layout(LayoutProperty::FlexGrow),"flex-grow"),(Property::Layout(LayoutProperty::FlexShrink),"flex-shrink"),
            $((Property::Layout(LayoutProperty::$len),stringify!($ln)),)*
        ];
        fn read(style:&ComputedStyle,property:Property,size:[f32;2])->Option<Value>{
            match property {
                Property::Transform=>Some(Value::Transform(style.transform.clone(),size)),
                Property::TransformOrigin=>Some(Value::TransformOrigin(style.transform_origin.clone())),
                Property::ZIndex=>match style.layer.z_index {crate::style::layer::ZIndex::Integer(v)=>Some(Value::ZIndex(v)),_=>None},
                Property::Visibility=>Some(Value::Visibility(style.layer.visibility)),
                Property::Color=>Some(Value::Color(style.text.color)),
                Property::Background=>Some(Value::Color(style.paint.background.resolve(style.text.color))),
                Property::BorderColor=>Some(Value::Color(style.paint.border_color.resolve(style.text.color))),
                Property::BorderRadius=>Some(Value::Number(style.paint.border_radius)),
                Property::FontSize=>Some(Value::Number(style.text.font_size)),
                Property::FontWeight=>Some(Value::Number(style.text.font.weight.0)),
                Property::LineHeight=>match style.text.line_height {
                    LineHeight::Pixels(v)=>Some(Value::LineHeight(v,false)),
                    LineHeight::Relative(v)=>Some(Value::LineHeight(v,true)),_=>None,
                },
                Property::BoxShadow=>Some(Value::Shadows(style.paint.box_shadow.iter().map(|shadow| {
                    let mut shadow=*shadow;shadow.color=shadow.color.resolve(style.text.color);shadow
                }).collect::<Vec<_>>().into())),
                Property::Layout(p) if style.sticky_inset.is_some() && matches!(p,LayoutProperty::Top|LayoutProperty::Right|LayoutProperty::Bottom|LayoutProperty::Left)=> {
                    let inset=style.sticky_inset.as_ref().unwrap();
                    AnimatedLength::value(match p {LayoutProperty::Top=>inset.top,LayoutProperty::Right=>inset.right,LayoutProperty::Bottom=>inset.bottom,_=>inset.left})
                },
                Property::Layout(p)=>match p {
                    $(LayoutProperty::$len=>AnimatedLength::value(style.layout.$($lf).+),)*
                    LayoutProperty::FlexGrow=>Some(Value::Number(style.layout.flex_grow)),
                    LayoutProperty::FlexShrink=>Some(Value::Number(style.layout.flex_shrink)), _=>None,
                },_=>None,
            }
        }
        fn write(style:&mut ComputedStyle,property:Property,value:Value){
            match (property,value) {
                (Property::Transform,Value::Transform(v,_))=>style.transform=v,
                (Property::TransformOrigin,Value::TransformOrigin(v))=>style.transform_origin=v,
                (Property::ZIndex,Value::ZIndex(v))=>style.layer.z_index=crate::style::layer::ZIndex::Integer(v),
                (Property::Visibility,Value::Visibility(v))=>style.layer.visibility=v,
                (Property::Color,Value::Color(v))=>style.text.color=v,
                (Property::Background,Value::Color(v))=>style.paint.background=v,
                (Property::BorderColor,Value::Color(v))=>style.paint.border_color=v,
                (Property::BorderRadius,Value::Number(v))=>style.paint.border_radius=v.max(0.0),
                (Property::FontSize,Value::Number(v))=>style.text.font_size=v.max(f32::EPSILON),
                (Property::FontWeight,Value::Number(v))=>style.text.font.weight.0=v.clamp(1.0,1000.0),
                (Property::LineHeight,Value::LineHeight(v,relative))=>style.text.line_height=if relative {LineHeight::Relative(v.max(f32::EPSILON))}else{LineHeight::Pixels(v.max(f32::EPSILON))},
                (Property::BoxShadow,Value::Shadows(v))=>style.paint.box_shadow=v,
                (Property::Layout(p),v) if style.sticky_inset.is_some() && matches!(p,LayoutProperty::Top|LayoutProperty::Right|LayoutProperty::Bottom|LayoutProperty::Left)=> {
                    if let Some(v)=LengthPercentageAuto::from_value(&v,true) {
                        let inset=style.sticky_inset.as_mut().unwrap();
                        *match p {LayoutProperty::Top=>&mut inset.top,LayoutProperty::Right=>&mut inset.right,LayoutProperty::Bottom=>&mut inset.bottom,_=>&mut inset.left}=v;
                    }
                },
                $( (Property::Layout(LayoutProperty::$len),v)=>{if let Some(v)=<$lt as AnimatedLength>::from_value(&v,$signed){style.layout.$($lf).+ = v;}},)*
                (Property::Layout(LayoutProperty::FlexGrow),Value::Number(v))=>style.layout.flex_grow=v.max(0.0),
                (Property::Layout(LayoutProperty::FlexShrink),Value::Number(v))=>style.layout.flex_shrink=v.max(0.0),_=>{},
            }
        }
    }
}
crate::style::properties::layout_properties!(animation_properties);

#[derive(Debug)]
struct Running {
    property: Property,
    from: Value,
    to: Value,
    reversing_start: Value,
    shortening: f64,
    /// Epoch plus a signed offset avoids Instant underflow for negative delays.
    epoch: Instant,
    delay: f64,
    duration: f64,
    easing: Easing,
}
impl Running {
    fn progress(&self, now: Instant) -> f64 {
        let elapsed = now.saturating_duration_since(self.epoch).as_secs_f64() - self.delay;
        if elapsed < 0.0 {
            return self.easing.evaluate(0.0, true);
        }
        if self.duration <= 0.0 {
            return 1.0;
        }
        self.easing
            .evaluate((elapsed / self.duration).clamp(0.0, 1.0), false)
    }
    fn finished(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.epoch).as_secs_f64() >= self.delay + self.duration
    }
    fn sample(&self, now: Instant) -> Value {
        if self.finished(now) {
            self.to.clone()
        } else {
            self.from
                .interpolate(&self.to, self.progress(now) as f32)
                .expect("validated transition endpoints")
        }
    }
}

/// Allocated only for elements that have running transitions. Target values stay
/// separate from displayed values so each frame cannot accidentally restart them.
#[derive(Debug, Default)]
pub(crate) struct Transitions {
    running: Vec<Running>,
}
impl Transitions {
    pub(crate) fn update(
        state: &mut Option<Box<Self>>,
        before: &ComputedStyle,
        target: &mut ComputedStyle,
        spec: &TransitionStyle,
        now: Instant,
        style_change: bool,
        visible: bool,
        size: [f32; 2],
    ) {
        if !visible {
            *state = None;
            return;
        }
        if state.is_none() && (!style_change || !spec.enabled()) {
            return;
        }
        if style_change {
            for &(property, name) in PROPERTIES {
                let existing = state
                    .as_ref()
                    .and_then(|s| s.running.iter().position(|r| r.property == property));
                let timing = spec.timing(name);
                // Preserve keyword-driven currentColor and inherited list semantics;
                // these values follow the foreground animation at paint time.
                let unchanged = match property {
                    Property::Transform => before.transform == target.transform,
                    Property::TransformOrigin => before.transform_origin == target.transform_origin,
                    Property::Background => before.paint.background == target.paint.background,
                    Property::BorderColor => before.paint.border_color == target.paint.border_color,
                    Property::BoxShadow => before.paint.box_shadow == target.paint.box_shadow,
                    _ => false,
                };
                if existing.is_none()
                    && (timing
                        .as_ref()
                        .is_none_or(|(duration, delay, _)| duration + delay <= 0.0)
                        || unchanged)
                {
                    continue;
                }
                let to = read(target, property, size);
                if let Some(index) = existing {
                    let running = &state.as_ref().unwrap().running[index];
                    if timing.is_some() && to.as_ref().is_some_and(|v| v.same_target(&running.to)) {
                        continue;
                    }
                }
                let old = existing.map(|i| state.as_mut().unwrap().running.swap_remove(i));
                let Some((mut duration, mut delay, easing)) = timing else {
                    continue;
                };
                if duration + delay <= 0.0 {
                    continue;
                }
                let (Some(from), Some(to)) = (
                    old.as_ref()
                        .map(|r| r.sample(now))
                        .or_else(|| read(before, property, size)),
                    to,
                ) else {
                    continue;
                };
                if from.same_target(&to) || from.interpolate(&to, 0.5).is_none() {
                    continue;
                }
                let mut shortening = 1.0;
                let mut reversing_start = from.clone();
                if let Some(old) = old
                    && old.reversing_start.same_target(&to)
                {
                    shortening = (old.progress(now) * old.shortening + (1.0 - old.shortening))
                        .abs()
                        .clamp(0.0, 1.0);
                    duration *= shortening;
                    if delay < 0.0 {
                        delay *= shortening;
                    }
                    reversing_start = old.to;
                }
                state
                    .get_or_insert_with(Default::default)
                    .running
                    .push(Running {
                        property,
                        from,
                        to,
                        reversing_start,
                        shortening,
                        epoch: now,
                        delay,
                        duration,
                        easing,
                    });
            }
        }
        if let Some(state) = state {
            for r in &mut state.running {
                // Percentage matrix suffixes use the latest reference box during resize.
                for value in [&mut r.from, &mut r.to] {
                    if let Value::Transform(_, basis) = value {
                        *basis = size;
                    }
                }
                write(target, r.property, r.sample(now));
            }
            state.running.retain(|r| !r.finished(now));
        }
        if state.as_ref().is_some_and(|s| s.running.is_empty()) {
            *state = None;
        }
    }
    pub(crate) fn next_frame(&self, now: Instant) -> Option<Instant> {
        self.running
            .iter()
            .filter_map(|r| {
                let elapsed = now.saturating_duration_since(r.epoch).as_secs_f64();
                let wait = if elapsed < r.delay {
                    r.delay - elapsed
                } else if let Easing::Steps(count, _) = &r.easing {
                    // A steps() value is constant between jumps. Wake at the next
                    // boundary instead of presenting identical frames at VSync.
                    let progress = (elapsed - r.delay) / r.duration.max(f64::MIN_POSITIVE);
                    let next = ((progress * f64::from(*count)).floor() + 1.0) / f64::from(*count);
                    (r.delay + r.duration * next.min(1.0) - elapsed).max(0.0)
                } else {
                    0.0
                };
                Duration::try_from_secs_f64(wait)
                    .ok()
                    .and_then(|wait| now.checked_add(wait))
            })
            .min()
    }
}
