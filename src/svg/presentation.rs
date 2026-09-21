//! Import SVG presentation attributes without treating geometry or metadata as CSS.
use crate::style::{
    css::properties::parse_property, declaration::Declaration, media::MediaProperty,
};

/// Only attributes with CSS presentation semantics participate in the cascade.
/// Native geometry (`d`, `points`, `transform`, etc.) stays on the document node
/// and reaches the SVG renderer unchanged. In particular, SVG `d="M…"` is not
/// CSS `d: path("M…")`: tokenizing a glyph outline as CSS only to reject it can
/// dominate the cost of importing an entire document.
fn is_presentation(name: &str) -> bool {
    match MediaProperty::from_name(name) {
        Some(MediaProperty::D | MediaProperty::ObjectFit | MediaProperty::ObjectPosition) => false,
        Some(_) => true,
        // These non-media properties are shared with the UI's CSS cascade.
        // Other SVG attributes remain available to the native SVG renderer.
        None => matches!(
            name,
            "color"
                | "display"
                | "visibility"
                | "overflow"
                | "width"
                | "height"
                | "font-family"
                | "font-size"
                | "font-style"
                | "font-weight"
                | "font-stretch"
                | "font-variant"
                | "letter-spacing"
                | "word-spacing"
                | "text-decoration"
                | "text-anchor"
                | "dominant-baseline"
                | "alignment-baseline"
                | "baseline-shift"
                | "writing-mode"
                | "direction"
                | "unicode-bidi"
                | "cursor"
                | "pointer-events"
        ),
    }
}

pub(super) fn parse(name: &str, value: &str) -> Vec<Declaration> {
    if !is_presentation(name) {
        return Vec::new();
    }
    if let Ok(declarations) = parse_property(name, value) {
        return declarations;
    }
    // SVG permits unitless user-space lengths where CSS requires a unit.
    if matches!(name, "width" | "height" | "font-size") && value.parse::<f32>().is_ok() {
        return parse_property(name, &format!("{value}px")).unwrap_or_default();
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_and_metadata_never_enter_the_css_parser() {
        for name in [
            "d",
            "points",
            "transform",
            "viewBox",
            "id",
            "class",
            "href",
            "xlink:href",
            "data-note",
            "--custom",
        ] {
            assert!(!is_presentation(name), "{name}");
            assert!(parse(name, "var(--geometry)").is_empty(), "{name}");
        }
    }

    #[test]
    fn presentation_attributes_keep_units_and_deferred_values() {
        for (name, value) in [
            ("fill", "red"),
            ("fill", "var(--ink)"),
            ("width", "20"),
            ("height", "50%"),
            ("font-size", "16"),
            ("cx", "5"),
            ("visibility", "hidden"),
        ] {
            assert!(!parse(name, value).is_empty(), "{name}={value}");
        }
        assert!(parse("fill", "not-a-color").is_empty());
        assert!(parse_property("d", "M0 0 L10 10").is_err());
        assert!(parse_property("d", "path('M0 0 L10 10')").is_ok());
    }
}
