//! Standard replaced-image and SVG paint properties. Sparse, shared storage keeps
//! ordinary div/text styles allocation-free; each longhand cascades independently.
use super::value::CssValue;
use smol_str::SmolStr;
use std::{str::FromStr, sync::Arc};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ObjectFit {
    #[default]
    Fill,
    Contain,
    Cover,
    None,
    ScaleDown,
}

/// Percentages use the free space (content size minus object size), as in CSS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionOffset {
    pub percent: f32,
    pub pixels: f32,
}
impl PositionOffset {
    pub fn resolve(self, free: f32) -> f32 {
        self.percent * free + self.pixels
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectPosition {
    pub x: PositionOffset,
    pub y: PositionOffset,
}
impl Default for ObjectPosition {
    fn default() -> Self {
        Self {
            x: PositionOffset {
                percent: 0.5,
                pixels: 0.,
            },
            y: PositionOffset {
                percent: 0.5,
                pixels: 0.,
            },
        }
    }
}
impl FromStr for ObjectPosition {
    type Err = String;
    fn from_str(raw: &str) -> Result<Self, String> {
        super::css::media::position(raw)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MediaValue {
    Fit(ObjectFit),
    Position(ObjectPosition),
    Sampling(crate::render::ImageSampling),
    /// Validated SVG CSS, retained until the SVG viewport and font are known.
    Svg(SmolStr),
}
macro_rules! properties {
    ($( $id:ident => ($name:literal, $inherited:literal, $initial:expr) ),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum MediaProperty { $($id),* }
        impl MediaProperty {
            pub const ALL: &'static [Self] = &[$(Self::$id),*];
            pub fn name(self) -> &'static str { match self { $(Self::$id => $name),* } }
            pub fn inherited(self) -> bool { match self { $(Self::$id => $inherited),* } }
            pub fn initial(self) -> MediaValue { match self { $(Self::$id => $initial),* } }
            pub fn from_name(name: &str) -> Option<Self> { match name { $($name => Some(Self::$id),)* _ => None } }
        }
    }
}
use MediaValue::{Fit, Position, Sampling, Svg};
properties! {
    ObjectFit => ("object-fit", false, Fit(ObjectFit::Fill)),
    ObjectPosition => ("object-position", false, Position(ObjectPosition::default())),
    ImageRendering => ("image-rendering", true, Sampling(crate::render::ImageSampling::Smooth)),
    Fill => ("fill", true, Svg("black".into())),
    Stroke => ("stroke", true, Svg("none".into())),
    FillOpacity => ("fill-opacity", true, Svg("1".into())),
    StrokeOpacity => ("stroke-opacity", true, Svg("1".into())),
    StrokeWidth => ("stroke-width", true, Svg("1".into())),
    StrokeLinecap => ("stroke-linecap", true, Svg("butt".into())),
    StrokeLinejoin => ("stroke-linejoin", true, Svg("miter".into())),
    StrokeMiterlimit => ("stroke-miterlimit", true, Svg("4".into())),
    StrokeDasharray => ("stroke-dasharray", true, Svg("none".into())),
    StrokeDashoffset => ("stroke-dashoffset", true, Svg("0".into())),
    FillRule => ("fill-rule", true, Svg("nonzero".into())),
    ClipRule => ("clip-rule", true, Svg("nonzero".into())),
    ClipPath => ("clip-path", false, Svg("none".into())),
    Mask => ("mask", false, Svg("none".into())),
    Filter => ("filter", false, Svg("none".into())),
    Opacity => ("opacity", false, Svg("1".into())),
    StopColor => ("stop-color", false, Svg("black".into())),
    StopOpacity => ("stop-opacity", false, Svg("1".into())),
    FloodColor => ("flood-color", false, Svg("black".into())),
    FloodOpacity => ("flood-opacity", false, Svg("1".into())),
    PaintOrder => ("paint-order", true, Svg("normal".into())),
    ShapeRendering => ("shape-rendering", true, Svg("auto".into())),
    Cx => ("cx", false, Svg("0".into())), Cy => ("cy", false, Svg("0".into())),
    R => ("r", false, Svg("0".into())), Rx => ("rx", false, Svg("auto".into())), Ry => ("ry", false, Svg("auto".into())),
    X => ("x", false, Svg("0".into())), Y => ("y", false, Svg("0".into())),
    D => ("d", false, Svg("none".into())),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaDeclaration {
    pub property: MediaProperty,
    pub value: CssValue<MediaValue>,
}
impl MediaDeclaration {
    pub fn parse(name: &str, value: &str) -> Result<Self, String> {
        super::css::media::parse(name, value)
    }
    pub fn apply(&self, target: &mut super::style::Style) {
        target.media.set(self.clone());
    }
}

/// Absent properties consume one null pointer and allocate no backing storage.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaStyle(Option<Arc<Vec<MediaDeclaration>>>);
impl MediaStyle {
    pub fn get(&self, p: MediaProperty) -> Option<&CssValue<MediaValue>> {
        self.0
            .as_ref()?
            .iter()
            .find(|d| d.property == p)
            .map(|d| &d.value)
    }
    pub fn set(&mut self, d: MediaDeclaration) {
        let entries = Arc::make_mut(self.0.get_or_insert_with(|| Arc::new(Vec::new())));
        if let Some(entry) = entries.iter_mut().find(|e| e.property == d.property) {
            *entry = d;
        } else {
            entries.push(d);
        }
    }
    pub(crate) fn copy_property(&self, p: MediaProperty, target: &mut Self) {
        if let Some(value) = self.get(p) {
            target.set(MediaDeclaration {
                property: p,
                value: value.clone(),
            });
        }
    }
    pub fn value(&self, p: MediaProperty) -> MediaValue {
        match self.get(p) {
            Some(CssValue::Value(v)) => v.clone(),
            _ => p.initial(),
        }
    }
    pub fn object_fit(&self) -> ObjectFit {
        match self.value(MediaProperty::ObjectFit) {
            Fit(v) => v,
            _ => unreachable!(),
        }
    }
    pub fn object_position(&self) -> ObjectPosition {
        match self.value(MediaProperty::ObjectPosition) {
            Position(v) => v,
            _ => unreachable!(),
        }
    }
    pub fn sampling(&self) -> crate::render::ImageSampling {
        match self.value(MediaProperty::ImageRendering) {
            Sampling(v) => v,
            _ => unreachable!(),
        }
    }
    pub fn opacity(&self) -> f32 {
        match self.value(MediaProperty::Opacity) {
            Svg(v) => v.parse().unwrap_or(1.),
            _ => 1.,
        }
    }
    pub(crate) fn resolve(&self, parent: &Self) -> Self {
        if self.0.is_none() && parent.0.is_none() {
            return Self::default();
        }
        if self.0.is_none()
            && parent
                .0
                .as_ref()
                .is_some_and(|v| v.iter().all(|d| d.property.inherited()))
        {
            return parent.clone();
        }
        let mut output = Self::default();
        for &p in MediaProperty::ALL {
            let inherited = match self.get(p) {
                Some(CssValue::Value(v)) => Some(v.clone()),
                Some(CssValue::Initial) => Some(p.initial()),
                Some(CssValue::Inherit) => Some(parent.value(p)),
                Some(CssValue::Unset) | None if p.inherited() => {
                    parent.get(p).map(|_| parent.value(p))
                }
                _ => None,
            };
            if let Some(v) = inherited {
                output.set(MediaDeclaration {
                    property: p,
                    value: v.into(),
                });
            }
        }
        output
    }
}

/// Fit geometry is independent of decoding, SVG parsing and GPU allocations.
pub fn object_rect(
    container: [f32; 2],
    intrinsic: [f32; 2],
    fit: ObjectFit,
    position: ObjectPosition,
) -> [f32; 4] {
    let [w, h] = container;
    let [iw, ih] = intrinsic;
    let scale = (w / iw).min(h / ih);
    let [ow, oh] = match fit {
        ObjectFit::Fill => [w, h],
        ObjectFit::None => [iw, ih],
        ObjectFit::Contain => [iw * scale, ih * scale],
        ObjectFit::Cover => {
            let s = (w / iw).max(h / ih);
            [iw * s, ih * s]
        }
        ObjectFit::ScaleDown => {
            let s = scale.min(1.);
            [iw * s, ih * s]
        }
    };
    [
        position.x.resolve(w - ow),
        position.y.resolve(h - oh),
        ow,
        oh,
    ]
}
