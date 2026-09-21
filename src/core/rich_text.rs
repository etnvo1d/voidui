//! Shared inline content for labels and editable documents. UTF-8 ranges address
//! the source string; style boundaries do not introduce boxes or line breaks.
#![doc = include_str!("../../docs/rich-text.md")]
use crate::render::{
    self, Font, FontStyle, FontWeight, Hsla, SharedString, StrikethroughStyle, TextRun,
    UnderlineStyle,
};
use std::{collections::BTreeMap, ops::Range, sync::Arc};

/// Sparse overrides of the surrounding typography. Unset fields inherit.
/// Metadata is retained with text but has no built-in visual or event behavior;
/// components can use it for links, mentions, annotations, or schema identifiers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InlineStyle {
    pub font: Option<Font>,
    pub font_weight: Option<FontWeight>,
    pub font_style: Option<FontStyle>,
    pub font_size: Option<f32>,
    pub line_height: Option<f32>,
    pub color: Option<Hsla>,
    pub background: Option<Hsla>,
    pub underline: Option<UnderlineStyle>,
    pub strikethrough: Option<StrikethroughStyle>,
    pub metadata: Option<Arc<BTreeMap<SharedString, SharedString>>>,
}
impl InlineStyle {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn bold(mut self) -> Self {
        self.font_weight = Some(FontWeight::BOLD);
        self
    }
    pub fn italic(mut self) -> Self {
        self.font_style = Some(FontStyle::Italic);
        self
    }
    pub fn font(mut self, font: Font) -> Self {
        self.font = Some(font);
        self
    }
    pub fn font_size(mut self, size: f32) -> Self {
        assert!(
            size.is_finite() && size > 0.0,
            "font_size must be finite and positive"
        );
        self.font_size = Some(size);
        self
    }
    pub fn line_height(mut self, height: f32) -> Self {
        assert!(
            height.is_finite() && height > 0.0,
            "line_height must be finite and positive"
        );
        self.line_height = Some(height);
        self
    }
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }
    pub fn background(mut self, color: impl Into<Hsla>) -> Self {
        self.background = Some(color.into());
        self
    }
    pub fn metadata(
        mut self,
        key: impl Into<SharedString>,
        value: impl Into<SharedString>,
    ) -> Self {
        Arc::make_mut(self.metadata.get_or_insert_with(Default::default))
            .insert(key.into(), value.into());
        self
    }
    /// Apply a child style without discarding inherited independent properties.
    /// Replacing a range with `InlineStyle::default()` removes its formatting.
    pub fn overlay(&self, child: &Self) -> Self {
        let mut result = self.clone();
        macro_rules! inherit {
            ($($field:ident),* $(,)?) => { $(if child.$field.is_some() { result.$field = child.$field.clone(); })* };
        }
        inherit!(
            font,
            font_weight,
            font_style,
            font_size,
            line_height,
            color,
            background,
            underline,
            strikethrough
        );
        if let Some(metadata) = &child.metadata {
            if result.metadata.is_none() {
                result.metadata = Some(metadata.clone());
            } else {
                Arc::make_mut(result.metadata.as_mut().unwrap())
                    .extend(metadata.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
        }
        result
    }
    pub fn validate(&self) -> render::Result<()> {
        anyhow::ensure!(
            self.font_size.is_none_or(|v| v.is_finite() && v > 0.0)
                && self.line_height.is_none_or(|v| v.is_finite() && v > 0.0)
                && self
                    .font_weight
                    .is_none_or(|v| v.0.is_finite() && v.0 > 0.0),
            "invalid inline typography"
        );
        // Reject malformed public fields at the document boundary, before a
        // mounted view attempts shaping or submits non-finite paint geometry.
        if let Some(font) = &self.font {
            anyhow::ensure!(
                font.weight.0.is_finite() && font.weight.0 > 0.0,
                "invalid font weight"
            );
            for (tag, value) in font.features.tag_value_list() {
                anyhow::ensure!(
                    tag.len() == 4
                        && tag.bytes().all(|b| (0x20..=0x7e).contains(&b))
                        && u16::try_from(*value).is_ok(),
                    "invalid OpenType feature"
                );
            }
        }
        for color in [
            self.color,
            self.background,
            self.underline.and_then(|v| v.color),
            self.strikethrough.and_then(|v| v.color),
        ]
        .into_iter()
        .flatten()
        {
            anyhow::ensure!(
                [color.h, color.s, color.l, color.a]
                    .into_iter()
                    .all(f32::is_finite),
                "invalid inline color"
            );
        }
        for thickness in [
            self.underline.map(|v| v.thickness),
            self.strikethrough.map(|v| v.thickness),
        ]
        .into_iter()
        .flatten()
        {
            let thickness = f32::from(thickness);
            anyhow::ensure!(
                thickness.is_finite() && thickness >= 0.0,
                "invalid decoration thickness"
            );
        }
        Ok(())
    }
    pub(crate) fn run(&self, len: usize, base: &Font) -> TextRun {
        let mut font = self.font.clone().unwrap_or_else(|| base.clone());
        if let Some(weight) = self.font_weight {
            font.weight = weight;
        }
        if let Some(style) = self.font_style {
            font.style = style;
        }
        TextRun {
            len,
            font,
            font_size: self.font_size,
            line_height: self.line_height,
            color: self.color.unwrap_or_else(render::black),
            color_is_explicit: self.color.is_some(),
            background_color: self.background,
            underline: self.underline,
            strikethrough: self.strikethrough,
        }
    }
}

/// A nonempty UTF-8 range with overrides. Stored spans are ordered, disjoint,
/// and coalesced; unformatted gaps require no span allocation.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleSpan {
    pub range: Range<usize>,
    pub style: InlineStyle,
}
impl StyleSpan {
    pub fn new(range: Range<usize>, style: InlineStyle) -> Self {
        Self { range, style }
    }
}

/// Immutable, cheaply cloned source and canonical inline spans. One instance is
/// one formatting context: adjacent spans share shaping, wrapping, and baselines.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RichText {
    text: SharedString,
    spans: Arc<[StyleSpan]>,
}
impl RichText {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            spans: Arc::default(),
        }
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn shared_text(&self) -> SharedString {
        self.text.clone()
    }
    pub fn spans(&self) -> &[StyleSpan] {
        &self.spans
    }
    /// Find semantic attributes at a hit-test byte offset. Gaps and the end of
    /// the source have no span; caret-style inheritance is handled by EditorState.
    pub fn span_at(&self, index: usize) -> Option<&StyleSpan> {
        if !self.text.is_char_boundary(index) {
            return None;
        }
        let first = self.spans.partition_point(|s| s.range.end <= index);
        self.spans.get(first).filter(|s| s.range.contains(&index))
    }
    /// Reject malformed or overlapping ranges before publishing content. Use
    /// nested `Inline` builders when styles should inherit or overlap.
    pub fn from_spans(
        text: impl Into<SharedString>,
        spans: impl IntoIterator<Item = StyleSpan>,
    ) -> render::Result<Self> {
        let text = text.into();
        let mut spans: Vec<_> = spans.into_iter().collect();
        spans.sort_by_key(|s| (s.range.start, s.range.end));
        validate_spans(&text, &spans)?;
        let mut output: Vec<StyleSpan> = Vec::with_capacity(spans.len());
        for span in spans {
            if span.range.is_empty() || span.style == InlineStyle::default() {
                continue;
            }
            if let Some(last) = output.last_mut() {
                if last.range.end == span.range.start && last.style == span.style {
                    last.range.end = span.range.end;
                    continue;
                }
            }
            output.push(span);
        }
        Ok(Self {
            text,
            spans: output.into(),
        })
    }
    /// Extract a self-contained fragment suitable for insertion or rich clipboard
    /// adapters. Coordinates in the returned fragment start at zero.
    pub fn slice(&self, range: Range<usize>) -> Option<Self> {
        let text = self.text.get(range.clone())?;
        let first = self.spans.partition_point(|s| s.range.end <= range.start);
        let spans = self.spans[first..]
            .iter()
            .take_while(|s| s.range.start < range.end)
            .filter_map(|s| {
                let a = s.range.start.max(range.start);
                let b = s.range.end.min(range.end);
                (a < b).then(|| StyleSpan::new(a - range.start..b - range.start, s.style.clone()))
            });
        Self::from_spans(text, spans).ok()
    }
}

pub(crate) fn validate_spans(text: &str, spans: &[StyleSpan]) -> render::Result<()> {
    let mut end = 0;
    for span in spans {
        anyhow::ensure!(
            span.range.start >= end && text.get(span.range.clone()).is_some(),
            "invalid or overlapping inline range"
        );
        span.style.validate()?;
        end = span.range.end;
    }
    Ok(())
}

/// Resolve only a requested paragraph, using binary search to skip earlier
/// spans. Metadata never enters the shaping key. Callers validate spans once
/// with `RichText::from_spans` or `EditorLayout::prepare_styled`.
pub fn resolve_runs(
    text: &str,
    spans: &[StyleSpan],
    range: Range<usize>,
    font: &Font,
) -> Vec<TextRun> {
    assert!(
        text.get(range.clone()).is_some(),
        "invalid UTF-8 paragraph range"
    );
    let mut runs: Vec<TextRun> = Vec::new();
    let mut cursor = range.start;
    let first = spans.partition_point(|s| s.range.end <= range.start);
    let mut push = |len, style: &InlineStyle| {
        if len == 0 {
            return;
        }
        let run = style.run(len, font);
        // Equal visual styles can cross metadata boundaries without splitting
        // shaping runs. Document spans still retain their semantic identities.
        if let Some(last) = runs.last_mut() {
            let old_len = last.len;
            last.len = len;
            let equal = *last == run;
            last.len = old_len;
            if equal {
                last.len += len;
                return;
            }
        }
        runs.push(run);
    };
    for span in &spans[first..] {
        if span.range.start >= range.end {
            break;
        }
        let a = span.range.start.max(range.start);
        let b = span.range.end.min(range.end);
        if cursor < a {
            push(a - cursor, &InlineStyle::default());
        }
        push(b - a, &span.style);
        cursor = b;
    }
    if cursor < range.end {
        push(range.end - cursor, &InlineStyle::default());
    }
    runs
}

/// A temporary nested inline builder. Flattening happens once when converted to
/// `RichText`; retained widgets contain no node or allocation per character.
#[derive(Debug, Clone, Default)]
pub struct Inline {
    style: InlineStyle,
    text: SharedString,
    children: Vec<Inline>,
}
pub fn span(content: impl Into<SharedString>) -> Inline {
    Inline::new(content)
}
impl Inline {
    pub fn new(content: impl Into<SharedString>) -> Self {
        Self {
            text: content.into(),
            ..Self::default()
        }
    }
    pub fn child(mut self, child: impl Into<Inline>) -> Self {
        self.children.push(child.into());
        self
    }
    pub fn style(mut self, style: InlineStyle) -> Self {
        self.style = self.style.overlay(&style);
        self
    }
    pub fn bold(self) -> Self {
        self.style(InlineStyle::new().bold())
    }
    pub fn italic(self) -> Self {
        self.style(InlineStyle::new().italic())
    }
    pub fn font_size(self, size: f32) -> Self {
        self.style(InlineStyle::new().font_size(size))
    }
    pub fn color(self, color: impl Into<Hsla>) -> Self {
        self.style(InlineStyle::new().color(color))
    }
    /// Explicitly finish a nested builder, checking public style field values.
    pub fn build(self) -> render::Result<RichText> {
        fn flatten(
            node: Inline,
            inherited: &InlineStyle,
            text: &mut String,
            spans: &mut Vec<StyleSpan>,
        ) -> render::Result<()> {
            node.style.validate()?;
            let style = inherited.overlay(&node.style);
            let start = text.len();
            text.push_str(&node.text);
            if start < text.len() && style != InlineStyle::default() {
                spans.push(StyleSpan::new(start..text.len(), style.clone()));
            }
            for child in node.children {
                flatten(child, &style, text, spans)?;
            }
            Ok(())
        }
        let mut text = String::new();
        let mut spans = Vec::new();
        flatten(self, &InlineStyle::default(), &mut text, &mut spans)?;
        RichText::from_spans(text, spans)
    }
}
impl From<&str> for Inline {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}
impl From<String> for Inline {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}
impl From<SharedString> for Inline {
    fn from(value: SharedString) -> Self {
        Self::new(value)
    }
}
impl From<Inline> for RichText {
    fn from(value: Inline) -> Self {
        value.build().expect("invalid inline content")
    }
}
impl From<&str> for RichText {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}
impl From<String> for RichText {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}
