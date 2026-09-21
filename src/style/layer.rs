//! CSS positioning, stacking and hit-testing values. Top-layer membership belongs
//! to the Rust document runtime; it is not an author-settable CSS property.
use super::value::CssValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}
impl From<crate::core::layout::Position> for Position {
    fn from(value: crate::core::layout::Position) -> Self {
        match value {
            crate::core::layout::Position::Relative => Self::Relative,
            crate::core::layout::Position::Absolute => Self::Absolute,
        }
    }
}
impl From<crate::core::layout::Position> for CssValue<Position> {
    fn from(value: crate::core::layout::Position) -> Self {
        Self::Value(value.into())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZIndex {
    #[default]
    Auto,
    Integer(i32),
}
impl From<i32> for CssValue<ZIndex> {
    fn from(value: i32) -> Self {
        Self::Value(ZIndex::Integer(value))
    }
}
impl From<super::value::Auto> for CssValue<ZIndex> {
    fn from(_: super::value::Auto) -> Self {
        Self::Value(ZIndex::Auto)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Isolation {
    #[default]
    Auto,
    Isolate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    #[default]
    Visible,
    Hidden,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PointerEvents {
    #[default]
    Auto,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayerStyle {
    pub position: Position,
    pub z_index: ZIndex,
    pub isolation: Isolation,
    pub visibility: Visibility,
    pub pointer_events: PointerEvents,
}
impl LayerStyle {
    pub(crate) fn resolve(style: &super::style::Style, parent: &Self) -> Self {
        let initial = Self::default();
        let position = if matches!(style.position, CssValue::Unset)
            && !style.is_marked(super::declaration::Property::Position)
            && style.layout.position == crate::core::layout::Position::Absolute
        {
            Position::Absolute // Preserve low-level Taffy presets.
        } else {
            style
                .position
                .resolve(&parent.position, &initial.position, false)
        };
        Self {
            position,
            z_index: style
                .z_index
                .resolve(&parent.z_index, &initial.z_index, false),
            isolation: style
                .isolation
                .resolve(&parent.isolation, &initial.isolation, false),
            visibility: style
                .visibility
                .resolve(&parent.visibility, &initial.visibility, true),
            pointer_events: style.pointer_events.resolve(
                &parent.pointer_events,
                &initial.pointer_events,
                true,
            ),
        }
    }
}
