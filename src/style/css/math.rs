//! Typed CSS arithmetic. Percentages remain expressions until layout supplies a basis.
use crate::style::math::Expression;
use cssparser::{ParseError, Parser, ParserInput, Token};

type Result<'i, T> = std::result::Result<T, ParseError<'i, String>>;

#[derive(Debug)]
pub(super) enum Value {
    Number(f64),
    Length(Expression),
}

pub(super) fn is_math(raw: &str) -> bool {
    let mut input = ParserInput::new(raw);
    matches!(Parser::new(&mut input).next(), Ok(Token::Function(name))
        if ["calc", "min", "max", "clamp"].iter().any(|v| name.eq_ignore_ascii_case(v)))
}

pub(super) fn parse(raw: &str) -> std::result::Result<Value, String> {
    let mut input = ParserInput::new(raw);
    let mut parser = Parser::new(&mut input);
    let result = Arithmetic { terms: 0 }
        .atom(&mut parser, 0)
        .map_err(|e| format!("invalid CSS math: {e:?}"))?;
    parser
        .expect_exhausted()
        .map_err(|_| "unexpected CSS math tokens")?;
    let finite = match &result {
        Value::Number(n) => n.is_finite(),
        Value::Length(e) => e.bounds().is_some(),
    };
    if !finite {
        return Err("CSS math arithmetic is outside the supported finite range".into());
    }
    Ok(result)
}

struct Arithmetic {
    terms: usize,
}

impl Arithmetic {
    fn atom<'i>(&mut self, p: &mut Parser<'i, '_>, depth: usize) -> Result<'i, Value> {
        // Bound both nesting and flat expression size before constructing recursive values.
        self.terms += 1;
        if depth > 64 || self.terms > 1024 {
            return Err(p.new_custom_error("CSS math expression is too complex"));
        }
        match p.next()?.clone() {
            Token::Number { value, .. } if value.is_finite() => Ok(Value::Number(value.into())),
            Token::Dimension { value, unit, .. }
                if value.is_finite() && unit.eq_ignore_ascii_case("px") =>
            {
                Ok(Value::Length(Expression::Pixels(value.into())))
            }
            Token::Percentage { unit_value, .. } if unit_value.is_finite() => {
                Ok(Value::Length(Expression::Percent(unit_value.into())))
            }
            Token::ParenthesisBlock => p.parse_nested_block(|p| self.sum(p, depth + 1)),
            Token::Function(name) => p.parse_nested_block(|p| {
                if name.eq_ignore_ascii_case("calc") {
                    return self.sum(p, depth + 1);
                }
                let minimum = name.eq_ignore_ascii_case("min");
                let maximum = name.eq_ignore_ascii_case("max");
                let clamp = name.eq_ignore_ascii_case("clamp");
                if !minimum && !maximum && !clamp {
                    return Err(p.new_custom_error(format!("unsupported math function: {name}")));
                }
                let mut values = p.parse_comma_separated(|p| self.sum(p, depth + 1))?;
                if clamp && values.len() != 3 {
                    return Err(p.new_custom_error("clamp() requires three arguments"));
                }
                if clamp {
                    // CSS gives the minimum priority when the two limits conflict.
                    let upper = values.pop().unwrap();
                    let center = values.pop().unwrap();
                    let lower = values.pop().unwrap();
                    let bounded = Self::compare(p, vec![center, upper], true)?;
                    Self::compare(p, vec![lower, bounded], false)
                } else {
                    Self::compare(p, values, minimum)
                }
            }),
            _ => Err(p.new_custom_error("expected a finite number, px, %, or math function")),
        }
    }

    fn compare<'i>(p: &Parser<'i, '_>, values: Vec<Value>, minimum: bool) -> Result<'i, Value> {
        if values.iter().all(|v| matches!(v, Value::Number(_))) {
            let numbers = values.into_iter().map(|v| match v {
                Value::Number(n) => n,
                _ => unreachable!(),
            });
            Ok(Value::Number(if minimum {
                numbers.fold(f64::INFINITY, f64::min)
            } else {
                numbers.fold(f64::NEG_INFINITY, f64::max)
            }))
        } else {
            let lengths = values
                .into_iter()
                .map(|v| match v {
                    Value::Length(e) => Ok(e),
                    _ => Err(p.new_custom_error("math arguments must have compatible types")),
                })
                .collect::<Result<'i, Vec<_>>>()?;
            Ok(Value::Length(if minimum {
                Expression::Min(lengths)
            } else {
                Expression::Max(lengths)
            }))
        }
    }

    fn product<'i>(&mut self, p: &mut Parser<'i, '_>, depth: usize) -> Result<'i, Value> {
        let mut left = self.atom(p, depth)?;
        loop {
            let operator = p.try_parse(|p| match p.next()?.clone() {
                Token::Delim(c @ ('*' | '/')) => Ok(c),
                t => Err(p.new_unexpected_token_error::<String>(t)),
            });
            let Ok(operator) = operator else { break };
            let right = self.atom(p, depth)?;
            left = match (operator, left, right) {
                ('*', Value::Number(a), Value::Number(b)) => Value::Number(a * b),
                ('*', Value::Length(e), Value::Number(n))
                | ('*', Value::Number(n), Value::Length(e)) => {
                    Value::Length(Expression::Scale(Box::new(e), n))
                }
                ('/', _, Value::Number(0.0)) => return Err(p.new_custom_error("division by zero")),
                ('/', Value::Number(a), Value::Number(b)) => Value::Number(a / b),
                ('/', Value::Length(e), Value::Number(n)) => {
                    Value::Length(Expression::Scale(Box::new(e), 1.0 / n))
                }
                _ => return Err(p.new_custom_error(
                    "multiplication requires a number; division requires a nonzero number divisor",
                )),
            };
            if matches!(left, Value::Number(n) if !n.is_finite()) {
                return Err(p.new_custom_error("CSS math number is outside the finite range"));
            }
        }
        Ok(left)
    }

    fn sum<'i>(&mut self, p: &mut Parser<'i, '_>, depth: usize) -> Result<'i, Value> {
        let mut left = self.product(p, depth)?;
        loop {
            let state = p.state();
            let mut space = false;
            let operator = loop {
                let Ok(token) = p.next_including_whitespace().cloned() else {
                    return Ok(left);
                };
                match token {
                    Token::WhiteSpace(_) => space = true,
                    Token::Delim(c @ ('+' | '-')) => break c,
                    _ => {
                        p.reset(&state);
                        return Ok(left);
                    }
                }
            };
            if !space || !matches!(p.next_including_whitespace()?, Token::WhiteSpace(_)) {
                return Err(p.new_custom_error("+ and - require surrounding whitespace"));
            }
            let right = self.product(p, depth)?;
            let sign = if operator == '+' { 1.0 } else { -1.0 };
            left = match (left, right) {
                (Value::Number(a), Value::Number(b)) => Value::Number(a + sign * b),
                (Value::Length(a), Value::Length(b)) => Value::Length(Expression::Sum(
                    Box::new(a),
                    Box::new(Expression::Scale(Box::new(b), sign)),
                )),
                _ => {
                    return Err(
                        p.new_custom_error("addition and subtraction require compatible types")
                    );
                }
            };
            if matches!(left, Value::Number(n) if !n.is_finite()) {
                return Err(p.new_custom_error("CSS math number is outside the finite range"));
            }
        }
    }
}
