//! A tiny, std-only arithmetic expression evaluator for the script language.
//!
//! This is what makes designs *parametric*: wherever a command takes a number,
//! a token containing `$` is evaluated here instead of parsed as a literal.
//! Variables (`$name`) resolve against the interpreter's variable store; the
//! grammar also supports `+ - * / %`, power `^`, unary sign, parentheses, a set
//! of math functions, and the constants `pi`/`tau`/`e`.
//!
//! Grammar (whitespace-insensitive), lowest to highest precedence:
//! ```text
//! expr   := term  (('+' | '-') term)*
//! term   := unary (('*' | '/' | '%') unary)*
//! unary  := ('-' | '+')? power
//! power  := primary ('^' unary)?             // right-assoc; binds tighter than
//!                                            // unary, so `-2^2` == `-(2^2)`
//! primary:= number | '$' ident | ident '(' [expr (',' expr)*] ')'
//!         | ident                            // a constant: pi / tau / e
//!         | '(' expr ')'
//! ```
//! Everything is `f64`. Errors are [`CadError::Parse`]/[`CadError::Arg`] with a
//! message that names the offending symbol.

use crate::error::{CadError, Result};
use crate::math::{PI, TAU};
use std::collections::HashMap;

/// Evaluate `src` to a number, resolving `$name` against `vars`.
pub fn eval(src: &str, vars: &HashMap<String, f64>) -> Result<f64> {
    let mut p = Parser {
        chars: src.chars().collect(),
        pos: 0,
        vars,
    };
    p.skip_ws();
    if p.chars.is_empty() || p.pos >= p.chars.len() {
        return Err(CadError::Parse(format!("empty expression in '{src}'")));
    }
    let v = p.expr()?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return Err(CadError::Parse(format!(
            "unexpected '{}' in expression '{src}'",
            p.chars[p.pos]
        )));
    }
    Ok(v)
}

/// Format a number back into a token string: integers lose the decimal point
/// (so `seg=$n` still parses as a whole number), others are trimmed of trailing
/// zeros. Keeps enough precision for geometry without `1.0000000001` noise.
pub fn fmt_num(x: f64) -> String {
    if !x.is_finite() {
        // Let the downstream number parser reject it with its own message.
        return x.to_string();
    }
    if x == x.trunc() && x.abs() < 1e15 {
        return format!("{}", x as i64);
    }
    let mut s = format!("{x:.6}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

struct Parser<'a> {
    chars: Vec<char>,
    pos: usize,
    vars: &'a HashMap<String, f64>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    /// Consume `c` if it is next (after whitespace). Returns whether it matched.
    fn eat(&mut self, c: char) -> bool {
        self.skip_ws();
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expr(&mut self) -> Result<f64> {
        let mut v = self.term()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('+') => {
                    self.pos += 1;
                    v += self.term()?;
                }
                Some('-') => {
                    self.pos += 1;
                    v -= self.term()?;
                }
                _ => break,
            }
        }
        Ok(v)
    }

    fn term(&mut self) -> Result<f64> {
        let mut v = self.unary()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('*') => {
                    self.pos += 1;
                    v *= self.unary()?;
                }
                Some('/') => {
                    self.pos += 1;
                    let d = self.unary()?;
                    if d == 0.0 {
                        return Err(CadError::Arg("division by zero".into()));
                    }
                    v /= d;
                }
                Some('%') => {
                    self.pos += 1;
                    let d = self.unary()?;
                    if d == 0.0 {
                        return Err(CadError::Arg("modulo by zero".into()));
                    }
                    v %= d;
                }
                _ => break,
            }
        }
        Ok(v)
    }

    fn unary(&mut self) -> Result<f64> {
        self.skip_ws();
        match self.peek() {
            Some('-') => {
                self.pos += 1;
                Ok(-self.unary()?)
            }
            Some('+') => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<f64> {
        let base = self.primary()?;
        self.skip_ws();
        if self.peek() == Some('^') {
            self.pos += 1;
            // Right-associative, and the exponent may carry its own sign
            // (`2^-2`): the RHS is a full unary.
            let exp = self.unary()?;
            return Ok(base.powf(exp));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<f64> {
        self.skip_ws();
        match self.peek() {
            Some('(') => {
                self.pos += 1;
                let v = self.expr()?;
                if !self.eat(')') {
                    return Err(CadError::Parse("expected ')'".into()));
                }
                Ok(v)
            }
            Some('$') => {
                self.pos += 1;
                let name = self.ident();
                if name.is_empty() {
                    return Err(CadError::Parse("expected a variable name after '$'".into()));
                }
                self.vars
                    .get(&name)
                    .copied()
                    .ok_or_else(|| CadError::Arg(format!("unknown variable `${name}`")))
            }
            Some(c) if c.is_ascii_digit() || c == '.' => self.number(),
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                let name = self.ident();
                self.skip_ws();
                if self.peek() == Some('(') {
                    self.pos += 1;
                    let mut args = Vec::new();
                    self.skip_ws();
                    if self.peek() != Some(')') {
                        loop {
                            args.push(self.expr()?);
                            if self.eat(',') {
                                continue;
                            }
                            break;
                        }
                    }
                    if !self.eat(')') {
                        return Err(CadError::Parse(format!("expected ')' after {name}(")));
                    }
                    call(&name, &args)
                } else {
                    constant(&name)
                }
            }
            Some(c) => Err(CadError::Parse(format!("unexpected '{c}' in expression"))),
            None => Err(CadError::Parse("unexpected end of expression".into())),
        }
    }

    fn ident(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '_' {
                s.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        s
    }

    fn number(&mut self) -> Result<f64> {
        let start = self.pos;
        let mut seen_dot = false;
        let mut seen_exp = false;
        while let Some(c) = self.peek() {
            match c {
                '0'..='9' => self.pos += 1,
                '.' if !seen_dot && !seen_exp => {
                    seen_dot = true;
                    self.pos += 1;
                }
                'e' | 'E' if !seen_exp => {
                    seen_exp = true;
                    self.pos += 1;
                    if matches!(self.peek(), Some('+') | Some('-')) {
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        s.parse::<f64>()
            .map_err(|_| CadError::Parse(format!("'{s}' is not a number")))
    }
}

/// A bare identifier used as a value: a named constant.
fn constant(name: &str) -> Result<f64> {
    match name {
        "pi" => Ok(PI),
        "tau" => Ok(TAU),
        "e" => Ok(std::f64::consts::E),
        other => Err(CadError::Arg(format!(
            "unknown name `{other}` (did you mean `${other}` for a variable?)"
        ))),
    }
}

/// A function call `name(args...)`.
fn call(name: &str, args: &[f64]) -> Result<f64> {
    let one = |f: fn(f64) -> f64| -> Result<f64> {
        if args.len() != 1 {
            return Err(CadError::Arg(format!("{name}() takes 1 argument")));
        }
        Ok(f(args[0]))
    };
    match name {
        "sin" => one(f64::sin),
        "cos" => one(f64::cos),
        "tan" => one(f64::tan),
        "asin" => one(f64::asin),
        "acos" => one(f64::acos),
        "atan" => one(f64::atan),
        "sqrt" => one(f64::sqrt),
        "abs" => one(f64::abs),
        "floor" => one(f64::floor),
        "ceil" => one(f64::ceil),
        "round" => one(f64::round),
        "sign" => one(f64::signum),
        // Degrees <-> radians, matching crate::math::{rad, deg}.
        "rad" => one(|d| d * PI / 180.0),
        "deg" => one(|r| r * 180.0 / PI),
        "atan2" => {
            if args.len() != 2 {
                return Err(CadError::Arg("atan2() takes 2 arguments".into()));
            }
            Ok(args[0].atan2(args[1]))
        }
        "pow" => {
            if args.len() != 2 {
                return Err(CadError::Arg("pow() takes 2 arguments".into()));
            }
            Ok(args[0].powf(args[1]))
        }
        "hypot" => {
            if args.len() != 2 {
                return Err(CadError::Arg("hypot() takes 2 arguments".into()));
            }
            Ok(args[0].hypot(args[1]))
        }
        "min" => {
            if args.is_empty() {
                return Err(CadError::Arg("min() needs at least 1 argument".into()));
            }
            Ok(args.iter().copied().fold(f64::INFINITY, f64::min))
        }
        "max" => {
            if args.is_empty() {
                return Err(CadError::Arg("max() needs at least 1 argument".into()));
            }
            Ok(args.iter().copied().fold(f64::NEG_INFINITY, f64::max))
        }
        "clamp" => {
            if args.len() != 3 {
                return Err(CadError::Arg(
                    "clamp() takes 3 arguments (x, lo, hi)".into(),
                ));
            }
            Ok(args[0].clamp(args[1], args[2]))
        }
        other => Err(CadError::Arg(format!("unknown function `{other}()`"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str) -> f64 {
        eval(s, &HashMap::new()).unwrap()
    }

    fn vars() -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("w".to_string(), 40.0);
        m.insert("n".to_string(), 8.0);
        m
    }

    #[test]
    fn precedence_and_parens() {
        assert_eq!(ev("2+3*4"), 14.0);
        assert_eq!(ev("(2+3)*4"), 20.0);
        assert_eq!(ev("2-3-4"), -5.0); // left-assoc
        assert_eq!(ev("12/2/3"), 2.0); // left-assoc
    }

    #[test]
    fn unary_and_power() {
        assert_eq!(ev("-5"), -5.0);
        assert_eq!(ev("-2^2"), -4.0); // ^ binds tighter than unary: -(2^2)
        assert_eq!(ev("2^3^2"), 512.0); // right-assoc: 2^(3^2)
        assert_eq!(ev("2^-2"), 0.25); // signed exponent
        assert_eq!(ev("3 + -2"), 1.0);
    }

    #[test]
    fn modulo() {
        assert_eq!(ev("10 % 3"), 1.0);
    }

    #[test]
    fn functions_and_constants() {
        assert_eq!(ev("sqrt(16)"), 4.0);
        assert_eq!(ev("max(3, 7, 1)"), 7.0);
        assert_eq!(ev("min(3, 7, 1)"), 1.0);
        assert_eq!(ev("abs(-9)"), 9.0);
        assert_eq!(ev("clamp(12, 0, 10)"), 10.0);
        assert!((ev("pi") - std::f64::consts::PI).abs() < 1e-12);
        assert!((ev("cos(0)") - 1.0).abs() < 1e-12);
        assert!((ev("rad(180)") - std::f64::consts::PI).abs() < 1e-12);
        assert!((ev("deg(pi)") - 180.0).abs() < 1e-12);
    }

    #[test]
    fn variables() {
        let v = vars();
        assert_eq!(eval("$w", &v).unwrap(), 40.0);
        assert_eq!(eval("$w*2", &v).unwrap(), 80.0);
        assert_eq!(eval("$w/4", &v).unwrap(), 10.0);
        assert_eq!(eval("360/$n", &v).unwrap(), 45.0);
        assert_eq!(eval("$w * 2 + 1", &v).unwrap(), 81.0); // whitespace ok
    }

    #[test]
    fn errors() {
        let v = vars();
        assert!(eval("$missing", &v).is_err());
        assert!(eval("1/0", &v).is_err());
        assert!(eval("10%0", &v).is_err());
        assert!(eval("2+", &v).is_err());
        assert!(eval("(2+3", &v).is_err());
        assert!(eval("2 3", &v).is_err()); // trailing input
        assert!(eval("bogus(2)", &v).is_err());
        assert!(eval("nope", &v).is_err()); // unknown constant
        assert!(eval("", &v).is_err());
    }

    #[test]
    fn format_numbers() {
        assert_eq!(fmt_num(8.0), "8");
        assert_eq!(fmt_num(40.0), "40");
        assert_eq!(fmt_num(10.5), "10.5");
        assert_eq!(fmt_num(-3.0), "-3");
        assert_eq!(fmt_num(1.0 / 3.0), "0.333333");
        assert_eq!(fmt_num(2.5e-1), "0.25");
    }

    #[test]
    fn scientific_literal() {
        assert_eq!(ev("1e3"), 1000.0);
        assert_eq!(ev("2.5e-1"), 0.25);
    }
}
