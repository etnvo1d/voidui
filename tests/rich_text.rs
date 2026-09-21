//! Rich labels use real font shaping without a window or system font service.
use std::{borrow::Cow, sync::Arc};
use voidui::{
    InlineStyle, RichText, StyleSpan,
    core::{
        layout::{Size, TaffyMaxContent},
        text::{PreparedText, TextLayoutOptions},
        widget_tree::WidgetTree,
    },
    div,
    render::{self, ParleyTextSystem, TextAlign, TextLayoutCache, TextSystem, font},
    rich_text, span, text,
};
fn cache() -> TextLayoutCache {
    let backend = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    backend
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(backend))))
}
fn options() -> TextLayoutOptions {
    TextLayoutOptions {
        font: font("IBM Plex Sans"),
        color: render::black(),
        font_size: 16.0,
        line_height: 24.0,
        wrap_width: None,
        line_clamp: None,
    }
}
#[test]
fn nested_inline_inherits_independent_properties_and_coalesces() {
    let content = span("")
        .bold()
        .child("a")
        .child(span("中").italic())
        .child("bc")
        .child("d")
        .build()
        .unwrap();
    assert_eq!(content.text(), "a中bcd");
    assert_eq!(content.spans().len(), 3);
    assert_eq!(content.spans()[0].range, 0..1);
    assert_eq!(content.spans()[1].range, 1..4);
    assert_eq!(content.spans()[2].range, 4..7);
    assert!(content.spans()[1].style.font_weight.is_some());
    assert!(content.spans()[1].style.font_style.is_some());
    assert!(content.spans()[0].style.font.is_none());
}
#[test]
fn malformed_spans_are_rejected_before_shaping() {
    for spans in [
        vec![StyleSpan::new(1..2, InlineStyle::new().bold())],
        vec![
            StyleSpan::new(0..3, InlineStyle::new().bold()),
            StyleSpan::new(0..4, InlineStyle::new().italic()),
        ],
        vec![StyleSpan::new(
            0..4,
            InlineStyle {
                font_size: Some(f32::NAN),
                ..Default::default()
            },
        )],
    ] {
        assert!(RichText::from_spans("中a", spans).is_err());
    }
}
#[test]
fn fragment_offsets_and_metadata_are_self_contained() {
    let content = RichText::from_spans(
        "Hello world",
        [StyleSpan::new(
            6..11,
            InlineStyle::new()
                .bold()
                .metadata("link", "https://example.invalid"),
        )],
    )
    .unwrap();
    let fragment = content.slice(4..9).unwrap();
    assert_eq!(fragment.text(), "o wor");
    assert_eq!(fragment.spans()[0].range, 2..5);
    assert_eq!(
        fragment.spans()[0]
            .style
            .metadata
            .as_ref()
            .unwrap()
            .get("link")
            .unwrap()
            .as_str(),
        "https://example.invalid"
    );
}
#[test]
fn spans_share_a_line_and_native_selection_context() {
    let cache = cache();
    let content: RichText = span("Hello ").child(span("world").bold()).child("!").into();
    let mut prepared = PreparedText::shape_rich(&cache, &content, &options()).unwrap();
    assert_eq!(prepared.paragraph().line_count(), 1);
    assert_eq!(prepared.source(), "Hello world!");
    let rects = prepared
        .paragraph()
        .selection_rectangles(0..12, 500.0, TextAlign::Left);
    assert_eq!(rects.len(), 1);
    let before = cache.system().stats().paragraphs_shaped;
    prepared.reflow(Some(50.0));
    assert!(prepared.paragraph().line_count() > 1);
    prepared.reflow(None);
    assert_eq!(prepared.paragraph().line_count(), 1);
    assert_eq!(cache.system().stats().paragraphs_shaped, before);
}
#[test]
fn rich_widget_does_not_pollute_uniform_weak_cache() {
    let cache = cache();
    let content = span("one ").child(span("two").font_size(32.0));
    let mut tree = WidgetTree::new();
    tree.build_root(
        div()
            .display(voidui::core::layout::Display::Flex)
            .font("IBM Plex Sans")
            .child(rich_text(content).id("rich"))
            .child(text("one two").id("plain")),
    );
    tree.layout(Size::MAX_CONTENT, &cache);
    let rich = tree.find_by_id("rich").unwrap();
    let plain = tree.find_by_id("plain").unwrap();
    assert!(tree.bounds(rich).size.width > tree.bounds(plain).size.width);
    let before = cache.system().stats().paragraphs_shaped;
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_eq!(cache.system().stats().paragraphs_shaped, before);
}
#[test]
fn semantic_boundaries_do_not_split_identical_visual_runs() {
    let content = RichText::from_spans(
        "ab",
        [
            StyleSpan::new(0..1, InlineStyle::new().metadata("id", "a")),
            StyleSpan::new(1..2, InlineStyle::new().metadata("id", "b")),
        ],
    )
    .unwrap();
    let runs = voidui::core::rich_text::resolve_runs(
        content.text(),
        content.spans(),
        0..2,
        &font("IBM Plex Sans"),
    );
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].len, 2);
}

#[test]
fn public_style_fields_cannot_publish_invalid_render_inputs() {
    let invalid = [
        InlineStyle {
            color: Some(render::Hsla {
                h: f32::NAN,
                s: 0.0,
                l: 0.0,
                a: 1.0,
            }),
            ..Default::default()
        },
        InlineStyle {
            underline: Some(render::UnderlineStyle {
                thickness: render::px(f32::INFINITY),
                ..Default::default()
            }),
            ..Default::default()
        },
        InlineStyle::new().font(render::Font {
            weight: render::FontWeight(f32::NAN),
            ..render::font("IBM Plex Sans")
        }),
    ];
    for style in invalid {
        assert!(RichText::from_spans("x", [StyleSpan::new(0..1, style)]).is_err());
    }
    let content = span("a")
        .child(span("中").style(InlineStyle::new().metadata("id", "word")))
        .build()
        .unwrap();
    assert!(content.span_at(0).is_none());
    assert!(content.span_at(1).unwrap().style.metadata.is_some());
    assert!(content.span_at(2).is_none());
    assert!(content.span_at(4).is_none());
}

#[test]
fn identical_rich_labels_share_shaping_and_rejoin_after_intrinsic_probes() {
    let cache = cache();
    let content = span("shared ").child(span("bold").bold()).build().unwrap();
    let mut tree = WidgetTree::new();
    let mut root = div().font("IBM Plex Sans");
    for _ in 0..100 {
        root = root.child(rich_text(content.clone()));
    }
    tree.build_root(root);
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_eq!(cache.system().stats().paragraphs_shaped, 1);
    tree.layout(
        Size {
            width: voidui::core::layout::AvailableSpace::Definite(70.0),
            height: voidui::core::layout::AvailableSpace::MaxContent,
        },
        &cache,
    );
    assert_eq!(cache.system().stats().paragraphs_shaped, 1);
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_eq!(cache.system().stats().paragraphs_shaped, 1);
}
