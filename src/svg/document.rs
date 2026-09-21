//! Immutable SVG source graphs, XML import and attribute-preserving serialization.
use super::SvgNode;
use crate::style::{css::Stylesheet, declaration::Declaration, style::Style};
use anyhow::{Result, ensure};
use std::rc::Rc;
use winit::keyboard::SmolStr;

pub(crate) const NONE: u32 = u32::MAX;
#[derive(Clone, Debug)]
pub(crate) struct DocNode {
    pub tag: SmolStr,
    pub attrs: Vec<(SmolStr, SmolStr)>,
    pub text: SmolStr,
    pub tail: SmolStr,
    pub parent: u32,
    pub first: u32,
    pub last: u32,
    pub next: u32,
    pub previous: u32,
    pub presentation: crate::style::list::StyleList<Declaration>,
    pub inline: crate::style::list::StyleList<(Declaration, bool)>,
}
impl DocNode {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
    pub fn base_style(&self) -> Style {
        let mut style = Style::default();
        for d in self.presentation.iter() {
            d.apply(&mut style);
        }
        style
    }
}
#[derive(Clone, Debug)]
struct DocumentData {
    nodes: Vec<DocNode>,
    style_sources: Vec<String>,
    limits: crate::media::MediaLimits,
    disabled: bool,
}
/// Cloneable parsed input. Reuse a document across components to avoid rebuilding
/// graph descriptions; styles and rasterizations remain specific to each view.
#[derive(Clone, Debug)]
pub struct SvgDocument(Rc<DocumentData>);
impl SvgDocument {
    pub fn parse(source: &str) -> Result<Self> {
        Self::parse_with_limits(source, crate::media::MediaLimits::default())
    }
    pub fn parse_with_limits(source: &str, limits: crate::media::MediaLimits) -> Result<Self> {
        ensure!(
            source.len() <= limits.max_input_bytes,
            "SVG source exceeds MediaLimits"
        );
        let xml = resvg::usvg::roxmltree::Document::parse(source)?;
        ensure!(
            xml.descendants().count() <= limits.max_svg_nodes,
            "SVG node count exceeds MediaLimits"
        );
        ensure!(
            xml.root_element().tag_name().name() == "svg",
            "SVG document needs an svg root"
        );
        fn convert(
            node: resvg::usvg::roxmltree::Node<'_, '_>,
            depth: usize,
            limits: crate::media::MediaLimits,
        ) -> Result<SvgNode> {
            ensure!(
                depth <= limits.max_svg_depth,
                "SVG nesting exceeds MediaLimits"
            );
            let mut out = SvgNode::new(node.tag_name().name());
            for attr in node.attributes() {
                let name = if attr.namespace() == Some("http://www.w3.org/1999/xlink") {
                    format!("xlink:{}", attr.name())
                } else {
                    attr.name().to_owned()
                };
                out = out.attr(name, attr.value());
            }
            for child in node.children() {
                if child.is_element() {
                    out.children.push(convert(child, depth + 1, limits)?);
                } else if child.is_text()
                    && let Some(text) = child.text()
                {
                    let slot = out
                        .children
                        .last_mut()
                        .map(|c| &mut c.tail)
                        .unwrap_or(&mut out.text);
                    *slot = format!("{slot}{text}").into();
                }
            }
            Ok(out)
        }
        Self::from_node_with_limits(convert(xml.root_element(), 0, limits)?, limits)
    }
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let bytes = crate::media::MediaLimits::default().read(path.as_ref())?;
        Self::parse(std::str::from_utf8(&bytes)?)
    }
    pub fn from_node(node: SvgNode) -> Result<Self> {
        Self::from_node_with_limits(node, crate::media::MediaLimits::default())
    }
    pub fn from_node_with_limits(node: SvgNode, limits: crate::media::MediaLimits) -> Result<Self> {
        ensure!(node.tag == "svg", "SVG document needs an svg root");
        let mut data = DocumentData {
            nodes: Vec::new(),
            style_sources: Vec::new(),
            limits,
            disabled: false,
        };
        append(&mut data, node, NONE, NONE, 0)?;
        data.disabled = disabled_viewbox(data.nodes[0].attr("viewBox"));
        Ok(Self(Rc::new(data)))
    }
    pub fn node_count(&self) -> usize {
        self.0.nodes.len()
    }
    pub(crate) fn node(&self, i: u32) -> &DocNode {
        &self.0.nodes[i as usize]
    }
    pub(crate) fn same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
            || (self.0.nodes.len() == other.0.nodes.len()
                && self.0.nodes.iter().zip(&other.0.nodes).all(|(a, b)| {
                    a.tag == b.tag
                        && a.attrs == b.attrs
                        && a.text == b.text
                        && a.tail == b.tail
                        && a.parent == b.parent
                }))
    }
    pub(super) fn disabled(&self) -> bool {
        self.0.disabled
    }
    pub(super) fn root_attr(&mut self, name: &str, value: String) {
        if name == "viewBox" {
            Rc::make_mut(&mut self.0).disabled = disabled_viewbox(Some(&value));
        }
        let node = &mut Rc::make_mut(&mut self.0).nodes[0];
        if let Some((_, v)) = node.attrs.iter_mut().find(|(n, _)| n == name) {
            *v = value.into();
        } else {
            node.attrs.push((name.into(), value.into()));
        }
    }
    pub(super) fn append_child(&mut self, node: SvgNode) -> Result<bool> {
        let data = Rc::make_mut(&mut self.0);
        let previous = data.nodes[0].last;
        let sheets = data.style_sources.len();
        let child = append(data, node, 0, previous, 1)?;
        if previous == NONE {
            data.nodes[0].first = child;
        } else {
            data.nodes[previous as usize].next = child;
        }
        data.nodes[0].last = child;
        Ok(sheets != data.style_sources.len())
    }
    pub(crate) fn stylesheets(&self) -> Vec<Stylesheet> {
        self.0
            .style_sources
            .iter()
            .map(|s| Stylesheet::parse(s).expect("document CSS was validated during parsing"))
            .collect()
    }
    pub(super) fn intrinsic_size(&self) -> [f32; 2] {
        let root = self.node(0);
        let view = root
            .attr("viewBox")
            .and_then(|s| s.parse::<svgtypes::ViewBox>().ok());
        let length = |name: &str| {
            root.attr(name)
                .and_then(|s| s.parse::<svgtypes::Length>().ok())
                .filter(|v| {
                    matches!(
                        v.unit,
                        svgtypes::LengthUnit::None | svgtypes::LengthUnit::Px
                    ) && v.number > 0.
                })
                .map(|v| v.number as f32)
        };
        let ratio = view
            .map(|v| v.w as f32 / v.h as f32)
            .filter(|v| v.is_finite() && *v > 0.);
        match (length("width"), length("height"), ratio) {
            (Some(w), Some(h), _) => [w, h],
            (Some(w), _, Some(r)) => [w, w / r],
            (_, Some(h), Some(r)) => [h * r, h],
            (w, h, _) => [
                w.unwrap_or_else(|| view.map_or(300., |v| v.w as f32)),
                h.unwrap_or_else(|| view.map_or(150., |v| v.h as f32)),
            ],
        }
    }
    /// Serialize the document with escaped attributes and text.
    pub fn to_xml(&self) -> String {
        let mut out = String::new();
        self.write_node(0, &mut out, None, None);
        out
    }
    pub(crate) fn write_node(
        &self,
        id: u32,
        out: &mut String,
        styles: Option<&[crate::style::css::sheet::SvgStyle]>,
        viewport: Option<[f32; 2]>,
    ) {
        let node = self.node(id);
        if styles.is_some() && node.tag == "style" {
            return;
        }
        out.push('<');
        out.push_str(&node.tag);
        if id == 0 {
            out.push_str(" xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"");
        }
        for (name, value) in &node.attrs {
            if styles.is_some_and(|s| s[id as usize].attributes.iter().any(|(n, _)| n == name))
                || name.starts_with("xmlns")
                || (styles.is_some() && name == "style")
                || (id == 0 && viewport.is_some() && matches!(name.as_str(), "width" | "height"))
            {
                continue;
            }
            attribute(out, name, value);
        }
        if id == 0
            && let Some([w, h]) = viewport
        {
            attribute(out, "width", &w.to_string());
            attribute(out, "height", &h.to_string());
        }
        if let Some(styles) = styles {
            for (name, value) in &styles[id as usize].attributes {
                attribute(out, name, value);
            }
            attribute(out, "style", &styles[id as usize].css);
        }
        out.push('>');
        escape(out, &node.text);
        let mut child = node.first;
        while child != NONE {
            self.write_node(child, out, styles, None);
            escape(out, &self.node(child).tail);
            child = self.node(child).next;
        }
        out.push_str("</");
        out.push_str(&node.tag);
        out.push('>');
    }
}
fn append(
    data: &mut DocumentData,
    node: SvgNode,
    parent: u32,
    previous: u32,
    depth: usize,
) -> Result<u32> {
    ensure!(
        depth <= data.limits.max_svg_depth && data.nodes.len() < data.limits.max_svg_nodes,
        "SVG graph exceeds node/depth limit"
    );
    ensure!(
        !node.tag.is_empty()
            && node
                .tag
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "invalid SVG element name"
    );
    let mut presentation = Vec::new();
    let mut inline = Vec::new();
    for (name, value) in &node.attrs {
        ensure!(
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':')),
            "invalid SVG attribute name"
        );
        if name == "style" {
            inline = crate::style::css::parser::parse_inline(value)?;
        } else {
            presentation.extend(super::presentation::parse(name, value));
        }
    }
    if node.tag == "style" {
        Stylesheet::parse(&node.text)?;
        data.style_sources.push(node.text.to_string());
    }
    let id = data.nodes.len() as u32;
    data.nodes.push(DocNode {
        tag: node.tag,
        attrs: node.attrs,
        text: node.text,
        tail: node.tail,
        parent,
        previous,
        next: NONE,
        first: NONE,
        last: NONE,
        presentation: presentation.into(),
        inline: inline.into(),
    });
    let mut prev = NONE;
    for child in node.children {
        let next = append(data, child, id, prev, depth + 1)?;
        if prev == NONE {
            data.nodes[id as usize].first = next;
        } else {
            data.nodes[prev as usize].next = next;
        }
        data.nodes[id as usize].last = next;
        prev = next;
    }
    Ok(id)
}
pub(crate) fn escape(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}
fn attribute(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    escape(out, value);
    out.push('"');
}

fn disabled_viewbox(raw: Option<&str>) -> bool {
    let Some(raw) = raw else {
        return false;
    };
    let values = svgtypes::NumberListParser::from(raw).collect::<Result<Vec<_>, _>>();
    matches!(values.as_deref(),Ok([_,_,w,h]) if *w==0. || *h==0.)
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_geometry_and_metadata_are_preserved_without_css_declarations() {
        let document = super::SvgDocument::parse(
            r#"<svg><path id="glyph" d="M0 0H10V10H0Z" transform="translate(2 3)" data-label="var(--literal)"/></svg>"#,
        ).unwrap();
        let path = document.node(1);
        assert!(path.presentation.is_empty());
        assert_eq!(path.attr("d"), Some("M0 0H10V10H0Z"));
        let roundtrip = super::SvgDocument::parse(&document.to_xml()).unwrap();
        assert_eq!(roundtrip.node(1).attrs, path.attrs);
    }

    #[test]
    fn svg_nodes_do_not_carry_widget_state() {
        let compact = std::mem::size_of::<super::DocNode>();
        let widget = std::mem::size_of::<crate::core::widget_tree::Node>();
        assert!(
            compact * 4 < widget,
            "SVG node: {compact}, widget: {widget}"
        );
    }
}
