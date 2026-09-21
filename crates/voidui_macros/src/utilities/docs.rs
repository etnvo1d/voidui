//! Describe preset values from the same catalog that generates their setters.
//! Numeric examples are documentation only; runtime resolution stays in voidui.
use super::Utility;

/// A conventional example size, not a value installed in the application's theme.
const EXAMPLE_FONT_PX: f64 = 16.0;

#[derive(Clone, Copy)]
struct Quantity<'a> {
    value: f64,
    unit: &'a str,
}

fn function<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    value
        .strip_prefix(name)?
        .strip_prefix('(')?
        .strip_suffix(')')
}

fn fallback(value: &str) -> &str {
    function(value, "var")
        .and_then(|args| args.split_once(','))
        .map_or(value, |(_, default)| fallback(default.trim()))
}

// Recognize only the catalog's numeric literals and scalar products/quotients.
// Unknown CSS remains verbatim in the docs instead of receiving a guessed px
// equivalent. Units in colors, grid tracks and shadows are not scalar lengths.
fn numeric(value: &str) -> Option<Quantity<'_>> {
    let value = fallback(value.trim());
    if let Some(expression) = function(value, "calc") {
        let mut depth = 0;
        for (offset, ch) in expression.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                '*' | '/' if depth == 0 => {
                    let left = numeric(&expression[..offset])?;
                    let right = numeric(&expression[offset + 1..])?;
                    let (value, unit) = match ch {
                        '*' if right.unit.is_empty() => (left.value * right.value, left.unit),
                        '*' if left.unit.is_empty() => (left.value * right.value, right.unit),
                        '/' if right.unit.is_empty() && right.value != 0.0 => {
                            (left.value / right.value, left.unit)
                        }
                        _ => return None,
                    };
                    return value.is_finite().then_some(Quantity { value, unit });
                }
                _ => (),
            }
        }
        return None;
    }
    for unit in ["rem", "px", "%", "vw", "vh", ""] {
        if let Some(number) = value.strip_suffix(unit)
            && let Ok(value) = number.parse::<f64>()
            && value.is_finite()
        {
            return Some(Quantity { value, unit });
        }
    }
    None
}

fn number(value: f64) -> String {
    format!("{value:.6}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn describe(property: &str, css: &str, font_px: f64) -> String {
    let default = if css.contains("var(") {
        " by default"
    } else {
        ""
    };
    let Some(Quantity { value, unit }) = numeric(css) else {
        let value = fallback(css);
        let context = match value {
            "auto" if property == "cursor" => {
                "; selects the pointer automatically, including a text cursor over selectable text"
            }
            "auto" | "min-content" | "max-content" | "fit-content" => {
                "; layout determines the used size or placement, with no fixed pixel value"
            }
            _ => "",
        };
        return format!("`{value}`{default}{context}");
    };
    let literal = format!("`{}{unit}`{default}", number(value));
    match unit {
        "rem" => format!(
            "{literal} (`{}px` at a `{}px` root font size; scales with the root font size)",
            number(value * EXAMPLE_FONT_PX),
            number(EXAMPLE_FONT_PX)
        ),
        "px" => format!("{literal} (logical pixels; independent of root font size)"),
        "%" => {
            let basis = match property {
                "width" | "min-width" | "max-width" => "the containing block's width",
                "height" | "min-height" | "max-height" => {
                    "the containing block's height, which must be definite to resolve this percentage"
                }
                "flex-basis" => "the flex container's inner main-axis size",
                _ => "the property's percentage reference size",
            };
            let approximation = if function(fallback(css), "calc").is_some() {
                "approximately "
            } else {
                ""
            };
            format!("{approximation}{literal} of {basis}; no fixed pixel value")
        }
        "vw" | "vh" => format!(
            "{literal} ({}% of the logical window {}; follows window resizing)",
            number(value),
            if unit == "vw" { "width" } else { "height" }
        ),
        "" if property == "line-height" => format!(
            "about {} times the element's computed font size{default} (`{}px` at a `{}px` font size)",
            number(value),
            number(value * font_px),
            number(font_px)
        ),
        "" if property == "opacity" => format!("{literal} ({}% opacity)", number(value * 100.0)),
        _ => literal,
    }
}

pub(super) fn method(utility: &Utility) -> String {
    let font_px = utility
        .properties
        .iter()
        .find(|(property, _)| property == "font-size")
        .and_then(|(_, value)| numeric(value))
        .and_then(|q| match q.unit {
            "rem" => Some(q.value * EXAMPLE_FONT_PX),
            "px" => Some(q.value),
            _ => None,
        })
        .unwrap_or(EXAMPLE_FONT_PX);
    let descriptions = utility
        .properties
        .iter()
        .map(|(property, value)| format!("`{property}` to {}", describe(property, value, font_px)))
        .collect::<Vec<_>>()
        .join("; ");
    let declarations = utility
        .properties
        .iter()
        .map(|(property, value)| format!("{property}: {value};"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut doc = format!(
        "Set {descriptions}.\n\nTailwind utility: `{}`.\n\n```css\n{declarations}\n```\n",
        utility.class
    );
    if utility
        .properties
        .iter()
        .any(|(_, value)| value.contains("var("))
    {
        doc.push_str("\nThe CSS theme variables above can override these defaults. Relative values resolve during styling.\n");
    }
    if utility.properties.iter().any(|(name, _)| name == "cursor") {
        doc.push_str("\nSets pointer appearance only; dragging or resizing behavior must be implemented separately. Native cursor shapes depend on the platform.\n");
    } else {
        doc.push_str(
            "\nPixel examples use logical pixels; display scaling determines physical pixels.\n",
        );
    }
    doc.push_str("\nThis is inline style: later setters override the same properties.\n");
    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(class: &str, properties: &[(&str, &str)]) -> String {
        method(&Utility {
            method: None,
            class: class.into(),
            properties: properties
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        })
    }

    #[test]
    fn spacing_and_border_use_distinct_units() {
        let width = doc("w-16", &[("width", "calc(var(--spacing, 0.25rem) * 16)")]);
        assert!(
            width
                .starts_with("Set `width` to `4rem` by default (`64px` at a `16px` root font size")
        );
        assert!(width.contains("width: calc(var(--spacing, 0.25rem) * 16);"));
        assert!(width.contains("theme variables"));
        let border = doc("border-r", &[("border-right-width", "1px")]);
        assert!(border.starts_with("Set `border-right-width` to `1px`"));
        assert!(border.contains("independent of root font size"));
        assert!(!border.contains("theme variables"));
    }

    #[test]
    fn defaults_are_derived_from_values_instead_of_class_names() {
        let custom = doc("w-16", &[("width", "calc(var(--spacing, 0.5rem) * 16)")]);
        assert!(custom.contains("`8rem` by default (`128px`"));
        assert!(
            doc(
                "p-0.5",
                &[("padding", "calc(var(--spacing, 0.25rem) * 0.5)")]
            )
            .contains("`0.125rem` by default (`2px`")
        );
        assert!(
            doc(
                "rounded-lg",
                &[("border-radius", "var(--radius-lg, 0.5rem)")]
            )
            .contains("`8px` at a `16px` root font size")
        );
    }

    #[test]
    fn percentages_and_viewport_units_have_no_constant_pixel_size() {
        let width = doc("w-full", &[("width", "100%")]);
        assert!(width.contains("`100%` of the containing block's width; no fixed pixel value"));
        let fraction = doc("w-1/3", &[("width", "calc(100% / 3)")]);
        assert!(fraction.contains("33.333333%"));
        assert!(doc("h-full", &[("height", "100%")]).contains("must be definite"));
        assert!(
            doc("w-screen", &[("width", "100vw")]).contains("100% of the logical window width")
        );
        assert!(
            doc("h-screen", &[("height", "100vh")]).contains("100% of the logical window height")
        );
    }

    #[test]
    fn text_size_and_line_height_share_the_font_size_example() {
        let text = doc(
            "text-sm",
            &[
                ("font-size", "var(--text-sm, 0.875rem)"),
                (
                    "line-height",
                    "var(--text-sm--line-height, calc(1.25 / 0.875))",
                ),
            ],
        );
        assert!(text.contains("`14px` at a `16px` root font size"));
        assert!(text.contains("`20px` at a `14px` font size"));
        assert!(
            doc(
                "leading-loose",
                &[("line-height", "var(--leading-loose, 2)")]
            )
            .contains("`32px` at a `16px` font size")
        );
    }

    #[test]
    fn non_lengths_keep_their_actual_values() {
        let color = doc(
            "bg-gray-50",
            &[(
                "background-color",
                "var(--color-gray-50, oklch(98.5% 0.002 247.839))",
            )],
        );
        assert!(color.contains("`oklch(98.5% 0.002 247.839)` by default"));
        assert!(!color.contains("root font size"));
        assert!(doc("opacity-50", &[("opacity", "0.5")]).contains("50% opacity"));
        assert!(doc("w-auto", &[("width", "auto")]).contains("no fixed pixel value"));
        for unsupported in ["min(1rem, 20px)", "calc(1rem * 2rem)", "calc(1rem / 0)"] {
            assert!(numeric(unsupported).is_none());
        }
    }

    #[test]
    fn cursor_docs_describe_pointer_appearance_without_size_claims() {
        let resize = doc("cursor-col-resize", &[("cursor", "col-resize")]);
        assert!(resize.contains("Set `cursor` to `col-resize`"));
        assert!(resize.contains("resizing behavior must be implemented separately"));
        assert!(!resize.contains("Pixel examples"));
        let auto = doc("cursor-auto", &[("cursor", "auto")]);
        assert!(auto.contains("selects the pointer automatically"));
        assert!(!auto.contains("used size"));
    }
}
