//! Compact SVG graph authoring with standard attributes.
use winit::keyboard::SmolStr;

/// SVG attributes retain their standard, case-sensitive names.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgNode {
    pub(crate) tag: SmolStr,
    pub(crate) attrs: Vec<(SmolStr, SmolStr)>,
    pub(super) children: Vec<SvgNode>,
    pub(super) text: SmolStr,
    pub(super) tail: SmolStr,
}
impl SvgNode {
    pub fn new(tag: impl Into<SmolStr>) -> Self {
        Self {
            tag: tag.into(),
            attrs: Vec::new(),
            children: Vec::new(),
            text: SmolStr::default(),
            tail: SmolStr::default(),
        }
    }
    pub fn attr(mut self, name: impl Into<SmolStr>, value: impl std::fmt::Display) -> Self {
        let name = name.into();
        let value: SmolStr = value.to_string().into();
        if let Some((_, v)) = self.attrs.iter_mut().find(|(n, _)| *n == name) {
            *v = value;
        } else {
            self.attrs.push((name, value));
        }
        self
    }
    pub fn child(mut self, node: SvgNode) -> Self {
        self.children.push(node);
        self
    }
    pub fn children(mut self, nodes: impl IntoIterator<Item = SvgNode>) -> Self {
        self.children.extend(nodes);
        self
    }
    /// Text is escaped when serialized; it cannot inject SVG markup.
    pub fn text(mut self, text: impl Into<SmolStr>) -> Self {
        self.text = text.into();
        self
    }
}
macro_rules! nodes { ($( $fun:ident => $tag:literal ),* $(,)?)=> { $(
    #[doc=concat!("Create an SVG `<",$tag,">` node.")]
    pub fn $fun()->SvgNode { SvgNode::new($tag) }
)* }; }
nodes! { path=>"path",rect=>"rect",circle=>"circle",ellipse=>"ellipse",line=>"line",polyline=>"polyline",polygon=>"polygon",g=>"g",defs=>"defs",linear_gradient=>"linearGradient",radial_gradient=>"radialGradient",stop=>"stop",clip_path=>"clipPath",mask=>"mask",filter=>"filter",text=>"text",tspan=>"tspan",image=>"image",symbol=>"symbol",use_node=>"use",title=>"title",desc=>"desc" }
macro_rules! attributes { ($( $name:ident => $attr:literal ),* $(,)?)=> { impl SvgNode { $(
    #[doc=concat!("Set the standard SVG `",$attr,"` attribute.")]
    pub fn $name(self,value:impl std::fmt::Display)->Self { self.attr($attr,value) }
)* } }; }
attributes! { d=>"d",id=>"id",class=>"class",style=>"style",fill=>"fill",stroke=>"stroke",stroke_width=>"stroke-width",stroke_linecap=>"stroke-linecap",stroke_linejoin=>"stroke-linejoin",stroke_dasharray=>"stroke-dasharray",fill_rule=>"fill-rule",opacity=>"opacity",transform=>"transform",x=>"x",y=>"y",width=>"width",height=>"height",cx=>"cx",cy=>"cy",r=>"r",rx=>"rx",ry=>"ry",x1=>"x1",y1=>"y1",x2=>"x2",y2=>"y2",points=>"points",href=>"href",offset=>"offset",stop_color=>"stop-color",stop_opacity=>"stop-opacity" }
