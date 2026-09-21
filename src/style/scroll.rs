//! Standard CSS scrolling values. Layout-only Taffy presets remain supported;
//! CSS values stay separate so `auto` is never confused with `scroll`.
use super::{color::Color, value::CssValue};
use crate::core::layout;
use std::{str::FromStr, sync::Arc};

macro_rules! keywords {
    ($name:ident, $default:ident, {$($value:ident => $css:literal),+ $(,)?}) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($value),+ }
        impl Default for $name { fn default() -> Self { Self::$default } }
        impl FromStr for $name {
            type Err = String;
            fn from_str(raw: &str) -> Result<Self, String> {
                match raw { $($css => Ok(Self::$value),)+ _ => Err(format!("invalid {}: {raw}", stringify!($name))) }
            }
        }
    }
}
keywords!(Overflow, Visible, {Visible => "visible", Clip => "clip", Hidden => "hidden", Auto => "auto", Scroll => "scroll"});
keywords!(ScrollbarWidth, Auto, {Auto => "auto", Thin => "thin", None => "none"});
keywords!(ScrollbarGutter, Auto, {Auto => "auto", Stable => "stable", StableBothEdges => "stable both-edges"});
keywords!(OverscrollBehavior, Auto, {Auto => "auto", Contain => "contain", None => "none"});
impl Overflow {
    pub fn is_scroll_container(self) -> bool {
        matches!(self, Self::Hidden | Self::Auto | Self::Scroll)
    }
    pub fn allows_user_scroll(self) -> bool {
        matches!(self, Self::Auto | Self::Scroll)
    }
    pub(crate) fn layout(self, reserve: bool) -> layout::Overflow {
        match self {
            Self::Visible => layout::Overflow::Visible,
            Self::Clip => layout::Overflow::Clip,
            _ if reserve => layout::Overflow::Scroll,
            Self::Scroll => layout::Overflow::Scroll,
            _ => layout::Overflow::Hidden,
        }
    }
}
impl From<layout::Overflow> for Overflow {
    fn from(v: layout::Overflow) -> Self {
        match v {
            layout::Overflow::Visible => Self::Visible,
            layout::Overflow::Clip => Self::Clip,
            layout::Overflow::Hidden => Self::Hidden,
            layout::Overflow::Scroll => Self::Scroll,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarColors {
    pub thumb: Color,
    pub track: Color,
}
/// `None` selects the application's scrollbar palette (`scrollbar-color: auto`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScrollStyle {
    pub overflow: layout::Point<Overflow>,
    pub width: ScrollbarWidth,
    pub gutter: ScrollbarGutter,
    pub colors: Option<Arc<ScrollbarColors>>,
    pub overscroll: layout::Point<OverscrollBehavior>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum ScrollValue {
    Overflow(Overflow),
    Width(ScrollbarWidth),
    Gutter(ScrollbarGutter),
    Colors(Option<Arc<ScrollbarColors>>),
    Overscroll(OverscrollBehavior),
}
macro_rules! properties {
    ($($id:ident => ($name:literal, $initial:expr)),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum ScrollProperty { $($id),+ }
        impl ScrollProperty {
            pub const ALL: &'static [Self] = &[$(Self::$id),+];
            pub fn from_name(name: &str) -> Option<Self> { match name { $($name => Some(Self::$id),)+ _ => None } }
            pub fn initial(self) -> ScrollValue { match self { $(Self::$id => $initial),+ } }
        }
    }
}
properties! {
    OverflowX => ("overflow-x", ScrollValue::Overflow(Overflow::Visible)),
    OverflowY => ("overflow-y", ScrollValue::Overflow(Overflow::Visible)),
    Width => ("scrollbar-width", ScrollValue::Width(ScrollbarWidth::Auto)),
    Gutter => ("scrollbar-gutter", ScrollValue::Gutter(ScrollbarGutter::Auto)),
    Colors => ("scrollbar-color", ScrollValue::Colors(None)),
    OverscrollX => ("overscroll-behavior-x", ScrollValue::Overscroll(OverscrollBehavior::Auto)),
    OverscrollY => ("overscroll-behavior-y", ScrollValue::Overscroll(OverscrollBehavior::Auto)),
}
#[derive(Debug, Clone, PartialEq)]
pub struct ScrollDeclaration {
    pub property: ScrollProperty,
    pub value: CssValue<ScrollValue>,
}
/// Ordinary nodes use one null pointer; explicit declarations share storage on clone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScrollDeclarations(Option<Arc<Vec<ScrollDeclaration>>>);
impl ScrollDeclarations {
    pub fn get(&self, p: ScrollProperty) -> Option<&CssValue<ScrollValue>> {
        self.0
            .as_ref()?
            .iter()
            .find(|d| d.property == p)
            .map(|d| &d.value)
    }
    pub fn set(&mut self, property: ScrollProperty, value: CssValue<ScrollValue>) {
        let entries = Arc::make_mut(self.0.get_or_insert_with(|| Arc::new(Vec::new())));
        if let Some(entry) = entries.iter_mut().find(|d| d.property == property) {
            entry.value = value;
        } else {
            entries.push(ScrollDeclaration { property, value });
        }
    }
    pub(crate) fn copy_property(&self, p: ScrollProperty, target: &mut Self) {
        if let Some(v) = self.get(p) {
            target.set(p, v.clone());
        }
    }
    pub(crate) fn resolve(
        &self,
        parent: &ScrollStyle,
        raw: &layout::LayoutStyle,
        color: Color,
    ) -> ScrollStyle {
        let mut result = ScrollStyle::default();
        for &p in ScrollProperty::ALL {
            let initial = p.initial();
            let value = match self.get(p) {
                Some(v) => v.resolve(&parent.value(p), &initial, p == ScrollProperty::Colors),
                None => match p {
                    ScrollProperty::OverflowX => ScrollValue::Overflow(raw.overflow.x.into()),
                    ScrollProperty::OverflowY => ScrollValue::Overflow(raw.overflow.y.into()),
                    ScrollProperty::Colors => parent.value(p),
                    _ => initial,
                },
            };
            match (p, value) {
                (ScrollProperty::OverflowX, ScrollValue::Overflow(v)) => result.overflow.x = v,
                (ScrollProperty::OverflowY, ScrollValue::Overflow(v)) => result.overflow.y = v,
                (_, ScrollValue::Width(v)) => result.width = v,
                (_, ScrollValue::Gutter(v)) => result.gutter = v,
                (_, ScrollValue::Colors(v)) => {
                    result.colors = v.map(|v| {
                        if v.thumb == Color::CurrentColor || v.track == Color::CurrentColor {
                            Arc::new(ScrollbarColors {
                                thumb: v.thumb.resolve(color),
                                track: v.track.resolve(color),
                            })
                        } else {
                            v
                        }
                    })
                }
                (ScrollProperty::OverscrollX, ScrollValue::Overscroll(v)) => {
                    result.overscroll.x = v
                }
                (ScrollProperty::OverscrollY, ScrollValue::Overscroll(v)) => {
                    result.overscroll.y = v
                }
                _ => unreachable!("scroll declaration type does not match its property"),
            }
        }
        // CSS Overflow 3: visible/clip compute to auto/hidden when the other axis scrolls.
        let normalize = |a: Overflow, b: Overflow| {
            if b.is_scroll_container() {
                match a {
                    Overflow::Visible => Overflow::Auto,
                    Overflow::Clip => Overflow::Hidden,
                    _ => a,
                }
            } else {
                a
            }
        };
        result.overflow = layout::Point {
            x: normalize(result.overflow.x, result.overflow.y),
            y: normalize(result.overflow.y, result.overflow.x),
        };
        result
    }
}
impl ScrollStyle {
    fn value(&self, p: ScrollProperty) -> ScrollValue {
        match p {
            ScrollProperty::OverflowX => ScrollValue::Overflow(self.overflow.x),
            ScrollProperty::OverflowY => ScrollValue::Overflow(self.overflow.y),
            ScrollProperty::Width => ScrollValue::Width(self.width),
            ScrollProperty::Gutter => ScrollValue::Gutter(self.gutter),
            ScrollProperty::Colors => ScrollValue::Colors(self.colors.clone()),
            ScrollProperty::OverscrollX => ScrollValue::Overscroll(self.overscroll.x),
            ScrollProperty::OverscrollY => ScrollValue::Overscroll(self.overscroll.y),
        }
    }
    pub(crate) fn same_layout(&self, other: &Self) -> bool {
        self.overflow == other.overflow && self.width == other.width && self.gutter == other.gutter
    }
}

impl<W> crate::core::widget::WidgetBuilder<W> {
    /// Set standard scrollbar thickness; custom metrics belong to ScrollOptions.
    pub fn scrollbar_width(mut self, value: ScrollbarWidth) -> Self {
        self.props
            .style
            .scroll
            .set(ScrollProperty::Width, ScrollValue::Width(value).into());
        self
    }
    /// Reserve a stable gutter with classic scrollbars, including when content fits.
    pub fn scrollbar_gutter(mut self, value: ScrollbarGutter) -> Self {
        self.props
            .style
            .scroll
            .set(ScrollProperty::Gutter, ScrollValue::Gutter(value).into());
        self
    }
    /// Set inherited thumb/track colors. None restores the application's palette.
    pub fn scrollbar_color(mut self, value: Option<ScrollbarColors>) -> Self {
        self.props.style.scroll.set(
            ScrollProperty::Colors,
            ScrollValue::Colors(value.map(Arc::new)).into(),
        );
        self
    }
    /// Control scroll chaining at this container's boundary on both axes.
    pub fn overscroll_behavior(mut self, value: OverscrollBehavior) -> Self {
        for p in [ScrollProperty::OverscrollX, ScrollProperty::OverscrollY] {
            self.props
                .style
                .scroll
                .set(p, ScrollValue::Overscroll(value).into());
        }
        self
    }
}
