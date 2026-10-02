use super::{
    selector::{SelectorParser, WidgetSelectors},
    sheet::{Rule, Stylesheet},
};
use crate::style::declaration::Declaration;
use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, Delimiter, ParseError, Parser, ParserInput,
    ParserState, QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser, StyleSheetParser, Token,
};
use selectors::parser::{ParseRelative, SelectorList};
use std::{fmt, rc::Rc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssError {
    pub line: u32,
    pub column: u32,
    pub message: String,
}
impl fmt::Display for CssError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CSS {}:{}: {}", self.line, self.column, self.message)
    }
}
impl std::error::Error for CssError {}
fn own_error(error: ParseError<'_, String>) -> CssError {
    CssError {
        line: error.location.line + 1,
        column: error.location.column,
        message: format!("{:?}", error.kind),
    }
}

pub(crate) fn parse(source: &str) -> Result<Stylesheet, CssError> {
    let mut input = ParserInput::new(source);
    let mut input = Parser::new(&mut input);
    let mut rules_parser = Rules {
        selectors: SelectorParser::default(),
        keyframes: Default::default(),
    };
    let mut rules = Vec::new();
    for rule in StyleSheetParser::new(&mut input, &mut rules_parser) {
        rules.extend(rule.map_err(|(error, _)| own_error(error))?);
    }
    Ok(Stylesheet::compile(
        rules,
        rules_parser.selectors.uses_state.get(),
        rules_parser.keyframes,
    ))
}
struct Rules {
    selectors: SelectorParser,
    keyframes: std::collections::HashMap<String, Rc<Keyframes>>,
}
/// Equal offsets cascade in source order; different named rules never merge.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Keyframe {
    pub offset: f64,
    pub declarations: Vec<Declaration>,
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Keyframes(pub Vec<Keyframe>);

impl<'i> AtRuleParser<'i> for Rules {
    type Prelude = String;
    type AtRule = Vec<Rule>;
    type Error = String;
    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<String, ParseError<'i, String>> {
        if !name.eq_ignore_ascii_case("keyframes") {
            return Err(input.new_custom_error("unsupported at-rule"));
        }
        let start = input.position();
        input.next()?;
        let name = super::animation::name(input.slice_from(start))
            .map_err(|e| input.new_custom_error(e))?;
        input.expect_exhausted()?;
        name.0
            .ok_or_else(|| input.new_custom_error("keyframes name cannot be none"))
    }
    fn parse_block<'t>(
        &mut self,
        name: String,
        _: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Vec<Rule>, ParseError<'i, String>> {
        let mut frames = Vec::<Keyframe>::new();
        for item in RuleBodyParser::new(input, &mut KeyframeParser) {
            for frame in item.map_err(|(e, _)| e)? {
                if let Some(previous) = frames.iter_mut().find(|v| v.offset == frame.offset) {
                    previous.declarations.extend(frame.declarations);
                } else {
                    frames.push(frame);
                }
            }
        }
        frames.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        self.keyframes.insert(name, Rc::new(Keyframes(frames)));
        Ok(Vec::new())
    }
}
struct KeyframeParser;
impl<'i> AtRuleParser<'i> for KeyframeParser {
    type Prelude = ();
    type AtRule = Vec<Keyframe>;
    type Error = String;
}
impl<'i> DeclarationParser<'i> for KeyframeParser {
    type Declaration = Vec<Keyframe>;
    type Error = String;
}
impl<'i> RuleBodyItemParser<'i, Vec<Keyframe>, String> for KeyframeParser {
    fn parse_declarations(&self) -> bool {
        false
    }
    fn parse_qualified(&self) -> bool {
        true
    }
}
impl<'i> QualifiedRuleParser<'i> for KeyframeParser {
    type Prelude = Vec<f64>;
    type QualifiedRule = Vec<Keyframe>;
    type Error = String;
    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Vec<f64>, ParseError<'i, String>> {
        input.parse_comma_separated(|p| {
            let offset = match p.next()? {
                Token::Ident(v) if v.eq_ignore_ascii_case("from") => 0.0,
                Token::Ident(v) if v.eq_ignore_ascii_case("to") => 1.0,
                Token::Percentage { unit_value, .. } => f64::from(*unit_value),
                _ => return Err(p.new_custom_error("invalid keyframe selector")),
            };
            if !(0.0..=1.0).contains(&offset) {
                return Err(p.new_custom_error("keyframe offset outside 0%..100%"));
            }
            Ok(offset)
        })
    }
    fn parse_block<'t>(
        &mut self,
        offsets: Vec<f64>,
        _: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Vec<Keyframe>, ParseError<'i, String>> {
        let mut declarations = Vec::new();
        for item in RuleBodyParser::new(input, &mut Declarations) {
            // CSS ignores important declarations inside keyframes.
            declarations.extend(
                item.map_err(|(e, _)| e)?
                    .into_iter()
                    .filter_map(|(d, important)| (!important).then_some(d)),
            );
        }
        Ok(offsets
            .into_iter()
            .map(|offset| Keyframe {
                offset,
                declarations: declarations.clone(),
            })
            .collect())
    }
}
impl<'i> QualifiedRuleParser<'i> for Rules {
    type Prelude = SelectorList<WidgetSelectors>;
    type QualifiedRule = Vec<Rule>;
    type Error = String;
    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, String>> {
        SelectorList::parse(&self.selectors, input, ParseRelative::No).map_err(|error| ParseError {
            location: error.location,
            kind: cssparser::ParseErrorKind::Custom(format!("invalid selector: {:?}", error.kind)),
        })
    }
    fn parse_block<'t>(
        &mut self,
        selectors: Self::Prelude,
        _: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Vec<Rule>, ParseError<'i, String>> {
        let mut declarations = Vec::new();
        let mut parser = Declarations;
        for item in RuleBodyParser::new(input, &mut parser) {
            declarations.extend(item.map_err(|(error, _)| error)?);
        }
        let declarations: Rc<[(Declaration, bool)]> = declarations.into();
        Ok(selectors
            .slice()
            .iter()
            .map(|s| Rule {
                selector: s.clone(),
                declarations: declarations.clone(),
            })
            .collect())
    }
}
struct Declarations;
impl<'i> DeclarationParser<'i> for Declarations {
    type Declaration = Vec<(Declaration, bool)>;
    type Error = String;
    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        _: &ParserState,
    ) -> Result<Self::Declaration, ParseError<'i, String>> {
        let value = input.parse_until_before(Delimiter::Bang, |value| {
            let start = value.position();
            // Consume CSS tokens, not split-on-semicolon text. Nested functions,
            // quoted strings and comments are kept inside their declaration.
            while value.next_including_whitespace_and_comments().is_ok() {}
            let raw = value.slice_from(start);
            super::properties::parse_property(&name, raw)
                .map_err(|message| value.new_custom_error(message))
        })?;
        let important = input.try_parse(cssparser::parse_important).is_ok();
        input.expect_exhausted()?;
        Ok(value.into_iter().map(|v| (v, important)).collect())
    }
}
impl<'i> AtRuleParser<'i> for Declarations {
    type Prelude = ();
    type AtRule = Vec<(Declaration, bool)>;
    type Error = String;
}
impl<'i> QualifiedRuleParser<'i> for Declarations {
    type Prelude = ();
    type QualifiedRule = Vec<(Declaration, bool)>;
    type Error = String;
}
impl<'i> RuleBodyItemParser<'i, Vec<(Declaration, bool)>, String> for Declarations {
    fn parse_declarations(&self) -> bool {
        true
    }
    fn parse_qualified(&self) -> bool {
        false
    }
}

pub(super) fn clean_value(raw: &str) -> Result<String, String> {
    let mut input = ParserInput::new(raw);
    let mut parser = Parser::new(&mut input);
    let mut output = String::new();
    while let Ok(token) = parser.next_including_whitespace_and_comments() {
        match token {
            Token::Comment(_) => output.push(' '),
            _ => {
                use cssparser::ToCss;
                output.push_str(&token.to_css_string());
            }
        }
        // next() skips the contents of a nested block. Property parsing needs
        // that content intact, so serialized-token cleaning is not used there.
        if matches!(
            token,
            Token::Function(_)
                | Token::ParenthesisBlock
                | Token::SquareBracketBlock
                | Token::CurlyBracketBlock
        ) {
            return Ok(raw.trim().to_owned());
        }
    }
    Ok(output.trim().to_owned())
}

/// Parse an SVG style attribute with the same grammar and importance handling
/// as author stylesheets; this is not a second CSS dialect.
pub(crate) fn parse_inline(source: &str) -> Result<Vec<(Declaration, bool)>, CssError> {
    let mut input = ParserInput::new(source);
    let mut input = Parser::new(&mut input);
    let mut declarations = Vec::new();
    let mut parser = Declarations;
    for item in RuleBodyParser::new(&mut input, &mut parser) {
        declarations.extend(item.map_err(|(e, _)| own_error(e))?);
    }
    Ok(declarations)
}
