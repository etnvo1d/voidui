//! Strict token parsing: unsupported 3D functions invalidate the whole declaration.
use super::properties::{number, tokens, unit};
use crate::{
    render::Affine,
    style::{math::MathLength, transform::*},
};
use cssparser::{Parser, ParserInput, Token};

pub(crate) fn length(raw: &str) -> Result<TransformLength, String> {
    if super::math::is_math(raw) {
        let super::math::Value::Length(expression) = super::math::parse(raw)? else {
            return Err("expected a length-percentage".into());
        };
        return Ok(TransformLength::Math(MathLength::new(expression, false)));
    }
    let (n, u) = unit(raw)?;
    match u.as_str() {
        "px" => Ok(TransformLength::Pixels(n)),
        "%" => Ok(TransformLength::Percent(n)),
        _ => Err("transform lengths support px and %".into()),
    }
}
fn angle(raw: &str) -> Result<f32, String> {
    let mut input = ParserInput::new(raw);
    let mut p = Parser::new(&mut input);
    let v = match p.next().map_err(|_| "missing angle")? {
        Token::Number { value, .. } if *value == 0. => 0.,
        Token::Dimension { value, unit, .. } => {
            *value
                * match unit.to_ascii_lowercase().as_str() {
                    "deg" => std::f32::consts::PI / 180.,
                    "rad" => 1.,
                    "grad" => std::f32::consts::PI / 200.,
                    "turn" => std::f32::consts::TAU,
                    _ => return Err("invalid angle unit".into()),
                }
        }
        _ => return Err("expected an angle".into()),
    };
    p.expect_exhausted()
        .map_err(|_| "unexpected angle tokens")?;
    if !v.is_finite() {
        return Err("angle must be finite".into());
    }
    Ok(v)
}
pub(crate) fn parse(raw: &str) -> Result<Transform, String> {
    let raw = super::parser::clean_value(raw)?;
    if raw.eq_ignore_ascii_case("none") {
        return Ok(Transform::default());
    }
    let mut input = ParserInput::new(&raw);
    let mut p = Parser::new(&mut input);
    let mut list = Vec::new();
    while !p.is_exhausted() {
        let name = p
            .expect_function()
            .map_err(|_| "expected a transform function")?
            .to_ascii_lowercase();
        let args = p
            .parse_nested_block(|p| {
                p.parse_comma_separated(|p| {
                    let start = p.position();
                    while !p.is_exhausted() {
                        if matches!(p.next()?, Token::Function(_) | Token::ParenthesisBlock) {
                            p.parse_nested_block(|n| {
                                while n.next_including_whitespace_and_comments().is_ok() {}
                                Ok::<_, cssparser::ParseError<'_, ()>>(())
                            })?;
                        }
                    }
                    Ok::<_, cssparser::ParseError<'_, ()>>(p.slice_from(start).trim().to_owned())
                })
            })
            .map_err(|_| "invalid transform arguments")?;
        let a = |i: usize| {
            args.get(i)
                .map(String::as_str)
                .ok_or_else(|| "missing transform argument".to_owned())
        };
        let z = || TransformLength::Pixels(0.);
        let f = match (name.as_str(), args.len()) {
            ("translate", 1 | 2) => TransformFunction::Translate(
                length(a(0)?)?,
                if args.len() == 2 { length(a(1)?)? } else { z() },
            ),
            ("translatex", 1) => TransformFunction::Translate(length(a(0)?)?, z()),
            ("translatey", 1) => TransformFunction::Translate(z(), length(a(0)?)?),
            ("scale", 1 | 2) => {
                let x = number(a(0)?, false)?;
                TransformFunction::Scale(
                    x,
                    if args.len() == 2 {
                        number(a(1)?, false)?
                    } else {
                        x
                    },
                )
            }
            ("scalex", 1) => TransformFunction::Scale(number(a(0)?, false)?, 1.),
            ("scaley", 1) => TransformFunction::Scale(1., number(a(0)?, false)?),
            ("rotate", 1) => TransformFunction::Rotate(angle(a(0)?)?),
            ("skew", 1 | 2) => TransformFunction::Skew(
                angle(a(0)?)?,
                if args.len() == 2 { angle(a(1)?)? } else { 0. },
            ),
            ("skewx", 1) => TransformFunction::SkewX(angle(a(0)?)?),
            ("skewy", 1) => TransformFunction::SkewY(angle(a(0)?)?),
            ("matrix", 6) => TransformFunction::Matrix(Affine([
                number(a(0)?, false)?,
                number(a(1)?, false)?,
                number(a(2)?, false)?,
                number(a(3)?, false)?,
                number(a(4)?, false)?,
                number(a(5)?, false)?,
            ])),
            _ => return Err(format!("unsupported transform or argument count: {name}")),
        };
        list.push(f);
    }
    if list.is_empty() {
        return Err("empty transform".into());
    }
    Ok(Transform(list.into()))
}
pub(crate) fn origin(raw: &str) -> Result<TransformOrigin, String> {
    let raw = super::parser::clean_value(raw)?.to_ascii_lowercase();
    let parts = tokens(&raw)?;
    if parts.is_empty() || parts.len() > 3 {
        return Err("invalid transform-origin".into());
    }
    if parts.len() == 3 && super::properties::px(&parts[2], false)? != 0. {
        return Err("3D transform origins are unsupported".into());
    }
    let mut x = parts[0].as_str();
    let mut y = parts.get(1).map(String::as_str).unwrap_or("center");
    if (matches!(x, "top" | "bottom") && matches!(y, "left" | "center" | "right"))
        || (matches!(y, "left" | "right") && matches!(x, "top" | "center" | "bottom"))
    {
        std::mem::swap(&mut x, &mut y);
    }
    let coordinate = |v: &str, horizontal: bool| -> Result<TransformLength, String> {
        match v {
            "center" => Ok(TransformLength::Percent(0.5)),
            "left" if horizontal => Ok(TransformLength::Percent(0.)),
            "right" if horizontal => Ok(TransformLength::Percent(1.)),
            "top" if !horizontal => Ok(TransformLength::Percent(0.)),
            "bottom" if !horizontal => Ok(TransformLength::Percent(1.)),
            _ => length(v),
        }
    };
    Ok(TransformOrigin {
        x: coordinate(x, true)?,
        y: coordinate(y, false)?,
    })
}
