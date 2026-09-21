//! Parsing helpers for paint effects and animation lists. Commas inside functions
//! are consumed by cssparser rather than mistaken for separators between layers.
use super::properties::{color, px, tokens};
use crate::style::shadow::{BoxShadow, BoxShadows};
use cssparser::{Parser, ParserInput};

type Result<T> = std::result::Result<T, String>;

pub(super) fn comma_list(raw: &str) -> Result<Vec<String>> {
    let mut input = ParserInput::new(raw);
    Parser::new(&mut input)
        .parse_comma_separated(|p| {
            let start = p.position();
            while p.next_including_whitespace_and_comments().is_ok() {}
            let value = p.slice_from(start).trim();
            if value.is_empty() {
                return Err(p.new_custom_error::<_, ()>(()));
            }
            Ok(value.to_owned())
        })
        .map_err(|_| "invalid comma-separated list".into())
}

pub(super) fn shadows(raw: &str) -> Result<BoxShadows> {
    if raw.eq_ignore_ascii_case("none") {
        return Ok(BoxShadows::default());
    }
    comma_list(raw)?
        .iter()
        .map(|raw| {
            let mut lengths = Vec::new();
            let mut shadow = BoxShadow::default();
            let mut has_color = false;
            let mut lengths_finished = false;
            for token in tokens(raw)? {
                if token.eq_ignore_ascii_case("inset") && !shadow.inset {
                    shadow.inset = true;
                    lengths_finished |= !lengths.is_empty();
                } else if let Ok(c) = color(&token) {
                    if has_color {
                        return Err("duplicate shadow color".into());
                    }
                    shadow.color = c;
                    has_color = true;
                    lengths_finished |= !lengths.is_empty();
                } else {
                    if lengths_finished {
                        return Err("shadow lengths must be contiguous".into());
                    }
                    lengths.push(px(&token, false)?);
                }
            }
            if !(2..=4).contains(&lengths.len()) {
                return Err("box-shadow needs two to four lengths".into());
            }
            shadow.offset_x = lengths[0];
            shadow.offset_y = lengths[1];
            shadow.blur = lengths.get(2).copied().unwrap_or(0.0);
            shadow.spread = lengths.get(3).copied().unwrap_or(0.0);
            if shadow.blur < 0.0 {
                return Err("negative shadow blur".into());
            }
            Ok(shadow)
        })
        .collect::<Result<Vec<_>>>()
        .map(Into::into)
}

pub(super) fn time(raw: &str, nonnegative: bool) -> Result<f64> {
    let mut input = ParserInput::new(raw);
    let mut p = Parser::new(&mut input);
    let seconds = match p.next().map_err(|_| "missing time")? {
        cssparser::Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("s") => {
            f64::from(*value)
        }
        cssparser::Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("ms") => {
            f64::from(*value) / 1000.0
        }
        _ => return Err("time requires s or ms".into()),
    };
    p.expect_exhausted().map_err(|_| "unexpected time tokens")?;
    if !seconds.is_finite() || (nonnegative && seconds < 0.0) {
        return Err("invalid transition time".into());
    }
    Ok(seconds)
}

pub(super) fn easing(raw: &str) -> Result<crate::style::transition::Easing> {
    use crate::style::transition::{Easing as E, StepPosition as S};
    let raw = raw.to_ascii_lowercase();
    let result = match raw.as_str() {
        "linear" => E::Linear,
        "ease" => E::EASE,
        "ease-in" => E::EASE_IN,
        "ease-out" => E::EASE_OUT,
        "ease-in-out" => E::EASE_IN_OUT,
        "step-start" => E::Steps(1, S::Start),
        "step-end" => E::Steps(1, S::End),
        _ => {
            let (name, inner) = raw.split_once('(').ok_or("invalid timing function")?;
            let parts = comma_list(inner.strip_suffix(')').ok_or("unclosed timing function")?)?;
            match (name, parts.as_slice()) {
                ("linear", _) => E::PiecewiseLinear(linear_easing(&parts)?.into()),
                ("cubic-bezier", [a, b, c, d]) => E::CubicBezier(
                    super::properties::number(a, false)? as f64,
                    super::properties::number(b, false)? as f64,
                    super::properties::number(c, false)? as f64,
                    super::properties::number(d, false)? as f64,
                ),
                ("steps", [n]) => {
                    E::Steps(n.parse().map_err(|_| "steps needs an integer")?, S::End)
                }
                ("steps", [n, p]) => E::Steps(
                    n.parse().map_err(|_| "steps needs an integer")?,
                    match p.as_str() {
                        "start" | "jump-start" => S::Start,
                        "end" | "jump-end" => S::End,
                        "jump-none" => S::JumpNone,
                        "jump-both" => S::JumpBoth,
                        _ => return Err("invalid step position".into()),
                    },
                ),
                _ => return Err("invalid timing function arguments".into()),
            }
        }
    };
    if !result.is_valid() {
        return Err("timing function outside its CSS range".into());
    }
    Ok(result)
}

pub(super) fn transition_property(
    raw: &str,
) -> Result<crate::style::transition::TransitionProperty> {
    let mut input = ParserInput::new(raw);
    let mut p = Parser::new(&mut input);
    let name = p
        .expect_ident_cloned()
        .map_err(|_| "expected transition property")?;
    p.expect_exhausted()
        .map_err(|_| "unexpected transition property tokens")?;
    if matches_ignore_ascii_case(
        &name,
        &["inherit", "initial", "unset", "revert", "revert-layer"],
    ) {
        return Err("CSS-wide keyword cannot occur inside a transition list".into());
    }
    Ok(crate::style::transition::TransitionProperty::from(
        name.as_ref(),
    ))
}
fn matches_ignore_ascii_case(s: &str, values: &[&str]) -> bool {
    values.iter().any(|v| s.eq_ignore_ascii_case(v))
}

pub(super) fn transitions(raw: &str) -> Result<crate::style::transition::TransitionStyle> {
    use crate::style::transition::{TransitionProperty, TransitionStyle};
    let mut result = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let items = comma_list(raw)?;
    for item in &items {
        let (mut property, mut duration, mut delay, mut timing) = (None, None, None, None);
        for token in tokens(item)? {
            if let Ok(value) = time(&token, false) {
                if duration.is_none() {
                    if value < 0.0 {
                        return Err("negative transition duration".into());
                    }
                    duration = Some(value);
                } else if delay.replace(value).is_some() {
                    return Err("too many transition times".into());
                }
            } else if let Ok(value) = easing(&token) {
                if timing.replace(value).is_some() {
                    return Err("duplicate transition timing function".into());
                }
            } else if property.replace(transition_property(&token)?).is_some() {
                return Err("duplicate transition property".into());
            }
        }
        let property = property.unwrap_or_else(|| TransitionProperty::from("all"));
        if property.0 == "none" && items.len() != 1 {
            return Err("none cannot be combined with other transitions".into());
        }
        result.0.push(property);
        result.1.push(duration.unwrap_or(0.0));
        result.2.push(delay.unwrap_or(0.0));
        result.3.push(timing.unwrap_or_default());
    }
    Ok(TransitionStyle {
        properties: result.0.into(),
        durations: result.1.into(),
        delays: result.2.into(),
        easing: result.3.into(),
    })
}

/// Linear easing uses the same monotonic-position fixup as gradient stops, but
/// outputs may overshoot and inputs are percentages rather than lengths.
fn linear_easing(parts: &[String]) -> Result<Vec<(f64, f64)>> {
    let mut points = Vec::new();
    for part in parts {
        let tokens = tokens(part)?;
        if tokens.is_empty() || tokens.len() > 3 {
            return Err("invalid linear easing stop".into());
        }
        let output = super::properties::number(&tokens[0], false)? as f64;
        if tokens.len() == 1 {
            points.push((f64::NAN, output));
        }
        for position in &tokens[1..] {
            let (value, unit) = super::properties::unit(position)?;
            if unit != "%" {
                return Err("linear easing positions require percentages".into());
            }
            points.push((value as f64, output));
        }
    }
    if points.len() < 2 {
        return Err("linear easing needs two stops".into());
    }
    if points[0].0.is_nan() {
        points[0].0 = 0.0;
    }
    let last = points.len() - 1;
    if points[last].0.is_nan() {
        points[last].0 = 1.0;
    }
    let mut previous = points[0].0;
    for p in &mut points {
        if !p.0.is_nan() {
            p.0 = p.0.max(previous);
            previous = p.0;
        }
    }
    let mut start = 0;
    for end in 1..points.len() {
        if points[end].0.is_nan() {
            continue;
        }
        for i in start + 1..end {
            points[i].0 = points[start].0
                + (points[end].0 - points[start].0) * (i - start) as f64 / (end - start) as f64;
        }
        start = end;
    }
    Ok(points)
}
