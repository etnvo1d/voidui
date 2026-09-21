//! Strict parsing of the CSS properties supported by the current layout/paint engine.
use crate::{
    core::layout::*,
    style::{
        color::Color,
        declaration::Declaration,
        properties::{LayoutKeyword, LayoutProperty},
        text::{FontSize, LineHeight, TextAlignment},
        value::{CssValue, ValidateCssLength},
    },
};
use cssparser::{Parser, ParserInput, Token};
use std::str::FromStr;
use taffy::Dimension;
use voidui_gpui_wgpu::{FontFallbacks, FontStyle, FontWeight, SharedString};

type Result<T> = std::result::Result<T, String>;
fn native<T: FromStr>(raw: &str) -> Result<T> {
    raw.parse().map_err(|_| format!("invalid value: {raw}"))
}
pub(super) fn wide<T>(raw: &str) -> Option<CssValue<T>> {
    match raw {
        "inherit" => Some(CssValue::Inherit),
        "initial" => Some(CssValue::Initial),
        "unset" => Some(CssValue::Unset),
        _ => None,
    }
}
fn finite(value: f32, positive: bool) -> Result<f32> {
    if value.is_finite() && (!positive || value >= 0.0) {
        Ok(value)
    } else {
        Err("number is outside the supported range".into())
    }
}

trait CssParse: Sized {
    fn css(raw: &str) -> Result<Self>;
}
macro_rules! native_parse {($($ty:ty),*)=>{$(impl CssParse for $ty{fn css(raw:&str)->Result<Self>{native(raw)}})*};}
native_parse!(
    BoxSizing,
    FlexDirection,
    FlexWrap,
    GridAutoFlow,
    AlignItems,
    AlignContent
);
impl CssParse for f32 {
    fn css(raw: &str) -> Result<Self> {
        px(raw, true)
    }
}
pub(super) fn number(raw: &str, nonnegative: bool) -> Result<f32> {
    if super::math::is_math(raw) {
        let super::math::Value::Number(value) = super::math::parse(raw)? else {
            return Err("this property requires a number, not a length".into());
        };
        let value = finite(value as f32, false)?;
        return Ok(if nonnegative { value.max(0.0) } else { value });
    }
    let mut input = ParserInput::new(raw);
    let mut p = Parser::new(&mut input);
    let v = p
        .expect_number()
        .map_err(|_| format!("expected number: {raw}"))?;
    p.expect_exhausted()
        .map_err(|_| "unexpected trailing tokens")?;
    finite(v, nonnegative)
}
pub(super) fn unit(raw: &str) -> Result<(f32, String)> {
    if super::math::is_math(raw) {
        let super::math::Value::Length(value) = super::math::parse(raw)? else {
            return Err("this property requires a length".into());
        };
        if value.has_percentage() {
            return Err(
                "percentage math is currently supported in layout length properties only".into(),
            );
        }
        return Ok((finite(value.evaluate(0.0) as f32, false)?, "px".into()));
    }
    let mut input = ParserInput::new(raw);
    let mut p = Parser::new(&mut input);
    let result = match p.next().map_err(|_| "missing length")?.clone() {
        Token::Dimension { value, unit, .. } => (value, unit.to_ascii_lowercase()),
        Token::Percentage { unit_value, .. } => (unit_value, "%".into()),
        Token::Number { value, .. } if value == 0.0 => (value, "px".into()),
        _ => return Err(format!("expected px or percentage length: {raw}")),
    };
    p.expect_exhausted()
        .map_err(|_| "unexpected length tokens")?;
    finite(result.0, false)?;
    Ok(result)
}
pub(super) fn px(raw: &str, nonnegative: bool) -> Result<f32> {
    let (v, u) = unit(raw)?;
    if u != "px" {
        return Err("this property requires px".into());
    }
    if super::math::is_math(raw) && nonnegative {
        Ok(v.max(0.0))
    } else {
        finite(v, nonnegative)
    }
}
impl CssParse for LengthPercentage {
    fn css(raw: &str) -> Result<Self> {
        let (v, u) = unit(raw)?;
        match u.as_str() {
            "px" => Ok(Self::length(v)),
            "%" => Ok(Self::percent(v)),
            _ => Err("layout lengths currently support px and %".into()),
        }
    }
}
impl CssParse for LengthPercentageAuto {
    fn css(raw: &str) -> Result<Self> {
        if raw == "auto" {
            Ok(Self::auto())
        } else {
            Ok(LengthPercentage::css(raw)?.into())
        }
    }
}
impl CssParse for Dimension {
    fn css(raw: &str) -> Result<Self> {
        match raw {
            "auto" => Ok(Self::auto()),
            "min-content" | "max-content" | "fit-content" | "stretch" => native(raw),
            _ => Ok(LengthPercentage::css(raw)?.into()),
        }
    }
}
impl<T: FromStr> CssParse for Vec<T> {
    fn css(raw: &str) -> Result<Self> {
        if raw == "none" {
            return Ok(vec![]);
        }
        tokens(raw)?.iter().map(|v| native(v)).collect()
    }
}
impl CssParse for Line<GridPlacement> {
    fn css(raw: &str) -> Result<Self> {
        let mut parts = raw.split('/');
        let start = native(parts.next().unwrap().trim())?;
        let end = match parts.next() {
            Some(v) => native(v.trim())?,
            None => GridPlacement::Auto,
        };
        if parts.next().is_some() {
            return Err("too many grid placement parts".into());
        }
        Ok(Line { start, end })
    }
}

pub(super) fn tokens(raw: &str) -> Result<Vec<String>> {
    let mut input = ParserInput::new(raw);
    let mut p = Parser::new(&mut input);
    let mut result = Vec::new();
    while !p.is_exhausted() {
        p.skip_whitespace();
        let start = p.position();
        let nested = matches!(
            p.next().map_err(|_| "invalid token")?,
            Token::Function(_) | Token::ParenthesisBlock | Token::SquareBracketBlock
        );
        if nested {
            p.parse_nested_block(|n| {
                while n.next_including_whitespace_and_comments().is_ok() {}
                Ok::<_, cssparser::ParseError<'_, ()>>(())
            })
            .map_err(|_| "invalid nested value")?;
        }
        result.push(p.slice_from(start).trim().to_owned());
    }
    Ok(result)
}
fn four<T: Clone>(values: Vec<T>) -> Result<[T; 4]> {
    match values.as_slice() {
        [a] => Ok([a.clone(), a.clone(), a.clone(), a.clone()]),
        [a, b] => Ok([a.clone(), b.clone(), a.clone(), b.clone()]),
        [a, b, c] => Ok([a.clone(), b.clone(), c.clone(), b.clone()]),
        [a, b, c, d] => Ok([a.clone(), b.clone(), c.clone(), d.clone()]),
        _ => Err("expected one to four values".into()),
    }
}
pub(super) fn color(raw: &str) -> Result<Color> {
    // Variable substitution preserves token boundaries with comments. The color
    // library accepts comments inside functions, but not around the whole color.
    // Trim only surrounding CSS trivia; never join separate tokens into a color.
    let mut input = ParserInput::new(raw);
    let mut parser = Parser::new(&mut input);
    parser.skip_whitespace();
    let start = parser.position();
    let function = matches!(
        parser.next().map_err(|_| "expected a color")?,
        Token::Function(_)
    );
    if function {
        parser
            .parse_nested_block(|nested| {
                while nested.next_including_whitespace_and_comments().is_ok() {}
                Ok::<_, cssparser::ParseError<'_, ()>>(())
            })
            .map_err(|_| "invalid color function")?;
    }
    let end = parser.position();
    parser
        .expect_exhausted()
        .map_err(|_| "unexpected tokens after color")?;
    parser.slice(start..end).parse()
}
fn font_families(raw: &str) -> Result<Vec<String>> {
    let mut input = ParserInput::new(raw);
    let mut p = Parser::new(&mut input);
    p.parse_comma_separated(|p| {
        if let Ok(s) = p.try_parse(|p| p.expect_string_cloned()) {
            return Ok(s.to_string());
        }
        let mut words = Vec::new();
        while let Ok(s) = p.try_parse(|p| p.expect_ident_cloned()) {
            words.push(s.to_string());
        }
        if words.is_empty() {
            Err(p.new_custom_error::<_, String>("expected font family"))
        } else {
            Ok(words.join(" "))
        }
    })
    .map_err(|_| "invalid font-family list".into())
}

macro_rules! generated_layout_parser{
    (layout_setters{$($(#[$pd:meta])* $plain:ident=>$pn:ident($pt:ty)=>$($pf:ident).+;)*}
     optional_setters{$($(#[$od:meta])* $opt:ident=>$on:ident($ot:ty)=>$of:ident;)*}
     length_setters{$($(#[$ld:meta])* $len:ident=>$ln:ident($lt:ty),$signed:literal=>$($lf:ident).+;)*})=>{
        fn layout_property(name:&str,raw:&str)->Result<Option<Declaration>>{
            let key=name.replace('-',"_");
            // Self-alignment's auto value delegates to the container. Taffy
            // represents that absence of an override as None, like normal.
            let raw=if raw=="auto" && matches!(name,"align-self"|"justify-self"){"normal"}else{raw};
            let keyword=match raw{"inherit"=>Some(LayoutKeyword::Inherit),"initial"|"unset"=>Some(LayoutKeyword::Initial),_=>None};
            let declaration=match key.as_str(){
                $(stringify!($pn)=>{if let Some(k)=keyword{Declaration::LayoutKeyword(LayoutProperty::$plain,k)}else{Declaration::$plain(<$pt>::css(raw)?)}},)*
                $(stringify!($on)=>{if let Some(k)=keyword{Declaration::LayoutKeyword(LayoutProperty::$opt,k)}else{Declaration::$opt(if raw=="normal"{None}else{Some(<$ot>::css(raw)?)})}},)*
                $(stringify!($ln)=>{
                    if let Some(k)=keyword {
                        Declaration::LayoutKeyword(LayoutProperty::$len,k)
                    } else if super::math::is_math(raw) {
                        let super::math::Value::Length(expression) = super::math::parse(raw)? else {
                            return Err("layout math must produce a length".into());
                        };
                        if expression.has_percentage() {
                            Declaration::MathLength(LayoutProperty::$len, crate::style::math::MathLength::new(expression, !$signed))
                        } else {
                            // Preserve intrinsic sizing for constant expressions: Taffy
                            // treats every opaque calculation as percentage-dependent.
                            let n = finite(expression.evaluate(0.0) as f32, false)?;
                            Declaration::$len(<$lt>::length(if $signed { n } else { n.max(0.0) }))
                        }
                    } else {
                        let v=<$lt>::css(raw)?;
                        if let Some(n)=v.numeric(){finite(n,!$signed)?;}
                        Declaration::$len(v)
                    }
                },)*
                _=>return Ok(None),
            };Ok(Some(declaration))
        }
    };
}
crate::style::properties::layout_properties!(generated_layout_parser);

pub(crate) fn parse_property(name: &str, raw: &str) -> Result<Vec<Declaration>> {
    super::values::parse(name, raw)
}

pub(crate) fn parse_static_property(name: &str, raw: &str) -> Result<Vec<Declaration>> {
    if crate::style::media::MediaProperty::from_name(&name.to_ascii_lowercase()).is_some() {
        return Ok(vec![Declaration::Media(super::media::parse(
            &name.to_ascii_lowercase(),
            raw,
        )?)]);
    }

    use Declaration as D;
    let name = name.to_ascii_lowercase();
    let raw = super::parser::clean_value(raw)?;
    let lower = raw.to_ascii_lowercase();
    let value = lower.as_str();
    if let Some(p) = crate::style::scroll::ScrollProperty::from_name(&name) {
        return Ok(vec![super::scroll::parse(p, value)?]);
    }
    let declaration = match name.as_str() {
        "user-select" | "-webkit-user-select" => D::UserSelect(if let Some(v) = wide(value) {
            v
        } else {
            match value {
                "auto" => crate::style::selection::UserSelect::Auto,
                "text" => crate::style::selection::UserSelect::Text,
                "none" => crate::style::selection::UserSelect::None,
                "all" => crate::style::selection::UserSelect::All,
                "contain" => crate::style::selection::UserSelect::Contain,
                _ => return Err("invalid user-select".into()),
            }
            .into()
        }),
        "caret-color" => D::CaretColor(if let Some(v) = wide(value) {
            v
        } else if value == "auto" {
            Color::CurrentColor.into()
        } else {
            color(value)?.into()
        }),
        "caret-animation" => D::CaretAnimation(if let Some(v) = wide(value) {
            v
        } else {
            match value {
                "auto" => true,
                "manual" => false,
                _ => return Err("expected auto or manual".into()),
            }
            .into()
        }),
        "cursor" => D::Cursor(if let Some(v) = wide(value) {
            v
        } else {
            cursor(value)?.into()
        }),
        "transform" => D::Transform(if let Some(v) = wide(value) {
            v
        } else {
            super::transform::parse(value)?.into()
        }),
        "transform-origin" => D::TransformOrigin(if let Some(v) = wide(value) {
            v
        } else {
            super::transform::origin(value)?.into()
        }),
        "position" => D::Position(if let Some(v) = wide(value) {
            v
        } else {
            match value {
                "static" => crate::style::layer::Position::Static,
                "relative" => crate::style::layer::Position::Relative,
                "absolute" => crate::style::layer::Position::Absolute,
                "fixed" => crate::style::layer::Position::Fixed,
                "sticky" => crate::style::layer::Position::Sticky,
                _ => return Err("supported position: static, relative, absolute, fixed".into()),
            }
            .into()
        }),
        "z-index" => D::ZIndex(if let Some(v) = wide(value) {
            v
        } else {
            if value == "auto" {
                crate::style::layer::ZIndex::Auto.into()
            } else {
                let mut input = ParserInput::new(value);
                let mut p = Parser::new(&mut input);
                let n = p
                    .expect_integer()
                    .map_err(|_| "z-index needs an integer or auto")?;
                p.expect_exhausted()
                    .map_err(|_| "unexpected z-index tokens")?;
                crate::style::layer::ZIndex::Integer(n).into()
            }
        }),
        "isolation" => D::Isolation(if let Some(v) = wide(value) {
            v
        } else {
            match value {
                "auto" => crate::style::layer::Isolation::Auto,
                "isolate" => crate::style::layer::Isolation::Isolate,
                _ => return Err("invalid isolation".into()),
            }
            .into()
        }),
        "visibility" => D::Visibility(if let Some(v) = wide(value) {
            v
        } else {
            match value {
                "visible" => crate::style::layer::Visibility::Visible,
                "hidden" => crate::style::layer::Visibility::Hidden,
                _ => return Err("supported visibility: visible, hidden".into()),
            }
            .into()
        }),
        "pointer-events" => D::PointerEvents(if let Some(v) = wide(value) {
            v
        } else {
            match value {
                "auto" => crate::style::layer::PointerEvents::Auto,
                "none" => crate::style::layer::PointerEvents::None,
                _ => return Err("supported pointer-events: auto, none".into()),
            }
            .into()
        }),
        "color" => D::Color(if let Some(v) = wide(value) {
            v
        } else {
            color(&raw)?.into()
        }),
        "background-color" => D::Background(if let Some(v) = wide(value) {
            v
        } else {
            color(&raw)?.into()
        }),
        "background" => {
            if let Some(value) = wide::<Color>(value) {
                let image = match value {
                    CssValue::Inherit => CssValue::Inherit,
                    CssValue::Initial => CssValue::Initial,
                    _ => CssValue::Unset,
                };
                return Ok(vec![D::Background(value), D::BackgroundImage(image)]);
            }
            let mut background = Color::from(crate::style::color::Rgba8::new(0, 0, 0, 0));
            let mut images = Vec::new();
            let layers = super::effects::comma_list(&raw)?;
            for (index, layer) in layers.iter().enumerate() {
                let mut image = None;
                let mut seen_color = false;
                for token in tokens(layer)? {
                    if let Ok(c) = color(&token) {
                        if seen_color || index + 1 != layers.len() {
                            return Err("background color must occur once in the last layer".into());
                        }
                        background = c;
                        seen_color = true;
                    } else {
                        if image.is_some() {
                            return Err("unsupported background shorthand component".into());
                        }
                        image = Some(if token.eq_ignore_ascii_case("none") {
                            crate::style::gradient::BackgroundImage::None
                        } else {
                            crate::style::gradient::BackgroundImage::Gradient(
                                super::gradient::parse(&token)?,
                            )
                        });
                    }
                }
                images.push(image.unwrap_or(crate::style::gradient::BackgroundImage::None));
            }
            return Ok(vec![
                D::Background(background.into()),
                D::BackgroundImage(crate::style::gradient::BackgroundImages::from(images).into()),
            ]);
        }
        "background-image" => D::BackgroundImage(if let Some(v) = wide(value) {
            v
        } else {
            super::gradient::images(&raw)?.into()
        }),
        "box-shadow" => D::BoxShadow(if let Some(v) = wide(value) {
            v
        } else {
            super::effects::shadows(&raw)?.into()
        }),
        "transition" => {
            if wide::<()>(value).is_some() {
                let names = [
                    "transition-property",
                    "transition-duration",
                    "transition-delay",
                    "transition-timing-function",
                ];
                return names.iter().try_fold(Vec::new(), |mut out, name| {
                    out.extend(parse_property(name, value)?);
                    Ok(out)
                });
            }
            let t = super::effects::transitions(&raw)?;
            return Ok(vec![
                D::TransitionProperty(t.properties.into()),
                D::TransitionDuration(t.durations.into()),
                D::TransitionDelay(t.delays.into()),
                D::TransitionTimingFunction(t.easing.into()),
            ]);
        }
        "transition-property" => D::TransitionProperty(if let Some(v) = wide(value) {
            v
        } else {
            let list = super::effects::comma_list(value)?
                .iter()
                .map(|p| super::effects::transition_property(p))
                .collect::<Result<Vec<_>>>()?;
            if list.len() > 1 && list.iter().any(|p| p.0 == "none") {
                return Err("none must be the sole transition-property".into());
            }
            crate::style::list::StyleList::from(list).into()
        }),
        "transition-duration" | "transition-delay" => {
            let values = if let Some(v) = wide(value) {
                v
            } else {
                crate::style::list::StyleList::from(
                    super::effects::comma_list(value)?
                        .iter()
                        .map(|v| super::effects::time(v, name == "transition-duration"))
                        .collect::<Result<Vec<_>>>()?,
                )
                .into()
            };
            if name == "transition-duration" {
                D::TransitionDuration(values)
            } else {
                D::TransitionDelay(values)
            }
        }
        "transition-timing-function" => D::TransitionTimingFunction(if let Some(v) = wide(value) {
            v
        } else {
            crate::style::list::StyleList::from(
                super::effects::comma_list(value)?
                    .iter()
                    .map(|v| super::effects::easing(v))
                    .collect::<Result<Vec<_>>>()?,
            )
            .into()
        }),
        "border-color" => D::BorderColor(if let Some(v) = wide(value) {
            v
        } else {
            color(&raw)?.into()
        }),
        "border-radius" => D::BorderRadius(if let Some(v) = wide(value) {
            v
        } else {
            px(value, true)?.into()
        }),
        "font-family" => {
            if let Some(v) = wide::<SharedString>(value) {
                let fallback = match &v {
                    CssValue::Inherit => CssValue::Inherit,
                    CssValue::Initial => CssValue::Initial,
                    _ => CssValue::Unset,
                };
                return Ok(vec![D::FontFamily(v), D::FontFallbacks(fallback)]);
            }
            let names = font_families(&raw)?;
            return Ok(vec![
                D::FontFamily(SharedString::from(names[0].clone()).into()),
                D::FontFallbacks(
                    if names.len() > 1 {
                        Some(FontFallbacks::from_fonts(names[1..].to_vec()))
                    } else {
                        None
                    }
                    .into(),
                ),
            ]);
        }
        "font-size" => D::FontSize(if let Some(v) = wide(value) {
            v
        } else {
            let (n, u) = unit(value)?;
            if n <= 0.0 {
                return Err("font-size must be positive in the current text renderer".into());
            }
            match u.as_str() {
                "px" => FontSize::Pixels(n),
                "em" => FontSize::Em(n),
                "%" => FontSize::Percent(n),
                _ => return Err("font-size supports px, em, %".into()),
            }
            .into()
        }),
        "font-weight" => D::FontWeight(if let Some(v) = wide(value) {
            v
        } else {
            let n = match value {
                "normal" => 400.0,
                "bold" => 700.0,
                _ => number(value, true)?,
            };
            if !(1.0..=1000.0).contains(&n) {
                return Err("font-weight must be 1..1000".into());
            }
            FontWeight(n).into()
        }),
        "font-style" => D::FontStyle(if let Some(v) = wide(value) {
            v
        } else {
            match value {
                "normal" => FontStyle::Normal,
                "italic" => FontStyle::Italic,
                _ => return Err("supported font-style: normal, italic".into()),
            }
            .into()
        }),
        "line-height" => D::LineHeight(if let Some(v) = wide(value) {
            v
        } else {
            let h = if value == "normal" {
                LineHeight::Normal
            } else if let Ok(n) = number(value, true) {
                LineHeight::Relative(n)
            } else {
                let (n, u) = unit(value)?;
                match u.as_str() {
                    "px" => LineHeight::Pixels(n),
                    "em" => LineHeight::Em(n),
                    "%" => LineHeight::Percent(n),
                    _ => return Err("line-height supports number, px, em, %".into()),
                }
            };
            if h.resolve(1.0) <= 0.0 {
                return Err("line-height must be positive in the current text renderer".into());
            }
            h.into()
        }),
        "text-align" => D::TextAlign(if let Some(v) = wide(value) {
            v
        } else {
            match value {
                "start" => TextAlignment::Start,
                "end" => TextAlignment::End,
                "left" => TextAlignment::Left,
                "right" => TextAlignment::Right,
                "center" => TextAlignment::Center,
                _ => return Err("unsupported text-align".into()),
            }
            .into()
        }),
        "direction" => D::Direction(if let Some(v) = wide(value) {
            v
        } else {
            native::<Direction>(value)?.into()
        }),
        "text-wrap" | "text-wrap-mode" => D::TextWrap(if let Some(v) = wide(value) {
            v
        } else {
            match value {
                "wrap" => true,
                "nowrap" => false,
                _ => return Err("supported text-wrap: wrap, nowrap".into()),
            }
            .into()
        }),
        "display" => D::Display(match value {
            "block" | "initial" | "unset" => Display::Block,
            "flow-root" => Display::FlowRoot,
            "flex" => Display::Flex,
            "grid" => Display::Grid,
            "none" => Display::None,
            _ => return Err("supported display: block, flow-root, flex, grid, none".into()),
        }),
        "margin" | "padding" | "border-width" | "inset" => {
            let names = match name.as_str() {
                "margin" => ["margin-top", "margin-right", "margin-bottom", "margin-left"],
                "padding" => [
                    "padding-top",
                    "padding-right",
                    "padding-bottom",
                    "padding-left",
                ],
                "border-width" => [
                    "border-top-width",
                    "border-right-width",
                    "border-bottom-width",
                    "border-left-width",
                ],
                _ => ["top", "right", "bottom", "left"],
            };
            let values = if wide::<()>(value).is_some() {
                [raw.clone(), raw.clone(), raw.clone(), raw]
            } else {
                four(tokens(&raw)?)?
            };
            return names
                .into_iter()
                .zip(values)
                .try_fold(Vec::new(), |mut out, (name, v)| {
                    out.extend(parse_property(name, &v)?);
                    Ok(out)
                });
        }
        "gap" | "overflow" | "overscroll-behavior" => {
            let parts = tokens(&raw)?;
            if parts.is_empty() || parts.len() > 2 {
                return Err("expected one or two values".into());
            }
            if parts.len() > 1 && parts.iter().any(|p| wide::<()>(p).is_some()) {
                return Err("CSS-wide keywords must be the entire shorthand value".into());
            }
            let names = if name == "gap" {
                ["row-gap", "column-gap"]
            } else {
                if name == "overflow" {
                    ["overflow-x", "overflow-y"]
                } else {
                    ["overscroll-behavior-x", "overscroll-behavior-y"]
                }
            };
            let mut out = parse_property(names[0], &parts[0])?;
            out.extend(parse_property(names[1], parts.get(1).unwrap_or(&parts[0]))?);
            return Ok(out);
        }
        "border" => {
            if wide::<()>(value).is_some() {
                let mut out = parse_static_property("border-width", value)?;
                out.extend(parse_static_property("border-color", value)?);
                return Ok(out);
            }
            let mut width = "medium".to_string();
            let mut color = "currentcolor".to_string();
            let mut solid = false;
            for token in tokens(&raw)? {
                if token == "solid" {
                    solid = true;
                } else if token == "none" {
                    width = "0".into();
                    solid = true;
                } else if px(&token, true).is_ok() {
                    width = token;
                } else if self::color(&token).is_ok() {
                    color = token;
                } else {
                    return Err("supported border: <px> solid <color>, or none".into());
                }
            }
            if !solid {
                return Err("only solid/none borders are currently rendered".into());
            }
            if width == "medium" {
                width = "3px".into();
            }
            let mut out = parse_property("border-width", &width)?;
            out.extend(parse_property("border-color", &color)?);
            return Ok(out);
        }
        "flex-grow" | "flex-shrink" | "aspect-ratio" => {
            let p = match name.as_str() {
                "flex-grow" => LayoutProperty::FlexGrow,
                "flex-shrink" => LayoutProperty::FlexShrink,
                _ => LayoutProperty::AspectRatio,
            };
            if wide::<()>(value).is_some() {
                return Ok(vec![D::LayoutKeyword(
                    p,
                    if value == "inherit" {
                        LayoutKeyword::Inherit
                    } else {
                        LayoutKeyword::Initial
                    },
                )]);
            }
            match name.as_str() {
                "flex-grow" => D::FlexGrow(number(value, true)?),
                "flex-shrink" => D::FlexShrink(number(value, true)?),
                _ => {
                    let ratio = if value == "auto" {
                        None
                    } else {
                        let v = value
                            .split('/')
                            .map(|p| number(p.trim(), true))
                            .collect::<Result<Vec<_>>>()?;
                        if v.is_empty() || v.len() > 2 || v.iter().any(|n| *n <= 0.0) {
                            return Err("invalid aspect-ratio".into());
                        }
                        Some(v[0] / v.get(1).copied().unwrap_or(1.0))
                    };
                    D::AspectRatio(ratio)
                }
            }
        }
        "flex" => {
            let (grow, shrink, basis) = match value {
                "none" => (0.0, 0.0, "auto".into()),
                "auto" => (1.0, 1.0, "auto".into()),
                "initial" | "unset" => (0.0, 1.0, "auto".into()),
                "inherit" => {
                    return Ok(vec![
                        D::LayoutKeyword(LayoutProperty::FlexGrow, LayoutKeyword::Inherit),
                        D::LayoutKeyword(LayoutProperty::FlexShrink, LayoutKeyword::Inherit),
                        D::LayoutKeyword(LayoutProperty::FlexBasis, LayoutKeyword::Inherit),
                    ]);
                }
                _ => {
                    let p = tokens(value)?;
                    match p.as_slice() {
                        [g] if number(g, true).is_ok() => (number(g, true)?, 1.0, "0%".into()),
                        [basis] => (1.0, 1.0, basis.clone()),
                        [g, s] if number(s, true).is_ok() => {
                            (number(g, true)?, number(s, true)?, "0%".into())
                        }
                        [g, b] => (number(g, true)?, 1.0, b.clone()),
                        [g, s, b] => (number(g, true)?, number(s, true)?, b.clone()),
                        _ => return Err("invalid flex shorthand".into()),
                    }
                }
            };
            let mut out = vec![D::FlexGrow(grow), D::FlexShrink(shrink)];
            out.extend(parse_property("flex-basis", &basis)?);
            return Ok(out);
        }
        _ => match layout_property(
            &name,
            if name.starts_with("grid-") {
                &raw
            } else if (name == "max-width" || name == "max-height") && value == "none" {
                "auto"
            } else {
                value
            },
        )? {
            Some(v) => v,
            None => return Err(format!("unsupported CSS property: {name}")),
        },
    };
    Ok(vec![declaration])
}

fn cursor(value: &str) -> Result<crate::style::selection::Cursor> {
    use crate::style::selection::Cursor as C;
    use winit::window::CursorIcon as I;
    Ok(match value {
        "auto" => C::Auto,
        "none" => C::None,
        other => C::Icon(match other {
            "default" => I::Default,
            "context-menu" => I::ContextMenu,
            "help" => I::Help,
            "pointer" => I::Pointer,
            "progress" => I::Progress,
            "wait" => I::Wait,
            "cell" => I::Cell,
            "crosshair" => I::Crosshair,
            "text" => I::Text,
            "vertical-text" => I::VerticalText,
            "alias" => I::Alias,
            "copy" => I::Copy,
            "move" => I::Move,
            "no-drop" => I::NoDrop,
            "not-allowed" => I::NotAllowed,
            "grab" => I::Grab,
            "grabbing" => I::Grabbing,
            "all-scroll" => I::AllScroll,
            "col-resize" => I::ColResize,
            "row-resize" => I::RowResize,
            "n-resize" => I::NResize,
            "e-resize" => I::EResize,
            "s-resize" => I::SResize,
            "w-resize" => I::WResize,
            "ne-resize" => I::NeResize,
            "nw-resize" => I::NwResize,
            "se-resize" => I::SeResize,
            "sw-resize" => I::SwResize,
            "ew-resize" => I::EwResize,
            "ns-resize" => I::NsResize,
            "nesw-resize" => I::NeswResize,
            "nwse-resize" => I::NwseResize,
            "zoom-in" => I::ZoomIn,
            "zoom-out" => I::ZoomOut,
            _ => return Err("unsupported cursor keyword or image cursor".into()),
        }),
    })
}
