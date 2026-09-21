//! Run `cargo run --example editor_extensions`. The example composes extensions
//! instead of adding Markdown, image or table branches to the editing engine.
use std::ops::Range;
use voidui::editing::Selection;
use voidui::{core::geometry::Size, editing::*, *};

struct MarkdownView;
impl EditorExtension for MarkdownView {
    fn project(&mut self, cx: ExtensionContext<'_>) -> Result<Projection, EditError> {
        let source = cx.snapshot.text.read(0..cx.snapshot.text.len()).unwrap();
        let mut out = Projection::new();
        let mut id = 0;
        let mut offset = 0;
        let lines: Vec<_> = source.split_inclusive('\n').collect();
        let mut table: Option<Range<usize>> = None;
        for line in lines {
            let range = offset..offset + line.len();
            if line.contains('|') {
                if let Some(table) = &mut table {
                    table.end = range.end;
                } else {
                    table = Some(range.clone());
                }
            }
            if line.starts_with("# ") {
                out = out.style(StyleSpan::new(
                    range.clone(),
                    InlineStyle::new().bold().font_size(28.0).line_height(40.0),
                ));
                out.replacements
                    .push(Replacement::hide(ViewId(id), offset..offset + 2).reveal_on_selection());
                id += 1;
            }
            if line.starts_with("> ") {
                out.paragraphs.push(BlockStyle {
                    range: range.clone(),
                    style: ParagraphStyle {
                        inset_left: 18.0,
                        space_before: 8.0,
                        space_after: 8.0,
                        leading_rule: Some((3.0, render::rgb(0x8877bb).into())),
                        ..Default::default()
                    },
                });
                out.replacements
                    .push(Replacement::hide(ViewId(id), offset..offset + 2).reveal_on_selection());
                id += 1;
            }
            if line.starts_with("- ") {
                out.paragraphs.push(BlockStyle {
                    range: range.clone(),
                    style: ParagraphStyle {
                        inset_left: 18.0,
                        first_line_indent: -14.0,
                        ..Default::default()
                    },
                });
            }
            offset += line.len();
        }
        if let Some(table) = table {
            out.blocks.push(BlockView::new(ViewId(10000), table));
        }
        if let Some(at) = source.find("[image]") {
            out.replacements
                .push(Replacement::object(ViewId(10001), at..at + 7));
        }
        if let Some(at) = source.find("[button]") {
            out.replacements
                .push(Replacement::object(ViewId(10002), at..at + 8));
        }
        let mut cursor = 0;
        while let Some(a) = source[cursor..].find("**") {
            let a = cursor + a;
            let Some(end) = source[a + 2..].find("**") else {
                break;
            };
            let z = a + 2 + end;
            let range = a..z + 2;
            out.styles
                .push(StyleSpan::new(a + 2..z, InlineStyle::new().bold()));
            if !cx
                .snapshot
                .selections
                .iter()
                .any(|s| s.text_range().start <= range.end && s.text_range().end >= range.start)
            {
                out.replacements
                    .push(Replacement::hide(ViewId(id), a..a + 2));
                id += 1;
                out.replacements
                    .push(Replacement::hide(ViewId(id), z..z + 2));
                id += 1;
            }
            cursor = z + 2;
        }
        Ok(out)
    }
}
struct Table;
impl BlockLayout for Table {
    fn layout(
        &self,
        source: Range<usize>,
        m: &mut dyn BlockMeasure,
    ) -> render::Result<BlockArrangement> {
        let text = m.source().read(source.clone()).unwrap().into_owned();
        let mut rows = Vec::new();
        let mut offset = source.start;
        for line in text.split_inclusive('\n') {
            let mut cells = Vec::new();
            let mut at = offset;
            for cell in line.trim_end_matches('\n').split('|') {
                cells.push(at..at + cell.len());
                at += cell.len() + 1;
            }
            rows.push(cells);
            offset += line.len();
        }
        let columns = rows.iter().map(Vec::len).max().unwrap_or(1);
        for row in &mut rows {
            while row.len() < columns {
                row.push(source.end..source.end);
            }
        }
        GridBlock {
            rows,
            column_weights: vec![1.0; columns],
            padding: 8.0,
            gap: 1.0,
            rule: Some((1.0, render::rgb(0xccccdd).into())),
        }
        .layout(source, m)
    }
}
fn main() -> anyhow::Result<()> {
    let editor = Editor::new(
        "# Extensible editing\nClick inside **these words** to reveal their markers.\n> Quotes use block decorations.\n- Hanging indentation belongs to paragraph layout, including lines that wrap inside a narrow viewport.\nInline image [image] and widget [button] stay in the text flow.\n\nName|Value\nStorage|Shared rope\nLayout|Viewport cache\n\nTab moves between table cells. Undo edits source text, not the presentation.\n",
    );
    editor.update(|s| s.select(SelectionSet::single(Selection::caret(s.document().len()))))?;
    let pixel = media::Image::from_rgba(1, 1, vec![110, 90, 190, 255])?;
    let views = EditorViews::new()
        .block(ViewId(10000), Table)
        .register(ViewId(10001), move || {
            let image = pixel.clone();
            Box::new(WidgetView::new(move || {
                img(image.clone()).width(32.0).height(32.0).into_element()
            }))
        })
        .register(ViewId(10002), || {
            Box::new(WidgetView::new(|| {
                div()
                    .tag("button")
                    .background(style::color::Rgba8::from_rgb8(225, 220, 245))
                    .padding(6.0)
                    .child("Run")
                    .on_click(|| println!("Embedded widget activated"))
                    .into_element()
            }))
        });
    let root = div()
        .padding(24.0)
        .child(text("Editor extensions").font_size(30.0))
        .child(
            rich_editor(&editor)
                .id("editor")
                .extensions(
                    EditorExtensions::new().register(ViewId(1), 0, || Box::new(MarkdownView)),
                )
                .views(views)
                .width(700.0)
                .height(520.0),
        );
    Application::new().css("*{font-family:system-ui;font-size:17px;line-height:26px} textarea{padding:16px;border:1px solid #ccc;background:#fafafa;caret-animation:manual}")?.window(WindowOptions{title:"voidui — editor extensions".into(),size:Size::new(770.0,640.0),..Default::default()},root).run()
}
