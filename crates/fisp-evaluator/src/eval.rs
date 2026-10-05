use std::ops::Deref;
use std::{iter::once, rc::Rc};

use crate::{
    builtins::{self, Op},
    defs::Definitions,
    env::Environment,
    expr::{Expression, Value},
};

#[derive(Debug)]
pub enum EvalError {
    UnresolvedSymbol(Expression),
    NotFunction(Value),
    Type {
        expected: &'static str,
        found: Value,
    },
    DivisionByZero,
    Overflow,
    /// A value passed to `eval` that is not valid code.
    Malformed(Value),
}

pub fn evaluate(expression: &Expression, defs: &Definitions) -> Result<Value, EvalError> {
    let environment = Rc::new(Environment::new());
    evaluate_with_env(expression, environment, defs)
}

pub fn evaluate_with_env(
    expression: &Expression,
    env: Rc<Environment>,
    defs: &Definitions,
) -> Result<Value, EvalError> {
    let mut cur_expr = expression;
    let mut next_application_body: Rc<Expression>;
    let mut cur_env = env;

    loop {
        match cur_expr {
            Expression::String(s) => return Ok(Value::String(s.clone())),
            Expression::Symbol(s) => {
                return match cur_env
                    .get(s)
                    .or_else(|| defs.get(s))
                    .cloned()
                    .or_else(|| builtins::lookup(s))
                {
                    Some(value) => Ok(value),
                    None => Err(EvalError::UnresolvedSymbol(cur_expr.clone())),
                };
            }
            Expression::Literal(value) => return Ok(value.clone()),
            Expression::If(cond, then_branch, else_branch) => {
                let cond_value = evaluate_with_env(cond, cur_env.clone(), defs)?;

                cur_expr = match cond_value {
                    Value::Nil => else_branch.as_ref(),
                    _ => then_branch.as_ref(),
                };
            }
            Expression::Lambda(param, body) => {
                return Ok(Value::Closure {
                    param: param.clone(),
                    body: body.clone(),
                    env: cur_env.clone(),
                });
            }
            Expression::Application(func_expr, args_exprs) => {
                let mut func = evaluate_with_env(func_expr, cur_env.clone(), defs)?;
                let (last_arg_expr, rest_args_exprs) =
                    args_exprs.split_last().ok_or_else(|| {
                        // TODO: should be a different error for no arguments supplied
                        EvalError::NotFunction(func.clone())
                    })?;

                for arg_expr in rest_args_exprs {
                    let arg = evaluate_with_env(arg_expr, cur_env.clone(), defs)?;
                    func = apply(func, arg, defs)?;
                }

                // last argument + application evaluation in tail position
                let last_arg_val = evaluate_with_env(last_arg_expr, cur_env.clone(), defs)?;
                let (next_body, next_env) = enter_frame(func, last_arg_val, defs)?;

                cur_env = next_env;
                next_application_body = next_body;
                cur_expr = &next_application_body;
            }
            Expression::Primitive(Op::Eval) => {
                let code = builtins::param(&cur_env, 0)?;

                next_application_body = Rc::new(unquote(&code)?);
                cur_expr = &next_application_body;
                cur_env = Rc::new(Environment::new());
            }
            Expression::Primitive(op) => return builtins::run(*op, &cur_env),
            Expression::Quote(expr) => return Ok(quote_expr(expr)),
        }
    }
}

pub fn quote_expr(expression: &Expression) -> Value {
    match expression {
        Expression::String(s) => Value::String(s.clone()),
        Expression::Symbol(s) => Value::Symbol(s.clone()),
        Expression::Literal(value) => value.clone(),
        Expression::If(cond, then_branch, else_branch) => Value::from(vec![
            Value::Symbol("if".to_owned()),
            quote_expr(cond.deref()),
            quote_expr(then_branch.deref()),
            quote_expr(else_branch.deref()),
        ]),
        Expression::Lambda(param, body) => Value::from(vec![
            Value::Symbol("lambda".to_owned()),
            Value::Symbol(param.to_owned().deref().to_owned()),
            quote_expr(body),
        ]),
        Expression::Application(func_expr, args_exprs) => once(quote_expr(func_expr.deref()))
            .chain(args_exprs.iter().map(quote_expr))
            .collect(),
        Expression::Primitive(op) => Value::Symbol(op.to_string()),
        Expression::Quote(expr) => Value::from(vec![
            Value::Symbol("quote".to_owned()),
            quote_expr(expr.deref()),
        ]),
    }
}

/// Turns quoted code back into an expression.
pub fn unquote(value: &Value) -> Result<Expression, EvalError> {
    let malformed = || EvalError::Malformed(value.clone());

    let items = match value {
        Value::Symbol(s) => return Ok(Expression::Symbol(s.clone())),
        Value::Cons(..) => list_items(value).ok_or_else(malformed)?,
        _ => return Ok(Expression::Literal(value.clone())),
    };

    let boxed = |value| unquote(value).map(Box::new);

    match items.as_slice() {
        [Value::Symbol(s), cond, then_branch, else_branch] if s == "if" => Ok(Expression::If(
            boxed(cond)?,
            boxed(then_branch)?,
            boxed(else_branch)?,
        )),
        [Value::Symbol(s), Value::Symbol(param), body] if s == "lambda" || s == "λ" => Ok(
            Expression::Lambda(param.as_str().into(), Rc::new(unquote(body)?)),
        ),
        [Value::Symbol(s), expr] if s == "quote" => Ok(Expression::Quote(boxed(expr)?)),
        [Value::Symbol(s), ..] if ["if", "lambda", "λ", "quote"].contains(&s.as_str()) => {
            Err(malformed())
        }
        [head] => unquote(head),
        [head, args @ ..] => Ok(Expression::Application(
            boxed(head)?,
            args.iter()
                .map(|arg| unquote(arg))
                .collect::<Result<_, _>>()?,
        )),
        [] => unreachable!("a cons has at least one item"),
    }
}

/// Collects the items of a proper list, or `None` if the list is improper.
fn list_items(mut value: &Value) -> Option<Vec<&Value>> {
    let mut items = Vec::new();
    loop {
        match value {
            Value::Nil => return Some(items),
            Value::Cons(car, cdr) => {
                items.push(car.as_ref());
                value = cdr;
            }
            _ => return None,
        }
    }
}

pub fn enter_frame(
    func: Value,
    arg: Value,
    defs: &Definitions,
) -> Result<(Rc<Expression>, Rc<Environment>), EvalError> {
    match func {
        Value::Closure { param, body, env } => {
            let mut frame = Environment::new().with_parent(env);
            frame.set(param.to_string(), arg);
            Ok((body, Rc::new(frame)))
        }
        Value::Symbol(ref name) => {
            let func = evaluate(&Expression::Symbol(name.clone()), defs)?;
            enter_frame(func, arg, defs)
        }
        _ => Err(EvalError::NotFunction(func)),
    }
}

pub fn apply(func: Value, arg: Value, defs: &Definitions) -> Result<Value, EvalError> {
    let (body, env) = enter_frame(func, arg, defs)?;
    evaluate_with_env(&body, env, defs)
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use super::*;
    fn lit(value: impl Into<Value>) -> Expression {
        Expression::Literal(value.into())
    }

    fn cons_v(a: Value, b: Value) -> Value {
        Value::Cons(Box::new(a), Box::new(b))
    }

    fn cons_e(a: Expression, b: Expression) -> Expression {
        Expression::Application(Box::new(Expression::Symbol("cons".to_owned())), vec![a, b])
    }

    fn list_v(values: Vec<Value>) -> Value {
        let mut result = Value::Nil;
        for value in values.into_iter().rev() {
            result = cons_v(value, result);
        }
        result
    }

    #[test]
    fn string_should_return_string_value() {
        let expr = Expression::String("H".to_owned());
        let result = evaluate(&expr, &Definitions::new()).unwrap();

        assert_eq!(result, Value::String("H".to_owned()));
    }

    #[test]
    fn number_should_return_number_value() {
        let expr = lit(42);
        let result = evaluate(&expr, &Definitions::new()).unwrap();

        assert_eq!(result, Value::Number(42));
    }

    #[test]
    fn nil_should_return_nil_value() {
        let expr = lit(Value::Nil);
        let result = evaluate(&expr, &Definitions::new()).unwrap();

        assert_eq!(result, Value::Nil);
    }

    #[test]
    fn true_should_return_true_value() {
        let expr = lit(Value::True);
        let result = evaluate(&expr, &Definitions::new()).unwrap();

        assert_eq!(result, Value::True);
    }

    #[test]
    fn unresolved_symbol_should_error() {
        let expr = Expression::Symbol("x".to_owned());
        let result = evaluate(&expr, &Definitions::new());

        assert!(result.is_err());
    }

    #[test]
    fn if_should_return_true_branch() {
        let expr = Expression::If(
            Box::new(lit(Value::True)),
            Box::new(lit(1)),
            Box::new(lit(2)),
        );
        let result = evaluate(&expr, &Definitions::new()).unwrap();

        assert_eq!(result, Value::Number(1));
    }

    #[test]
    fn if_should_return_false_branch() {
        let expr = Expression::If(
            Box::new(lit(Value::Nil)),
            Box::new(lit(1)),
            Box::new(lit(2)),
        );
        let result = evaluate(&expr, &Definitions::new()).unwrap();

        assert_eq!(result, Value::Number(2));
    }

    #[test]
    fn lambda_should_return_closure() {
        let expr = Expression::Lambda("x".into(), Rc::new(Expression::Symbol("x".to_owned())));

        let result = evaluate(&expr, &Definitions::new()).unwrap();

        match result {
            Value::Closure {
                param,
                body,
                env: _,
            } => {
                assert_eq!(param.deref(), "x");
                assert_eq!(body.deref(), &Expression::Symbol("x".to_owned()));
            }
            _ => panic!("Expected closure"),
        }
    }

    #[test]
    fn application_should_return_result_of_function() {
        let expr = Expression::Application(
            Box::new(Expression::Lambda(
                "x".into(),
                Rc::new(Expression::Symbol("x".to_owned())),
            )),
            vec![lit(42)],
        );

        let result = evaluate(&expr, &Definitions::new()).unwrap();

        assert_eq!(result, Value::Number(42));
    }

    #[test]
    fn curried_application_should_apply_one_argument_at_a_time() {
        let expr = Expression::Application(
            Box::new(Expression::Lambda(
                "a".into(),
                Rc::new(Expression::Lambda(
                    "b".into(),
                    Rc::new(Expression::Symbol("a".to_owned())),
                )),
            )),
            vec![lit(1), lit(2)],
        );

        let result = evaluate(&expr, &Definitions::new()).unwrap();

        assert_eq!(result, Value::Number(1));
    }

    #[test]
    fn closure_should_capture_its_environment() {
        let expr = Expression::Application(
            Box::new(Expression::Application(
                Box::new(Expression::Lambda(
                    "a".into(),
                    Rc::new(Expression::Lambda(
                        "b".into(),
                        Rc::new(Expression::Symbol("a".to_owned())),
                    )),
                )),
                vec![lit(1)],
            )),
            vec![lit(2)],
        );

        let result = evaluate(&expr, &Definitions::new()).unwrap();

        assert_eq!(result, Value::Number(1));
    }

    #[test]
    fn applying_non_function_should_error() {
        let expr = Expression::Application(Box::new(lit(1)), vec![lit(2)]);

        assert!(matches!(
            evaluate(&expr, &Definitions::new()),
            Err(EvalError::NotFunction(_))
        ));
    }

    fn identity() -> Expression {
        Expression::Lambda("x".into(), Rc::new(Expression::Symbol("x".to_owned())))
    }

    #[test]
    fn symbol_should_resolve_to_definition() {
        let mut defs = Definitions::new();
        defs.set("answer".to_owned(), Value::Number(42));

        let result = evaluate(&Expression::Symbol("answer".to_owned()), &defs).unwrap();

        assert_eq!(result, Value::Number(42));
    }

    #[test]
    fn parameter_should_shadow_definition() {
        let mut defs = Definitions::new();
        defs.set("x".to_owned(), Value::Number(1));

        let expr = Expression::Application(Box::new(identity()), vec![lit(2)]);
        let result = evaluate(&expr, &defs).unwrap();

        assert_eq!(result, Value::Number(2));
    }

    #[test]
    fn closure_should_look_up_globals_at_call_time() {
        // f = (lambda (x) (g x)) is created before g exists. g is only defined afterwards.
        let mut defs = Definitions::new();
        let f_expr = Expression::Lambda(
            "x".into(),
            Rc::new(Expression::Application(
                Box::new(Expression::Symbol("g".to_owned())),
                vec![Expression::Symbol("x".to_owned())],
            )),
        );
        let f = evaluate(&f_expr, &defs).unwrap();
        let g = evaluate(&identity(), &defs).unwrap();
        defs.set("f".to_owned(), f);
        defs.set("g".to_owned(), g);

        let expr =
            Expression::Application(Box::new(Expression::Symbol("f".to_owned())), vec![lit(7)]);
        let result = evaluate(&expr, &defs).unwrap();

        assert_eq!(result, Value::Number(7));
    }

    #[test]
    fn removed_definition_should_be_unresolved() {
        let mut defs = Definitions::new();
        defs.set("answer".to_owned(), Value::Number(42));
        defs.remove("answer");

        let result = evaluate(&Expression::Symbol("answer".to_owned()), &defs);

        assert!(matches!(result, Err(EvalError::UnresolvedSymbol(_))));
    }

    #[test]
    fn cons_should_build_list() {
        let expr = cons_e(lit(1), cons_e(lit(2), lit(Value::Nil)));
        let result = evaluate(&expr, &Definitions::new()).unwrap();

        assert_eq!(result, list_v(vec![Value::Number(1), Value::Number(2)]));
    }

    #[test]
    fn factorial_should_compute_correctly() {
        let mut defs = Definitions::new();
        let fact_expr = Expression::Lambda(
            "n".into(),
            Rc::new(Expression::If(
                Box::new(Expression::Application(
                    Box::new(Expression::Symbol("=".to_owned())),
                    vec![Expression::Symbol("n".to_owned()), lit(0)],
                )),
                Box::new(lit(1)),
                Box::new(Expression::Application(
                    Box::new(Expression::Symbol("*".to_owned())),
                    vec![
                        Expression::Symbol("n".to_owned()),
                        Expression::Application(
                            Box::new(Expression::Symbol("fact".to_owned())),
                            vec![Expression::Application(
                                Box::new(Expression::Symbol("-".to_owned())),
                                vec![Expression::Symbol("n".to_owned()), lit(1)],
                            )],
                        ),
                    ],
                )),
            )),
        );
        let fact = evaluate(&fact_expr, &defs).unwrap();
        defs.set("fact".to_owned(), fact);

        let expr = Expression::Application(
            Box::new(Expression::Symbol("fact".to_owned())),
            vec![lit(5)],
        );
        let result = evaluate(&expr, &defs).unwrap();

        assert_eq!(result, Value::Number(120));
    }
}
