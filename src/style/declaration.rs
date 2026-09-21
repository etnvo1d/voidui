//! Typed longhands shared by CSS parsing, cascade, and inline-style tracking.
use crate::core::layout::*;
use crate::style::{
    color::Color,
    properties::{LayoutKeyword, LayoutProperty},
    style::Style,
    text::{FontSize, LineHeight, TextAlignment},
    value::CssValue,
};
use voidui_gpui_wgpu::{FontFallbacks, FontFeatures, FontStyle, FontWeight, SharedString};

macro_rules! define_declarations {
    (layout_setters { $( $(#[$pdoc:meta])* $plain:ident => $pname:ident($pty:ty) => $($pf:ident).+; )* }
     optional_setters { $( $(#[$odoc:meta])* $optional:ident => $oname:ident($oty:ty) => $of:ident; )* }
     length_setters { $( $(#[$ldoc:meta])* $length:ident => $lname:ident($lty:ty), $signed:literal => $($lf:ident).+; )* }) => {
        #[derive(Debug, Clone, PartialEq)]
        pub enum Declaration {
            CustomProperty(String, std::rc::Rc<crate::style::css::values::Tokens>),
            Deferred(Property, std::rc::Rc<crate::style::css::values::PendingValue>),
            MathLength(LayoutProperty, crate::style::math::MathLength),
            Media(crate::style::media::MediaDeclaration),
            Scroll(crate::style::scroll::ScrollDeclaration),
            $($plain($pty),)* $($optional(Option<$oty>),)* $($length($lty),)*
            FlexGrow(f32), FlexShrink(f32), AspectRatio(Option<f32>), OverflowX(Overflow), OverflowY(Overflow),
            Display(Display),
            CaretColor(CssValue<Color>), CaretAnimation(CssValue<bool>),
            Color(CssValue<Color>), Background(CssValue<Color>), BorderColor(CssValue<Color>), BorderRadius(CssValue<f32>),
            FontFamily(CssValue<SharedString>), FontWeight(CssValue<FontWeight>), FontStyle(CssValue<FontStyle>),
            FontFeatures(CssValue<FontFeatures>), FontFallbacks(CssValue<Option<FontFallbacks>>),
            FontSize(CssValue<FontSize>), LineHeight(CssValue<LineHeight>), TextAlign(CssValue<TextAlignment>),
            Direction(CssValue<Direction>), TextWrap(CssValue<bool>),
            BackgroundImage(CssValue<crate::style::gradient::BackgroundImages>),
            BoxShadow(CssValue<crate::style::shadow::BoxShadows>),
            TransitionProperty(CssValue<crate::style::list::StyleList<crate::style::transition::TransitionProperty>>),
            TransitionDuration(CssValue<crate::style::list::StyleList<f64>>),
            TransitionDelay(CssValue<crate::style::list::StyleList<f64>>),
            TransitionTimingFunction(CssValue<crate::style::list::StyleList<crate::style::transition::Easing>>),
            Transform(CssValue<crate::style::transform::Transform>),
            TransformOrigin(CssValue<crate::style::transform::TransformOrigin>),
            Position(CssValue<crate::style::layer::Position>),
            ZIndex(CssValue<crate::style::layer::ZIndex>),
            Isolation(CssValue<crate::style::layer::Isolation>),
            Visibility(CssValue<crate::style::layer::Visibility>),
            PointerEvents(CssValue<crate::style::layer::PointerEvents>),
            UserSelect(CssValue<crate::style::selection::UserSelect>),
            Cursor(CssValue<crate::style::selection::Cursor>),
            LayoutKeyword(LayoutProperty, LayoutKeyword),
        }
        impl Declaration {
            pub fn property(&self) -> Property {
                match self {
                    Self::CustomProperty(..) => Property::Custom,
                    Self::Deferred(p, _) => *p,
                    Self::MathLength(p, _) => Property::Layout(*p),
                    $(Self::$plain(_) => Property::Layout(LayoutProperty::$plain),)*
                    $(Self::$optional(_) => Property::Layout(LayoutProperty::$optional),)*
                    $(Self::$length(_) => Property::Layout(LayoutProperty::$length),)*
                    Self::FlexGrow(_) => Property::Layout(LayoutProperty::FlexGrow),
                    Self::FlexShrink(_) => Property::Layout(LayoutProperty::FlexShrink),
                    Self::AspectRatio(_) => Property::Layout(LayoutProperty::AspectRatio),
                    Self::OverflowX(_) => Property::Layout(LayoutProperty::OverflowX),
                    Self::OverflowY(_) => Property::Layout(LayoutProperty::OverflowY),
                    Self::LayoutKeyword(p, _) => Property::Layout(*p),
                    Self::Media(d) => Property::Media(d.property),
                    Self::Scroll(d) => Property::Scroll(d.property),
                    Self::Display(_) => Property::Display, Self::Color(_) => Property::Color,
                    Self::Background(_) => Property::Background, Self::BorderColor(_) => Property::BorderColor,
                    Self::BorderRadius(_) => Property::BorderRadius, Self::FontFamily(_) => Property::FontFamily,
                    Self::FontWeight(_) => Property::FontWeight, Self::FontStyle(_) => Property::FontStyle,
                    Self::FontFeatures(_) => Property::FontFeatures, Self::FontFallbacks(_) => Property::FontFallbacks,
                    Self::FontSize(_) => Property::FontSize, Self::LineHeight(_) => Property::LineHeight,
                    Self::TextAlign(_) => Property::TextAlign, Self::Direction(_) => Property::Direction,
                    Self::TextWrap(_) => Property::TextWrap,
                    Self::UserSelect(_) => Property::UserSelect,
                    Self::Cursor(_) => Property::Cursor,
                    Self::CaretColor(_) => Property::CaretColor,
                    Self::CaretAnimation(_) => Property::CaretAnimation,

                    Self::Transform(_) => Property::Transform,
                    Self::TransformOrigin(_) => Property::TransformOrigin,
                    Self::Position(_) => Property::Position,
                    Self::ZIndex(_) => Property::ZIndex,
                    Self::Isolation(_) => Property::Isolation,
                    Self::Visibility(_) => Property::Visibility,
                    Self::PointerEvents(_) => Property::PointerEvents,

                    Self::BackgroundImage(_) => Property::BackgroundImage,
                    Self::BoxShadow(_) => Property::BoxShadow,
                    Self::TransitionProperty(_) => Property::TransitionProperty,
                    Self::TransitionDuration(_) => Property::TransitionDuration,
                    Self::TransitionDelay(_) => Property::TransitionDelay,
                    Self::TransitionTimingFunction(_) => Property::TransitionTimingFunction,

                }
            }
            /// Apply a single specified longhand and remember explicit default-valued writes.
            pub fn apply(&self, target: &mut Style) {
                if let Self::CustomProperty(name, value) = self {
                    target.custom_properties.insert(name.clone(), value.clone());
                    return;
                }
                target.mark(self.property());
                if let Self::Deferred(p, value) = self {
                    target.deferred.push((*p, value.clone()));
                    return;
                }
                if let Self::LayoutKeyword(p, value) = self { target.set_layout_keyword(*p, *value); return; }
                if let Property::Layout(p) = self.property() { target.clear_layout_keyword(p); }
                match self {
                    Self::CustomProperty(..) | Self::Deferred(..) => unreachable!(),
                    Self::MathLength(p, value) => {
                        match p {
                            $(LayoutProperty::$length => target.layout.$($lf).+ = <$lty>::calc(value.handle()),)*
                            _ => panic!("math expressions require a layout length property"),
                        }
                        target.math_lengths.retain(|(key, _)| key != p);
                        target.math_lengths.push((*p, value.clone()));
                    }
                    $(Self::$plain(value) => target.layout.$($pf).+.clone_from(value),)*
                    $(Self::$optional(value) => target.layout.$of = *value,)*
                    $(Self::$length(value) => {
                        target.layout.$($lf).+ = *value;
                        target.retain_length(LayoutProperty::$length, *value);
                    },)*
                    Self::FlexGrow(v) => target.layout.flex_grow = *v,
                    Self::FlexShrink(v) => target.layout.flex_shrink = *v,
                    Self::AspectRatio(v) => target.layout.aspect_ratio = *v,
                    // Legacy layout declarations write through the CSS overflow
                    // boundary, so later writes cannot leave two competing values.
                    Self::OverflowX(v) => {
                        target.layout.overflow.x = *v;
                        target.scroll.set(crate::style::scroll::ScrollProperty::OverflowX,
                            crate::style::scroll::ScrollValue::Overflow((*v).into()).into());
                    }
                    Self::OverflowY(v) => {
                        target.layout.overflow.y = *v;
                        target.scroll.set(crate::style::scroll::ScrollProperty::OverflowY,
                            crate::style::scroll::ScrollValue::Overflow((*v).into()).into());
                    }
                    Self::Media(d) => d.apply(target),
                    Self::Scroll(d) => target.scroll.set(d.property, d.value.clone()),
                    Self::Display(v) => target.layout.display = *v,
                    Self::Color(v) => target.color = *v, Self::Background(v) => target.background = *v,
                    Self::BorderColor(v) => target.border_color = *v, Self::BorderRadius(v) => target.border_radius = *v,
                    Self::FontFamily(v) => target.font_family.clone_from(v), Self::FontWeight(v) => target.font_weight.clone_from(v),
                    Self::FontStyle(v) => target.font_style.clone_from(v), Self::FontFeatures(v) => target.font_features.clone_from(v),
                    Self::FontFallbacks(v) => target.font_fallbacks.clone_from(v), Self::FontSize(v) => target.font_size = *v,
                    Self::LineHeight(v) => target.line_height = *v, Self::TextAlign(v) => target.text_align = *v,
                    Self::Direction(v) => target.direction = *v, Self::TextWrap(v) => target.text_wrap = *v,
                    Self::BackgroundImage(v) => target.background_image.clone_from(v),
                    Self::BoxShadow(v) => target.box_shadow.clone_from(v),
                    Self::TransitionProperty(v) => target.transition_property.clone_from(v),
                    Self::TransitionDuration(v) => target.transition_duration.clone_from(v),
                    Self::TransitionDelay(v) => target.transition_delay.clone_from(v),
                    Self::TransitionTimingFunction(v) => target.transition_timing_function.clone_from(v),
                    Self::Transform(v) => target.transform.clone_from(v),
                    Self::TransformOrigin(v) => target.transform_origin.clone_from(v),
                    Self::Position(v) => target.position = *v,
                    Self::ZIndex(v) => target.z_index = *v,
                    Self::Isolation(v) => target.isolation = *v,
                    Self::Visibility(v) => target.visibility = *v,
                    Self::PointerEvents(v) => target.pointer_events = *v,
                    Self::UserSelect(v) => target.user_select = *v,
                    Self::Cursor(v) => target.cursor = *v,
                    Self::CaretColor(v) => target.caret_color = *v,
                    Self::CaretAnimation(v) => target.caret_animation = *v,
                    Self::LayoutKeyword(_, _) => unreachable!(),
                }
            }
        }
        impl LayoutProperty {
            pub(crate) fn differs(self, a: &Style, b: &Style) -> bool {
                if a.layout_keyword(self) != b.layout_keyword(self) { return true; }
                match self {
                    $(Self::$plain => a.layout.$($pf).+ != b.layout.$($pf).+,)*
                    $(Self::$optional => a.layout.$of != b.layout.$of,)*
                    $(Self::$length => a.layout.$($lf).+ != b.layout.$($lf).+,)*
                    Self::FlexGrow => a.layout.flex_grow != b.layout.flex_grow,
                    Self::FlexShrink => a.layout.flex_shrink != b.layout.flex_shrink,
                    Self::AspectRatio => a.layout.aspect_ratio != b.layout.aspect_ratio,
                    Self::OverflowX => a.layout.overflow.x != b.layout.overflow.x,
                    Self::OverflowY => a.layout.overflow.y != b.layout.overflow.y,
                }
            }
        }
    };
}
crate::style::properties::layout_properties!(define_declarations);

macro_rules! style_properties {
    ($( $name:ident => $($field:ident).+ ),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Property { Custom, Scroll(crate::style::scroll::ScrollProperty), Media(crate::style::media::MediaProperty), Layout(LayoutProperty), $($name),* }
        impl Property {
            pub(crate) const EXTRA: &'static [Self] = &[$(Self::$name),*];
            fn index(self) -> usize {
                match self {
                    Self::Custom | Self::Scroll(_) | Self::Media(_) => unreachable!("sparse properties track their own presence"),
                    Self::Layout(p) => p as usize,
                    p => LayoutProperty::ALL.len() + Self::EXTRA.iter().position(|v| *v == p).unwrap(),
                }
            }
            pub(crate) fn copy(self, source: &Style, target: &mut Style) {
                target.mark(self);
                match self {
                    Self::Custom => {},
                    Self::Media(p) => source.media.copy_property(p, &mut target.media),
                    Self::Scroll(p) => source.scroll.copy_property(p, &mut target.scroll),
                    Self::Layout(p) => {
                        target.clear_layout_keyword(p);
                        p.copy(&source.layout, &mut target.layout);
                        target.math_lengths.retain(|(key, _)| *key != p);
                        if let Some((_, value)) = source.math_lengths.iter().find(|(key, _)| *key == p) {
                            target.math_lengths.push((p, value.clone()));
                        }
                        if let Some(keyword) = source.layout_keyword(p) { target.set_layout_keyword(p, keyword); }
                    }
                    $(Self::$name => target.$($field).+.clone_from(&source.$($field).+)),*
                }
            }
            pub(crate) fn differs(self, a: &Style, b: &Style) -> bool {
                match self { Self::Custom => false, Self::Scroll(p) => a.scroll.get(p) != b.scroll.get(p), Self::Media(p) => a.media.get(p) != b.media.get(p), Self::Layout(p) => p.differs(a,b), $(Self::$name => a.$($field).+ != b.$($field).+),* }
            }
        }
    };
}
style_properties! { Display => layout.display, Color => color, Background => background, BorderColor => border_color,
BorderRadius => border_radius, FontFamily => font_family, FontWeight => font_weight, FontStyle => font_style,
FontFeatures => font_features, FontFallbacks => font_fallbacks, FontSize => font_size, LineHeight => line_height,
TextAlign => text_align, Direction => direction, TextWrap => text_wrap,
BackgroundImage => background_image,
BoxShadow => box_shadow,
TransitionProperty => transition_property,
TransitionDuration => transition_duration,
TransitionDelay => transition_delay,
TransitionTimingFunction => transition_timing_function,
Transform => transform,
TransformOrigin => transform_origin,
Position => position,
ZIndex => z_index,
Isolation => isolation,
Visibility => visibility,
PointerEvents => pointer_events,UserSelect => user_select,Cursor => cursor,
CaretColor => caret_color, CaretAnimation => caret_animation }

/// Fixed-size tracking: no per-setter allocations and explicit `auto`/`unset` survives cascade.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct PropertyMask(u128);
impl PropertyMask {
    pub(crate) fn insert(&mut self, p: Property) {
        if !matches!(
            p,
            Property::Custom | Property::Media(_) | Property::Scroll(_)
        ) {
            self.0 |= 1u128 << p.index();
        }
    }
    pub(crate) fn contains(self, p: Property) -> bool {
        !matches!(
            p,
            Property::Custom | Property::Media(_) | Property::Scroll(_)
        ) && self.0 & (1u128 << p.index()) != 0
    }
}
const _: () = assert!(LayoutProperty::ALL.len() + Property::EXTRA.len() <= 128);
