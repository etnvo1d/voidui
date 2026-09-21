//! CSS selection constraints and highlight colors. `auto` is not ordinary
//! inheritance: it propagates a parent's used none/all, but never contain.
use super::{
    color::{Color, Rgba8},
    value::CssValue,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UserSelect {
    #[default]
    Auto,
    Text,
    None,
    All,
    Contain,
}
impl UserSelect {
    pub(crate) fn used(self, parent: Self) -> Self {
        match self {
            Self::Auto => match parent {
                Self::None | Self::All => parent,
                _ => Self::Text,
            },
            value => value,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cursor {
    #[default]
    Auto,
    None,
    Icon(winit::window::CursorIcon),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionStyle {
    /// CSS computed keyword, retained separately from its parent-dependent used value.
    pub user_select: UserSelect,
    pub used_user_select: UserSelect,
    pub cursor: Cursor,
}
impl Default for SelectionStyle {
    fn default() -> Self {
        Self {
            user_select: UserSelect::Auto,
            used_user_select: UserSelect::Text,
            cursor: Cursor::Auto,
        }
    }
}
impl SelectionStyle {
    pub(crate) fn resolve(style: &super::style::Style, parent: &Self) -> Self {
        let value = style
            .user_select
            .resolve(&parent.user_select, &UserSelect::Auto, false);
        Self {
            user_select: value,
            used_user_select: value.used(parent.used_user_select),
            cursor: style.cursor.resolve(&parent.cursor, &Cursor::Auto, true),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct HighlightDeclarations {
    pub color: Option<CssValue<Color>>,
    pub background: Option<CssValue<Color>>,
}
/// Inherited highlight values, independent of ordinary element inheritance.
/// `None` retains absence of author values for the CSS paired-default rule.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HighlightStyle {
    pub color: Option<Color>,
    pub background: Option<Color>,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionColors {
    pub color: Color,
    pub background: Color,
}
impl Default for SelectionColors {
    fn default() -> Self {
        Self {
            color: Rgba8::from_rgb8(255, 255, 255).into(),
            background: Rgba8::from_rgb8(38, 100, 210).into(),
        }
    }
}
impl HighlightStyle {
    pub(crate) fn resolve(declarations: HighlightDeclarations, parent: Self) -> Self {
        let resolve = |value: Option<CssValue<Color>>,
                       parent: Option<Color>,
                       initial: Color,
                       inherited: Color| match value {
            Some(CssValue::Value(v)) => Some(v),
            Some(CssValue::Initial) => Some(initial),
            Some(CssValue::Inherit | CssValue::Unset) => Some(parent.unwrap_or(inherited)),
            None => parent,
        };
        let transparent = Rgba8::new(0, 0, 0, 0).into();
        Self {
            color: resolve(
                declarations.color,
                parent.color,
                Rgba8::from_rgb8(0, 0, 0).into(),
                Color::CurrentColor,
            ),
            background: resolve(
                declarations.background,
                parent.background,
                transparent,
                transparent,
            ),
        }
    }
    /// Any author color disables both UA defaults. Unspecified foreground then
    /// uses the element's currentColor; unspecified background becomes transparent.
    pub fn colors(self, current: Color, defaults: SelectionColors) -> SelectionColors {
        if self.color.is_none() && self.background.is_none() {
            return defaults;
        }
        SelectionColors {
            color: self.color.unwrap_or(Color::CurrentColor).resolve(current),
            background: self
                .background
                .unwrap_or(Rgba8::new(0, 0, 0, 0).into())
                .resolve(current),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct HighlightState {
    pub specified: Option<Box<super::style::Style>>,
    pub declarations: HighlightDeclarations,
    pub computed: HighlightStyle,
}
