#![doc = include_str!("../../docs/text.md")]

use std::{borrow::Cow, sync::Arc};

use taffy::util::ResolveOrZero;
use voidui_gpui_wgpu::{Painter, Result, SharedString};

use crate::{
    core::{
        context::{DrawContext, LayoutContext},
        element::{Element, ElementProps, IntoElement},
        layout::{AvailableSpace, LayoutInput, LayoutOutput, RunMode, Size, layout_leaf},
        text::{PreparedText, Text as TextContent, TextLayoutOptions},
        widget::{Widget, WidgetBuilder},
    },
    style::style::Style,
};

/// A text leaf with intrinsic sizing, width-dependent wrapping, and inherited style.
pub struct Text {
    content: SharedString,
    prepared: Option<PreparedText>,
    rich: Option<Box<crate::RichText>>,
}

impl Text {
    pub fn new(content: impl Into<SharedString>) -> Self {
        Self {
            content: content.into(),
            prepared: None,
            rich: None,
        }
    }

    /// Create a selectable rich label using the same layout and event path as text.
    pub fn from_rich(content: impl Into<crate::RichText>) -> Self {
        let content = content.into();
        Self {
            content: content.shared_text(),
            prepared: None,
            rich: (!content.spans().is_empty()).then(|| Box::new(content)),
        }
    }
    pub fn set_rich_content(&mut self, content: impl Into<crate::RichText>) {
        let next = Self::from_rich(content);
        if self.content != next.content || self.rich != next.rich {
            *self = next;
        }
    }
    pub fn rich_content(&self) -> Option<&crate::RichText> {
        self.rich.as_deref()
    }

    pub fn content(&self) -> &SharedString {
        &self.content
    }

    /// Change the label. Run layout again before drawing the widget.
    pub fn set_content(&mut self, content: impl Into<SharedString>) {
        let content = content.into();
        if self.content != content || self.rich.is_some() {
            self.content = content;
            self.rich = None;
            self.prepared = None;
        }
    }
}

/// Create a styled text leaf. Bare string children use exactly this constructor.
pub fn text(content: impl Into<SharedString>) -> WidgetBuilder<Text> {
    WidgetBuilder {
        events: Default::default(),
        widget: Text::new(content),
        props: ElementProps::new(Style::default()),
        children: Vec::new(),
    }
}

/// Build one inline formatting context, including nested style inheritance.
/// Use `span("Hello ").child(span("world").bold())` to compose runs.
pub fn rich_text(content: impl Into<crate::RichText>) -> WidgetBuilder<Text> {
    WidgetBuilder {
        events: Default::default(),
        widget: Text::from_rich(content),
        props: ElementProps::new(Style::default()),
        children: Vec::new(),
    }
}

impl Widget for Text {
    fn reconcile(&mut self, next: &dyn Widget) -> crate::core::widget::WidgetUpdate {
        use crate::core::widget::WidgetUpdate;
        let Some(next) = (next as &dyn std::any::Any).downcast_ref::<Self>() else {
            return WidgetUpdate::Replace;
        };
        if self.content == next.content && self.rich == next.rich {
            return WidgetUpdate::Unchanged;
        }
        self.content = next.content.clone();
        self.rich = next.rich.clone();
        self.prepared = None;
        WidgetUpdate::Changed
    }

    fn tag_name(&self) -> &'static str {
        "text"
    }
    fn text_content(&self) -> Option<&str> {
        Some(self.content.as_str())
    }
    fn set_text_content(&mut self, text: SharedString) -> bool {
        self.set_content(text);
        true
    }
    fn prepared_text(&self) -> Option<&PreparedText> {
        self.prepared.as_ref()
    }
    fn layout(&mut self, inputs: LayoutInput, ctx: LayoutContext<'_, '_>) -> LayoutOutput {
        let typography = ctx.text_style();
        let style = ctx.layout_style();
        let mut options = TextLayoutOptions {
            font: typography.font.clone(),
            color: typography.color.into(),
            font_size: typography.font_size,
            line_height: typography.line_height.resolve(typography.font_size),
            wrap_width: None,
            line_clamp: None,
        };
        // Intrinsic probes temporarily rebreak the same paragraph. Restore its
        // final width after measuring so a later Taffy probe cannot corrupt paint.
        let restore = self.prepared.as_ref().map(|p| p.wrap_width());
        let mut measured = None;
        let mut output = layout_leaf(style, inputs, |_, available| {
            options.wrap_width = match (typography.wrap, available.width) {
                (true, AvailableSpace::Definite(w)) => Some(w.max(0.0)),
                _ => None,
            };
            if self
                .prepared
                .as_ref()
                .is_none_or(|p| !p.matches(ctx.text_layout(), &options))
            {
                self.prepared = Some(
                    if let Some(rich) = &self.rich {
                        PreparedText::shape_rich(ctx.text_layout(), rich, &options)
                    } else {
                        TextContent::new(self.content.clone()).shape(ctx.text_layout(), &options)
                    }
                    .expect("failed to shape text during widget layout"),
                );
            }
            let prepared = self.prepared.as_mut().unwrap();
            let intrinsic = inputs.run_mode != RunMode::PerformLayout
                && matches!(
                    available.width,
                    AvailableSpace::MinContent | AvailableSpace::MaxContent
                );
            let measurement = if intrinsic {
                prepared.intrinsic(
                    typography.wrap && matches!(available.width, AvailableSpace::MinContent),
                )
            } else {
                if typography.wrap && matches!(available.width, AvailableSpace::MinContent) {
                    options.wrap_width = Some(prepared.min_content_width());
                }
                prepared.reflow(options.wrap_width);
                prepared.measurement()
            };
            measured = Some(measurement);
            let size = measurement.size;
            Size {
                width: size.width,
                height: size.height,
            }
        });
        if let Some(measurement) = measured {
            let prepared = self.prepared.as_mut().unwrap();
            let inset_top = style
                .padding
                .top
                .resolve_or_zero(inputs.parent_size.width, crate::core::layout::resolve_calc)
                + style
                    .border
                    .top
                    .resolve_or_zero(inputs.parent_size.width, crate::core::layout::resolve_calc);
            output.baselines.first = measurement.first.map(|v| v + inset_top);
            output.baselines.last = measurement.last.map(|v| v + inset_top);
            if inputs.run_mode != RunMode::PerformLayout
                && let Some(width) = restore
            {
                prepared.reflow(width);
            }
        }
        if let Some(prepared) = &mut self.prepared {
            prepared.share(ctx.text_layout());
        }
        output
    }

    fn draw(&self, painter: &mut Painter<'_>, ctx: DrawContext) -> Result<()> {
        let prepared = self.prepared.as_ref().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Text widget must be laid out before drawing",
            )
        })?;
        if let Some(selection) = &ctx.selection {
            prepared.paint_selection(
                painter,
                ctx.content_bounds,
                ctx.text_align,
                ctx.color.into(),
                selection,
            )
        } else {
            prepared.paint_with_color(
                painter,
                ctx.content_bounds,
                ctx.text_align,
                ctx.color.into(),
            )
        }
    }
}

// Implement concrete string conversions rather than a blanket AsRef<str> impl:
// user-defined element types remain free to implement IntoElement themselves.
impl IntoElement for &str {
    fn into_element(self) -> Element {
        text(self).into_element()
    }
}

impl IntoElement for String {
    fn into_element(self) -> Element {
        text(self).into_element()
    }
}

impl IntoElement for &String {
    fn into_element(self) -> Element {
        self.as_str().into_element()
    }
}

impl IntoElement for SharedString {
    fn into_element(self) -> Element {
        text(self).into_element()
    }
}

impl IntoElement for &SharedString {
    fn into_element(self) -> Element {
        text(self).into_element()
    }
}

impl IntoElement for Arc<str> {
    fn into_element(self) -> Element {
        text(self).into_element()
    }
}

impl IntoElement for Cow<'_, str> {
    fn into_element(self) -> Element {
        text(self).into_element()
    }
}
