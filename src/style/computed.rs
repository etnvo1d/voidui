//! Computed styles are independent of layout output. Animation changes these
//! values first; only geometry or font-metric changes invalidate Taffy caches.
use super::{paint::PaintStyle, style::Style, text::TextStyle};
use crate::core::layout::LayoutStyle;

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub(crate) custom_properties: super::css::values::CustomProperties,
    pub(crate) root_font_size: f32,
    pub(crate) viewport_size: [f32; 2],
    pub transform: super::transform::Transform,
    pub transform_origin: super::transform::TransformOrigin,
    pub(crate) sticky_inset:
        Option<Box<crate::core::layout::Rect<crate::core::layout::LengthPercentageAuto>>>,
    pub scroll: super::scroll::ScrollStyle,
    pub media: super::media::MediaStyle,
    pub text: TextStyle,
    pub paint: PaintStyle,
    pub layout: LayoutStyle,
    pub(crate) math_lengths: Vec<(super::properties::LayoutProperty, super::math::MathLength)>,
    pub layer: super::layer::LayerStyle,
    pub selection: super::selection::SelectionStyle,
}
impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            custom_properties: Default::default(),
            root_font_size: TextStyle::default().font_size,
            viewport_size: [0.0; 2],
            transform: Default::default(),
            transform_origin: Default::default(),
            sticky_inset: None,
            scroll: Default::default(),
            media: Default::default(),
            text: TextStyle::default(),
            paint: PaintStyle::default(),
            layout: Style::default().layout,
            math_lengths: Vec::new(),
            layer: Default::default(),
            selection: Default::default(),
        }
    }
}
impl ComputedStyle {
    /// Top-layer styling changes the computed position before insets are mapped
    /// to Taffy, including CSS-wide inherited inset values on the element/backdrop.
    pub(crate) fn resolve_in_layer(style: &Style, parent: &Self, top_layer: bool) -> Self {
        Self::resolve_with_context(
            style,
            parent,
            top_layer,
            Some(parent.root_font_size),
            parent.viewport_size,
        )
    }

    pub(crate) fn resolve_with_context(
        style: &Style,
        parent: &Self,
        top_layer: bool,
        root_font: Option<f32>,
        viewport: [f32; 2],
    ) -> Self {
        let values = super::css::values::resolve(style, parent, root_font, viewport);
        Self::resolve_values(values, parent, top_layer, viewport)
    }

    pub(crate) fn resolve_values(
        values: super::css::values::ResolvedValues<'_>,
        parent: &Self,
        top_layer: bool,
        viewport: [f32; 2],
    ) -> Self {
        let style = &values.style;
        let text = style.resolve_text(&parent.text);
        let inherited_layout = if let Some(inset) = &parent.sticky_inset {
            let mut layout = parent.layout.clone();
            layout.inset = **inset;
            std::borrow::Cow::Owned(layout)
        } else {
            std::borrow::Cow::Borrowed(&parent.layout)
        };
        let mut layout = style.resolve_layout(&inherited_layout, text.direction);
        let scroll = style.scroll.resolve(&parent.scroll, &layout, text.color);
        layout.overflow = scroll.overflow.map(|v| v.layout(false));
        let mut layer = super::layer::LayerStyle::resolve(style, &parent.layer);
        if top_layer
            && !matches!(
                layer.position,
                super::layer::Position::Absolute | super::layer::Position::Fixed
            )
        {
            layer.position = super::layer::Position::Absolute;
        }
        layout.position = match layer.position {
            super::layer::Position::Absolute | super::layer::Position::Fixed => {
                crate::core::layout::Position::Absolute
            }
            _ => crate::core::layout::Position::Relative,
        };
        let math_lengths = super::math::retain_layout(&layout);
        let sticky_inset =
            (layer.position == super::layer::Position::Sticky).then(|| Box::new(layout.inset));
        if matches!(
            layer.position,
            super::layer::Position::Static | super::layer::Position::Sticky
        ) {
            layout.inset = crate::core::layout::Rect {
                left: crate::core::layout::LengthPercentageAuto::auto(),
                right: crate::core::layout::LengthPercentageAuto::auto(),
                top: crate::core::layout::LengthPercentageAuto::auto(),
                bottom: crate::core::layout::LengthPercentageAuto::auto(),
            };
        }
        Self {
            custom_properties: values.custom,
            root_font_size: values.root_font,
            viewport_size: viewport,
            transform: style
                .transform
                .resolve(&parent.transform, &Default::default(), false),
            transform_origin: style.transform_origin.resolve(
                &parent.transform_origin,
                &Default::default(),
                false,
            ),
            sticky_inset,
            scroll,
            media: style.media.resolve(&parent.media),
            text,
            layer,
            selection: super::selection::SelectionStyle::resolve(style, &parent.selection),
            paint: style.resolve_paint(&parent.paint),
            math_lengths,
            layout,
        }
    }
    pub(crate) fn same_layout(&self, other: &Self) -> bool {
        self.scroll.same_layout(&other.scroll)
            && self.transform.is_none() == other.transform.is_none()
            && self.layer.position == other.layer.position
            && self.layout == other.layout
            && self.text.font == other.text.font
            && self.text.font_size == other.text.font_size
            && self.text.line_height == other.text.line_height
            && self.text.wrap == other.text.wrap
            && self.text.direction == other.text.direction
    }
}
