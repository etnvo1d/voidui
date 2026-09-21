//! Token-preserving custom properties and contextual lengths.
//!
//! Cascade chooses longhands first. Only the winners are substituted and parsed,
//! so an invalid variable resets its property instead of reviving an older rule.
use crate::style::{
    computed::ComputedStyle,
    declaration::{Declaration, Property},
    style::Style,
};
use cssparser::{Parser, ParserInput, Token};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

const MAX_DEPTH: usize = 64;
const MAX_TOKENS: usize = 16384;
const MAX_BYTES: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq)]
enum Component {
    Literal(String),
    Relative(f32, RelativeUnit),
    Block(String, Tokens, char),
    Variable(String, Option<Tokens>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RelativeUnit {
    Font,
    RootFont,
    ViewportWidth,
    ViewportHeight,
}
impl RelativeUnit {
    fn parse(unit: &str) -> Option<Self> {
        match unit.to_ascii_lowercase().as_str() {
            "em" => Some(Self::Font),
            "rem" => Some(Self::RootFont),
            "vw" => Some(Self::ViewportWidth),
            "vh" => Some(Self::ViewportHeight),
            _ => None,
        }
    }
    fn basis(self, context: LengthContext) -> f32 {
        match self {
            Self::Font => context.font,
            Self::RootFont => context.root_font,
            Self::ViewportWidth => context.viewport[0] / 100.0,
            Self::ViewportHeight => context.viewport[1] / 100.0,
        }
    }
}

/// A CSS component-value list. Strings and identifiers are never interpolated.
#[derive(Debug, Clone, PartialEq)]
pub struct Tokens(Vec<Component>);

/// A shared pending shorthand or longhand, evaluated after cascade.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingValue {
    name: String,
    tokens: Tokens,
}

pub(crate) type CustomProperties = Rc<BTreeMap<String, Option<Tokens>>>;

impl Tokens {
    pub(crate) fn parse(raw: &str) -> Result<Self, String> {
        if raw.len() > MAX_BYTES {
            return Err("CSS value exceeds the size limit".into());
        }
        let mut input = ParserInput::new(raw);
        Self::read(&mut Parser::new(&mut input), 0)
            .map_err(|_| "invalid CSS component value or var()".into())
    }
    fn read<'i>(
        p: &mut Parser<'i, '_>,
        depth: usize,
    ) -> Result<Self, cssparser::ParseError<'i, ()>> {
        if depth > MAX_DEPTH {
            return Err(p.new_custom_error(()));
        }
        let mut out = Vec::new();
        while !p.is_exhausted() {
            let start = p.position();
            let token = p.next_including_whitespace_and_comments()?.clone();
            let opening = p.slice_from(start).to_owned();
            let value = match token {
                Token::Function(ref name) if name.eq_ignore_ascii_case("var") => p
                    .parse_nested_block(|p| {
                        let name = p.expect_ident_cloned()?.to_string();
                        if !name.starts_with("--") || name == "--" {
                            return Err(p.new_custom_error(()));
                        }
                        let fallback = if p.is_exhausted() {
                            None
                        } else {
                            p.expect_comma()?;
                            Some(Self::read(p, depth + 1)?)
                        };
                        Ok(Component::Variable(name, fallback))
                    })?,
                Token::Function(_)
                | Token::ParenthesisBlock
                | Token::SquareBracketBlock
                | Token::CurlyBracketBlock => {
                    let close = match token {
                        Token::SquareBracketBlock => ']',
                        Token::CurlyBracketBlock => '}',
                        _ => ')',
                    };
                    Component::Block(
                        opening,
                        p.parse_nested_block(|p| Self::read(p, depth + 1))?,
                        close,
                    )
                }
                Token::Dimension {
                    value, ref unit, ..
                } => {
                    if let Some(unit) = RelativeUnit::parse(unit) {
                        if !value.is_finite() {
                            return Err(p.new_custom_error(()));
                        }
                        Component::Relative(value, unit)
                    } else {
                        Component::Literal(opening)
                    }
                }
                Token::BadString(_)
                | Token::BadUrl(_)
                | Token::CloseParenthesis
                | Token::CloseSquareBracket
                | Token::CloseCurlyBracket => return Err(p.new_custom_error(())),
                _ => Component::Literal(opening),
            };
            out.push(value);
            if out.len() > MAX_TOKENS {
                return Err(p.new_custom_error(()));
            }
        }
        Ok(Self(out))
    }
    fn dynamic(&self) -> bool {
        self.0.iter().any(|v| match v {
            Component::Variable(..) | Component::Relative(..) => true,
            Component::Block(_, t, _) => t.dynamic(),
            _ => false,
        })
    }
    fn has_variables(&self) -> bool {
        self.0.iter().any(|v| match v {
            Component::Variable(..) => true,
            Component::Block(_, t, _) => t.has_variables(),
            _ => false,
        })
    }
    fn references(&self, out: &mut BTreeSet<String>) {
        for v in &self.0 {
            match v {
                Component::Variable(name, fallback) => {
                    out.insert(name.clone());
                    if let Some(t) = fallback {
                        t.references(out);
                    }
                }
                Component::Block(_, t, _) => t.references(out),
                _ => (),
            }
        }
    }
    fn substitute(
        &self,
        lookup: &mut impl FnMut(&str) -> Option<Tokens>,
        budget: &mut usize,
    ) -> Option<Self> {
        let mut out = Vec::new();
        for v in &self.0 {
            *budget = budget.checked_sub(1)?;
            match v {
                Component::Variable(name, fallback) => {
                    let tokens = match lookup(name) {
                        Some(t) => t,
                        None => fallback.as_ref()?.substitute(lookup, budget)?,
                    };
                    *budget = budget.checked_sub(tokens.weight())?;
                    out.extend(tokens.0);
                }
                Component::Block(open, t, close) => out.push(Component::Block(
                    open.clone(),
                    t.substitute(lookup, budget)?,
                    *close,
                )),
                _ => out.push(v.clone()),
            }
        }
        Some(Self(out))
    }
    fn weight(&self) -> usize {
        self.0
            .iter()
            .map(|v| {
                std::mem::size_of::<Component>()
                    + match v {
                        Component::Block(open, t, _) => open.len() + t.weight(),
                        Component::Literal(s) => s.len(),
                        _ => 0,
                    }
            })
            .sum()
    }
    fn css(&self, context: LengthContext) -> Result<String, String> {
        let mut out = String::new();
        for v in &self.0 {
            // A substitution must not turn `var(--number)px` into one dimension.
            if !out.is_empty() {
                out.push_str("/**/");
            }
            match v {
                Component::Literal(s) => out.push_str(s),
                Component::Relative(value, unit) => {
                    let pixels = value * unit.basis(context);
                    if !pixels.is_finite() {
                        return Err("relative length overflow".into());
                    }
                    out.push_str(&format!("{pixels}px"));
                }
                Component::Block(open, tokens, close) => {
                    out.push_str(open);
                    out.push_str(&tokens.css(context)?);
                    out.push(*close);
                }
                Component::Variable(..) => return Err("unresolved CSS variable".into()),
            }
            if out.len() > MAX_BYTES {
                return Err("expanded CSS value exceeds the size limit".into());
            }
        }
        Ok(out)
    }
    fn keyword(&self) -> Option<String> {
        let mut words = self.0.iter().filter_map(|v| match v {
            Component::Literal(s) if !s.trim().is_empty() && !s.starts_with("/*") => {
                Some(s.trim().to_ascii_lowercase())
            }
            Component::Literal(_) => None,
            _ => Some(String::new()),
        });
        let first = words.next()?;
        if words.next().is_some() {
            return None;
        }
        let mut input = ParserInput::new(&first);
        let mut parser = Parser::new(&mut input);
        let keyword = parser.expect_ident().ok()?.to_ascii_lowercase();
        parser.expect_exhausted().ok()?;
        Some(keyword)
    }
}

#[derive(Clone, Copy)]
struct LengthContext {
    font: f32,
    root_font: f32,
    viewport: [f32; 2],
}

/// Keep custom names case-sensitive; ordinary property names remain insensitive.
pub(crate) fn parse(name: &str, raw: &str) -> Result<Vec<Declaration>, String> {
    let tokens = Tokens::parse(raw)?;
    if name.starts_with("--") {
        if name == "--" {
            return Err("empty custom property name".into());
        }
        return Ok(vec![Declaration::CustomProperty(
            name.into(),
            Rc::new(tokens),
        )]);
    }
    if !tokens.dynamic() {
        return super::properties::parse_static_property(name, raw);
    }
    // Reuse the property parser's shorthand expansion as the property registry.
    let defaults = super::properties::parse_static_property(name, "unset")?;
    if !tokens.has_variables() {
        let font = crate::style::text::TextStyle::default().font_size;
        let css = tokens.css(LengthContext {
            font,
            root_font: font,
            viewport: [100.0; 2],
        })?;
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "font-size" | "line-height"
        ) && super::math::is_math(&css)
        {
            // Validate dimensions now, but check the final positive font size in
            // context: calc(1em - 20px) can be valid with a larger parent font.
            super::properties::unit(&css)?;
        } else {
            super::properties::parse_static_property(name, &css)?;
        }
    }
    let value = Rc::new(PendingValue {
        name: name.into(),
        tokens,
    });
    Ok(defaults
        .into_iter()
        .map(|d| Declaration::Deferred(d.property(), value.clone()))
        .collect())
}

/// Detect cycles through every reference, including references in unused fallbacks.
/// Inherited values are already substituted, so descendants cannot rebind them.
fn custom_properties(style: &Style, inherited: &CustomProperties) -> CustomProperties {
    if style.custom_properties.is_empty() {
        return inherited.clone();
    }
    let mut raw = (**inherited).clone();
    for (name, tokens) in &style.custom_properties {
        match tokens.keyword().as_deref() {
            Some("inherit" | "unset") => {
                raw.insert(name.clone(), inherited.get(name).cloned().flatten());
            }
            Some("initial") => {
                raw.insert(name.clone(), None);
            }
            _ => {
                raw.insert(name.clone(), Some((**tokens).clone()));
            }
        }
    }
    fn cycles(
        name: &str,
        raw: &BTreeMap<String, Option<Tokens>>,
        path: &mut Vec<String>,
        done: &mut BTreeSet<String>,
        invalid: &mut BTreeSet<String>,
    ) {
        if let Some(index) = path.iter().position(|n| n == name) {
            invalid.extend(path[index..].iter().cloned());
            return;
        }
        if done.contains(name) {
            return;
        }
        if path.len() >= MAX_DEPTH {
            invalid.insert(name.into());
            return;
        }
        path.push(name.into());
        if let Some(Some(tokens)) = raw.get(name) {
            let mut references = BTreeSet::new();
            tokens.references(&mut references);
            for next in references {
                cycles(&next, raw, path, done, invalid);
            }
        }
        path.pop();
        done.insert(name.into());
    }
    let mut invalid = BTreeSet::new();
    let mut done = BTreeSet::new();
    for name in raw.keys() {
        cycles(name, &raw, &mut Vec::new(), &mut done, &mut invalid);
    }
    fn resolve(
        name: &str,
        raw: &BTreeMap<String, Option<Tokens>>,
        memo: &mut BTreeMap<String, Option<Tokens>>,
        invalid: &BTreeSet<String>,
        depth: usize,
    ) -> Option<Tokens> {
        if let Some(value) = memo.get(name) {
            return value.clone();
        }
        if invalid.contains(name) || depth >= MAX_DEPTH {
            return None;
        }
        let mut budget = MAX_BYTES;
        let value = raw.get(name)?.as_ref()?.substitute(
            &mut |n| resolve(n, raw, memo, invalid, depth + 1),
            &mut budget,
        );
        memo.insert(name.into(), value.clone());
        value
    }
    let mut memo = BTreeMap::new();
    for name in raw.keys() {
        let value = resolve(name, &raw, &mut memo, &invalid, 0);
        memo.insert(name.clone(), value);
    }
    Rc::new(memo)
}

pub(crate) struct ResolvedValues<'a> {
    pub style: Cow<'a, Style>,
    pub custom: CustomProperties,
    pub root_font: f32,
}

/// Resolve font-size first, then lengths based on this element's computed font.
/// Root font-size resolves its own rem against the initial font size.
pub(crate) fn resolve<'a>(
    style: &'a Style,
    parent: &ComputedStyle,
    root_font: Option<f32>,
    viewport: [f32; 2],
) -> ResolvedValues<'a> {
    let custom = custom_properties(style, &parent.custom_properties);
    if style.deferred.is_empty() {
        return ResolvedValues {
            style: Cow::Borrowed(style),
            custom,
            root_font: root_font.unwrap_or_else(|| style.resolve_text(&parent.text).font_size),
        };
    }
    let mut resolved = style.clone();
    resolved.deferred.clear();
    let initial_font = crate::style::text::TextStyle::default().font_size;
    let mut context = LengthContext {
        font: parent.text.font_size,
        root_font: root_font.unwrap_or(initial_font),
        viewport,
    };
    let apply =
        |property: Property, pending: &PendingValue, target: &mut Style, context: LengthContext| {
            let mut budget = MAX_BYTES;
            let parsed = pending
                .tokens
                .substitute(&mut |name| custom.get(name).cloned().flatten(), &mut budget)
                .ok_or_else(|| "invalid variable".to_owned())
                .and_then(|tokens| tokens.css(context))
                .and_then(|css| super::properties::parse_static_property(&pending.name, &css))
                .or_else(|_| super::properties::parse_static_property(&pending.name, "unset"));
            if let Ok(declarations) = parsed {
                for d in declarations {
                    if d.property() == property {
                        d.apply(target);
                    }
                }
            }
        };
    for (p, value) in &style.deferred {
        if *p == Property::FontSize {
            apply(*p, value, &mut resolved, context);
        }
    }
    context.font = resolved.resolve_text(&parent.text).font_size;
    let root_font = root_font.unwrap_or(context.font);
    context.root_font = root_font;
    for (p, value) in &style.deferred {
        if *p != Property::FontSize {
            apply(*p, value, &mut resolved, context);
        }
    }
    ResolvedValues {
        style: Cow::Owned(resolved),
        custom,
        root_font,
    }
}
