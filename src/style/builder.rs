//! Canonical fluent style implementations shared by widgets and components.
//! Each setter changes only its property; shorthands expand at the call site.
use crate::style::value::ValidateCssLength;
pub use crate::{
    core::layout::*,
    style::{
        color::Color,
        declaration::Property,
        properties::{LayoutKeyword, LayoutProperty},
        text::{FontSize, LineHeight, TextAlignment},
        value::{CssValue, IntoCssLength},
    },
};
pub use taffy;
pub use taffy::Dimension;
pub use voidui_gpui_wgpu::{
    Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, SharedString,
};

/// A borrowed editor shared by all fluent APIs. Creating it adds no UI node.
#[doc(hidden)]
pub struct StyleBuilder<'a> {
    pub(crate) style: &'a mut crate::style::style::Style,
}
impl<'a> StyleBuilder<'a> {
    pub fn new(style: &'a mut crate::style::style::Style) -> Self {
        Self { style }
    }
}

/// Storage access used by generated inherent methods; callers need no trait import.
#[doc(hidden)]
pub trait StyleTarget {
    fn __style_mut(&mut self) -> &mut crate::style::style::Style;
}
impl<W> StyleTarget for crate::core::widget::WidgetBuilder<W> {
    fn __style_mut(&mut self) -> &mut crate::style::style::Style {
        &mut self.props.style
    }
}

// Signatures and explicit-property tracking come from the existing layout table.
macro_rules! builder_layout_properties {
    (layout_setters { $( $(#[$pd:meta])* $plain:ident => $pn:ident($pt:ty) => $($pf:ident).+; )* }
     optional_setters { $( $(#[$od:meta])* $opt:ident => $on:ident($ot:ty) => $of:ident; )* }
     length_setters { $( $(#[$ld:meta])* $len:ident => $ln:ident($lt:ty), $signed:literal => $($lf:ident).+; )* }) => {
        #[voidui_macros::style_methods(__voidui_layout_styles)]
        impl<'a> StyleBuilder<'a> {
            $( $(#[$pd])* pub fn $pn(self, value: $pt) -> Self {
                self.style.clear_layout_keyword(LayoutProperty::$plain);
                self.style.layout.$($pf).+ = value; self
            } )*
            $( $(#[$od])* pub fn $on(self, value: $ot) -> Self {
                self.style.clear_layout_keyword(LayoutProperty::$opt);
                self.style.layout.$of = Some(value); self
            } )*
            $( $(#[$ld])* pub fn $ln(self, value: impl IntoCssLength<$lt>) -> Self {
                let value = value.into_css_length();
                self.style.clear_layout_keyword(LayoutProperty::$len);
                self.style.layout.$($lf).+ = value.validate(stringify!($ln), $signed);
                self.style.retain_length(LayoutProperty::$len, value); self
            } )*
        }
    };
}
crate::style::properties::layout_properties!(builder_layout_properties);

macro_rules! text_setters {
    ($( $(#[$doc:meta])* $prop:ident => $name:ident($ty:ty) => $field:ident; )*) => {
        #[voidui_macros::style_methods(__voidui_text_styles)]
        impl<'a> StyleBuilder<'a> { $(
            $(#[$doc])*
            pub fn $name(self, value: impl Into<CssValue<$ty>>) -> Self {
                self.style.mark(Property::$prop);
                self.style.$field = value.into(); self
            }
        )* }
    };
}
text_setters! {
    /// Set inherited color, or use CssValue::{Inherit, Initial, Unset}.
    Color => color(Color) => color;
    /// Set only font-family; inherited weight/style are preserved.
    FontFamily => font(SharedString) => font_family;
    /// Set normal/italic/oblique font selection independently of the family.
    FontStyle => font_style(FontStyle) => font_style;
    /// Set OpenType font feature overrides.
    FontFeatures => font_features(FontFeatures) => font_features;
    /// Set or clear the font fallback list.
    FontFallbacks => font_fallbacks(Option<FontFallbacks>) => font_fallbacks;
    /// Set inherited text alignment, including logical start/end.
    TextAlign => text_align(TextAlignment) => text_align;
    /// Set inherited direction for boxes and logical text alignment.
    Direction => direction(Direction) => direction;
    /// Enable/disable inherited soft wrapping; explicit newlines are preserved.
    TextWrap => wrap(bool) => text_wrap;
}

#[voidui_macros::style_methods(__voidui_base_styles)]
impl<'a> StyleBuilder<'a> {
    /// Select the layout mode supported by the box engine.
    pub fn display(self, value: Display) -> Self {
        self.style.mark(Property::Display);
        self.style.layout.display = value;
        self
    }

    /// Replace the layout value for reusable low-level presets; no callback is needed.
    pub fn layout_style(self, style: LayoutStyle) -> Self {
        // Retain copied calculation handles before releasing any previous style.
        self.style.math_lengths = super::math::retain_layout(&style);
        for p in LayoutProperty::ALL {
            self.style.mark(Property::Layout(*p));
        }
        self.style.mark(Property::Display);
        self.style.mark(Property::Direction);
        self.style.clear_layout_keywords();
        self.style.direction = CssValue::Value(style.direction);
        self.style.position = crate::style::layer::Position::from(style.position).into();
        self.style.mark(Property::Position);
        self.style.scroll.set(
            super::scroll::ScrollProperty::OverflowX,
            super::scroll::ScrollValue::Overflow(style.overflow.x.into()).into(),
        );
        self.style.scroll.set(
            super::scroll::ScrollProperty::OverflowY,
            super::scroll::ScrollValue::Overflow(style.overflow.y.into()).into(),
        );
        self.style.layout = style;
        self
    }

    /// Explicitly inherit a supported layout longhand, even though layout
    /// properties do not inherit by default. Percentages remain percentages.
    pub fn inherit(self, property: LayoutProperty) -> Self {
        self.style
            .set_layout_keyword(property, LayoutKeyword::Inherit);
        self
    }

    /// Restore a layout property's initial value. Inline display is not modeled
    /// by Taffy; Display is therefore intentionally excluded from LayoutProperty.
    pub fn initial(self, property: LayoutProperty) -> Self {
        self.style
            .set_layout_keyword(property, LayoutKeyword::Initial);
        self
    }

    /// Unset has the initial-value behavior for these non-inherited properties.
    pub fn unset(self, property: LayoutProperty) -> Self {
        self.initial(property)
    }

    /// Enable a horizontal Flexbox container.
    pub fn flex_row(self) -> Self {
        self.flex().flex_direction(FlexDirection::Row)
    }
    /// Enable a vertical Flexbox container.
    pub fn flex_col(self) -> Self {
        self.flex().flex_direction(FlexDirection::Column)
    }
    /// Enable a horizontal Flexbox container in reverse order.
    pub fn flex_row_reverse(self) -> Self {
        self.flex().flex_direction(FlexDirection::RowReverse)
    }
    /// Enable a vertical Flexbox container in reverse order.
    pub fn flex_col_reverse(self) -> Self {
        self.flex().flex_direction(FlexDirection::ColumnReverse)
    }
    pub fn block(self) -> Self {
        self.display(Display::Block)
    }
    pub fn flow_root(self) -> Self {
        self.display(Display::FlowRoot)
    }
    pub fn flex(self) -> Self {
        self.display(Display::Flex)
    }
    pub fn grid(self) -> Self {
        self.display(Display::Grid)
    }
    pub fn hidden(self) -> Self {
        self.display(Display::None)
    }
    /// Choose CSS static, relative, absolute or viewport-fixed positioning.
    pub fn position(self, value: impl Into<CssValue<crate::style::layer::Position>>) -> Self {
        crate::style::declaration::Declaration::Position(value.into()).apply(self.style);
        self
    }
    /// Keep this in-flow box inside its nearest scrollport while scrolling.
    pub fn sticky(self) -> Self {
        self.position(crate::style::layer::Position::Sticky)
    }
    /// Apply a CSS 2D transform after layout. Percentages use this box's size.
    pub fn transform(self, value: impl Into<CssValue<super::transform::Transform>>) -> Self {
        super::declaration::Declaration::Transform(value.into()).apply(self.style);
        self
    }
    /// Set the transform pivot, measured from the border-box top-left corner.
    pub fn transform_origin(
        self,
        value: impl Into<CssValue<super::transform::TransformOrigin>>,
    ) -> Self {
        super::declaration::Declaration::TransformOrigin(value.into()).apply(self.style);
        self
    }
    pub fn fixed(self) -> Self {
        self.position(crate::style::layer::Position::Fixed)
    }
    pub fn static_position(self) -> Self {
        self.position(crate::style::layer::Position::Static)
    }
    /// Integer z-index creates a stacking context on positioned boxes and flex/grid items.
    pub fn z_index(self, value: impl Into<CssValue<crate::style::layer::ZIndex>>) -> Self {
        crate::style::declaration::Declaration::ZIndex(value.into()).apply(self.style);
        self
    }
    pub fn isolation(self, value: impl Into<CssValue<crate::style::layer::Isolation>>) -> Self {
        crate::style::declaration::Declaration::Isolation(value.into()).apply(self.style);
        self
    }
    pub fn visibility(self, value: impl Into<CssValue<crate::style::layer::Visibility>>) -> Self {
        crate::style::declaration::Declaration::Visibility(value.into()).apply(self.style);
        self
    }
    pub fn pointer_events(
        self,
        value: impl Into<CssValue<crate::style::layer::PointerEvents>>,
    ) -> Self {
        crate::style::declaration::Declaration::PointerEvents(value.into()).apply(self.style);
        self
    }
    /// Control user-initiated selection using CSS auto/text/none/all/contain.
    pub fn user_select(
        self,
        value: impl Into<CssValue<crate::style::selection::UserSelect>>,
    ) -> Self {
        crate::style::declaration::Declaration::UserSelect(value.into()).apply(self.style);
        self
    }
    /// Choose a standard cursor keyword. Auto uses a text cursor over selectable text.
    pub fn cursor(self, value: impl Into<CssValue<crate::style::selection::Cursor>>) -> Self {
        crate::style::declaration::Declaration::Cursor(value.into()).apply(self.style);
        self
    }
    pub fn relative(self) -> Self {
        self.position(Position::Relative)
    }
    pub fn absolute(self) -> Self {
        self.position(Position::Absolute)
    }
    pub fn items_center(self) -> Self {
        self.align_items(AlignItems::CENTER)
    }
    pub fn justify_center(self) -> Self {
        self.justify_content(JustifyContent::CENTER)
    }
    pub fn justify_between(self) -> Self {
        self.justify_content(JustifyContent::SPACE_BETWEEN)
    }
    pub fn bold(self) -> Self {
        self.font_weight(FontWeight::BOLD)
    }
    pub fn italic(self) -> Self {
        self.font_style(FontStyle::Italic)
    }

    /// Set both dimensions, without changing min/max sizes.
    pub fn size(
        self,
        width: impl IntoCssLength<Dimension>,
        height: impl IntoCssLength<Dimension>,
    ) -> Self {
        self.width(width).height(height)
    }
    /// Set the same flex/grid gap on both axes. Block flow ignores gaps.
    pub fn gap(self, value: impl IntoCssLength<LengthPercentage>) -> Self {
        let value = value.into_css_length();
        self.row_gap(value).column_gap(value)
    }
    /// Set every margin from an explicit edge structure.
    pub fn margin_edges(self, v: Rect<LengthPercentageAuto>) -> Self {
        self.margin_top(v.top)
            .margin_right(v.right)
            .margin_bottom(v.bottom)
            .margin_left(v.left)
    }
    /// Set every padding edge, validating each value.
    pub fn padding_edges(self, v: Rect<LengthPercentage>) -> Self {
        self.padding_top(v.top)
            .padding_right(v.right)
            .padding_bottom(v.bottom)
            .padding_left(v.left)
    }
    /// Set every border width.
    pub fn border_widths(self, v: Rect<LengthPercentage>) -> Self {
        self.border_top_width(v.top)
            .border_right_width(v.right)
            .border_bottom_width(v.bottom)
            .border_left_width(v.left)
    }
    /// Set every positioning inset.
    pub fn inset_edges(self, v: Rect<LengthPercentageAuto>) -> Self {
        self.top(v.top).right(v.right).bottom(v.bottom).left(v.left)
    }
    /// Set every margin; a later side setter overrides only that side.
    pub fn margin(self, value: impl IntoCssLength<LengthPercentageAuto>) -> Self {
        let v = value.into_css_length();
        self.margin_edges(Rect {
            top: v,
            right: v,
            bottom: v,
            left: v,
        })
    }
    pub fn margin_x(self, value: impl IntoCssLength<LengthPercentageAuto>) -> Self {
        let v = value.into_css_length();
        self.margin_left(v).margin_right(v)
    }
    pub fn margin_y(self, value: impl IntoCssLength<LengthPercentageAuto>) -> Self {
        let v = value.into_css_length();
        self.margin_top(v).margin_bottom(v)
    }
    /// Set every padding edge. CSS resolves all percentages against parent width.
    pub fn padding(self, value: impl IntoCssLength<LengthPercentage>) -> Self {
        let v = value.into_css_length();
        self.padding_edges(Rect {
            top: v,
            right: v,
            bottom: v,
            left: v,
        })
    }
    pub fn padding_x(self, value: impl IntoCssLength<LengthPercentage>) -> Self {
        let v = value.into_css_length();
        self.padding_left(v).padding_right(v)
    }
    pub fn padding_y(self, value: impl IntoCssLength<LengthPercentage>) -> Self {
        let v = value.into_css_length();
        self.padding_top(v).padding_bottom(v)
    }
    /// Set all widths of the currently supported solid border.
    pub fn border_width(self, value: impl IntoCssLength<LengthPercentage>) -> Self {
        let v = value.into_css_length();
        self.border_widths(Rect {
            top: v,
            right: v,
            bottom: v,
            left: v,
        })
    }
    pub fn inset(self, value: impl IntoCssLength<LengthPercentageAuto>) -> Self {
        let v = value.into_css_length();
        self.inset_edges(Rect {
            top: v,
            right: v,
            bottom: v,
            left: v,
        })
    }
    /// Set independent CSS overflow axes; legacy Taffy overflow values also convert.
    pub fn overflow_axes(self, value: Point<impl Into<super::scroll::Overflow>>) -> Self {
        self.overflow_x(value.x).overflow_y(value.y)
    }
    /// Set both CSS overflow axes. Auto shows scrollbars only when content overflows.
    pub fn overflow(self, value: impl Into<super::scroll::Overflow>) -> Self {
        let value = value.into();
        self.overflow_x(value).overflow_y(value)
    }
    /// Set horizontal overflow without changing vertical overflow.
    pub fn overflow_x(self, value: impl Into<super::scroll::Overflow>) -> Self {
        self.style.clear_layout_keyword(LayoutProperty::OverflowX);
        self.style.scroll.set(
            super::scroll::ScrollProperty::OverflowX,
            super::scroll::ScrollValue::Overflow(value.into()).into(),
        );
        self
    }
    /// Set vertical overflow without changing horizontal overflow.
    pub fn overflow_y(self, value: impl Into<super::scroll::Overflow>) -> Self {
        self.style.clear_layout_keyword(LayoutProperty::OverflowY);
        self.style.scroll.set(
            super::scroll::ScrollProperty::OverflowY,
            super::scroll::ScrollValue::Overflow(value.into()).into(),
        );
        self
    }
    pub fn flex_grow(self, value: impl Into<f64>) -> Self {
        let value = value.into() as f32;
        self.style.clear_layout_keyword(LayoutProperty::FlexGrow);
        assert!(
            value.is_finite() && value >= 0.0,
            "flex_grow must be finite and nonnegative"
        );
        self.style.layout.flex_grow = value;
        self
    }
    pub fn flex_shrink(self, value: impl Into<f64>) -> Self {
        let value = value.into() as f32;
        self.style.clear_layout_keyword(LayoutProperty::FlexShrink);
        assert!(
            value.is_finite() && value >= 0.0,
            "flex_shrink must be finite and nonnegative"
        );
        self.style.layout.flex_shrink = value;
        self
    }
    pub fn aspect_ratio(self, value: impl Into<f64>) -> Self {
        let value = value.into() as f32;
        self.style.clear_layout_keyword(LayoutProperty::AspectRatio);
        assert!(
            value.is_finite() && value > 0.0,
            "aspect_ratio must be finite and positive"
        );
        self.style.layout.aspect_ratio = Some(value);
        self
    }
    pub fn background(self, color: impl Into<CssValue<Color>>) -> Self {
        self.style.mark(Property::Background);
        self.style.background = color.into();
        self
    }
    /// Set CSS background-image layers without resetting background-color.
    pub fn background_image(
        self,
        images: impl Into<CssValue<crate::style::gradient::BackgroundImages>>,
    ) -> Self {
        self.style.mark(Property::BackgroundImage);
        self.style.background_image = images.into();
        self
    }
    /// The first box shadow is painted on top. Shadows never affect layout size.
    pub fn box_shadow(
        self,
        shadows: impl Into<CssValue<crate::style::shadow::BoxShadows>>,
    ) -> Self {
        self.style.mark(Property::BoxShadow);
        let shadows = shadows.into();
        if let CssValue::Value(list) = &shadows {
            for s in list.iter() {
                assert!(
                    [s.offset_x, s.offset_y, s.blur, s.spread]
                        .iter()
                        .all(|v| v.is_finite())
                        && s.blur >= 0.0,
                    "invalid box shadow"
                );
            }
        }
        self.style.box_shadow = shadows;
        self
    }
    /// Set all transition longhands together, following CSS shorthand semantics.
    pub fn transition(
        self,
        transitions: impl Into<crate::style::list::StyleList<crate::style::transition::Transition>>,
    ) -> Self {
        let transitions = transitions.into();
        let mut properties = Vec::new();
        let mut durations = Vec::new();
        let mut delays = Vec::new();
        let mut timing = Vec::new();
        for t in transitions.iter() {
            assert!(
                t.delay.is_finite() && t.easing.is_valid(),
                "invalid transition"
            );
            properties.push(t.property.clone());
            durations.push(t.duration.as_secs_f64());
            delays.push(t.delay);
            timing.push(t.easing.clone());
        }
        if properties.is_empty() {
            properties.push("none".into());
        }
        use crate::style::{declaration::Declaration as D, list::StyleList};
        for d in [
            D::TransitionProperty(StyleList::from(properties).into()),
            D::TransitionDuration(StyleList::from(durations).into()),
            D::TransitionDelay(StyleList::from(delays).into()),
            D::TransitionTimingFunction(StyleList::from(timing).into()),
        ] {
            d.apply(self.style);
        }
        self
    }
    /// Set transition-property independently of the other transition longhands.
    pub fn transition_property(
        self,
        value: impl Into<
            CssValue<crate::style::list::StyleList<crate::style::transition::TransitionProperty>>,
        >,
    ) -> Self {
        crate::style::declaration::Declaration::TransitionProperty(value.into()).apply(self.style);
        self
    }
    /// Durations in seconds; each value must be finite and nonnegative.
    pub fn transition_duration(
        self,
        value: impl Into<CssValue<crate::style::list::StyleList<f64>>>,
    ) -> Self {
        let value = value.into();
        if let CssValue::Value(v) = &value {
            assert!(
                v.iter().all(|v| v.is_finite() && *v >= 0.0),
                "invalid transition duration"
            );
        }
        crate::style::declaration::Declaration::TransitionDuration(value).apply(self.style);
        self
    }
    /// Delays in seconds. Negative values skip ahead into the transition.
    pub fn transition_delay(
        self,
        value: impl Into<CssValue<crate::style::list::StyleList<f64>>>,
    ) -> Self {
        let value = value.into();
        if let CssValue::Value(v) = &value {
            assert!(v.iter().all(|v| v.is_finite()), "invalid transition delay");
        }
        crate::style::declaration::Declaration::TransitionDelay(value).apply(self.style);
        self
    }
    /// Set a list of easing functions using CSS's list repetition rules.
    pub fn transition_timing_function(
        self,
        value: impl Into<CssValue<crate::style::list::StyleList<crate::style::transition::Easing>>>,
    ) -> Self {
        let value = value.into();
        if let CssValue::Value(v) = &value {
            assert!(v.iter().all(|v| v.is_valid()), "invalid transition easing");
        }
        crate::style::declaration::Declaration::TransitionTimingFunction(value).apply(self.style);
        self
    }
    pub fn border_color(self, color: impl Into<CssValue<Color>>) -> Self {
        self.style.mark(Property::BorderColor);
        self.style.border_color = color.into();
        self
    }
    /// Set a uniform logical-pixel radius, or explicitly default this paint property.
    pub fn border_radius(self, radius: impl Into<CssValue<f32>>) -> Self {
        self.style.mark(Property::BorderRadius);
        let radius = radius.into();
        if let CssValue::Value(value) = radius {
            assert!(
                value.is_finite() && value >= 0.0,
                "border_radius must be finite and nonnegative"
            );
        }
        self.style.border_radius = radius;
        self
    }
    /// CSS font-family alias for `font`, without resetting weight or style.
    pub fn font_family(self, family: impl Into<CssValue<SharedString>>) -> Self {
        self.font(family)
    }

    /// Set an absolute CSS font weight (1..=1000), independently of the family.
    pub fn font_weight(self, value: impl Into<CssValue<FontWeight>>) -> Self {
        self.style.mark(Property::FontWeight);
        let value = value.into();
        if let CssValue::Value(weight) = &value {
            assert!(
                weight.0.is_finite() && (1.0..=1000.0).contains(&weight.0),
                "font_weight must be between 1 and 1000"
            );
        }
        self.style.font_weight = value;
        self
    }

    /// Expand a full font descriptor into independently overridable longhands.
    pub fn font_options(self, font: Font) -> Self {
        self.style.set_font(font);
        self
    }
    pub fn font_size(self, value: impl Into<CssValue<FontSize>>) -> Self {
        self.style.mark(Property::FontSize);
        let value = value.into();
        if let CssValue::Value(size) = value {
            assert!(
                size.resolve(1.0).is_finite() && size.resolve(1.0) > 0.0,
                "font_size must be finite and positive"
            );
        }
        self.style.font_size = value;
        self
    }
    pub fn line_height(self, value: impl Into<CssValue<LineHeight>>) -> Self {
        self.style.mark(Property::LineHeight);
        let value = value.into();
        if let CssValue::Value(height) = value {
            assert!(
                height.resolve(1.0).is_finite() && height.resolve(1.0) > 0.0,
                "line_height must be finite and positive"
            );
        }
        self.style.line_height = value;
        self
    }
}

impl From<&str> for CssValue<SharedString> {
    fn from(value: &str) -> Self {
        Self::Value(value.into())
    }
}
impl From<String> for CssValue<SharedString> {
    fn from(value: String) -> Self {
        Self::Value(value.into())
    }
}
impl From<&String> for CssValue<SharedString> {
    fn from(value: &String) -> Self {
        Self::Value(value.as_str().into())
    }
}

#[voidui_macros::style_methods(__voidui_media_styles)]
impl<'a> StyleBuilder<'a> {
    /// Set CSS object-fit without affecting the image's intrinsic dimensions.
    pub fn object_fit(self, value: crate::style::media::ObjectFit) -> Self {
        use crate::style::media::*;
        self.style.media.set(MediaDeclaration {
            property: MediaProperty::ObjectFit,
            value: MediaValue::Fit(value).into(),
        });
        self
    }
    pub fn object_position(self, value: crate::style::media::ObjectPosition) -> Self {
        use crate::style::media::*;
        self.style.media.set(MediaDeclaration {
            property: MediaProperty::ObjectPosition,
            value: MediaValue::Position(value).into(),
        });
        self
    }
    /// Select smooth, crisp-edges or pixelated image sampling.
    pub fn image_rendering(self, value: crate::render::ImageSampling) -> Self {
        use crate::style::media::*;
        self.style.media.set(MediaDeclaration {
            property: MediaProperty::ImageRendering,
            value: MediaValue::Sampling(value).into(),
        });
        self
    }
}
macro_rules! svg_paint_setters { ($( $name:ident => $property:literal ),* $(,)?)=> { #[voidui_macros::style_methods(__voidui_svg_styles)] impl<'a> StyleBuilder<'a> { $(
    #[doc=concat!("Set standard CSS `",$property,"`. Invalid values panic; use Stylesheet::parse for fallible input.")]
    pub fn $name(self,value:impl std::fmt::Display)->Self {
        crate::style::media::MediaDeclaration::parse($property,&value.to_string()).expect(concat!("invalid ",$property)).apply(self.style);self
    }
)* } }; }
svg_paint_setters! { fill=>"fill",stroke=>"stroke",stroke_width=>"stroke-width",stroke_linecap=>"stroke-linecap",stroke_linejoin=>"stroke-linejoin",stroke_dasharray=>"stroke-dasharray",stroke_dashoffset=>"stroke-dashoffset",fill_rule=>"fill-rule",fill_opacity=>"fill-opacity",stroke_opacity=>"stroke-opacity",opacity=>"opacity" }

// Reexport at the definition site so IDE name resolution does not depend on
// macro_use importing a procedural macro's generated macro into another module.
#[doc(hidden)]
pub use {
    __voidui_base_styles, __voidui_layout_styles, __voidui_media_styles, __voidui_svg_styles,
    __voidui_text_styles,
};
