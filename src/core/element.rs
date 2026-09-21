use winit::keyboard::SmolStr;

use crate::{core::widget::Widget, style::style::Style};

pub struct Element {
    pub(crate) events: super::interaction::EventBindings,
    pub(crate) kind: ElementKind,
    pub(crate) props: ElementProps,
    pub(crate) children: Vec<Element>,
}

pub(crate) enum ElementKind {
    Widget(Box<dyn Widget>),
    Component(Box<super::component::ComponentElement>),
}

#[derive(Clone, PartialEq)]
pub struct ElementProps {
    pub(crate) window_control_area: Option<super::decoration::WindowControlArea>,
    pub(crate) scrollbar_mode: Option<super::scroll::ScrollbarMode>,
    pub(crate) tag: SmolStr,
    pub(crate) attributes: std::collections::BTreeMap<SmolStr, SmolStr>,
    pub(crate) id: Option<SmolStr>,
    pub(crate) key: Option<SmolStr>,
    pub(crate) classes: Vec<SmolStr>,
    pub(crate) style: Style,
}

impl ElementProps {
    pub fn new(style: Style) -> Self {
        Self {
            window_control_area: None,
            scrollbar_mode: None,
            tag: "widget".into(),
            attributes: Default::default(),
            id: None,
            key: None,
            classes: Vec::new(),
            style,
        }
    }
}

pub trait IntoElement {
    fn into_element(self) -> Element;
}

impl IntoElement for Element {
    fn into_element(self) -> Element {
        self
    }
}

impl Element {
    crate::core::interaction::event_methods!();

    /// Mark a native control or drag region. Client excludes interactive content from dragging.
    pub fn window_control_area(mut self, area: super::decoration::WindowControlArea) -> Self {
        self.props.window_control_area = Some(area);
        self
    }
}
