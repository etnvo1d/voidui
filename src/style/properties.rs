//! One property table drives setters and explicit layout defaulting. Adding a
//! supported longhand here keeps its setter and inheritance mapping in sync.
use crate::core::layout::LayoutStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKeyword {
    Inherit,
    Initial,
}

macro_rules! layout_properties {
    ($callback:ident) => { $callback! {
        layout_setters {
            /// Choose whether width/height include padding and border.
            BoxSizing => box_sizing(BoxSizing) => box_sizing;
            /// Set only flex-direction; use flex_row/flex_col to also enable Flexbox.
            FlexDirection => flex_direction(FlexDirection) => flex_direction;
            /// Select wrapping and its cross-axis order.
            FlexWrap => flex_wrap(FlexWrap) => flex_wrap;
            /// Set explicit grid column tracks, including fractions and repeat().
            GridTemplateColumns => grid_template_columns(Vec<GridTemplateComponent<String>>) => grid_template_columns;
            /// Set explicit grid row tracks.
            GridTemplateRows => grid_template_rows(Vec<GridTemplateComponent<String>>) => grid_template_rows;
            /// Set implicit grid column tracks.
            GridAutoColumns => grid_auto_columns(Vec<TrackSizingFunction>) => grid_auto_columns;
            /// Set implicit grid row tracks.
            GridAutoRows => grid_auto_rows(Vec<TrackSizingFunction>) => grid_auto_rows;
            /// Choose row/column auto placement and dense packing.
            GridAutoFlow => grid_auto_flow(GridAutoFlow) => grid_auto_flow;
            /// Place a grid item using start/end lines or spans.
            GridColumn => grid_column(Line<GridPlacement>) => grid_column;
            /// Place a grid item using row start/end lines or spans.
            GridRow => grid_row(Line<GridPlacement>) => grid_row;
        }
        optional_setters {
            /// Align children on the cross/block axis.
            AlignItems => align_items(AlignItems) => align_items;
            /// Override this item's cross/block alignment.
            AlignSelf => align_self(AlignSelf) => align_self;
            /// Distribute lines or grid tracks on the cross/block axis.
            AlignContent => align_content(AlignContent) => align_content;
            /// Distribute items/tracks on the main/inline axis.
            JustifyContent => justify_content(JustifyContent) => justify_content;
            /// Align grid items in their grid areas.
            JustifyItems => justify_items(JustifyItems) => justify_items;
            /// Override this grid item's inline alignment.
            JustifySelf => justify_self(JustifySelf) => justify_self;
        }
        length_setters {
            /// Set CSS width in pixels, percent, auto, or intrinsic units.
            Width => width(taffy::Dimension), false => size.width;
            /// Set CSS height; percentages need a definite containing block height.
            Height => height(taffy::Dimension), false => size.height;
            /// Set minimum width; auto preserves CSS automatic minimum sizing.
            MinWidth => min_width(LengthPercentageAuto), false => min_size.width;
            /// Set minimum height.
            MinHeight => min_height(LengthPercentageAuto), false => min_size.height;
            /// Set maximum width.
            MaxWidth => max_width(LengthPercentageAuto), false => max_size.width;
            /// Set maximum height.
            MaxHeight => max_height(LengthPercentageAuto), false => max_size.height;
            /// Set the initial main-axis size of a flex item.
            FlexBasis => flex_basis(taffy::Dimension), false => flex_basis;
            /// Set vertical spacing between flex lines or grid rows.
            RowGap => row_gap(LengthPercentage), false => gap.height;
            /// Set horizontal spacing between items or grid columns.
            ColumnGap => column_gap(LengthPercentage), false => gap.width;
            /// Set only the top margin; negative values and auto are allowed.
            MarginTop => margin_top(LengthPercentageAuto), true => margin.top;
            /// Set only the right margin.
            MarginRight => margin_right(LengthPercentageAuto), true => margin.right;
            /// Set only the bottom margin.
            MarginBottom => margin_bottom(LengthPercentageAuto), true => margin.bottom;
            /// Set only the left margin.
            MarginLeft => margin_left(LengthPercentageAuto), true => margin.left;
            /// Set only the top padding.
            PaddingTop => padding_top(LengthPercentage), false => padding.top;
            /// Set only the right padding.
            PaddingRight => padding_right(LengthPercentage), false => padding.right;
            /// Set only the bottom padding.
            PaddingBottom => padding_bottom(LengthPercentage), false => padding.bottom;
            /// Set only the left padding.
            PaddingLeft => padding_left(LengthPercentage), false => padding.left;
            /// Set the top border width.
            BorderTopWidth => border_top_width(LengthPercentage), false => border.top;
            /// Set the right border width.
            BorderRightWidth => border_right_width(LengthPercentage), false => border.right;
            /// Set the bottom border width.
            BorderBottomWidth => border_bottom_width(LengthPercentage), false => border.bottom;
            /// Set the left border width.
            BorderLeftWidth => border_left_width(LengthPercentage), false => border.left;
            /// Set the top positioning offset.
            Top => top(LengthPercentageAuto), true => inset.top;
            /// Set the right positioning offset.
            Right => right(LengthPercentageAuto), true => inset.right;
            /// Set the bottom positioning offset.
            Bottom => bottom(LengthPercentageAuto), true => inset.bottom;
            /// Set the left positioning offset.
            Left => left(LengthPercentageAuto), true => inset.left;
        }
    } };
}
pub(crate) use layout_properties;

macro_rules! define_layout_properties {
    (layout_setters { $( $(#[$pdoc:meta])* $plain:ident => $pname:ident($pty:ty) => $($pf:ident).+; )* }
     optional_setters { $( $(#[$odoc:meta])* $optional:ident => $oname:ident($oty:ty) => $of:ident; )* }
     length_setters { $( $(#[$ldoc:meta])* $length:ident => $lname:ident($lty:ty), $signed:literal => $($lf:ident).+; )* }) => {
        /// Non-inherited layout longhands available to inherit/initial/unset.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u8)]
        pub enum LayoutProperty { $($plain,)* $($optional,)* $($length,)*
            FlexGrow, FlexShrink, AspectRatio, OverflowX, OverflowY,
        }
        impl LayoutProperty {
            pub const ALL: &'static [Self] = &[$(Self::$plain,)* $(Self::$optional,)* $(Self::$length,)*
                Self::FlexGrow, Self::FlexShrink, Self::AspectRatio, Self::OverflowX, Self::OverflowY];
            pub(crate) fn copy(self, source: &LayoutStyle, target: &mut LayoutStyle) {
                match self {
                    $(Self::$plain => target.$($pf).+.clone_from(&source.$($pf).+),)*
                    $(Self::$optional => target.$of = source.$of,)*
                    $(Self::$length => target.$($lf).+ = source.$($lf).+,)*
                    Self::FlexGrow => target.flex_grow = source.flex_grow,
                    Self::FlexShrink => target.flex_shrink = source.flex_shrink,
                    Self::AspectRatio => target.aspect_ratio = source.aspect_ratio,
                    Self::OverflowX => target.overflow.x = source.overflow.x,
                    Self::OverflowY => target.overflow.y = source.overflow.y,
                }
            }
        }
    };
}
layout_properties!(define_layout_properties);
