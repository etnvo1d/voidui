//! Servo selector adapter over the existing widget tree; no parallel DOM.
use crate::core::{
    widget::{WidgetId, WidgetStatus},
    widget_tree::WidgetTree,
};
use cssparser::{CowRcStr, ParseError, SourceLocation, ToCss};
use selectors::{
    Element, OpaqueElement,
    attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint},
    bloom::BloomFilter,
    context::MatchingContext,
    matching::ElementSelectorFlags,
    parser::{NonTSPseudoClass, Parser, PseudoElement, SelectorImpl, SelectorParseErrorKind},
};
use std::{
    borrow::Borrow,
    cell::Cell,
    fmt,
    hash::{Hash, Hasher},
    sync::Arc,
};

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct Atom {
    value: Arc<str>,
    hash: u32,
}
impl From<&str> for Atom {
    fn from(value: &str) -> Self {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        value.hash(&mut h);
        Self {
            value: value.into(),
            hash: h.finish() as u32,
        }
    }
}
impl Default for Atom {
    fn default() -> Self {
        Self::from("")
    }
}
impl AsRef<str> for Atom {
    fn as_ref(&self) -> &str {
        &self.value
    }
}
impl Borrow<str> for Atom {
    fn borrow(&self) -> &str {
        &self.value
    }
}
impl precomputed_hash::PrecomputedHash for Atom {
    fn precomputed_hash(&self) -> u32 {
        self.hash
    }
}
impl ToCss for Atom {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        cssparser::serialize_identifier(&self.value, dest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WidgetSelectors;
impl SelectorImpl for WidgetSelectors {
    type ExtraMatchingData<'a> = ();
    type AttrValue = Atom;
    type Identifier = Atom;
    type LocalName = Atom;
    type NamespaceUrl = Atom;
    type NamespacePrefix = Atom;
    type BorrowedNamespaceUrl = str;
    type BorrowedLocalName = str;
    type NonTSPseudoClass = State;
    type PseudoElement = WidgetPseudoElement;
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum State {
    Hover,
    Active,
    Focus,
    FocusWithin,
    Disabled,
    Enabled,
    Modal,
    PopoverOpen,
    ReadOnly,
    ReadWrite,
    PlaceholderShown,
}
impl ToCss for State {
    fn to_css<W: fmt::Write>(&self, out: &mut W) -> fmt::Result {
        out.write_str(match self {
            Self::Hover => ":hover",
            Self::Active => ":active",
            Self::Focus => ":focus",
            Self::FocusWithin => ":focus-within",
            Self::Disabled => ":disabled",
            Self::Enabled => ":enabled",
            Self::Modal => ":modal",
            Self::PopoverOpen => ":popover-open",
            Self::ReadOnly => ":read-only",
            Self::ReadWrite => ":read-write",
            Self::PlaceholderShown => ":placeholder-shown",
        })
    }
}
impl NonTSPseudoClass for State {
    type Impl = WidgetSelectors;
    fn is_active_or_hover(&self) -> bool {
        matches!(self, Self::Hover | Self::Active)
    }
    fn is_user_action_state(&self) -> bool {
        !matches!(self, Self::Disabled | Self::Enabled)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WidgetPseudoElement {
    Backdrop,
    Selection,
    Placeholder,
}
impl ToCss for WidgetPseudoElement {
    fn to_css<W: fmt::Write>(&self, out: &mut W) -> fmt::Result {
        out.write_str(match self {
            Self::Backdrop => "::backdrop",
            Self::Selection => "::selection",
            Self::Placeholder => "::placeholder",
        })
    }
}
impl PseudoElement for WidgetPseudoElement {
    type Impl = WidgetSelectors;
}

#[derive(Default)]
pub(crate) struct SelectorParser {
    pub uses_state: Cell<u8>,
}
impl<'i> Parser<'i> for SelectorParser {
    type Impl = WidgetSelectors;
    type Error = SelectorParseErrorKind<'i>;
    fn parse_is_and_where(&self) -> bool {
        true
    }
    fn parse_has(&self) -> bool {
        true
    }
    fn parse_nth_child_of(&self) -> bool {
        true
    }
    fn parse_pseudo_element(
        &self,
        location: SourceLocation,
        name: CowRcStr<'i>,
    ) -> Result<WidgetPseudoElement, ParseError<'i, Self::Error>> {
        if name.eq_ignore_ascii_case("backdrop") {
            Ok(WidgetPseudoElement::Backdrop)
        } else if name.eq_ignore_ascii_case("placeholder") {
            Ok(WidgetPseudoElement::Placeholder)
        } else if name.eq_ignore_ascii_case("selection") {
            Ok(WidgetPseudoElement::Selection)
        } else {
            Err(
                location.new_custom_error(SelectorParseErrorKind::UnsupportedPseudoClassOrElement(
                    name,
                )),
            )
        }
    }
    fn parse_non_ts_pseudo_class(
        &self,
        location: SourceLocation,
        name: CowRcStr<'i>,
    ) -> Result<State, ParseError<'i, Self::Error>> {
        let state = match name.to_ascii_lowercase().as_str() {
            "hover" => Ok(State::Hover),
            "active" => Ok(State::Active),
            "focus" => Ok(State::Focus),
            "focus-within" => Ok(State::FocusWithin),
            "disabled" => Ok(State::Disabled),
            "enabled" => Ok(State::Enabled),
            "modal" => Ok(State::Modal),
            "popover-open" => Ok(State::PopoverOpen),
            "read-only" => Ok(State::ReadOnly),
            "read-write" => Ok(State::ReadWrite),
            "placeholder-shown" => Ok(State::PlaceholderShown),
            _ => Err(location.new_custom_error(
                SelectorParseErrorKind::UnsupportedPseudoClassOrElement(name),
            )),
        }?;
        let bits = match state {
            State::Hover => WidgetStatus::Hover.bits(),
            State::Active => WidgetStatus::Active.bits(),
            State::Focus | State::FocusWithin => WidgetStatus::Focused.bits(),
            _ => 0,
        };
        self.uses_state.set(self.uses_state.get() | bits);
        Ok(state)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct WidgetElement<'a> {
    pub tree: &'a WidgetTree,
    pub id: WidgetId,
    pub svg_index: Option<u32>,
}
impl fmt::Debug for WidgetElement<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("WidgetElement").field(&self.id).finish()
    }
}
impl WidgetElement<'_> {
    fn with_id(self, id: WidgetId) -> Self {
        Self {
            id,
            svg_index: None,
            ..self
        }
    }
    pub(crate) fn document(&self) -> Option<&crate::svg::SvgDocument> {
        self.tree.nodes[self.id].widget.as_ref()?.svg_document()
    }
    pub(crate) fn svg_node(&self) -> Option<&crate::svg::DocNode> {
        self.svg_index.map(|i| self.document().unwrap().node(i))
    }
    pub(crate) fn with_svg(self, index: u32) -> Self {
        Self {
            svg_index: (index != 0).then_some(index),
            ..self
        }
    }
    pub fn tag(&self) -> &str {
        self.svg_node()
            .map(|n| n.tag.as_str())
            .unwrap_or(&self.tree.nodes[self.id].props.tag)
    }
    pub fn id_value(&self) -> Option<&str> {
        if let Some(n) = self.svg_node() {
            n.attr("id")
        } else {
            self.tree.nodes[self.id].props.id.as_deref()
        }
    }
    pub fn classes(&self) -> impl Iterator<Item = &str> {
        let props = &self.tree.nodes[self.id].props;
        let classes = if self.svg_index.is_some() {
            &[][..]
        } else {
            props.classes.as_slice()
        };
        classes.iter().map(|v| v.as_str()).chain(
            self.svg_node()
                .and_then(|n| n.attr("class"))
                .into_iter()
                .flat_map(str::split_ascii_whitespace),
        )
    }
    fn focus_within(&self) -> bool {
        self.tree.nodes[self.id].focused_descendants > 0
    }
}
impl Element for WidgetElement<'_> {
    type Impl = WidgetSelectors;
    fn opaque(&self) -> OpaqueElement {
        if let Some(n) = self.svg_node() {
            OpaqueElement::new(n)
        } else {
            OpaqueElement::new(&self.tree.nodes[self.id])
        }
    }
    fn parent_element(&self) -> Option<Self> {
        if let Some(n) = self.svg_node() {
            return Some(self.with_svg(n.parent));
        }
        self.tree.nodes[self.id].parent.map(|id| self.with_id(id))
    }
    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }
    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }
    fn is_pseudo_element(&self) -> bool {
        false
    }
    fn prev_sibling_element(&self) -> Option<Self> {
        if let Some(n) = self.svg_node() {
            return (n.previous != crate::svg::NONE).then(|| self.with_svg(n.previous));
        }
        self.tree.nodes[self.id]
            .previous_sibling
            .map(|id| self.with_id(id))
    }
    fn next_sibling_element(&self) -> Option<Self> {
        if let Some(n) = self.svg_node() {
            return (n.next != crate::svg::NONE).then(|| self.with_svg(n.next));
        }
        self.tree.nodes[self.id]
            .next_sibling
            .map(|id| self.with_id(id))
    }
    fn first_element_child(&self) -> Option<Self> {
        if let Some(doc) = self.document() {
            let first = doc.node(self.svg_index.unwrap_or(0)).first;
            return (first != crate::svg::NONE).then(|| self.with_svg(first));
        }
        self.tree.nodes[self.id]
            .children
            .first()
            .map(|id| self.with_id(*id))
    }
    fn is_html_element_in_html_document(&self) -> bool {
        self.document().is_none()
    }
    fn has_local_name(&self, name: &str) -> bool {
        self.tag() == name
    }
    fn has_namespace(&self, ns: &str) -> bool {
        ns.is_empty() || (self.document().is_some() && ns == "http://www.w3.org/2000/svg")
    }
    fn is_same_type(&self, other: &Self) -> bool {
        self.tag() == other.tag()
    }
    fn attr_matches(
        &self,
        ns: &NamespaceConstraint<&Atom>,
        name: &Atom,
        operation: &AttrSelectorOperation<&Atom>,
    ) -> bool {
        if matches!(ns,NamespaceConstraint::Specific(ns) if !ns.as_ref().is_empty()) {
            return false;
        }
        if let Some(n) = self.svg_node() {
            return n.attr(name.as_ref()).is_some_and(|v| operation.eval_str(v));
        }
        let props = &self.tree.nodes[self.id].props;
        match name.as_ref() {
            "id" => props.id.as_deref().is_some_and(|v| operation.eval_str(v)),
            "class" => !props.classes.is_empty() && operation.eval_str(&props.classes.join(" ")),
            name => props
                .attributes
                .get(name)
                .is_some_and(|v| operation.eval_str(v)),
        }
    }
    fn match_non_ts_pseudo_class(
        &self,
        pc: &State,
        _: &mut MatchingContext<WidgetSelectors>,
    ) -> bool {
        if self.svg_index.is_some() {
            return matches!(pc, State::ReadOnly);
        }
        let node = &self.tree.nodes[self.id];
        match pc {
            State::ReadOnly | State::ReadWrite => {
                let editable = self.tree.text_input(self.id).is_some()
                    && !node.props.attributes.contains_key("readonly")
                    && !node.props.attributes.contains_key("disabled");
                editable == matches!(pc, State::ReadWrite)
            }
            State::PlaceholderShown => {
                node.props
                    .attributes
                    .get("placeholder")
                    .is_some_and(|p| !p.is_empty())
                    && self
                        .tree
                        .text_input(self.id)
                        .is_some_and(|input| input.is_empty())
            }
            State::Hover => node.status.is_hovered(),
            State::Active => node.status.is_active(),
            State::Focus => node.status.is_focused(),
            State::FocusWithin => self.focus_within(),
            State::Disabled => node.props.attributes.contains_key("disabled"),
            State::Enabled => !node.props.attributes.contains_key("disabled"),
            State::Modal => node.top_layer == Some(crate::core::top_layer::TopLayerKind::Modal),
            State::PopoverOpen => {
                node.top_layer == Some(crate::core::top_layer::TopLayerKind::Popover)
            }
        }
    }
    fn match_pseudo_element(
        &self,
        pe: &WidgetPseudoElement,
        _: &mut MatchingContext<WidgetSelectors>,
    ) -> bool {
        let _ = pe;
        false
    }
    fn apply_selector_flags(&self, _: ElementSelectorFlags) {}
    fn is_link(&self) -> bool {
        false
    }
    fn is_html_slot_element(&self) -> bool {
        false
    }
    fn has_id(&self, id: &Atom, sensitivity: CaseSensitivity) -> bool {
        self.id_value()
            .is_some_and(|v| sensitivity.eq(v.as_bytes(), id.as_ref().as_bytes()))
    }
    fn has_class(&self, class: &Atom, sensitivity: CaseSensitivity) -> bool {
        self.classes()
            .any(|v| sensitivity.eq(v.as_bytes(), class.as_ref().as_bytes()))
    }
    fn has_custom_state(&self, _: &Atom) -> bool {
        false
    }
    fn imported_part(&self, _: &Atom) -> Option<Atom> {
        None
    }
    fn is_part(&self, _: &Atom) -> bool {
        false
    }
    fn is_empty(&self) -> bool {
        if let Some(doc) = self.document() {
            let n = doc.node(self.svg_index.unwrap_or(0));
            return n.first == crate::svg::NONE && n.text.is_empty();
        }
        let n = &self.tree.nodes[self.id];
        n.children.is_empty()
            && n.widget
                .as_ref()
                .is_none_or(|w| w.text_content().is_none_or(str::is_empty))
    }
    fn is_root(&self) -> bool {
        self.svg_index.is_none() && self.tree.nodes[self.id].parent.is_none()
    }
    fn add_element_unique_hashes(&self, _: &mut BloomFilter) -> bool {
        false
    }
}
