#![doc = include_str!("../../docs/style.md")]

pub mod color;
pub mod style;
pub mod text;

#[doc(hidden)]
pub mod builder;
pub mod math;
pub mod value;
pub use text::{FontSize, LineHeight, TextAlignment};
pub use value::CssValue;
pub use value::{AUTO, pct};

pub mod properties;
pub use properties::LayoutProperty;

pub mod paint;

pub mod css;
pub mod declaration;

pub mod gradient;
pub mod list;
pub mod shadow;
pub mod transition;

pub mod computed;

pub mod layer;

pub mod selection;

pub mod media;

pub mod scroll;

pub mod transform;

pub mod tailwind;

// Import generated macro exports through their defining modules. Explicit paths
// keep the same names visible to rustc and rust-analyzer without macro_use hops.
#[doc(hidden)]
pub use builder::{
    __voidui_base_styles, __voidui_layout_styles, __voidui_media_styles, __voidui_svg_styles,
    __voidui_text_styles,
};
#[doc(hidden)]
pub use tailwind::{
    __voidui_dimension_aliases, __voidui_length_aliases, __voidui_margin_aliases,
    __voidui_paint_aliases, __voidui_tailwind_styles,
};
impl<W> crate::core::widget::WidgetBuilder<W> {
    crate::__voidui_style_methods!();
}
impl crate::core::component::ComponentElement {
    crate::__voidui_style_methods!();
}

/// Shared inherent methods for generated component builders. Named inputs take
/// precedence over style setters; `.build()` exposes the unambiguous root API.
#[doc(hidden)]
#[macro_export]
macro_rules! __voidui_style_methods {
    ($($excluded:ident),* $(,)?) => {
        $crate::style::__voidui_layout_styles!($($excluded),*);
        $crate::style::__voidui_text_styles!($($excluded),*);
        $crate::style::__voidui_base_styles!($($excluded),*);
        $crate::style::__voidui_media_styles!($($excluded),*);
        $crate::style::__voidui_svg_styles!($($excluded),*);
        $crate::style::__voidui_dimension_aliases!($($excluded),*);
        $crate::style::__voidui_margin_aliases!($($excluded),*);
        $crate::style::__voidui_length_aliases!($($excluded),*);
        $crate::style::__voidui_paint_aliases!($($excluded),*);
        $crate::style::__voidui_tailwind_styles!($($excluded),*);
    };
}
