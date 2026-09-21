//! Strict standard longhands; platform scrollbar policy is intentionally Rust-only.
use super::properties::{tokens, wide};
use crate::style::{color::Color, declaration::Declaration, scroll::*};
use std::sync::Arc;

pub(super) fn parse(p: ScrollProperty, raw: &str) -> Result<Declaration, String> {
    let value = if let Some(v) = wide(raw) {
        v
    } else {
        match p {
            ScrollProperty::OverflowX | ScrollProperty::OverflowY => {
                ScrollValue::Overflow(raw.parse()?)
            }
            ScrollProperty::Width => ScrollValue::Width(raw.parse()?),
            ScrollProperty::Gutter => ScrollValue::Gutter(match tokens(raw)?.as_slice() {
                [a, b]
                    if (a == "stable" && b == "both-edges")
                        || (a == "both-edges" && b == "stable") =>
                {
                    ScrollbarGutter::StableBothEdges
                }
                [a] => a.parse()?,
                _ => {
                    return Err(
                        "scrollbar-gutter requires auto, stable or stable both-edges".into(),
                    );
                }
            }),
            ScrollProperty::OverscrollX | ScrollProperty::OverscrollY => {
                ScrollValue::Overscroll(raw.parse()?)
            }
            ScrollProperty::Colors => ScrollValue::Colors(if raw == "auto" {
                None
            } else {
                let parts = tokens(raw)?;
                if parts.len() != 2 {
                    return Err("scrollbar-color requires auto or two colors".into());
                }
                Some(Arc::new(ScrollbarColors {
                    thumb: parts[0].parse::<Color>()?,
                    track: parts[1].parse::<Color>()?,
                }))
            }),
        }
        .into()
    };
    Ok(Declaration::Scroll(ScrollDeclaration {
        property: p,
        value,
    }))
}
