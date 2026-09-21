//! Reproduce Parley 0.11.1 line-height behavior using only its public APIs.
//! Run with `cargo test -p voidui_gpui_wgpu --test parley_line_height -- --nocapture`.
use parley::{
    FontContext, FontFamily, FontFeature, FontWeight, InlineBox, InlineBoxKind, Layout,
    LayoutContext, LineHeight, StyleProperty, TextStyle,
};

#[derive(Clone, Copy, Debug)]
enum Builder {
    Indexed,
    Ranged,
}

#[derive(Clone, Copy, Debug)]
enum Split {
    None,
    Features,
    Synthesis,
    InlineBox,
}

fn shape(builder: Builder, sizes: [f32; 3], heights: [LineHeight; 3], split: Split) -> Layout<u8> {
    let text = "a\nb\nc";
    let ranges = [0..2, 2..4, 4..5];
    let features = [FontFeature::new(parley::setting::Tag::new(b"liga"), 0)];
    let mut fonts = FontContext {
        collection: parley::fontique::Collection::new(parley::fontique::CollectionOptions {
            system_fonts: false,
            shared: false,
        }),
        source_cache: Default::default(),
    };
    fonts.collection.register_fonts(
        include_bytes!("fonts/IBMPlexSans-Regular.ttf")
            .as_slice()
            .to_vec()
            .into(),
        None,
    );
    let mut context = LayoutContext::new();
    let styles: Vec<_> = (0..3)
        .map(|i| TextStyle {
            font_family: FontFamily::from("IBM Plex Sans"),
            font_size: sizes[i],
            line_height: heights[i],
            font_features: if matches!(split, Split::Features) && i == 1 {
                features.as_slice().into()
            } else {
                (&[] as &[FontFeature]).into()
            },
            font_weight: FontWeight::new(if matches!(split, Split::Synthesis) && i == 1 {
                700.0
            } else {
                400.0
            }),
            brush: i as u8,
            ..Default::default()
        })
        .collect();
    let boxes = [2, 4].map(|index| InlineBox {
        id: index as u64,
        index,
        kind: InlineBoxKind::OutOfFlow,
        width: 0.0,
        height: 0.0,
    });
    let mut layout = match builder {
        Builder::Indexed => {
            let mut builder = context.style_run_builder(&mut fonts, text, 1.0, false);
            for (style, range) in styles.into_iter().zip(ranges) {
                let index = builder.push_style(style);
                builder.push_style_run(index, range);
            }
            if matches!(split, Split::InlineBox) {
                for inline_box in boxes {
                    builder.push_inline_box(inline_box);
                }
            }
            builder.build(text)
        }
        Builder::Ranged => {
            let mut builder = context.ranged_builder(&mut fonts, text, 1.0, false);
            for (style, range) in styles.into_iter().zip(ranges) {
                builder.push(StyleProperty::FontFamily(style.font_family), range.clone());
                builder.push(StyleProperty::FontSize(style.font_size), range.clone());
                builder.push(StyleProperty::LineHeight(style.line_height), range.clone());
                builder.push(
                    StyleProperty::FontFeatures(style.font_features),
                    range.clone(),
                );
                builder.push(StyleProperty::FontWeight(style.font_weight), range.clone());
                builder.push(StyleProperty::Brush(style.brush), range);
            }
            if matches!(split, Split::InlineBox) {
                for inline_box in boxes {
                    builder.push_inline_box(inline_box);
                }
            }
            builder.build(text)
        }
    };
    layout.break_all_lines(None);
    layout
}

fn heights(layout: &Layout<u8>) -> Vec<f32> {
    layout
        .lines()
        .map(|line| line.metrics().line_height)
        .collect()
}

fn report(label: &str, builder: Builder, split: Split, layout: &Layout<u8>) {
    println!(
        "{label}: builder={builder:?} split={split:?} line_heights={:?}",
        heights(layout)
    );
    for line in layout.lines() {
        for run in line.runs() {
            println!(
                "  bytes={:?} font_size={} run_height={} synthesis={:?}",
                run.text_range(),
                run.font_size(),
                run.metrics().line_height,
                run.synthesis()
            );
        }
    }
}

#[test]
fn builder_and_segmentation_matrix_reproduces_native_absolute_height_defect() {
    for builder in [Builder::Indexed, Builder::Ranged] {
        for (label, sizes, expected_unsplit) in [
            ("height-only", [16.0; 3], vec![20.0; 3]),
            (
                "size-and-height",
                [16.0, 32.0, 12.0],
                vec![48.0, 20.0, 20.0],
            ),
        ] {
            for split in [
                Split::None,
                Split::Features,
                Split::Synthesis,
                Split::InlineBox,
            ] {
                let layout = shape(
                    builder,
                    sizes,
                    [24.0, 48.0, 20.0].map(LineHeight::Absolute),
                    split,
                );
                report(label, builder, split, &layout);
                let expected =
                    if sizes == [16.0; 3] && matches!(split, Split::Features | Split::InlineBox) {
                        vec![48.0, 20.0, 20.0]
                    } else {
                        expected_unsplit.clone()
                    };
                assert_eq!(heights(&layout), expected);
            }
        }
    }
}

#[test]
fn uniform_absolute_height_and_uniform_relative_multiplier_are_unaffected() {
    for builder in [Builder::Indexed, Builder::Ranged] {
        for (label, heights_spec, expected) in [
            (
                "uniform-absolute",
                [LineHeight::Absolute(24.0); 3],
                vec![24.0; 3],
            ),
            (
                "uniform-relative",
                [LineHeight::FontSizeRelative(1.5); 3],
                vec![24.0, 48.0, 18.0],
            ),
            (
                "varying-relative",
                [1.5, 1.5, 20.0 / 12.0].map(LineHeight::FontSizeRelative),
                vec![24.0, 32.0 * (20.0 / 12.0), 20.0],
            ),
        ] {
            let layout = shape(builder, [16.0, 32.0, 12.0], heights_spec, Split::None);
            report(label, builder, Split::None, &layout);
            assert_eq!(heights(&layout), expected);
        }
    }
}
