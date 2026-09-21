//! CSS token parsing for image placement and SVG presentation properties.
use super::properties::{number, tokens, wide};
use crate::style::{media::*, value::CssValue};
use cssparser::{Parser, ParserInput, Token};

pub(crate) fn parse(name: &str, raw: &str) -> Result<MediaDeclaration, String> {
    let property = MediaProperty::from_name(name).ok_or("unknown image/SVG property")?;
    let raw = super::parser::clean_value(raw)?;
    let lower = raw.to_ascii_lowercase();
    let value = if let Some(v) = wide(&lower) {
        v
    } else {
        use MediaProperty::*;
        let value = match property {
            ObjectFit => MediaValue::Fit(match lower.as_str() {
                "fill" => crate::style::media::ObjectFit::Fill,
                "contain" => crate::style::media::ObjectFit::Contain,
                "cover" => crate::style::media::ObjectFit::Cover,
                "none" => crate::style::media::ObjectFit::None,
                "scale-down" => crate::style::media::ObjectFit::ScaleDown,
                _ => return Err("invalid object-fit".into()),
            }),
            ObjectPosition => MediaValue::Position(position(&lower)?),
            ImageRendering => MediaValue::Sampling(match lower.as_str() {
                "auto" | "smooth" | "high-quality" | "optimizequality" => {
                    crate::render::ImageSampling::Smooth
                }
                "crisp-edges" | "optimizespeed" => crate::render::ImageSampling::CrispEdges,
                "pixelated" => crate::render::ImageSampling::Pixelated,
                _ => return Err("invalid image-rendering".into()),
            }),
            _ => MediaValue::Svg(svg_value(property, &raw, &lower)?.into()),
        };
        CssValue::Value(value)
    };
    Ok(MediaDeclaration { property, value })
}
fn svg_value(p: MediaProperty, raw: &str, lower: &str) -> Result<String, String> {
    use MediaProperty::*;
    let keyword = |allowed: &[&str]| {
        if allowed.contains(&lower) {
            Ok(lower.to_owned())
        } else {
            Err(format!("invalid {}", p.name()))
        }
    };
    let length = |nonnegative: bool| -> Result<String, String> {
        let v: svgtypes::Length = raw.parse().map_err(|_| "invalid SVG length")?;
        if !v.number.is_finite() || (nonnegative && v.number < 0.) {
            return Err("invalid SVG length range".into());
        }
        Ok(raw.into())
    };
    match p {
        Fill | Stroke => {
            if let Ok(color) = raw.parse::<crate::style::color::Color>() {
                return Ok(color_css(color));
            }
            svgtypes::Paint::from_str(raw).map_err(|_| "invalid SVG paint")?;
            Ok(raw.into())
        }
        StopColor | FloodColor => Ok(color_css(raw.parse()?)),
        Opacity | FillOpacity | StrokeOpacity | StopOpacity | FloodOpacity => {
            let n = if let Some(s) = lower.strip_suffix('%') {
                number(s, false)? / 100.
            } else {
                number(lower, false)?
            };
            Ok(n.clamp(0., 1.).to_string())
        }
        StrokeWidth | R => length(true),
        StrokeDashoffset | Cx | Cy | X | Y => length(false),
        Rx | Ry if lower == "auto" => Ok(lower.into()),
        Rx | Ry => length(true),
        StrokeMiterlimit => {
            let v = number(raw, false)?;
            if v < 1. {
                return Err("stroke-miterlimit must be at least 1".into());
            }
            Ok(v.to_string())
        }
        StrokeLinecap => keyword(&["butt", "round", "square"]),
        StrokeLinejoin => keyword(&["miter", "round", "bevel"]),
        FillRule | ClipRule => keyword(&["nonzero", "evenodd"]),
        ShapeRendering => keyword(&["auto", "optimizespeed", "crispedges", "geometricprecision"])
            .map(|v| match v.as_str() {
                "optimizespeed" => "optimizeSpeed".into(),
                "crispedges" => "crispEdges".into(),
                "geometricprecision" => "geometricPrecision".into(),
                _ => v,
            }),
        StrokeDasharray => {
            if lower == "none" {
                return Ok(lower.into());
            }
            let values = svgtypes::LengthListParser::from(raw)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| "invalid stroke-dasharray")?;
            if values.is_empty()
                || values
                    .iter()
                    .any(|v| !v.number.is_finite() || v.number < 0.)
            {
                return Err("invalid stroke-dasharray".into());
            }
            Ok(raw.into())
        }
        ClipPath | Mask => {
            if lower == "none" {
                return Ok(lower.into());
            }
            let mut input = ParserInput::new(raw);
            let mut parser = Parser::new(&mut input);
            parser.expect_url().map_err(|_| "expected url() or none")?;
            parser
                .expect_exhausted()
                .map_err(|_| "unexpected URL tokens")?;
            Ok(raw.into())
        }
        Filter => {
            if lower == "none" {
                return Ok(lower.into());
            }
            let values = svgtypes::FilterValueListParser::from(raw)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| "invalid filter")?;
            if values.is_empty() {
                return Err("empty filter".into());
            }
            Ok(raw.into())
        }
        PaintOrder => {
            if lower == "normal" {
                return Ok(lower.into());
            }
            let parts = tokens(lower)?;
            if parts.is_empty()
                || parts.len() > 3
                || parts.iter().enumerate().any(|(i, v)| {
                    !["fill", "stroke", "markers"].contains(&v.as_str()) || parts[..i].contains(v)
                })
            {
                return Err("invalid paint-order".into());
            }
            Ok(lower.into())
        }
        D => {
            if lower == "none" {
                return Ok(lower.into());
            }
            let mut input = ParserInput::new(raw);
            let mut parser = Parser::new(&mut input);
            parser
                .expect_function_matching("path")
                .map_err(|_| "d requires path(\"...\") or none")?;
            let path = parser
                .parse_nested_block(|p| {
                    let s = p.expect_string_cloned()?;
                    p.expect_exhausted()?;
                    Ok::<_, cssparser::ParseError<'_, ()>>(s.to_string())
                })
                .map_err(|_| "invalid path()")?;
            parser
                .expect_exhausted()
                .map_err(|_| "unexpected path tokens")?;
            for v in svgtypes::PathParser::from(path.as_str()) {
                v.map_err(|_| "invalid path data")?;
            }
            Ok(raw.into())
        }
        _ => unreachable!(),
    }
}

pub(crate) fn color_css(color: crate::style::color::Color) -> String {
    use crate::style::color::{Color, map_to_srgb};
    if color == Color::CurrentColor {
        return "currentColor".into();
    }
    let [r, g, b, a] = map_to_srgb(color.components());
    format!(
        "rgba({},{},{},{})",
        (r.clamp(0., 1.) * 255.).round() as u8,
        (g.clamp(0., 1.) * 255.).round() as u8,
        (b.clamp(0., 1.) * 255.).round() as u8,
        a.clamp(0., 1.)
    )
}
fn offset(raw: &str) -> Result<PositionOffset, String> {
    let lower = raw.to_ascii_lowercase();
    let mut input = ParserInput::new(&lower);
    let mut p = Parser::new(&mut input);
    fn term(p: &mut Parser<'_, '_>) -> Result<PositionOffset, String> {
        match p.next().map_err(|_| "missing position coordinate")? {
            Token::Dimension { value, unit, .. }
                if unit.eq_ignore_ascii_case("px") && value.is_finite() =>
            {
                Ok(PositionOffset {
                    pixels: *value,
                    percent: 0.,
                })
            }
            Token::Percentage { unit_value, .. } if unit_value.is_finite() => Ok(PositionOffset {
                percent: *unit_value,
                pixels: 0.,
            }),
            Token::Number { value, .. } if *value == 0. => Ok(PositionOffset {
                percent: 0.,
                pixels: 0.,
            }),
            _ => Err("position lengths support px, %, and additive calc()".into()),
        }
    }
    let result = if p.try_parse(|p| p.expect_function_matching("calc")).is_ok() {
        p.parse_nested_block(|p| {
            let mut value = term(p).map_err(|e| p.new_custom_error::<_, String>(e))?;
            while !p.is_exhausted() {
                let sign = match p.next()? {
                    Token::Delim('+') => 1.,
                    Token::Delim('-') => -1.,
                    _ => return Err(p.new_custom_error("expected + or - in calc()")),
                };
                let rhs = term(p).map_err(|e| p.new_custom_error::<_, String>(e))?;
                value.percent += sign * rhs.percent;
                value.pixels += sign * rhs.pixels;
            }
            Ok(value)
        })
        .map_err(|_| "invalid position calc()")?
    } else {
        term(&mut p)?
    };
    p.expect_exhausted()
        .map_err(|_| "unexpected position tokens")?;
    if !result.percent.is_finite() || !result.pixels.is_finite() {
        return Err("non-finite position".into());
    }
    Ok(result)
}
pub(crate) fn position(raw: &str) -> Result<ObjectPosition, String> {
    let parts = tokens(raw)?;
    fn coord(s: &str, horizontal: bool) -> Result<PositionOffset, String> {
        let value = match s {
            "center" => 0.5,
            "left" if horizontal => 0.,
            "right" if horizontal => 1.,
            "top" if !horizontal => 0.,
            "bottom" if !horizontal => 1.,
            _ => return offset(s),
        };
        Ok(PositionOffset {
            percent: value,
            pixels: 0.,
        })
    }
    match parts.as_slice() {
        [a] if matches!(a.as_str(), "top" | "bottom") => Ok(ObjectPosition {
            y: coord(a, false)?,
            ..Default::default()
        }),
        [a] => Ok(ObjectPosition {
            x: coord(a, true)?,
            ..Default::default()
        }),
        [a, b]
            if matches!(a.as_str(), "top" | "bottom") || matches!(b.as_str(), "left" | "right") =>
        {
            Ok(ObjectPosition {
                x: coord(b, true)?,
                y: coord(a, false)?,
            })
        }
        [a, b] => Ok(ObjectPosition {
            x: coord(a, true)?,
            y: coord(b, false)?,
        }),
        v if (3..=4).contains(&v.len()) => {
            let mut x = None;
            let mut y = None;
            let mut i = 0;
            while i < v.len() {
                let edge = v[i].as_str();
                let horizontal = matches!(edge, "left" | "right");
                if !horizontal && !matches!(edge, "top" | "bottom" | "center") {
                    return Err("expected position edge".into());
                }
                let mut value = coord(edge, horizontal)?;
                i += 1;
                if edge != "center"
                    && i < v.len()
                    && !["left", "right", "top", "bottom", "center"].contains(&v[i].as_str())
                {
                    let delta = offset(&v[i])?;
                    i += 1;
                    let sign = if matches!(edge, "right" | "bottom") {
                        -1.
                    } else {
                        1.
                    };
                    value.percent += sign * delta.percent;
                    value.pixels += sign * delta.pixels;
                }
                let slot = if horizontal
                    || (edge == "center"
                        && x.is_none()
                        && v.get(i)
                            .is_none_or(|s| !["left", "right"].contains(&s.as_str())))
                {
                    &mut x
                } else {
                    &mut y
                };
                if slot.replace(value).is_some() {
                    return Err("duplicate position axis".into());
                }
            }
            Ok(ObjectPosition {
                x: x.ok_or("missing x axis")?,
                y: y.ok_or("missing y axis")?,
            })
        }
        _ => Err("object-position requires one to four coordinates".into()),
    }
}

/// SVG 2 geometry CSS is lowered to SVG attributes after the cascade because
/// usvg's parser accepts path data as an attribute, not the CSS path() function.
pub(crate) fn path_data(raw: &str) -> String {
    if raw == "none" {
        return String::new();
    }
    let mut input = ParserInput::new(raw);
    let mut p = Parser::new(&mut input);
    if p.expect_function_matching("path").is_err() {
        return String::new();
    }
    p.parse_nested_block(|p| {
        p.expect_string_cloned()
            .map(|v| v.to_string())
            .map_err(Into::<cssparser::ParseError<'_, ()>>::into)
    })
    .unwrap_or_default()
}
