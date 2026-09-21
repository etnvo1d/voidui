//! Value-taking conveniences use the existing typed setters and validation.
use crate::style::builder::StyleBuilder;
use crate::{
    core::layout::{LengthPercentage, LengthPercentageAuto},
    style::{
        color::Color,
        value::{CssValue, IntoCssLength},
    },
};

macro_rules! lengths {
    ($group:ident; $ty:ty; $( $name:ident => $setter:ident; )*) => {
        #[voidui_macros::style_methods($group)]
        impl<'a> StyleBuilder<'a> { $(
            #[doc = concat!(
                "Alias for [`", stringify!($setter), "`](Self::", stringify!($setter), ").\n\n",
                "`", stringify!($name), "(16)` sets a length of **16 logical pixels**. ",
                "Numeric values are independent of the root font size and `--spacing`. ",
                "Display scaling determines the corresponding physical pixels.\n\n",
                "Also accepts typed CSS lengths, such as `pct(50.0)` for `50%`; ",
                "the property's layout rules determine the percentage reference size."
            )]
            pub fn $name(self, value: impl IntoCssLength<$ty>) -> Self { self.$setter(value) }
        )* }
    };
}
lengths! { __voidui_dimension_aliases; taffy::Dimension; w => width; h => height; basis => flex_basis; }
lengths! { __voidui_margin_aliases; LengthPercentageAuto;
    min_w => min_width; min_h => min_height; max_w => max_width; max_h => max_height;
    m => margin; mx => margin_x; my => margin_y; mt => margin_top; mr => margin_right; mb => margin_bottom; ml => margin_left;
}
lengths! { __voidui_length_aliases; LengthPercentage;
    p => padding; px => padding_x; py => padding_y; pt => padding_top; pr => padding_right; pb => padding_bottom; pl => padding_left;
    gap_x => column_gap; gap_y => row_gap;
    border => border_width; border_t => border_top_width; border_r => border_right_width;
    border_b => border_bottom_width; border_l => border_left_width;
}
#[voidui_macros::style_methods(__voidui_paint_aliases)]
impl<'a> StyleBuilder<'a> {
    /// Set the left and right border widths to the same length.
    ///
    /// `border_x(2)` sets `border-left-width: 2px; border-right-width: 2px`.
    /// These are logical pixels, independent of root font size and `--spacing`.
    /// Also accepts typed CSS lengths. The top and bottom widths are preserved.
    pub fn border_x(self, value: impl IntoCssLength<LengthPercentage>) -> Self {
        let value = value.into_css_length();
        self.border_left_width(value).border_right_width(value)
    }
    /// Set the top and bottom border widths to the same length.
    ///
    /// `border_y(2)` sets `border-top-width: 2px; border-bottom-width: 2px`.
    /// These are logical pixels, independent of root font size and `--spacing`.
    /// Also accepts typed CSS lengths. The left and right widths are preserved.
    pub fn border_y(self, value: impl IntoCssLength<LengthPercentage>) -> Self {
        let value = value.into_css_length();
        self.border_top_width(value).border_bottom_width(value)
    }
    /// Set `background-color` to the supplied color without resetting background images.
    pub fn bg(self, color: impl Into<CssValue<Color>>) -> Self {
        self.background(color)
    }
    /// Set every corner's radius in logical pixels.
    ///
    /// `rounded(8.0)` sets `border-radius: 8px`, independently of root font size
    /// and theme radius variables. Also accepts explicit CSS defaulting values.
    pub fn rounded(self, radius: impl Into<CssValue<f32>>) -> Self {
        self.border_radius(radius)
    }
}

#[doc(hidden)]
pub use {
    __voidui_dimension_aliases, __voidui_length_aliases, __voidui_margin_aliases,
    __voidui_paint_aliases,
};
