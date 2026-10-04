use std::rc::Rc;

use crate::{
    builtins,
    defs::Definitions,
    expr::{Expression, Value},
};

#[derive(Debug, PartialEq)]
pub enum ParseError {
    Incomplete,
    BadName,
}

pub enum Token<'a> {
    Atom(Value),
    /// A name that does not take arguments, written after a prefix (as in `@@f` or `^car`).
    Ref(&'a str),
    Name(&'a str),
    Apply,
    Lambda,
    If,
    Quote,
}

/// The parameters in scope, innermost first.
pub enum Scope<'a> {
    Empty,
    Binding(&'a str, &'a Scope<'a>),
}

impl Scope<'_> {
    fn contains(&self, name: &str) -> bool {
        match self {
            Scope::Empty => false,
            Scope::Binding(n, parent) => *n == name || parent.contains(name),
        }
    }
}

const RESERVED: [&str; 6] = ["lambda", "λ", "if", "quote", "nil", "t"];

pub fn parse(segments: &[&str], defs: &Definitions) -> Result<Expression, ParseError> {
    let mut tokens = tokenize(segments)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .peekable();
    let head = parse_expr(&mut tokens, &Scope::Empty, defs)?;

    let args = std::iter::from_fn(|| {
        tokens
            .peek()
            .is_some()
            .then(|| parse_expr(&mut tokens, &Scope::Empty, defs))
    })
    .collect::<Result<Vec<_>, _>>()?;

    Ok(application(head, args))
}

/// Splits a path into tokens, dropping empty and `.` segments.
pub fn tokenize<'a>(segments: &[&'a str]) -> impl Iterator<Item = Result<Token<'a>, ParseError>> {
    segments
        .iter()
        .copied()
        .filter(|s| !s.is_empty() && *s != ".")
        .flat_map(tokenize_segment)
}

fn tokenize_segment(segment: &str) -> impl Iterator<Item = Result<Token<'_>, ParseError>> {
    let rest = segment.trim_start_matches(['@', '^']);
    let prefix = &segment[..segment.len() - rest.len()];

    let marks = prefix.chars().map(|c| match c {
        '@' => Ok(Token::Apply),
        _ => Ok(Token::Quote),
    });

    let token = (!rest.is_empty()).then(|| match classify(rest) {
        // if the segment is a name, lambda, if, or quote, and it is prefixed with `@` or `^`, treat
        // it as a reference instead of a name, so that it does not take arguments
        Ok(Token::Name(_) | Token::Lambda | Token::If | Token::Quote) if !prefix.is_empty() => {
            Ok(Token::Ref(rest))
        }
        token => token,
    });

    marks.chain(token)
}

fn parse_expr<'a>(
    tokens: &mut impl Iterator<Item = Token<'a>>,
    scope: &Scope,
    defs: &Definitions,
) -> Result<Expression, ParseError> {
    match tokens.next().ok_or(ParseError::Incomplete)? {
        Token::Atom(value) => Ok(Expression::Literal(value)),

        Token::Ref(name) => Ok(Expression::Symbol(name.to_owned())),

        Token::Name(name) => {
            let arity = arity_of(name, scope, defs);
            let args = (0..arity)
                .map(|_| parse_expr(tokens, scope, defs))
                .collect::<Result<Vec<_>, _>>()?;

            Ok(application(Expression::Symbol(name.to_owned()), args))
        }

        Token::Apply => {
            let head = parse_expr(tokens, scope, defs)?;
            let arg = parse_expr(tokens, scope, defs)?;

            Ok(application(head, vec![arg]))
        }

        Token::Lambda => {
            let param_name = match tokens.next().ok_or(ParseError::Incomplete)? {
                Token::Name(name) if builtins::arity(name).is_none() => name,
                _ => return Err(ParseError::BadName),
            };

            let body = parse_expr(tokens, &Scope::Binding(param_name, scope), defs)?;

            Ok(Expression::Lambda(param_name.into(), Rc::new(body)))
        }

        Token::If => {
            let mut next = || parse_expr(tokens, scope, defs).map(Box::new);
            Ok(Expression::If(next()?, next()?, next()?))
        }

        Token::Quote => {
            // get next expr and wrap it in a literal
            let next = parse_expr(tokens, scope, defs)?;
            Ok(Expression::Quote(Box::new(next)))
        }
    }
}

/// Applies the head to the arguments, or returns the head alone if there are none.
fn application(head: Expression, args: Vec<Expression>) -> Expression {
    if args.is_empty() {
        head
    } else {
        Expression::Application(Box::new(head), args)
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
        "quote" => return Ok(Token::Quote),
        _ => {}
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

/// Returns how many arguments the parser consumes after a name.
///
/// Only builtins take arguments. Parameters and definitions are values, which are applied with
/// `@`. Unknown names are also values, looked up when they are evaluated (late binding), so a
/// definition can refer to one that does not exist yet.
fn arity_of(name: &str, scope: &Scope, defs: &Definitions) -> usize {
    if scope.contains(name) || defs.get(name).is_some() {
        0
    } else {
        builtins::arity(name).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::{EvalError, evaluate};

    fn parse_path(path: &str, defs: &Definitions) -> Result<Expression, ParseError> {
        parse(&path.split('/').collect::<Vec<_>>(), defs)
    }

    fn eval_with(path: &str, defs: &Definitions) -> Result<Value, EvalError> {
        evaluate(&parse_path(path, defs).unwrap(), defs)
    }

    fn run_with(path: &str, defs: &Definitions) -> Value {
        eval_with(path, defs).unwrap()
    }

    fn run(path: &str) -> Value {
        run_with(path, &Definitions::new())
    }

    fn error(path: &str) -> ParseError {
        parse_path(path, &Definitions::new()).unwrap_err()
    }

    fn list(values: Vec<Value>) -> Value {
        values.into_iter().collect()
    }

    fn defs_with(name: &str, value: Value) -> Definitions {
        let mut defs = Definitions::new();
        defs.set(name.to_owned(), value);
        defs
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
        assert_eq!(error("@/f"), ParseError::Incomplete);
        assert_eq!(error("@"), ParseError::Incomplete);
        assert_eq!(error(""), ParseError::Incomplete);
    }

    #[test]
    fn trailing_segments_should_apply_to_result() {
        assert_eq!(run("λ/x/x/0"), Value::Number(0));
        assert_eq!(run("λ/x/+/x/1/5"), Value::Number(6));
        assert_eq!(run("λ/a/λ/b/a/1/2"), Value::Number(1));
    }

    #[test]
    fn applying_a_non_function_should_fail_at_runtime() {
        let result = eval_with("+/1/2/3", &Definitions::new());

        assert!(matches!(result, Err(EvalError::NotFunction(_))));
    }

    #[test]
    fn apply_should_take_head_and_argument() {
        let defs = defs_with("add", run("λ/a/λ/b/+/a/b"));

        assert_eq!(run_with("@/@/add/1/2", &defs), Value::Number(3));
        assert_eq!(run_with("@@add/1/2", &defs), Value::Number(3));
        assert_eq!(run_with("@@/add/1/2", &defs), Value::Number(3));
        assert_eq!(run_with("@/@add/1/2", &defs), Value::Number(3));
    }

    #[test]
    fn prefixed_builtin_should_not_take_arguments() {
        assert_eq!(run("@@+/1/2"), Value::Number(3));
        assert_eq!(run("show/@car/1,2"), Value::String("1".to_owned()));
        assert_eq!(error("@/+/1/2"), ParseError::Incomplete);
    }

    #[test]
    fn lambda_should_be_passable_as_argument() {
        assert_eq!(run("@/λ/x/*/x/x/7"), Value::Number(49));
        assert_eq!(run("call/λ/x/*/x/x/7"), Value::Number(49));
    }

    #[test]
    fn if_should_return_functions() {
        assert_eq!(run("if/t/λ/x/x/λ/y/1/5"), Value::Number(5));
        assert_eq!(run("if/t/1/λ/x/x"), Value::Number(1));
    }

    #[test]
    fn parameter_should_not_take_arguments() {
        // The 1 is applied to the lambda, not to f.
        assert_eq!(run("λ/f/f/1"), Value::Number(1));
        assert_eq!(run("λ/f/@f/1/λ/x/+/x/1"), Value::Number(2));
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
    fn parameter_should_shadow_definition() {
        let defs = defs_with("g", Value::Number(1));

        assert_eq!(run_with("λ/g/g/5", &defs), Value::Number(5));
    }

    #[test]
    fn symbol_should_be_applied_as_global_function() {
        assert_eq!(run("@^car/1,2"), Value::Number(1));
        assert_eq!(run("λ/f/@f/1,2/^cdr"), list(vec![2.into()]));
        assert_eq!(run("call/call/^+/1/2"), Value::Number(3));
    }

    #[test]
    fn quote_should_take_full_expression() {
        let expected = Value::Cons(
            Box::new(Value::Symbol("car".to_owned())),
            Box::new(Value::Cons(
                Box::new(Value::Cons(
                    Box::new(Value::Symbol("cdr".to_owned())),
                    Box::new(Value::Cons(
                        Box::new(Value::Symbol("x".to_owned())),
                        Box::new(Value::Nil),
                    )),
                )),
                Box::new(Value::Nil),
            )),
        );

        assert_eq!(run("^/car/cdr/x"), expected);
    }

    #[test]
    fn quote_should_take_next_symbol() {
        assert_eq!(run("^car"), Value::Symbol("car".to_owned()));
        assert_eq!(run("^if"), Value::Symbol("if".to_owned()));
        assert_eq!(run("^5"), Value::Number(5));
    }

    #[test]
    fn quoted_apply_prefix_should_match_separate_segments() {
        assert_eq!(run("^@@f/x/y"), run("^/@/@/f/x/y"));
        assert_eq!(run("^/@car/@cdr/x"), run("^/car/cdr/x"));
    }

    #[test]
    fn unknown_name_should_fail_at_runtime() {
        let defs = Definitions::new();

        assert!(matches!(
            eval_with("foo", &defs),
            Err(EvalError::UnresolvedSymbol(_))
        ));
        assert!(matches!(
            eval_with("@foo/1", &defs),
            Err(EvalError::UnresolvedSymbol(_))
        ));
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
        let source = "lambda/n/if/=/n/0/1/*/n/@fact/-/n/1";

        let value = run_with(source, &defs);
        defs.set("fact".to_owned(), value);

        assert_eq!(run_with("fact/5", &defs), Value::Number(120));
    }
}
