//! Native Tailwind-style utilities, sharing CSS semantics with fluent methods.
#![doc = include_str!("../../../docs/tailwind.md")]

use super::{
    css::{CssError, Stylesheet, properties::parse_property},
    declaration::Declaration,
    style::Style,
};
use std::{
    cell::OnceCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    sync::OnceLock,
};

mod aliases;
mod catalog;

#[doc(hidden)]
pub use aliases::{
    __voidui_dimension_aliases, __voidui_length_aliases, __voidui_margin_aliases,
    __voidui_paint_aliases,
};
#[doc(hidden)]
pub use catalog::__voidui_tailwind_styles;

struct Utility {
    class: &'static str,
    properties: &'static [(&'static str, &'static str)],
}

thread_local! {
    // Declarations contain thread-local calculation handles and Rc values.
    // Parse only used presets, once per UI thread; repeated renders only apply
    // shared typed declarations. Never share these handles across threads.
    static DECLARATIONS: Vec<OnceCell<Rc<[Declaration]>>> =
        (0..catalog::UTILITIES.len()).map(|_| OnceCell::new()).collect();
}

fn apply_utility(style: &mut Style, index: usize) {
    DECLARATIONS.with(|cache| {
        let declarations = cache[index].get_or_init(|| {
            let utility = &catalog::UTILITIES[index];
            utility
                .properties
                .iter()
                .flat_map(|(name, value)| {
                    parse_property(name, value).unwrap_or_else(|error| {
                        panic!("invalid built-in utility {}: {error}", utility.class)
                    })
                })
                .collect::<Vec<_>>()
                .into()
        });
        for declaration in declarations.iter() {
            declaration.apply(style);
        }
    });
}

fn index() -> &'static BTreeMap<&'static str, usize> {
    static INDEX: OnceLock<BTreeMap<&'static str, usize>> = OnceLock::new();
    INDEX.get_or_init(|| {
        catalog::UTILITIES
            .iter()
            .enumerate()
            .map(|(i, u)| (u.class, i))
            .collect()
    })
}

/// List supported base classes in deterministic cascade order.
/// State prefixes are accepted by [`stylesheet`], without adding catalog entries.
pub fn classes() -> impl ExactSizeIterator<Item = &'static str> {
    catalog::UTILITIES.iter().map(|u| u.class)
}

// Variant order is explicit and stable. A combined variant uses a conjunction
// of pseudo-classes; class text is always escaped before becoming a selector.
const STATES: &[&str] = &[
    "hover",
    "focus",
    "focus-within",
    "active",
    "disabled",
    "enabled",
    "read-only",
    "read-write",
    "placeholder-shown",
];

/// Compile only the requested utility classes, including state variants.
///
/// Class-list order does not affect precedence. Within a variant, catalog order
/// wins (side utilities follow axis utilities, which follow all-edge utilities).
/// Fluent styles override normal classes; a trailing `!` makes a class important.
/// Unknown classes/variants return an error instead of silently doing nothing.
pub fn stylesheet(class_list: &str) -> Result<Stylesheet, CssError> {
    let mut rules = BTreeSet::new();
    for class in class_list.split_whitespace() {
        let (plain, important) = class
            .strip_suffix('!')
            .map_or((class, false), |c| (c, true));
        let mut parts: Vec<_> = plain.split(':').collect();
        let base = parts.pop().unwrap_or_default();
        let Some(&utility) = index().get(base) else {
            return Err(utility_error(format!(
                "unsupported Tailwind utility: {class}"
            )));
        };
        let mut states = Vec::new();
        for state in parts {
            let Some(rank) = STATES.iter().position(|known| *known == state) else {
                return Err(utility_error(format!(
                    "unsupported Tailwind variant '{state}' in '{class}'"
                )));
            };
            states.push(rank);
        }
        states.sort_unstable();
        states.dedup();
        rules.insert((states, utility, important, class));
    }
    let mut css = String::new();
    for (states, utility, important, class) in rules {
        css.push('.');
        cssparser::serialize_identifier(class, &mut css).expect("writing a String cannot fail");
        for state in states {
            css.push(':');
            css.push_str(STATES[state]);
        }
        css.push('{');
        for (name, value) in catalog::UTILITIES[utility].properties {
            css.push_str(name);
            css.push(':');
            css.push_str(value);
            if important {
                css.push_str(" !important");
            }
            css.push(';');
        }
        css.push_str("}\n");
    }
    Stylesheet::parse(&css)
}

/// Compile every base utility for applications that compose class names at runtime.
/// For smaller sheets or state variants, use [`stylesheet`] with an explicit list.
pub fn all() -> Result<Stylesheet, CssError> {
    stylesheet(&classes().collect::<Vec<_>>().join(" "))
}

fn utility_error(message: String) -> CssError {
    CssError {
        line: 1,
        column: 1,
        message,
    }
}
