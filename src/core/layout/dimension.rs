//! Public dimensions use CSS percentage numbers; Taffy stores fractions internally.

/// A dimension accepted by the fluent size setters.
///
/// `Dimension::percent(50.0)` means 50%. Convert with `.into()` when assigning
/// directly to a raw Taffy `LayoutStyle` field. Values above 100% remain valid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dimension(taffy::Dimension);

impl Dimension {
    pub const fn length(value: f32) -> Self {
        Self(taffy::Dimension::length(value))
    }
    pub const fn percent(value: f32) -> Self {
        Self(taffy::Dimension::percent(value / 100.0))
    }
    pub const fn auto() -> Self {
        Self(taffy::Dimension::auto())
    }
    pub const fn min_content() -> Self {
        Self(taffy::Dimension::min_content())
    }
    pub const fn max_content() -> Self {
        Self(taffy::Dimension::max_content())
    }
    pub const fn fit_content() -> Self {
        Self(taffy::Dimension::fit_content())
    }
    pub const fn fit_content_px(value: f32) -> Self {
        Self(taffy::Dimension::fit_content_px(value))
    }
    pub const fn fit_content_percent(value: f32) -> Self {
        Self(taffy::Dimension::fit_content_percent(value / 100.0))
    }
    pub const fn stretch() -> Self {
        Self(taffy::Dimension::stretch())
    }
    pub const fn content() -> Self {
        Self(taffy::Dimension::content())
    }
    /// Access the layout engine's representation, whose percentages are fractions.
    pub const fn into_taffy(self) -> taffy::Dimension {
        self.0
    }
}
impl From<Dimension> for taffy::Dimension {
    fn from(value: Dimension) -> Self {
        value.0
    }
}
impl From<taffy::Dimension> for Dimension {
    fn from(value: taffy::Dimension) -> Self {
        Self(value)
    }
}
impl crate::style::value::IntoCssLength<taffy::Dimension> for Dimension {
    fn into_css_length(self) -> taffy::Dimension {
        self.0
    }
}
