//! CSS animation grammar shares tokenization and easing with transitions.
use super::{
    effects::{comma_list, easing, time},
    properties::{tokens, wide},
};
use crate::style::{animation::*, declaration::Declaration as D};
use cssparser::{Parser, ParserInput, Token};

type Result<T> = std::result::Result<T, String>;

pub(super) fn name(raw: &str) -> Result<AnimationName> {
    let mut input = ParserInput::new(raw);
    let mut p = Parser::new(&mut input);
    let name = match p.next().map_err(|_| "missing animation name")? {
        Token::QuotedString(s) => AnimationName(Some(s.to_string())),
        Token::Ident(s) => {
            if [
                "initial",
                "inherit",
                "unset",
                "revert",
                "revert-layer",
                "default",
            ]
            .iter()
            .any(|v| s.eq_ignore_ascii_case(v))
            {
                return Err("reserved animation name".into());
            }
            AnimationName((!s.eq_ignore_ascii_case("none")).then(|| s.to_string()))
        }
        _ => return Err("expected animation name".into()),
    };
    p.expect_exhausted()
        .map_err(|_| "unexpected animation name tokens")?;
    Ok(name)
}
fn iterations(raw: &str) -> Result<f64> {
    if raw.eq_ignore_ascii_case("infinite") {
        return Ok(f64::INFINITY);
    }
    Ok(f64::from(super::properties::number(raw, true)?))
}
macro_rules! keywords {
    ($fn:ident, $ty:ident, {$($css:literal => $variant:ident),+}) => {
        fn $fn(raw: &str) -> Result<$ty> {
            match raw.to_ascii_lowercase().as_str() {
                $($css => Ok($ty::$variant),)+
                _ => Err(format!("invalid {}", stringify!($fn))),
            }
        }
    };
}
keywords!(direction, AnimationDirection, {"normal"=>Normal, "reverse"=>Reverse, "alternate"=>Alternate, "alternate-reverse"=>AlternateReverse});
keywords!(fill, AnimationFillMode, {"none"=>None, "forwards"=>Forwards, "backwards"=>Backwards, "both"=>Both});
keywords!(state, AnimationPlayState, {"running"=>Running, "paused"=>Paused});

pub(super) fn parse(property: &str, raw: &str) -> Result<Vec<D>> {
    let lower = raw.to_ascii_lowercase();
    macro_rules! list {
        ($variant:ident, $parse:expr) => {
            D::$variant(if let Some(value) = wide(&lower) {
                value
            } else {
                comma_list(raw)?
                    .iter()
                    .map($parse)
                    .collect::<Result<Vec<_>>>()?
                    .into()
            })
        };
    }
    Ok(vec![match property {
        "animation-name" => list!(AnimationName, |v: &String| name(v)),
        "animation-duration" => list!(AnimationDuration, |v: &String| time(v, true)),
        "animation-delay" => list!(AnimationDelay, |v: &String| time(v, false)),
        "animation-timing-function" => list!(AnimationTimingFunction, |v: &String| easing(v)),
        "animation-iteration-count" => list!(AnimationIterationCount, |v: &String| iterations(v)),
        "animation-direction" => list!(AnimationDirection, |v: &String| direction(v)),
        "animation-fill-mode" => list!(AnimationFillMode, |v: &String| fill(v)),
        "animation-play-state" => list!(AnimationPlayState, |v: &String| state(v)),
        "animation" => return shorthand(raw),
        _ => return Err(format!("unsupported CSS property: {property}")),
    }])
}
fn shorthand(raw: &str) -> Result<Vec<D>> {
    const LONGHANDS: &[&str] = &[
        "animation-name",
        "animation-duration",
        "animation-delay",
        "animation-timing-function",
        "animation-iteration-count",
        "animation-direction",
        "animation-fill-mode",
        "animation-play-state",
    ];
    if wide::<()>(&raw.to_ascii_lowercase()).is_some() {
        return LONGHANDS
            .iter()
            .map(|p| parse(p, raw))
            .collect::<Result<Vec<_>>>()
            .map(|v| v.into_iter().flatten().collect());
    }
    let (
        mut names,
        mut durations,
        mut delays,
        mut easings,
        mut counts,
        mut directions,
        mut fills,
        mut states,
    ) = (
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    for item in comma_list(raw)? {
        let (mut n, mut d, mut delay, mut e, mut count, mut dir, mut f, mut s) =
            (None, None, None, None, None, None, None, None);
        for token in tokens(&item)? {
            if let Ok(value) = time(&token, false) {
                if d.is_none() {
                    if value < 0.0 {
                        return Err("negative animation duration".into());
                    }
                    d = Some(value);
                } else if delay.replace(value).is_some() {
                    return Err("too many animation times".into());
                }
            } else if e.is_none() && easing(&token).is_ok() {
                e = Some(easing(&token)?);
            } else if count.is_none() && iterations(&token).is_ok() {
                count = Some(iterations(&token)?);
            } else if dir.is_none() && direction(&token).is_ok() {
                dir = Some(direction(&token)?);
            } else if f.is_none() && fill(&token).is_ok() {
                f = Some(fill(&token)?);
            } else if s.is_none() && state(&token).is_ok() {
                s = Some(state(&token)?);
            } else if n.replace(name(&token)?).is_some() {
                return Err("duplicate animation name".into());
            }
        }
        names.push(n.unwrap_or_default());
        durations.push(d.unwrap_or(0.0));
        delays.push(delay.unwrap_or(0.0));
        easings.push(e.unwrap_or_default());
        counts.push(count.unwrap_or(1.0));
        directions.push(dir.unwrap_or_default());
        fills.push(f.unwrap_or_default());
        states.push(s.unwrap_or_default());
    }
    Ok(vec![
        D::AnimationName(names.into()),
        D::AnimationDuration(durations.into()),
        D::AnimationDelay(delays.into()),
        D::AnimationTimingFunction(easings.into()),
        D::AnimationIterationCount(counts.into()),
        D::AnimationDirection(directions.into()),
        D::AnimationFillMode(fills.into()),
        D::AnimationPlayState(states.into()),
    ])
}
