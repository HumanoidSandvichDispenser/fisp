use std::rc::Rc;

use crate::{
    builtins,
    defs::Definitions,
    expr::{Expression, Type, Value},
};

#[derive(Debug, PartialEq)]
pub enum ParseError {
    Incomplete,
    Leftover,
    Unbound,
    TypeMismatch,
    BadName,
}

pub enum Token<'a> {
    Atom(Value),
    Ref(&'a str),
    Name(&'a str),
    Lambda,
    If,
    Quote,
}

pub enum Scope<'a> {
    Empty,
    Binding(&'a str, Type, &'a Scope<'a>),
}

impl Scope<'_> {
    fn get(&self, name: &str) -> Option<Type> {
        match self {
            Scope::Empty => None,
            Scope::Binding(n, t, _) if *n == name => Some(t.clone()),
            Scope::Binding(_, _, parent) => parent.get(name),
        }
    }
}

const RESERVED: [&str; 6] = ["lambda", "λ", "if", "quote", "nil", "t"];

pub fn parse(segments: &[&str], defs: &Definitions) -> Result<(Expression, Type), ParseError> {
    let mut tokens = tokenize(segments)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter();
    let parsed = parse_expr(&mut tokens, &Scope::Empty, defs)?;

    match tokens.next() {
        None => Ok(parsed),
        Some(_) => Err(ParseError::Leftover),
    }
}

/// Splits a path into tokens, dropping empty and `.` segments.
pub fn tokenize<'a>(segments: &[&'a str]) -> impl Iterator<Item = Result<Token<'a>, ParseError>> {
    segments
        .iter()
        .copied()
        .filter(|s| !s.is_empty() && *s != ".")
        .map(classify)
}

fn parse_expr<'a>(
    tokens: &mut impl Iterator<Item = Token<'a>>,
    scope: &Scope,
    defs: &Definitions,
) -> Result<(Expression, Type), ParseError> {
    match tokens.next().ok_or(ParseError::Incomplete)? {
        Token::Atom(value) => Ok((Expression::Literal(value), Type::Value)),

        Token::Ref(name) => {
            type_of(name, scope, defs)?;
            Ok((Expression::Symbol(name.to_owned()), Type::Value))
        }

        Token::Name(name) => {
            let arity = type_of(name, scope, defs)?.arity();
            let args = (0..arity)
                .map(|_| parse_value(tokens, scope, defs))
                .collect::<Result<Vec<_>, _>>()?;
            let symbol = Expression::Symbol(name.to_owned());

            let expr = if args.is_empty() {
                symbol
            } else {
                Expression::Application(Box::new(symbol), args)
            };

            Ok((expr, Type::Value))
        }

        Token::Lambda => {
            let param_name = match tokens.next().ok_or(ParseError::Incomplete)? {
                Token::Name(name) if !is_function_name(name, defs) => name,
                _ => return Err(ParseError::BadName),
            };

            let inner = Scope::Binding(param_name, Type::Value, scope);
            let (body, lambda_type) = parse_expr(tokens, &inner, defs)?;

            Ok((
                Expression::Lambda(param_name.into(), Rc::new(body)),
                Type::Function(Box::new(lambda_type)),
            ))
        }

        Token::If => {
            let cond = parse_value(tokens, scope, defs)?;
            let (then_branch, then_type) = parse_expr(tokens, scope, defs)?;
            let (else_branch, else_type) = parse_expr(tokens, scope, defs)?;

            if then_type != else_type {
                return Err(ParseError::TypeMismatch);
            }

            Ok((
                Expression::If(Box::new(cond), Box::new(then_branch), Box::new(else_branch)),
                then_type,
            ))
        }

        Token::Quote => {
            // get next expr and wrap it in a literal
            let next = parse_expr(tokens, scope, defs)?;
            Ok((Expression::Quote(Box::new(next.0)), Type::Value))
        }
    }
}

/// Parses an expression that must not be a function, such as an argument or a condition.
fn parse_value<'a>(
    tokens: &mut impl Iterator<Item = Token<'a>>,
    scope: &Scope,
    defs: &Definitions,
) -> Result<Expression, ParseError> {
    match parse_expr(tokens, scope, defs)? {
        (expr, Type::Value) => Ok(expr),
        _ => Err(ParseError::TypeMismatch),
    }
}

/// Reads a segment as a number, `nil`, `t`, or otherwise a symbol.
fn atom(segment: &str) -> Value {
    match segment {
        "nil" => Value::Nil,
        "t" => Value::True,
        _ => segment
            .parse()
            .map(Value::Number)
            .unwrap_or_else(|_| Value::Symbol(segment.to_owned())),
    }
}

fn classify(segment: &str) -> Result<Token<'_>, ParseError> {
    match segment {
        "lambda" | "λ" => return Ok(Token::Lambda),
        "if" => return Ok(Token::If),
        "quote" | "^" => return Ok(Token::Quote),
        _ => {}
    }

    if let Some(name) = segment.strip_prefix('@') {
        return if name.is_empty() {
            Err(ParseError::BadName)
        } else {
            Ok(Token::Ref(name))
        };
    }

    // `^` alone quotes the next expression (matched above), and `^name` is just the symbol.
    if let Some(name) = segment.strip_prefix('^') {
        return if name.is_empty() {
            Err(ParseError::BadName)
        } else {
            Ok(Token::Atom(Value::Symbol(name.to_owned())))
        };
    }

    if segment.contains(',') {
        let list = segment.split(',').filter(|s| !s.is_empty()).map(atom);
        return Ok(Token::Atom(list.collect()));
    }

    if segment.len() >= 2 && segment.starts_with('"') && segment.ends_with('"') {
        // TODO: escape sequences
        let string_content = &segment[1..segment.len() - 1];
        return Ok(Token::Atom(Value::String(string_content.to_owned())));
    }

    if is_valid_name(segment) {
        return Ok(Token::Name(segment));
    }

    match atom(segment) {
        Value::Symbol(_) => Err(ParseError::BadName),
        value => Ok(Token::Atom(value)),
    }
}

/// Returns true if the segment is a valid name for a parameter or reference.
///
/// A name is valid if it is not empty, does not start with a reserved character, does not contain a
/// reserved character (like filepaths), is not a number, and is not a reserved name.
fn is_valid_name(segment: &str) -> bool {
    !segment.is_empty()
        && !segment.starts_with(['.', '\'', '@', '^', '"'])
        && !segment.contains([',', '/', ':'])
        && segment.parse::<i64>().is_err()
        && !RESERVED.contains(&segment)
}

/// Returns true if the name is a builtin or a definition that takes arguments.
///
/// Parameters may not shadow these names. Otherwise, the arity of a name would depend on where a
/// lambda body ends, and a recursive definition could have more than one consistent arity.
fn is_function_name(name: &str, defs: &Definitions) -> bool {
    builtins::type_of(name).is_some() || defs.get(name).is_some_and(|def| def.def_type.arity() > 0)
}

fn type_of(name: &str, scope: &Scope, defs: &Definitions) -> Result<Type, ParseError> {
    scope
        .get(name)
        .or_else(|| defs.get(name).map(|def| def.def_type.clone()))
        .or_else(|| builtins::type_of(name))
        .ok_or(ParseError::Unbound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{defs::Definition, eval::evaluate};

    fn fun(ret: Type) -> Type {
        Type::Function(Box::new(ret))
    }

    fn parse_path(path: &str, defs: &Definitions) -> Result<(Expression, Type), ParseError> {
        parse(&path.split('/').collect::<Vec<_>>(), defs)
    }

    fn run_with(path: &str, defs: &Definitions) -> Value {
        let (expr, _) = parse_path(path, defs).unwrap();
        evaluate(&expr, defs).unwrap()
    }

    fn run(path: &str) -> Value {
        run_with(path, &Definitions::new())
    }

    fn error(path: &str) -> ParseError {
        parse_path(path, &Definitions::new()).unwrap_err()
    }

    fn type_of_path(path: &str) -> Type {
        parse_path(path, &Definitions::new()).unwrap().1
    }

    fn list(values: Vec<Value>) -> Value {
        values.into_iter().collect()
    }

    #[test]
    fn atoms_should_parse() {
        assert_eq!(run("42"), Value::Number(42));
        assert_eq!(run("-5"), Value::Number(-5));
        assert_eq!(run("nil"), Value::Nil);
        assert_eq!(run("t"), Value::True);
        assert_eq!(run("^abc"), Value::Symbol("abc".to_owned()));
        assert_eq!(run("\"hi\""), Value::String("hi".to_owned()));
    }

    #[test]
    fn list_literal_should_parse_atoms() {
        let expected = list(vec![1.into(), 2.into(), Value::Symbol("a".to_owned())]);

        assert_eq!(run("1,2,a"), expected);
    }

    #[test]
    fn nested_application_should_follow_arity() {
        assert_eq!(run("+/1/*/2/3"), Value::Number(7));
        assert_eq!(run("car/cdr/1,2,3"), Value::Number(2));
    }

    #[test]
    fn empty_and_dot_segments_should_be_dropped() {
        assert_eq!(run("+//1/./2"), Value::Number(3));
    }

    #[test]
    fn missing_arguments_should_be_incomplete() {
        assert_eq!(error("+/1"), ParseError::Incomplete);
        assert_eq!(error("lambda"), ParseError::Incomplete);
        assert_eq!(error("λ/x"), ParseError::Incomplete);
        assert_eq!(error(""), ParseError::Incomplete);
    }

    #[test]
    fn extra_segments_should_be_leftover() {
        assert_eq!(error("+/1/2/3"), ParseError::Leftover);
    }

    #[test]
    fn lambda_should_have_function_type() {
        assert_eq!(type_of_path("λ/x/+/x/1"), fun(Type::Value));
        assert_eq!(
            type_of_path("lambda/a/lambda/b/+/a/b"),
            fun(fun(Type::Value))
        );
    }

    #[test]
    fn parameter_should_not_be_applied() {
        assert_eq!(error("λ/f/f/1"), ParseError::Leftover);
    }

    #[test]
    fn bad_parameter_name_should_error() {
        assert_eq!(error("λ/1/1"), ParseError::BadName);
        assert_eq!(error("λ/if/1"), ParseError::BadName);
        assert_eq!(error("λ/car/1"), ParseError::BadName);
        assert_eq!(error("λ/^x/1"), ParseError::BadName);
        assert_eq!(error("λ/@x/1"), ParseError::BadName);
    }

    #[test]
    fn parameter_should_not_shadow_function_definition() {
        let mut defs = Definitions::new();
        defs.set(
            "g".to_owned(),
            Definition {
                def_type: fun(Type::Value),
                value: Value::Nil,
            },
        );
        defs.set(
            "c".to_owned(),
            Definition {
                def_type: Type::Value,
                value: Value::Number(1),
            },
        );

        assert_eq!(parse_path("λ/g/g", &defs).unwrap_err(), ParseError::BadName);
        assert_eq!(parse_path("λ/c/c", &defs).unwrap().1, fun(Type::Value));
    }

    #[test]
    fn reference_should_not_be_applied() {
        assert_eq!(type_of_path("@car"), Type::Value);
        assert_eq!(run("call/call/@+/1/2"), Value::Number(3));
    }

    #[test]
    fn if_branches_should_have_same_type() {
        assert_eq!(run("if/nil/1/2"), Value::Number(2));
        assert_eq!(type_of_path("if/t/1/@car"), Type::Value);
        assert_eq!(error("if/t/1/λ/x/x"), ParseError::TypeMismatch);
    }

    #[test]
    fn function_typed_argument_should_be_rejected() {
        assert_eq!(error("+/λ/x/x/1"), ParseError::TypeMismatch);
    }

    #[test]
    fn quote_should_take_full_expression() {
        let expected = Value::Cons(
            Box::new(Value::Symbol("car".to_owned())),
            Box::new(Value::Cons(
                Box::new(Value::Symbol("cdr".to_owned())),
                Box::new(Value::Cons(
                    Box::new(Value::Symbol("x".to_owned())),
                    Box::new(Value::Nil),
                )),
            )),
        );

        assert_eq!(run("^/car/cdr/x"), expected);
    }

    #[test]
    fn quote_should_take_next_symbol() {
        assert_eq!(run("^car"), Value::Symbol("car".to_owned()));
    }

    #[test]
    fn unknown_name_should_be_unbound() {
        assert_eq!(error("foo"), ParseError::Unbound);
        assert_eq!(error("@foo"), ParseError::Unbound);
    }

    #[test]
    fn malformed_segment_should_be_bad_name() {
        assert_eq!(error("\""), ParseError::BadName);
        assert_eq!(error("'"), ParseError::BadName);
        assert_eq!(error("a:b"), ParseError::BadName);
    }

    #[test]
    fn recursive_definition_should_parse_and_run() {
        let mut defs = Definitions::new();
        let source = "lambda/n/if/=/n/0/1/*/n/fact/-/n/1";

        defs.set(
            "fact".to_owned(),
            Definition {
                def_type: fun(Type::Value),
                value: Value::Nil,
            },
        );
        let (expr, ty) = parse_path(source, &defs).unwrap();
        assert_eq!(ty, fun(Type::Value));

        let value = evaluate(&expr, &defs).unwrap();
        defs.set(
            "fact".to_owned(),
            Definition {
                def_type: ty,
                value,
            },
        );

        assert_eq!(run_with("fact/5", &defs), Value::Number(120));
    }
}
