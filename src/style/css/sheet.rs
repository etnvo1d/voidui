use super::{
    parser::CssError,
    selector::{WidgetElement, WidgetPseudoElement, WidgetSelectors},
};
use crate::{
    core::{widget::WidgetId, widget_tree::WidgetTree},
    style::{declaration::Declaration, style::Style},
};
use selectors::{
    context::{
        MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, QuirksMode,
        SelectorCaches,
    },
    matching::matches_selector,
    parser::{Component, Selector},
};
use std::{collections::HashMap, rc::Rc};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Rule {
    pub selector: Selector<WidgetSelectors>,
    pub declarations: Rc<[(Declaration, bool)]>,
}

#[derive(Debug, Clone)]
pub struct Stylesheet(Rc<CompiledSheet>);
#[derive(Debug)]
struct CompiledSheet {
    rules: Vec<Rule>,
    normal: RuleIndex,
    backdrop: RuleIndex,
    selection: RuleIndex,
    placeholder: RuleIndex,
    uses_state: u8,
}

/// Separate indexes for elements and each stateless pseudo keep highlight rules
/// out of normal matching, without scanning every selection rule for every node.
#[derive(Debug, Default)]
struct RuleIndex {
    ids: HashMap<String, Vec<usize>>,
    classes: HashMap<String, Vec<usize>>,
    tags: HashMap<String, Vec<usize>>,
    universal: Vec<usize>,
}
impl RuleIndex {
    fn insert(&mut self, rule: &Rule, index: usize) {
        let mut iter = rule.selector.iter();
        if rule.selector.has_pseudo_element() {
            for _ in iter.by_ref() {}
            iter.next_sequence();
        }
        let (mut id, mut class, mut tag) = (None, None, None);
        for component in iter {
            match component {
                Component::ID(v) => id = Some(v.as_ref().to_owned()),
                Component::Class(v) => class = Some(v.as_ref().to_owned()),
                Component::LocalName(v) => tag = Some(v.lower_name.as_ref().to_owned()),
                _ => {}
            }
        }
        if let Some(key) = id {
            self.ids.entry(key).or_default().push(index);
        } else if let Some(key) = class {
            self.classes.entry(key).or_default().push(index);
        } else if let Some(key) = tag {
            self.tags.entry(key).or_default().push(index);
        } else {
            self.universal.push(index);
        }
    }
    fn for_each(&self, element: WidgetElement<'_>, mut visit: impl FnMut(usize)) {
        for &id in &self.universal {
            visit(id);
        }
        if let Some(list) = element.id_value().and_then(|id| self.ids.get(id)) {
            for &id in list {
                visit(id);
            }
        }
        let tag = element.tag();
        let folded = tag
            .bytes()
            .any(|c| c.is_ascii_uppercase())
            .then(|| tag.to_ascii_lowercase());
        if let Some(list) = self.tags.get(folded.as_deref().unwrap_or(tag)) {
            for &id in list {
                visit(id);
            }
        }
        for class in element.classes() {
            if let Some(list) = self.classes.get(class) {
                for &id in list {
                    visit(id);
                }
            }
        }
    }
    fn is_empty(&self) -> bool {
        self.ids.is_empty()
            && self.classes.is_empty()
            && self.tags.is_empty()
            && self.universal.is_empty()
    }
}

/// Cumulative cascade work. Unchanged layouts/exposures do not increase these counters.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CascadeStats {
    pub passes: u64,
    pub candidate_tests: u64,
    pub matched_selectors: u64,
}

impl Stylesheet {
    pub fn parse(source: &str) -> Result<Self, CssError> {
        super::parser::parse(source)
    }
    pub fn from_file(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        Ok(Self::parse(&std::fs::read_to_string(path)?)?)
    }
    pub fn rule_count(&self) -> usize {
        self.0.rules.len()
    }
    pub fn uses_state(&self) -> bool {
        self.0.uses_state != 0
    }
    pub(crate) fn state_mask(&self) -> crate::core::widget::WidgetStatus {
        crate::core::widget::WidgetStatus::from_bits_retain(self.0.uses_state)
    }
    pub(crate) fn same_rules(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0) || self.0.rules == other.0.rules
    }
    pub(crate) fn compile(rules: Vec<Rule>, uses_state: u8) -> Self {
        let mut sheet = CompiledSheet {
            rules,
            normal: Default::default(),
            backdrop: Default::default(),
            selection: Default::default(),
            placeholder: Default::default(),
            uses_state,
        };
        for (index, rule) in sheet.rules.iter().enumerate() {
            match rule.selector.pseudo_element() {
                Some(WidgetPseudoElement::Backdrop) => sheet.backdrop.insert(rule, index),
                Some(WidgetPseudoElement::Selection) => sheet.selection.insert(rule, index),
                Some(WidgetPseudoElement::Placeholder) => sheet.placeholder.insert(rule, index),
                None => sheet.normal.insert(rule, index),
            }
        }
        Self(Rc::new(sheet))
    }
}

/// A full invalidation is conservative for relational/sibling selectors. Candidate
/// indexing and Servo's per-pass nth/:has caches avoid scanning unrelated rules.
pub(crate) fn cascade(
    tree: &WidgetTree,
    sheets: &[Stylesheet],
    stats: &mut CascadeStats,
) -> Vec<(WidgetId, Style)> {
    cascade_nodes(tree, sheets, tree.nodes.keys(), stats)
}

/// Inline style edits cannot change selector matches on other elements. Reapply
/// the normal/inline/important cascade on the edited nodes only, including removed
/// declarations, rather than overlaying values onto an obsolete computed style.
pub(crate) fn cascade_nodes(
    tree: &WidgetTree,
    sheets: &[Stylesheet],
    ids: impl IntoIterator<Item = WidgetId>,
    stats: &mut CascadeStats,
) -> Vec<(WidgetId, Style)> {
    stats.passes += 1;
    let initial = Style::default();
    let mut caches = SelectorCaches::default();
    let mut matches = Vec::new();
    let ids = ids.into_iter();
    let mut output = Vec::with_capacity(ids.size_hint().0);
    for id in ids {
        matches.clear();
        let local = tree.nodes[id]
            .widget
            .as_ref()
            .map(|w| w.svg_stylesheets())
            .unwrap_or(&[]);
        let sheet_at = |i: usize| {
            if i < sheets.len() {
                &sheets[i]
            } else {
                &local[i - sheets.len()]
            }
        };

        let element = WidgetElement {
            tree,
            id,
            svg_index: None,
        };
        let mut context = MatchingContext::new(
            MatchingMode::Normal,
            None,
            &mut caches,
            QuirksMode::NoQuirks,
            NeedsSelectorFlags::No,
            MatchingForInvalidation::No,
        );
        for (sheet_index, sheet) in sheets.iter().chain(local).enumerate() {
            let sheet = &sheet.0;
            sheet.normal.for_each(element, |rule_index| {
                let rule = &sheet.rules[rule_index];
                stats.candidate_tests += 1;
                if matches_selector(&rule.selector, 0, None, &element, &mut context) {
                    stats.matched_selectors += 1;
                    matches.push((rule.selector.specificity(), sheet_index, rule_index));
                }
            });
        }
        matches.sort_unstable();
        let mut style = user_agent_style(tree, id);
        for &(_, s, r) in &matches {
            for (decl, important) in sheet_at(s).0.rules[r].declarations.iter() {
                if !important {
                    decl.apply(&mut style);
                }
            }
        }
        style.overlay_inline(&tree.nodes[id].props.style, &initial);
        for &(_, s, r) in &matches {
            for (decl, important) in sheet_at(s).0.rules[r].declarations.iter() {
                if *important {
                    decl.apply(&mut style);
                }
            }
        }
        if let Some(doc) = element.document() {
            for (decl, important) in doc.node(0).inline.iter() {
                if *important {
                    decl.apply(&mut style);
                }
            }
        }
        output.push((id, style));
    }
    output
}

/// Minimal HTML-like UA rules. These are defaults, below author and inline CSS;
/// there is no magic class or author property that inserts an element on top.
pub(crate) fn user_agent_style(tree: &WidgetTree, id: WidgetId) -> Style {
    use crate::{
        core::{layout::*, top_layer::TopLayerKind},
        style::{color::Rgba8, layer::Position as CssPosition},
    };
    let node = &tree.nodes[id];
    let mut style = node
        .widget
        .as_ref()
        .and_then(|w| w.default_style())
        .unwrap_or_default();
    if matches!(
        node.props.tag.as_str(),
        "button" | "meter" | "progress" | "select"
    ) {
        style.user_select = crate::style::selection::UserSelect::None.into();
    }
    let dialog = node.props.tag == "dialog";
    let popover = node.props.attributes.contains_key("popover");
    if dialog || popover {
        let open = node.top_layer == Some(TopLayerKind::Popover)
            || (dialog && node.props.attributes.contains_key("open"));
        if !open {
            style.layout.display = Display::None;
        }
        style.position = CssPosition::Fixed.into();
        style.layout.inset = Rect {
            left: LengthPercentageAuto::length(0.0),
            right: LengthPercentageAuto::length(0.0),
            top: LengthPercentageAuto::length(0.0),
            bottom: LengthPercentageAuto::length(0.0),
        };
        style.layout.margin = Rect {
            left: LengthPercentageAuto::auto(),
            right: LengthPercentageAuto::auto(),
            top: LengthPercentageAuto::auto(),
            bottom: LengthPercentageAuto::auto(),
        };
        style.layout.size = Size {
            width: taffy::Dimension::fit_content(),
            height: taffy::Dimension::fit_content(),
        };
        // Author CSS can completely restyle the ordinary element, including none.
        style.background = Rgba8::from_rgb8(255, 255, 255).into();
        style.color = Rgba8::from_rgb8(0, 0, 0).into();
    }
    style
}

/// ::backdrop is matched against its originating element using Servo's stateless
/// pseudo-element mode. It has independent initial values and no inline styles or
/// inheritance from the element; ordinary rules cannot leak into the pseudo box.
pub(crate) fn cascade_backdrop(tree: &WidgetTree, id: WidgetId, sheets: &[Stylesheet]) -> Style {
    use crate::core::layout::*;
    use crate::style::{color::Rgba8, layer::Position as CssPosition};
    let mut style = crate::style::style::Style::default();
    style.position = CssPosition::Fixed.into();
    style.layout.inset = Rect {
        left: LengthPercentageAuto::length(0.0),
        right: LengthPercentageAuto::length(0.0),
        top: LengthPercentageAuto::length(0.0),
        bottom: LengthPercentageAuto::length(0.0),
    };
    if tree.top_layer_kind(id) == Some(crate::core::top_layer::TopLayerKind::Modal) {
        style.background = Rgba8::new(0, 0, 0, 26).into();
    }
    cascade_pseudo_declarations(tree, id, sheets, |s| &s.backdrop, &mut style);
    // HTML defines this as a UA !important rule: nonmodal popover backdrops
    // cannot capture pointer input, even if author CSS says auto !important.
    if tree.top_layer_kind(id) == Some(crate::core::top_layer::TopLayerKind::Popover) {
        style.pointer_events = crate::style::layer::PointerEvents::None.into();
    }
    style
}

/// Highlight inheritance is resolved later in DOM order, independently of
/// normal foreground inheritance. Only applicable color declarations participate.
pub(crate) fn cascade_highlights(
    tree: &WidgetTree,
    sheets: &[Stylesheet],
) -> Vec<(WidgetId, Style)> {
    if sheets.iter().all(|s| s.0.selection.is_empty()) {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut caches = SelectorCaches::default();
    let mut matched = Vec::new();
    for id in tree.nodes.keys() {
        matched.clear();
        let element = WidgetElement {
            tree,
            id,
            svg_index: None,
        };
        let mut context = MatchingContext::new(
            MatchingMode::ForStatelessPseudoElement,
            None,
            &mut caches,
            QuirksMode::NoQuirks,
            NeedsSelectorFlags::No,
            MatchingForInvalidation::No,
        );
        for (s, sheet) in sheets.iter().enumerate() {
            sheet.0.selection.for_each(element, |r| {
                let rule = &sheet.0.rules[r];
                if matches_selector(&rule.selector, 0, None, &element, &mut context) {
                    matched.push((rule.selector.specificity(), s, r));
                }
            });
        }
        matched.sort_unstable();
        let mut declarations = Style::default();
        for importance in [false, true] {
            for &(_, s, r) in &matched {
                for (decl, important) in sheets[s].0.rules[r].declarations.iter() {
                    if *important != importance {
                        continue;
                    }
                    // Keep pending color values until the originating element's
                    // variables have been computed in parent-first order.
                    if matches!(
                        decl.property(),
                        crate::style::declaration::Property::Color
                            | crate::style::declaration::Property::Background
                            | crate::style::declaration::Property::Custom
                    ) {
                        decl.apply(&mut declarations);
                    }
                }
            }
        }
        if declarations != Style::default() {
            result.push((id, declarations));
        }
    }
    result
}

fn cascade_pseudo_declarations(
    tree: &WidgetTree,
    id: WidgetId,
    sheets: &[Stylesheet],
    index: impl Fn(&CompiledSheet) -> &RuleIndex,
    style: &mut Style,
) {
    let element = WidgetElement {
        tree,
        id,
        svg_index: None,
    };
    let mut caches = SelectorCaches::default();
    let mut context = MatchingContext::new(
        MatchingMode::ForStatelessPseudoElement,
        None,
        &mut caches,
        QuirksMode::NoQuirks,
        NeedsSelectorFlags::No,
        MatchingForInvalidation::No,
    );
    let mut matched = Vec::new();
    for (s, sheet) in sheets.iter().enumerate() {
        index(&sheet.0).for_each(element, |r| {
            let rule = &sheet.0.rules[r];
            if matches_selector(&rule.selector, 0, None, &element, &mut context) {
                matched.push((rule.selector.specificity(), s, r));
            }
        });
    }
    matched.sort_unstable();
    for importance in [false, true] {
        for &(_, s, r) in &matched {
            for (decl, important) in sheets[s].0.rules[r].declarations.iter() {
                if *important == importance {
                    decl.apply(style);
                }
            }
        }
    }
}
/// Placeholder typography inherits from the control; author rules use the same
/// specificity and !important order as other stateless pseudo-elements.
pub(crate) fn cascade_placeholder(
    tree: &WidgetTree,
    id: WidgetId,
    sheets: &[Stylesheet],
) -> crate::style::text::TextStyle {
    let parent = &tree.nodes[id].computed;
    let mut style = Style::default();
    style.color = crate::style::color::Rgba8::from_rgb8(117, 117, 117).into();
    cascade_pseudo_declarations(tree, id, sheets, |s| &s.placeholder, &mut style);
    super::values::resolve(
        &style,
        parent,
        Some(parent.root_font_size),
        parent.viewport_size,
    )
    .style
    .resolve_text(&parent.text)
}

/// Cascade compact SVG nodes against both document and UI ancestors. The same
/// Servo matching engine supplies specificity, combinators, :is/:where and :has.
pub(crate) struct SvgStyle {
    pub css: String,
    pub attributes: Vec<(String, String)>,
}
pub(crate) fn svg_styles(
    tree: &WidgetTree,
    host: WidgetId,
    doc: &crate::svg::SvgDocument,
    local: &[Stylesheet],
) -> Vec<SvgStyle> {
    use crate::style::{
        computed::ComputedStyle,
        layer::Visibility,
        media::{MediaProperty, MediaValue},
    };
    let mut output = Vec::with_capacity(doc.node_count());
    let mut computed = Vec::<(u32, ComputedStyle)>::new();
    let sheets: Vec<_> = tree.stylesheets.iter().chain(local).collect();
    let mut caches = SelectorCaches::default();
    let mut matched = Vec::new();
    for index in 0..doc.node_count() {
        let node = doc.node(index as u32);
        if index != 0 {
            while computed.last().is_some_and(|(id, _)| *id != node.parent) {
                computed.pop();
            }
        }
        let (resolved, specified) = if index == 0 {
            (tree.nodes[host].computed.clone(), None)
        } else {
            let element = WidgetElement {
                tree,
                id: host,
                svg_index: Some(index as u32),
            };
            matched.clear();
            let mut context = MatchingContext::new(
                MatchingMode::Normal,
                None,
                &mut caches,
                QuirksMode::NoQuirks,
                NeedsSelectorFlags::No,
                MatchingForInvalidation::No,
            );
            for (s, sheet) in sheets.iter().enumerate() {
                sheet.0.normal.for_each(element, |r| {
                    let rule = &sheet.0.rules[r];
                    if matches_selector(&rule.selector, 0, None, &element, &mut context) {
                        matched.push((rule.selector.specificity(), s, r));
                    }
                });
            }
            matched.sort_unstable();
            let mut style = node.base_style();
            for important in [false, true] {
                for &(_, s, r) in &matched {
                    for (d, imp) in sheets[s].0.rules[r].declarations.iter() {
                        if *imp == important {
                            d.apply(&mut style);
                        }
                    }
                }
                for (d, imp) in node.inline.iter() {
                    if *imp == important {
                        d.apply(&mut style);
                    }
                }
            }
            (
                ComputedStyle::resolve_in_layer(&style, &computed.last().unwrap().1, false),
                Some(style),
            )
        };
        let mut css = String::new();
        let mut write = |name: &str, value: &str| {
            css.push_str(name);
            css.push(':');
            css.push_str(value);
            css.push(';');
        };
        use crate::style::{declaration::Property, value::CssValue};
        let declared = |p| specified.as_ref().is_none_or(|s| s.is_marked(p));
        if declared(Property::Color) {
            write("color", &super::media::color_css(resolved.text.color));
        }
        // Keep unspecified inheritance in the SVG document. Baking every
        // descendant's inherited paint into an explicit declaration would break
        // the instance inheritance of <use> and unnecessarily expand markup.
        if declared(Property::FontFamily) {
            let mut family = String::new();
            cssparser::serialize_string(resolved.text.font.family.as_str(), &mut family).unwrap();
            write("font-family", &family);
        }
        if declared(Property::FontStyle) {
            write(
                "font-style",
                match resolved.text.font.style {
                    crate::render::FontStyle::Normal => "normal",
                    crate::render::FontStyle::Italic => "italic",
                    crate::render::FontStyle::Oblique => "oblique",
                },
            );
        }
        if declared(Property::FontSize) {
            write("font-size", &format!("{}px", resolved.text.font_size));
        }
        if declared(Property::FontWeight) {
            write("font-weight", &resolved.text.font.weight.0.to_string());
        }
        if specified
            .as_ref()
            .unwrap_or_else(|| tree.cascaded_style(host))
            .media
            .get(MediaProperty::ImageRendering)
            .is_some()
        {
            write(
                "image-rendering",
                match resolved.media.sampling() {
                    crate::render::ImageSampling::Smooth => "optimizeQuality",
                    _ => "optimizeSpeed",
                },
            );
        }
        let mut attributes = Vec::new();
        for &p in MediaProperty::ALL {
            let authored = specified.as_ref().and_then(|s| s.media.get(p));
            if (index == 0 || authored.is_some())
                && resolved.media.get(p).is_some()
                && let MediaValue::Svg(value) = resolved.media.value(p)
            {
                if matches!(
                    p,
                    MediaProperty::D
                        | MediaProperty::X
                        | MediaProperty::Y
                        | MediaProperty::Cx
                        | MediaProperty::Cy
                        | MediaProperty::R
                        | MediaProperty::Rx
                        | MediaProperty::Ry
                ) {
                    let v = if p == MediaProperty::D {
                        super::media::path_data(&value)
                    } else {
                        value.to_string()
                    };
                    attributes.push((p.name().to_owned(), v));
                } else {
                    let inherit = matches!(authored, Some(CssValue::Inherit))
                        || (p.inherited() && matches!(authored, Some(CssValue::Unset)));
                    write(p.name(), if inherit { "inherit" } else { &value });
                }
            }
        }
        if index != 0 {
            use crate::core::layout::ExpandedDimension;
            for (name, size, property) in [
                (
                    "width",
                    resolved.layout.size.width,
                    crate::style::properties::LayoutProperty::Width,
                ),
                (
                    "height",
                    resolved.layout.size.height,
                    crate::style::properties::LayoutProperty::Height,
                ),
            ] {
                match size.expand() {
                    ExpandedDimension::Length(v) => attributes.push((name.into(), v.to_string())),
                    ExpandedDimension::Percent(v) => {
                        attributes.push((name.into(), format!("{}%", v * 100.)))
                    }
                    _ if declared(Property::Layout(property)) => {
                        attributes.push((name.into(), "auto".into()))
                    }
                    _ => {}
                }
            }
        }
        if declared(Property::Display) {
            write(
                "display",
                if resolved.layout.display == crate::core::layout::Display::None {
                    "none"
                } else {
                    "inline"
                },
            );
        }
        if declared(Property::Visibility) {
            write(
                "visibility",
                if resolved.layer.visibility == Visibility::Visible {
                    "visible"
                } else {
                    "hidden"
                },
            );
        }
        output.push(SvgStyle { css, attributes });
        computed.push((index as u32, resolved));
    }
    output
}
