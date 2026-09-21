use crate::{
    core::layout::{BoxSizing, Direction, Display, LayoutStyle},
    style::{
        color::Color,
        declaration::{Property, PropertyMask},
        paint::PaintStyle,
        properties::{LayoutKeyword, LayoutProperty},
        text::{FontSize, LineHeight, TextAlignment, TextStyle},
        value::CssValue,
    },
};
use voidui_gpui_wgpu::{Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, SharedString};

/// Specified properties. Only CSS-inherited properties default to inheritance;
/// layout, spacing, backgrounds, and borders retain their own initial values.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub(crate) custom_properties:
        std::collections::BTreeMap<String, std::rc::Rc<super::css::values::Tokens>>,
    pub(crate) deferred: Vec<(Property, std::rc::Rc<super::css::values::PendingValue>)>,
    pub scroll: super::scroll::ScrollDeclarations,
    pub media: crate::style::media::MediaStyle,
    pub color: CssValue<Color>,
    pub caret_color: CssValue<Color>,
    pub caret_animation: CssValue<bool>,
    pub font_family: CssValue<SharedString>,
    pub font_weight: CssValue<FontWeight>,
    pub font_style: CssValue<FontStyle>,
    pub font_features: CssValue<FontFeatures>,
    pub font_fallbacks: CssValue<Option<FontFallbacks>>,
    pub font_size: CssValue<FontSize>,
    pub line_height: CssValue<LineHeight>,
    pub text_align: CssValue<TextAlignment>,
    pub direction: CssValue<Direction>,
    /// Soft wrapping inherits; explicit newlines remain preserved by the renderer.
    pub text_wrap: CssValue<bool>,
    pub background: CssValue<Color>,
    /// Defaults to currentColor on this element, without inheriting border color.
    pub border_color: CssValue<Color>,
    pub border_radius: CssValue<f32>,
    pub background_image: CssValue<crate::style::gradient::BackgroundImages>,
    pub box_shadow: CssValue<crate::style::shadow::BoxShadows>,
    pub transition_property:
        CssValue<crate::style::list::StyleList<crate::style::transition::TransitionProperty>>,
    pub transition_duration: CssValue<crate::style::list::StyleList<f64>>,
    pub transition_delay: CssValue<crate::style::list::StyleList<f64>>,
    pub transition_timing_function:
        CssValue<crate::style::list::StyleList<crate::style::transition::Easing>>,
    pub transform: CssValue<super::transform::Transform>,
    pub transform_origin: CssValue<super::transform::TransformOrigin>,
    pub position: CssValue<crate::style::layer::Position>,
    pub z_index: CssValue<crate::style::layer::ZIndex>,
    pub isolation: CssValue<crate::style::layer::Isolation>,
    pub visibility: CssValue<crate::style::layer::Visibility>,
    pub pointer_events: CssValue<crate::style::layer::PointerEvents>,
    pub user_select: CssValue<crate::style::selection::UserSelect>,
    pub cursor: CssValue<crate::style::selection::Cursor>,
    pub layout: LayoutStyle,
    pub(crate) math_lengths: Vec<(LayoutProperty, super::math::MathLength)>,
    layout_keywords: Vec<(LayoutProperty, LayoutKeyword)>,
    specified: PropertyMask,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            custom_properties: Default::default(),
            deferred: Vec::new(),
            scroll: Default::default(),
            media: Default::default(),
            color: CssValue::Unset,
            caret_color: CssValue::Unset,
            caret_animation: CssValue::Unset,
            font_family: CssValue::Unset,
            font_weight: CssValue::Unset,
            font_style: CssValue::Unset,
            font_features: CssValue::Unset,
            font_fallbacks: CssValue::Unset,
            font_size: CssValue::Unset,
            line_height: CssValue::Unset,
            text_align: CssValue::Unset,
            direction: CssValue::Unset,
            text_wrap: CssValue::Unset,
            background: CssValue::Unset,
            border_color: CssValue::Unset,
            border_radius: CssValue::Unset,
            background_image: CssValue::Unset,
            box_shadow: CssValue::Unset,
            transition_property: CssValue::Unset,
            transition_duration: CssValue::Unset,
            transition_delay: CssValue::Unset,
            transition_timing_function: CssValue::Unset,
            transform: CssValue::Unset,
            transform_origin: CssValue::Unset,
            position: CssValue::Unset,
            z_index: CssValue::Unset,
            isolation: CssValue::Unset,
            visibility: CssValue::Unset,
            pointer_events: CssValue::Unset,
            user_select: CssValue::Unset,
            cursor: CssValue::Unset,
            layout_keywords: Vec::new(),
            math_lengths: Vec::new(),
            specified: PropertyMask::default(),
            layout: LayoutStyle {
                display: Display::Block,
                box_sizing: BoxSizing::ContentBox,
                ..LayoutStyle::default()
            },
        }
    }
}

impl Style {
    /// Keep each authored length's owner with its raw Taffy handle, even while
    /// a CSS-wide keyword temporarily overrides that value during computation.
    pub(crate) fn retain_length(
        &mut self,
        property: LayoutProperty,
        value: impl super::value::ValidateCssLength,
    ) {
        let math = value.calc_handle().map(super::math::MathLength::retain);
        self.math_lengths.retain(|(key, _)| *key != property);
        if let Some(math) = math {
            self.math_lengths.push((property, math));
        }
    }

    pub fn new(color: impl Into<Color>, background: impl Into<Color>) -> Self {
        let mut style = Self {
            color: CssValue::Value(color.into()),
            background: CssValue::Value(background.into()),
            ..Self::default()
        };
        style.mark(Property::Color);
        style.mark(Property::Background);
        style
    }

    /// Record a raw default-valued write when editing Style fields directly.
    pub(crate) fn is_marked(&self, property: Property) -> bool {
        self.specified.contains(property)
    }

    pub fn mark(&mut self, property: Property) {
        self.specified.insert(property);
        self.deferred.retain(|(p, _)| *p != property);
    }

    pub(crate) fn overlay_inline(&mut self, source: &Style, initial: &Style) {
        self.custom_properties
            .extend(source.custom_properties.clone());
        for property in LayoutProperty::ALL
            .iter()
            .copied()
            .map(Property::Layout)
            .chain(Property::EXTRA.iter().copied())
            .chain(
                super::scroll::ScrollProperty::ALL
                    .iter()
                    .copied()
                    .map(Property::Scroll),
            )
            .chain(
                crate::style::media::MediaProperty::ALL
                    .iter()
                    .copied()
                    .map(Property::Media),
            )
        {
            if source.specified.contains(property) || property.differs(source, initial) {
                property.copy(source, self);
                if let Some((_, value)) = source.deferred.iter().find(|(p, _)| *p == property) {
                    self.deferred.push((property, value.clone()));
                }
            }
        }
    }

    pub(crate) fn layout_keyword(&self, property: LayoutProperty) -> Option<LayoutKeyword> {
        self.layout_keywords
            .iter()
            .find_map(|(key, value)| (*key == property).then_some(*value))
    }

    /// Set a full descriptor as independent longhands. Later family/weight/style
    /// setters replace only their own property, following declaration order.
    pub fn set_font(&mut self, font: Font) {
        for p in [
            Property::FontFamily,
            Property::FontWeight,
            Property::FontStyle,
            Property::FontFeatures,
            Property::FontFallbacks,
        ] {
            self.mark(p);
        }
        self.font_family = font.family.into();
        self.font_weight = font.weight.into();
        self.font_style = font.style.into();
        self.font_features = font.features.into();
        self.font_fallbacks = font.fallbacks.into();
    }

    pub fn set_layout_keyword(&mut self, property: LayoutProperty, keyword: LayoutKeyword) {
        if let Some(p) = match property {
            LayoutProperty::OverflowX => Some(super::scroll::ScrollProperty::OverflowX),
            LayoutProperty::OverflowY => Some(super::scroll::ScrollProperty::OverflowY),
            _ => None,
        } {
            self.scroll.set(
                p,
                match keyword {
                    LayoutKeyword::Inherit => CssValue::Inherit,
                    LayoutKeyword::Initial => CssValue::Initial,
                },
            );
        }
        self.mark(Property::Layout(property));
        if let Some((_, existing)) = self
            .layout_keywords
            .iter_mut()
            .find(|(key, _)| *key == property)
        {
            *existing = keyword;
        } else {
            self.layout_keywords.push((property, keyword));
        }
    }

    pub fn clear_layout_keyword(&mut self, property: LayoutProperty) {
        self.mark(Property::Layout(property));
        self.layout_keywords.retain(|(key, _)| *key != property);
    }

    pub fn clear_layout_keywords(&mut self) {
        self.layout_keywords.clear();
    }

    /// Preserve specified styles. Taffy receives a separately computed layout.
    pub(crate) fn resolve_layout(&self, parent: &LayoutStyle, direction: Direction) -> LayoutStyle {
        let mut resolved = self.layout.clone();
        if !self.layout_keywords.is_empty() {
            let initial = Self::default().layout;
            for (property, keyword) in &self.layout_keywords {
                property.copy(
                    match keyword {
                        LayoutKeyword::Inherit => parent,
                        LayoutKeyword::Initial => &initial,
                    },
                    &mut resolved,
                );
            }
        }
        resolved.direction = direction;
        resolved
    }

    pub fn resolve_paint(&self, parent: &PaintStyle) -> PaintStyle {
        let initial = PaintStyle::default();
        PaintStyle {
            background_image: self.background_image.resolve(
                &parent.background_image,
                &initial.background_image,
                false,
            ),
            box_shadow: self
                .box_shadow
                .resolve(&parent.box_shadow, &initial.box_shadow, false),
            background: self
                .background
                .resolve(&parent.background, &initial.background, false),
            border_color: self.border_color.resolve(
                &parent.border_color,
                &initial.border_color,
                false,
            ),
            border_radius: self.border_radius.resolve(
                &parent.border_radius,
                &initial.border_radius,
                false,
            ),
        }
    }

    pub(crate) fn resolve_transitions(
        &self,
        parent: &crate::style::transition::TransitionStyle,
    ) -> crate::style::transition::TransitionStyle {
        use crate::style::transition::TransitionStyle;
        let initial = TransitionStyle::default();
        TransitionStyle {
            properties: self.transition_property.resolve(
                &parent.properties,
                &initial.properties,
                false,
            ),
            durations: self.transition_duration.resolve(
                &parent.durations,
                &initial.durations,
                false,
            ),
            delays: self
                .transition_delay
                .resolve(&parent.delays, &initial.delays, false),
            easing: self
                .transition_timing_function
                .resolve(&parent.easing, &initial.easing, false),
        }
    }

    pub fn resolve_text(&self, parent: &TextStyle) -> TextStyle {
        self.resolve_text_with_defaults(parent, &TextStyle::default())
    }

    pub(crate) fn resolve_text_with_defaults(
        &self,
        parent: &TextStyle,
        initial: &TextStyle,
    ) -> TextStyle {
        let size = self
            .font_size
            .resolve(
                &FontSize::Pixels(parent.font_size),
                &FontSize::Pixels(initial.font_size),
                true,
            )
            .resolve(parent.font_size);
        let height = self
            .line_height
            .resolve(&parent.line_height, &initial.line_height, true)
            .computed(size);
        let resolved = TextStyle {
            caret_color: self
                .caret_color
                .resolve(&parent.caret_color, &initial.caret_color, true),
            caret_animation: self.caret_animation.resolve(
                &parent.caret_animation,
                &initial.caret_animation,
                true,
            ),
            // On color itself, currentColor is equivalent to inherited color.
            color: self
                .color
                .resolve(&parent.color, &initial.color, true)
                .resolve(parent.color),
            font: Font {
                family: self
                    .font_family
                    .resolve(&parent.font.family, &initial.font.family, true),
                weight: self
                    .font_weight
                    .resolve(&parent.font.weight, &initial.font.weight, true),
                style: self
                    .font_style
                    .resolve(&parent.font.style, &initial.font.style, true),
                features: self.font_features.resolve(
                    &parent.font.features,
                    &initial.font.features,
                    true,
                ),
                fallbacks: self.font_fallbacks.resolve(
                    &parent.font.fallbacks,
                    &initial.font.fallbacks,
                    true,
                ),
            },
            font_size: size,
            line_height: height,
            align: self.text_align.resolve(&parent.align, &initial.align, true),
            direction: self
                .direction
                .resolve(&parent.direction, &initial.direction, true),
            wrap: self.text_wrap.resolve(&parent.wrap, &initial.wrap, true),
        };
        assert!(
            resolved.font_size.is_finite() && resolved.font_size > 0.0,
            "font_size must be finite and positive"
        );
        let height = resolved.line_height.resolve(resolved.font_size);
        assert!(
            height.is_finite() && height > 0.0,
            "line_height must resolve to a finite positive value"
        );
        resolved
    }
}
