//! Safe arithmetic expression evaluator for computed (virtual) tags.
//!
//! Grammar (left-associative except `^`):
//!
//! ```text
//! expr   := term (('+' | '-') term)*
//! term   := unary (('*' | '/' | '%') unary)*
//! unary  := ('-' | '+')* power
//! power  := atom ('^' unary)?
//! atom   := NUMBER | NAME | '{' any-name '}' | NAME '(' args ')' | '(' expr ')'
//! ```
//!
//! Variables are resolved through a caller-supplied closure, so the engine
//! decides what a name means (tag labels, `p<poll>:<addr>` aliases…). There is
//! no reflection, no I/O and no way to call anything but the built-in math
//! functions, which makes evaluation safe for user-authored expressions.

use std::collections::HashMap;

// ------------------------------------------------------------------ tokens

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Name(String),
    Op(char), // + - * / % ^ ( ) ,
}

fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let ch: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < ch.len() {
        let c = ch[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // Brace-quoted variable: any characters up to '}' — lets users
        // reference tags whose names contain spaces or non-latin letters.
        if c == '{' {
            let mut j = i + 1;
            while j < ch.len() && ch[j] != '}' {
                j += 1;
            }
            if j >= ch.len() {
                return Err("не закрыта фигурная скобка «{»".into());
            }
            let name: String = ch[i + 1..j].iter().collect::<String>().trim().to_string();
            if name.is_empty() {
                return Err("пустое имя переменной в {}".into());
            }
            out.push(Tok::Name(name));
            i = j + 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && i + 1 < ch.len() && ch[i + 1].is_ascii_digit()) {
            let mut j = i;
            let mut dot = false;
            while j < ch.len() && (ch[j].is_ascii_digit() || (ch[j] == '.' && !dot)) {
                if ch[j] == '.' {
                    dot = true;
                }
                j += 1;
            }
            let s: String = ch[i..j].iter().collect();
            let v: f64 = s.parse().map_err(|_| format!("плохое число «{s}»"))?;
            out.push(Tok::Num(v));
            i = j;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while j < ch.len() && (ch[j].is_alphanumeric() || ch[j] == '_') {
                j += 1;
            }
            let name: String = ch[i..j].iter().collect();
            out.push(Tok::Name(name));
            i = j;
            continue;
        }
        if matches!(c, '+' | '-' | '*' | '/' | '%' | '^' | '(' | ')' | ',') {
            out.push(Tok::Op(c));
            i += 1;
            continue;
        }
        return Err(format!("непонятный символ «{c}»"));
    }
    Ok(out)
}

// --------------------------------------------------------------------- AST

#[derive(Debug, Clone)]
enum Node {
    Num(f64),
    Var(String),
    Neg(Box<Node>),
    Bin(char, Box<Node>, Box<Node>),
    Call(String, Vec<Node>),
}

/// A compiled expression, ready for repeated evaluation.
#[derive(Debug, Clone)]
pub struct Expr {
    src: String,
    ast: Node,
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn eat_op(&mut self, c: char) -> bool {
        if let Some(Tok::Op(x)) = self.peek() {
            if *x == c {
                self.pos += 1;
                return true;
            }
        }
        false
    }
    fn expect_op(&mut self, c: char) -> Result<(), String> {
        if self.eat_op(c) {
            Ok(())
        } else {
            Err(format!("ожидалось «{c}»"))
        }
    }

    fn expr(&mut self) -> Result<Node, String> {
        let mut left = self.term()?;
        loop {
            let (op, take) = if self.eat_op('+') {
                ('+', true)
            } else if self.eat_op('-') {
                ('-', true)
            } else {
                (' ', false)
            };
            if !take {
                break;
            }
            let right = self.term()?;
            left = Node::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn term(&mut self) -> Result<Node, String> {
        let mut left = self.unary()?;
        loop {
            let op = if self.eat_op('*') {
                '*'
            } else if self.eat_op('/') {
                '/'
            } else if self.eat_op('%') {
                '%'
            } else {
                ' '
            };
            if op == ' ' {
                break;
            }
            let right = self.unary()?;
            left = Node::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Node, String> {
        if self.eat_op('-') {
            return Ok(Node::Neg(Box::new(self.unary()?)));
        }
        if self.eat_op('+') {
            return self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> Result<Node, String> {
        let base = self.atom()?;
        if self.eat_op('^') {
            // right-associative: 2^-3^2 == 2^(-9)
            let exp = self.unary()?;
            return Ok(Node::Bin('^', Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<Node, String> {
        match self.peek().cloned() {
            Some(Tok::Num(v)) => {
                self.pos += 1;
                Ok(Node::Num(v))
            }
            Some(Tok::Name(name)) => {
                self.pos += 1;
                if self.eat_op('(') {
                    let mut args = Vec::new();
                    if !self.eat_op(')') {
                        loop {
                            args.push(self.expr()?);
                            if self.eat_op(',') {
                                continue;
                            }
                            self.expect_op(')')?;
                            break;
                        }
                    }
                    return Ok(Node::Call(name.to_lowercase(), args));
                }
                Ok(Node::Var(name))
            }
            Some(Tok::Op('(')) => {
                self.pos += 1;
                let e = self.expr()?;
                self.expect_op(')')?;
                Ok(e)
            }
            other => Err(match other {
                Some(t) => format!("неожиданный токен {t:?}"),
                None => "выражение оборвалось".into(),
            }),
        }
    }
}

// ------------------------------------------------------------------ builtin

fn call(name: &str, args: &[f64]) -> Result<f64, String> {
    let need = |n: usize| -> Result<(), String> {
        if args.len() == n {
            Ok(())
        } else {
            Err(format!("{}: ожидалось {n} аргумент(ов), передано {}", name, args.len()))
        }
    };
    match name {
        "min" => {
            if args.len() < 2 {
                return Err("min: нужно хотя бы 2 аргумента".into());
            }
            Ok(args.iter().cloned().fold(f64::INFINITY, f64::min))
        }
        "max" => {
            if args.len() < 2 {
                return Err("max: нужно хотя бы 2 аргумента".into());
            }
            Ok(args.iter().cloned().fold(f64::NEG_INFINITY, f64::max))
        }
        "avg" => {
            if args.is_empty() {
                return Err("avg: нужен хотя бы 1 аргумент".into());
            }
            Ok(args.iter().sum::<f64>() / args.len() as f64)
        }
        "abs" => {
            need(1)?;
            Ok(args[0].abs())
        }
        "sqrt" => {
            need(1)?;
            Ok(args[0].sqrt())
        }
        "round" => {
            need(1)?;
            Ok(args[0].round())
        }
        "floor" => {
            need(1)?;
            Ok(args[0].floor())
        }
        "ceil" => {
            need(1)?;
            Ok(args[0].ceil())
        }
        "pow" => {
            need(2)?;
            Ok(args[0].powf(args[1]))
        }
        other => Err(format!("неизвестная функция «{other}» (доступны min, max, avg, abs, sqrt, round, floor, ceil, pow)")),
    }
}

// --------------------------------------------------------------- public API

impl Expr {
    /// Parse and compile. The error message is user-facing (shown in the UI).
    pub fn compile(src: &str) -> Result<Expr, String> {
        let s = src.trim();
        if s.is_empty() {
            return Err("пустое выражение".into());
        }
        let toks = tokenize(s)?;
        if toks.is_empty() {
            return Err("пустое выражение".into());
        }
        let mut p = Parser { toks, pos: 0 };
        let ast = p.expr()?;
        if p.pos != p.toks.len() {
            return Err("лишние символы в конце выражения".into());
        }
        Ok(Expr { src: s.to_string(), ast })
    }

    pub fn source(&self) -> &str {
        &self.src
    }

    /// Evaluate; `vars` resolves a variable name to its value (`None` =
    /// unknown variable → error naming it).
    pub fn eval(
        &self,
        vars: &mut dyn FnMut(&str) -> Option<f64>,
    ) -> Result<f64, String> {
        let v = eval_node(&self.ast, vars)?;
        if v.is_finite() {
            Ok(v)
        } else {
            Err("результат не является конечным числом".into())
        }
    }
}

fn eval_node(n: &Node, vars: &mut dyn FnMut(&str) -> Option<f64>) -> Result<f64, String> {
    match n {
        Node::Num(v) => Ok(*v),
        Node::Var(name) => vars(name).ok_or_else(|| format!("нет тэга «{name}»")),
        Node::Neg(a) => Ok(-eval_node(a, vars)?),
        Node::Bin(op, a, b) => {
            let x = eval_node(a, vars)?;
            let y = eval_node(b, vars)?;
            match op {
                '+' => Ok(x + y),
                '-' => Ok(x - y),
                '*' => Ok(x * y),
                '/' => {
                    if y == 0.0 {
                        Err("деление на ноль".into())
                    } else {
                        Ok(x / y)
                    }
                }
                '%' => {
                    if y == 0.0 {
                        Err("деление на ноль (остаток)".into())
                    } else {
                        Ok(x % y)
                    }
                }
                '^' => Ok(x.powf(y)),
                other => Err(format!("неизвестная операция «{other}»")),
            }
        }
        Node::Call(name, args) => {
            let mut vals = Vec::with_capacity(args.len());
            for a in args {
                vals.push(eval_node(a, vars)?);
            }
            call(name, &vals)
        }
    }
}

/// Convenience: evaluate against a plain map (used in tests and tools).
pub fn eval_with_map(src: &str, map: &HashMap<String, f64>) -> Result<f64, String> {
    let e = Expr::compile(src)?;
    e.eval(&mut |n: &str| map.get(n).copied())
}

// -------------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn vars<'a>(list: &'a [(&'a str, f64)]) -> impl FnMut(&str) -> Option<f64> + 'a {
        let m: HashMap<String, f64> = list
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect();
        move |n: &str| m.get(n).copied()
    }

    #[test]
    fn arithmetic_and_precedence() {
        let v = vars(&[]);
        assert_eq!(Expr::compile("2 + 3 * 4").unwrap().eval(&mut vars(&[])).unwrap(), 14.0);
        assert_eq!(Expr::compile("(2 + 3) * 4").unwrap().eval(&mut vars(&[])).unwrap(), 20.0);
        assert_eq!(Expr::compile("2 ^ 3 ^ 2").unwrap().eval(&mut vars(&[])).unwrap(), 512.0);
        assert_eq!(Expr::compile("-3 + 5").unwrap().eval(&mut vars(&[])).unwrap(), 2.0);
        assert_eq!(Expr::compile("7 % 3").unwrap().eval(&mut vars(&[])).unwrap(), 1.0);
        assert_eq!(Expr::compile("10 / 4").unwrap().eval(&mut vars(&[])).unwrap(), 2.5);
        let _ = v;
    }

    #[test]
    fn variables_bare_and_braced() {
        let mut e = vars(&[("T1", 20.0), ("Т2 реактор", 30.0)]);
        assert_eq!(Expr::compile("T1 * 2").unwrap().eval(&mut e).unwrap(), 40.0);
        let mut e2 = vars(&[("Т2 реактор", 30.0)]);
        // brace syntax allows spaces / cyrillic
        assert_eq!(Expr::compile("{Т2 реактор} + 1").unwrap().eval(&mut e2).unwrap(), 31.0);
    }

    #[test]
    fn functions_work() {
        assert_eq!(Expr::compile("min(3, 2, 5)").unwrap().eval(&mut vars(&[])).unwrap(), 2.0);
        assert_eq!(Expr::compile("max(3, 2, 5)").unwrap().eval(&mut vars(&[])).unwrap(), 5.0);
        assert_eq!(Expr::compile("abs(0 - 7)").unwrap().eval(&mut vars(&[])).unwrap(), 7.0);
        assert_eq!(Expr::compile("sqrt(9)").unwrap().eval(&mut vars(&[])).unwrap(), 3.0);
        assert_eq!(Expr::compile("round(2.6)").unwrap().eval(&mut vars(&[])).unwrap(), 3.0);
        assert_eq!(Expr::compile("floor(2.9)").unwrap().eval(&mut vars(&[])).unwrap(), 2.0);
        assert_eq!(Expr::compile("ceil(2.1)").unwrap().eval(&mut vars(&[])).unwrap(), 3.0);
        assert_eq!(Expr::compile("pow(2, 10)").unwrap().eval(&mut vars(&[])).unwrap(), 1024.0);
        assert_eq!(Expr::compile("avg(2, 4, 6)").unwrap().eval(&mut vars(&[])).unwrap(), 4.0);
    }

    #[test]
    fn errors_are_user_friendly() {
        assert!(Expr::compile("").unwrap_err().contains("пуст"));
        assert!(Expr::compile("2 +").is_err());
        assert!(Expr::compile("{oops").is_err());
        assert!(Expr::compile("1 2").is_err());
        let e = Expr::compile("1/0").unwrap();
        assert_eq!(e.eval(&mut vars(&[])).unwrap_err(), "деление на ноль");
        let e = Expr::compile("НетТакого + 1").unwrap();
        assert!(e.eval(&mut vars(&[])).unwrap_err().contains("НетТакого"));
    }

    #[test]
    fn eval_with_map_helper() {
        let m: HashMap<String, f64> = [("a".to_string(), 3.0)].into_iter().collect();
        assert_eq!(eval_with_map("a ^ 2 + 1", &m).unwrap(), 10.0);
        assert!(eval_with_map("b + 1", &m).is_err());
    }
}
