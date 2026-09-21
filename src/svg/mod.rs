//! Inline SVG documents and compact graph builders. Only the outer viewport
//! participates in box layout; paths and groups retain SVG coordinates.
mod document;
mod nodes;
mod presentation;
mod widget;
pub use document::SvgDocument;
pub(crate) use document::{DocNode, NONE};
pub use nodes::*;
pub use widget::{Svg, svg, svg_from_str};
