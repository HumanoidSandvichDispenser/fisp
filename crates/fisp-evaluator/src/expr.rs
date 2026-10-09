use std::{fmt, rc::Rc};

use crate::{
    builtins::{self, Op, PARAMS},
    env::Environment,
    parser,
};

#[derive(Debug, Clone)]
pub enum Value {
    /// The empty list, which is also used to represent the boolean value false.
    Nil,

    /// The boolean value true (t).
    True,

    /// A numeric value.
    Number(i64),

    /// A string value.
    String(String),

    /// A unique identifier that can be used as a variable name.
    Symbol(String),

    /// A cons cell in a linked list.
    Cons(Box<Value>, Box<Value>),

    /// A closure that captures its environment.
    Closure {
        param: Rc<str>,
        body: Rc<Expression>,
        env: Rc<Environment>,
    },
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Nil, Value::Nil) => true,
            (Value::True, Value::True) => true,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Symbol(a), Value::Symbol(b)) => a == b,
            (Value::Cons(a1, a2), Value::Cons(b1, b2)) => a1 == b1 && a2 == b2,
            _ => false,
        }
    }

    fn ne(&self, other: &Self) -> bool {
        !self.eq(other)
    }
}

impl Value {
    /// Returns an s-expression representation of the value, e.g. `(1 2 3)`.
    pub fn sexp(&self) -> Sexp<'_> {
        Sexp(self)
    }
}

/// Displays a value as an s-expression. Created by [`Value::sexp`].
pub struct Sexp<'a>(&'a Value);

impl fmt::Display for Sexp<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Value::Nil => write!(f, "nil"),
            Value::True => write!(f, "t"),
            Value::Number(n) => write!(f, "{n}"),
            Value::Symbol(s) => write!(f, "{s}"),
            Value::String(s) => {
                write!(f, "\"")?;
                for c in s.chars() {
                    match c {
                        '\\' => write!(f, "\\\\")?,
                        '"' => write!(f, "\\\"")?,
                        '\n' => write!(f, "\\n")?,
                        '\r' => write!(f, "\\r")?,
                        '\t' => write!(f, "\\t")?,
                        _ => write!(f, "{c}")?,
                    }
                }
                write!(f, "\"")
            },
            Value::Cons(car, cdr) => {
                write!(f, "({}", car.sexp())?;
                let mut rest = cdr.as_ref();
                loop {
                    match rest {
                        Value::Nil => break,
                        Value::Cons(car, cdr) => {
                            write!(f, " {}", car.sexp())?;
                            rest = cdr;
                        }
                        other => {
                            write!(f, " . {}", other.sexp())?;
                            break;
                        }
                    }
                }
                write!(f, ")")
            }
            Value::Closure { param, .. } => write!(f, "#<lambda {param}>"),
        }
    }
}

impl Value {
    /// Returns an adapter that displays the value as a path expression that evaluates back to
    /// the value, e.g. `1,2,3`.
    pub fn path_expr(&self) -> PathExpr<'_> {
        PathExpr(self)
    }
}

/// Displays a value as a path expression. Created by [`Value::path_expr`].
pub struct PathExpr<'a>(&'a Value);

/// Words that are not valid names, but are read as a symbol after a `^` prefix.
const KEYWORDS: [&str; 4] = ["lambda", "λ", "if", "quote"];

impl fmt::Display for PathExpr<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Value::Nil => write!(f, "nil"),
            Value::True => write!(f, "t"),
            Value::Number(n) => write!(f, "{n}"),
            Value::String(s) => write_string(f, s),
            Value::Symbol(s) if parser::is_valid_name(s) || KEYWORDS.contains(&s.as_str()) => {
                write!(f, "^{s}")
            }
            // not a name, so take it out of a one-element list literal instead
            Value::Symbol(s) => match comma_atom(self.0) {
                // a leading `@` or `^` would be read as a prefix, so it cannot come first
                Some(_) if s.starts_with(['@', '^']) => write!(f, "car/cdr/nil,{s}"),
                Some(_) => write!(f, "car/{s},"),
                None => write!(f, "#<symbol {s}>"),
            },
            Value::Cons(car, cdr) => match comma_list(self.0) {
                Some(list) => write!(f, "{list}"),
                None => write!(f, "cons/{}/{}", car.path_expr(), cdr.path_expr()),
            },
            Value::Closure { param, body, env } => {
                write!(f, "λ/{param}/")?;
                write_expr(f, body, env, &[param])
            }
        }
    }
}

/// Writes a string literal, percent-encoding every ASCII character that is not URL-unreserved.
fn write_string(f: &mut fmt::Formatter, s: &str) -> fmt::Result {
    write!(f, ":")?;
    for c in s.chars() {
        if c.is_ascii() && !(c.is_ascii_alphanumeric() || "-_.~".contains(c)) {
            write!(f, "%{:02X}", c as u8)?;
        } else {
            write!(f, "{c}")?;
        }
    }
    Ok(())
}

fn comma_atom(value: &Value) -> Option<String> {
    match value {
        Value::Nil => Some("nil".to_owned()),
        Value::True => Some("t".to_owned()),
        Value::Number(n) => Some(n.to_string()),
        Value::Symbol(s)
            if !s.is_empty()
                && !s.contains([',', '/', '\0'])
                && !["nil", "t"].contains(&s.as_str())
                && s.parse::<i64>().is_err() =>
        {
            Some(s.clone())
        }
        _ => None,
    }
}

/// Writes a proper list as a list literal (`1,2,3`), if every element can be written as one.
fn comma_list(value: &Value) -> Option<String> {
    let mut atoms = Vec::new();
    let mut rest = value;

    while let Value::Cons(car, cdr) = rest {
        atoms.push(comma_atom(car)?);
        rest = cdr;
    }

    if !matches!(rest, Value::Nil) || atoms.first()?.starts_with(['@', '^']) {
        return None;
    }

    Some(match atoms.as_slice() {
        [atom] => format!("{atom},"),
        atoms => atoms.join(","),
    })
}

/// Writes a closure body, replacing captured names with their values. `bound` holds the
/// parameters in scope, which shadow the captured environment.
fn write_expr(
    f: &mut fmt::Formatter,
    expr: &Expression,
    env: &Environment,
    bound: &[&str],
) -> fmt::Result {
    let captured = |name: &str| (!bound.contains(&name)).then(|| env.get(name)).flatten();

    match expr {
        Expression::String(s) => write_string(f, s),
        Expression::Literal(value) => write!(f, "{}", value.path_expr()),
        Expression::Symbol(name) => match (captured(name), builtins::arity(name)) {
            (Some(value), _) => write!(f, "{}", value.path_expr()),
            // eta-expand, since a bare builtin name would take arguments
            (None, Some(arity)) => {
                let params = &PARAMS[..arity];
                for param in params {
                    write!(f, "λ/{param}/")?;
                }
                write!(f, "{}{name}", "@".repeat(arity))?;
                params.iter().try_for_each(|param| write!(f, "/{param}"))
            }
            (None, None) => write!(f, "{name}"),
        },
        Expression::Application(head, args) => {
            write!(f, "{}", "@".repeat(args.len()))?;
            match head.as_ref() {
                // `@name` reads the name as a reference, so it does not take arguments
                Expression::Symbol(name) if captured(name).is_none() => write!(f, "{name}")?,
                head => {
                    write!(f, "/")?;
                    write_expr(f, head, env, bound)?;
                }
            }
            args.iter().try_for_each(|arg| {
                write!(f, "/")?;
                write_expr(f, arg, env, bound)
            })
        }
        Expression::If(cond, then_branch, else_branch) => {
            write!(f, "if/")?;
            write_expr(f, cond, env, bound)?;
            write!(f, "/")?;
            write_expr(f, then_branch, env, bound)?;
            write!(f, "/")?;
            write_expr(f, else_branch, env, bound)
        }
        Expression::Lambda(param, body) => {
            write!(f, "λ/{param}/")?;
            write_expr(f, body, env, &[bound, &[param]].concat())
        }
        // quoted code is data, so nothing inside it is replaced
        Expression::Quote(quoted) => match quoted.as_ref() {
            Expression::Symbol(name) => write!(f, "^{name}"),
            quoted => {
                write!(f, "^/")?;
                write_expr(f, quoted, &Environment::new(), &[])
            }
        },
        // the body of a builtin, which reads its arguments from the parameters
        Expression::Primitive(op) => {
            let params = PARAMS[..op.arity()]
                .iter()
                .map(|param| Expression::Symbol((*param).to_owned()))
                .collect();
            let name = match op {
                Op::Mul => "mul".to_owned(),
                op => op.to_string(),
            };
            let call = Expression::Application(Box::new(Expression::Symbol(name)), params);
            write_expr(f, &call, env, bound)
        }
    }
}

impl FromIterator<Value> for Value {
    /// Builds a proper list from the items, in order.
    fn from_iter<I: IntoIterator<Item = Value>>(iter: I) -> Self {
        let items: Vec<Value> = iter.into_iter().collect();
        items.into_iter().rev().fold(Value::Nil, |tail, head| {
            Value::Cons(Box::new(head), Box::new(tail))
        })
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Number(n)
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.to_string())
    }
}

impl From<Rc<str>> for Value {
    fn from(s: Rc<str>) -> Self {
        Value::String(s.to_string())
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        if b { Value::True } else { Value::Nil }
    }
}

impl From<Vec<Value>> for Value {
    fn from(vec: Vec<Value>) -> Self {
        vec.into_iter().collect()
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum Expression {
    String(String),
    Symbol(String),
    Literal(Value),
    If(Box<Expression>, Box<Expression>, Box<Expression>),
    Lambda(Rc<str>, Rc<Expression>),
    Application(Box<Expression>, Vec<Expression>),
    Quote(Box<Expression>),
    /// Body of a built-in closure.
    Primitive(Op),
}

impl From<Value> for Expression {
    fn from(value: Value) -> Self {
        Expression::Literal(value)
    }
}

#[cfg(test)]
mod tests {
    use crate::runtime::Runtime;

    fn path_expr(runtime: &Runtime, source: &str) -> String {
        runtime.evaluate(source).unwrap().path_expr().to_string()
    }

    fn assert_path_expr(source: &str, expected: &str) {
        let runtime = Runtime::new();
        assert_eq!(path_expr(&runtime, source), expected);
        assert_eq!(
            runtime.evaluate(expected).unwrap(),
            runtime.evaluate(source).unwrap()
        );
    }

    fn assert_closure_path_expr(runtime: &Runtime, source: &str, expected: &str, arg: &str) {
        assert_eq!(path_expr(runtime, source), expected);
        assert_eq!(
            runtime.evaluate(&format!("@/{expected}/{arg}")).unwrap(),
            runtime.evaluate(&format!("@/{source}/{arg}")).unwrap()
        );
    }

    #[test]
    fn atoms_should_print_as_literals() {
        assert_path_expr("5", "5");
        assert_path_expr("-5", "-5");
        assert_path_expr("nil", "nil");
        assert_path_expr("t", "t");
    }

    #[test]
    fn strings_should_percent_encode_reserved_characters() {
        assert_path_expr(":hello%20world", ":hello%20world");
        assert_path_expr(":a%2Fb%2Cc%25", ":a%2Fb%2Cc%25");
        assert_path_expr(":line\\nbreak", ":line%0Abreak");
        assert_path_expr(":%C3%A9", ":é");
        assert_path_expr(":", ":");
    }

    #[test]
    fn symbols_should_print_quoted() {
        assert_path_expr("^abc", "^abc");
        assert_path_expr("^car", "^car");
        assert_path_expr("car/^/λ/x/x", "^lambda");
    }

    #[test]
    fn symbols_that_are_not_names_should_print_from_a_list() {
        assert_path_expr("car/:x,", "car/:x,");
        assert_path_expr("car/cdr/nil,@x", "car/cdr/nil,@x");
    }

    #[test]
    fn lists_of_atoms_should_print_as_list_literals() {
        assert_path_expr("1,2,3", "1,2,3");
        assert_path_expr("1,", "1,");
        assert_path_expr("cons/1/cons/nil/cons/t/nil", "1,nil,t");
        assert_path_expr("^/+/1/2", "+,1,2");
    }

    #[test]
    fn other_lists_should_print_as_cons() {
        assert_path_expr("cons/1/2", "cons/1/2");
        assert_path_expr("cons/1,2/cons/:x/nil", "cons/1,2/cons/:x/nil");
        assert_path_expr("cons/:x/1,2", "cons/:x/1,2");
        assert_path_expr("cons/car/cdr/nil,@x/nil", "cons/car/cdr/nil,@x/nil");
    }

    #[test]
    fn closures_should_print_as_lambdas() {
        let runtime = Runtime::new();
        assert_closure_path_expr(&runtime, "λ/x/+/x/1", "λ/x/@@+/x/1", "5");
        assert_closure_path_expr(&runtime, "λ/n/if/=/n/0/1/2", "λ/n/if/@@=/n/0/1/2", "0");
    }

    #[test]
    fn closures_should_substitute_captured_values() {
        let runtime = Runtime::new();
        assert_closure_path_expr(&runtime, "@/λ/a/λ/b/+/a/b/1", "λ/b/@@+/1/b", "5");
        assert_closure_path_expr(&runtime, "@/λ/s/λ/x/cons/s/x/:hi", "λ/x/@@cons/:hi/x", "1");
        assert_closure_path_expr(
            &runtime,
            "@/λ/f/λ/x/@f/x/λ/y/mul/y/2",
            "λ/x/@/λ/y/@@mul/y/2/x",
            "5",
        );
    }

    #[test]
    fn closures_should_not_substitute_shadowed_or_quoted_names() {
        let runtime = Runtime::new();
        assert_closure_path_expr(&runtime, "@/λ/a/λ/b/λ/a/+/a/b/1", "λ/b/λ/a/@@+/a/b", "5/2");
        assert_closure_path_expr(&runtime, "@/λ/a/λ/b/^a/1", "λ/b/^a", "5");
    }

    #[test]
    fn partial_builtins_should_print_as_calls() {
        let runtime = Runtime::new();
        assert_closure_path_expr(&runtime, "@+/1", "λ/b/@@+/1/b", "5");
        assert_closure_path_expr(&runtime, "@mul/3", "λ/b/@@mul/3/b", "5");
    }

    #[test]
    fn closures_should_keep_global_names() {
        let mut runtime = Runtime::new();
        runtime
            .define("factorial", "λ/n/if/=/n/0/1/mul/n/@factorial/-/n/1")
            .unwrap();
        assert_closure_path_expr(
            &runtime,
            "factorial",
            "λ/n/if/@@=/n/0/1/@@mul/n/@factorial/@@-/n/1",
            "5",
        );
    }
}
