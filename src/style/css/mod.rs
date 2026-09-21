//! Compile stylesheets once; match only invalidated trees.
#![doc = include_str!("../../../docs/css.md")]
pub(crate) mod parser;
pub(crate) mod properties;
pub mod reload;
mod selector;
pub(crate) mod sheet;
pub use parser::CssError;
pub use sheet::{CascadeStats, Stylesheet};

mod effects;
pub(crate) mod gradient;

pub(crate) mod media;

mod math;
mod scroll;

pub(crate) mod transform;

#[doc(hidden)]
pub mod values;
