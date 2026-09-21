//! Source-backed editing, projections and extensible viewport layout.
#![doc = include_str!("../../docs/editor-framework.md")]
mod buffer;
pub use buffer::{BufferOptions, TextBuffer, TextRead, TextSnapshot};
mod document;
pub(crate) mod highlights;
mod selection;
mod state;
pub use document::{Bias, ChangeSet, Document, Edit, EditError};
pub use selection::{Selection, SelectionSet};
pub use state::{
    Change, Composition, EditConstraint, EditKind, EditorState, HistoryOptions, Transaction,
};

pub(crate) mod handle;
pub use handle::Editor;

mod flow;
mod height_index;
mod layout;
pub use layout::{EditorLayout, LayoutOptions, LayoutStats, Motion, ViewportOptions};
mod commands;
pub use commands::{grapheme_boundary, source_grapheme_boundary, word_boundary};

mod format;
pub use crate::core::rich_text::{InlineStyle, RichText, StyleSpan};
pub use format::{Format, StyleChange, StylePatch};

mod projection;
pub use projection::{
    BlockStyle, BlockView, EditBehavior, ParagraphStyle, PositionMap, ProjectedObject,
    ProjectedText, Projection, ProjectionSnapshot, Replacement, ReplacementContent, ViewId,
};

mod views;
pub use views::{EditorViews, EmbeddedView, SourceViewport, ViewMetrics, WidgetView};
mod view_description;
pub use view_description::{ViewDescription, ViewSpec, WidgetDescription};
mod view_matching;

pub(crate) mod extensions;
pub use extensions::{Anchor, AnchorId, EditorExtension, EditorExtensions, ExtensionContext};

mod block_layout;
pub use block_layout::{BlockArrangement, BlockLayout, BlockMeasure, GridBlock, TextCell};
