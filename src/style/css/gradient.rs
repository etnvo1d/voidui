//! CSS Images gradient functions. Parsing and geometry are separate so relative
//! stop positions are resolved only when the element's painting box is known.
use super::{
    effects::comma_list,
    properties::{color, tokens, unit},
};
use crate::style::{
    color::{ColorSpace, HueDirection},
    gradient::*,
};
type Result<T> = std::result::Result<T, String>;

fn angle(raw: &str) -> Result<f32> {
    let (value, unit) = unit(raw)?;
    match unit.as_str() {
        "deg" => Ok(value),
        "grad" => Ok(value * 0.9),
        "turn" => Ok(value * 360.0),
        "rad" => Ok(value.to_degrees()),
        "px" if value == 0.0 => Ok(0.0),
        _ => Err("expected angle".into()),
    }
}
fn length(raw: &str, conic: bool) -> Result<Length> {
    let (value, unit) = unit(raw)?;
    if unit == "%" {
        return Ok(Length::Percent(value));
    }
    if conic {
        return Ok(Length::Percent(angle(raw)? / 360.0));
    }
    if unit == "px" {
        return Ok(Length::Pixels(value));
    }
    Err("gradient lengths support px and %".into())
}
fn position(parts: &[String]) -> Result<Position> {
    let coordinate = |p: &str, horizontal| match p {
        "center" => Ok(Length::Percent(0.5)),
        "left" if horizontal => Ok(Length::Percent(0.0)),
        "right" if horizontal => Ok(Length::Percent(1.0)),
        "top" if !horizontal => Ok(Length::Percent(0.0)),
        "bottom" if !horizontal => Ok(Length::Percent(1.0)),
        _ => length(p, false),
    };
    match parts {
        [a] if matches!(a.as_str(), "top" | "bottom") => Ok(Position {
            y: coordinate(a, false)?,
            ..Default::default()
        }),
        [a] => Ok(Position {
            x: coordinate(a, true)?,
            ..Default::default()
        }),
        [a, b]
            if matches!(a.as_str(), "top" | "bottom") || matches!(b.as_str(), "left" | "right") =>
        {
            Ok(Position {
                x: coordinate(b, true)?,
                y: coordinate(a, false)?,
            })
        }
        [a, b] => Ok(Position {
            x: coordinate(a, true)?,
            y: coordinate(b, false)?,
        }),
        _ => Err("gradient positions currently support one or two coordinates".into()),
    }
}
fn interpolation(parts: &mut Vec<String>) -> Result<(ColorSpace, HueDirection)> {
    let Some(index) = parts.iter().position(|p| p == "in") else {
        return Ok((ColorSpace::Oklab, HueDirection::Shorter));
    };
    let space = match parts.get(index + 1).map(String::as_str) {
        Some("srgb") => ColorSpace::Srgb,
        Some("srgb-linear") => ColorSpace::LinearSrgb,
        Some("display-p3") => ColorSpace::DisplayP3,
        Some("a98-rgb") => ColorSpace::A98Rgb,
        Some("prophoto-rgb") => ColorSpace::ProphotoRgb,
        Some("rec2020") => ColorSpace::Rec2020,
        Some("lab") => ColorSpace::Lab,
        Some("lch") => ColorSpace::Lch,
        Some("oklab") => ColorSpace::Oklab,
        Some("oklch") => ColorSpace::Oklch,
        Some("xyz") | Some("xyz-d65") => ColorSpace::XyzD65,
        Some("xyz-d50") => ColorSpace::XyzD50,
        Some("hsl") => ColorSpace::Hsl,
        Some("hwb") => ColorSpace::Hwb,
        _ => return Err("unknown gradient interpolation space".into()),
    };
    let mut count = 2;
    let hue = match parts.get(index + 2).map(String::as_str) {
        Some("shorter") => {
            count = 4;
            HueDirection::Shorter
        }
        Some("longer") => {
            count = 4;
            HueDirection::Longer
        }
        Some("increasing") => {
            count = 4;
            HueDirection::Increasing
        }
        Some("decreasing") => {
            count = 4;
            HueDirection::Decreasing
        }
        _ => HueDirection::Shorter,
    };
    if count == 4
        && (parts.get(index + 3).map(String::as_str) != Some("hue")
            || !matches!(
                space,
                ColorSpace::Lch | ColorSpace::Oklch | ColorSpace::Hsl | ColorSpace::Hwb
            ))
    {
        return Err("hue interpolation requires a polar space and the hue keyword".into());
    }
    parts.drain(index..index + count);
    Ok((space, hue))
}

pub(crate) fn parse(raw: &str) -> Result<Gradient> {
    let (name, body) = raw
        .trim()
        .split_once('(')
        .ok_or("expected gradient function")?;
    let name = name.trim().to_ascii_lowercase();
    let name = name.as_str();
    let repeating = name.starts_with("repeating-");
    let name = name.strip_prefix("repeating-").unwrap_or(name);
    if !matches!(
        name,
        "linear-gradient" | "radial-gradient" | "conic-gradient"
    ) {
        return Err("unsupported background image".into());
    }
    let body = body.strip_suffix(')').ok_or("unclosed gradient")?;
    let mut items = comma_list(body)?;
    if items.len() < 2 {
        return Err("gradient needs at least two color stops".into());
    }
    let first = tokens(&items[0])?;
    let first_is_stop = first.first().is_some_and(|p| color(p).is_ok());
    let mut header = if first_is_stop {
        Vec::new()
    } else {
        tokens(&items.remove(0).to_ascii_lowercase())?
    };
    let (space, hue) = interpolation(&mut header)?;
    let kind = match name {
        "linear-gradient" => GradientKind::Linear(match header.as_slice() {
            [] => LinearDirection::Angle(180.0),
            [a] => LinearDirection::Angle(angle(a)?),
            [to, sides @ ..] if to == "to" && (1..=2).contains(&sides.len()) => {
                let (mut x, mut y) = (0, 0);
                for side in sides {
                    match side.as_str() {
                        "left" if x == 0 => x = -1,
                        "right" if x == 0 => x = 1,
                        "top" if y == 0 => y = -1,
                        "bottom" if y == 0 => y = 1,
                        _ => return Err("invalid linear gradient direction".into()),
                    }
                }
                LinearDirection::Corner(x, y)
            }
            _ => return Err("invalid linear gradient header".into()),
        }),
        "conic-gradient" => {
            let mut center = Position::default();
            if let Some(at) = header.iter().position(|p| p == "at") {
                center = position(&header[at + 1..])?;
                header.truncate(at);
            }
            let angle = match header.as_slice() {
                [] => 0.0,
                [from, a] if from == "from" => angle(a)?,
                _ => return Err("invalid conic gradient header".into()),
            };
            GradientKind::Conic { angle, center }
        }
        _ => {
            let mut center = Position::default();
            if let Some(at) = header.iter().position(|p| p == "at") {
                center = position(&header[at + 1..])?;
                header.truncate(at);
            }
            let mut circle = None;
            let mut extent = None;
            let mut lengths = Vec::new();
            for part in header {
                match part.as_str() {
                    "circle" | "ellipse" => {
                        if circle.replace(part == "circle").is_some() {
                            return Err("duplicate radial shape".into());
                        }
                    }
                    "closest-side" | "farthest-side" | "closest-corner" | "farthest-corner" => {
                        if extent
                            .replace(match part.as_str() {
                                "closest-side" => RadialExtent::ClosestSide,
                                "farthest-side" => RadialExtent::FarthestSide,
                                "closest-corner" => RadialExtent::ClosestCorner,
                                _ => RadialExtent::FarthestCorner,
                            })
                            .is_some()
                        {
                            return Err("duplicate radial extent".into());
                        }
                    }
                    _ => {
                        let v = length(&part, false)?;
                        if v.resolve(1.0) < 0.0 {
                            return Err("negative radial size".into());
                        }
                        lengths.push(v);
                    }
                }
            }
            let circle = circle.unwrap_or(lengths.len() == 1);
            let extent = if lengths.is_empty() {
                RadialSize::Extent(extent.unwrap_or(RadialExtent::FarthestCorner))
            } else if extent.is_some() {
                return Err("cannot combine radial extent and lengths".into());
            } else {
                match lengths.as_slice() {
                    [a @ Length::Pixels(_)] if circle => RadialSize::Explicit(*a, *a),
                    [a, b] if !circle => RadialSize::Explicit(*a, *b),
                    _ => return Err("circle needs one px radius; ellipse needs two lengths".into()),
                }
            };
            GradientKind::Radial {
                circle,
                extent,
                center,
            }
        }
    };
    let conic = name == "conic-gradient";
    let mut stops = Vec::new();
    let mut count = 0;
    for item in items {
        let parts = tokens(&item)?;
        if let Some(c) = parts.first().and_then(|p| color(p).ok()) {
            match &parts[1..] {
                [] => stops.push(GradientItem::Stop {
                    color: c,
                    position: None,
                }),
                [p] => stops.push(GradientItem::Stop {
                    color: c,
                    position: Some(length(p, conic)?),
                }),
                [a, b] => {
                    stops.push(GradientItem::Stop {
                        color: c,
                        position: Some(length(a, conic)?),
                    });
                    stops.push(GradientItem::Stop {
                        color: c,
                        position: Some(length(b, conic)?),
                    });
                }
                _ => return Err("color stop accepts at most two positions".into()),
            }
            count += 1;
        } else if let [p] = parts.as_slice() {
            if stops.is_empty() || matches!(stops.last(), Some(GradientItem::Hint(_))) {
                return Err("hint must be between color stops".into());
            }
            stops.push(GradientItem::Hint(length(p, conic)?));
        } else {
            return Err("invalid gradient stop".into());
        }
    }
    if count < 2 || matches!(stops.last(), Some(GradientItem::Hint(_))) {
        return Err("gradient needs two colors and cannot end in a hint".into());
    }
    Ok(Gradient {
        kind,
        repeating,
        space,
        hue,
        stops: stops.into(),
    })
}

pub(super) fn images(raw: &str) -> Result<BackgroundImages> {
    comma_list(raw)?
        .iter()
        .map(|raw| {
            if raw.eq_ignore_ascii_case("none") {
                Ok(BackgroundImage::None)
            } else {
                parse(raw).map(BackgroundImage::Gradient)
            }
        })
        .collect::<Result<Vec<_>>>()
        .map(Into::into)
}
