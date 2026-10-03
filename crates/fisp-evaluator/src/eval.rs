use std::rc::Rc;

use crate::{
    defs::Definitions,
    env::Environment,
    expr::{Expression, Value},
};

#[derive(Debug)]
pub enum EvalError {
    UnresolvedSymbol(Expression),
    NotFunction(Value),
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
    match expression {
        Expression::Nil => Ok(Value::Nil),
        Expression::True => Ok(Value::True),
        Expression::Number(n) => Ok(Value::Number(*n)),
        Expression::String(s) => Ok(Value::String(s.clone())),
        // Locals shadow globals; globals are looked up at call time, not captured.
        Expression::Symbol(s) => match env.get(s).or_else(|| defs.get(s)).cloned() {
            Some(value) => Ok(value),
            None => Err(EvalError::UnresolvedSymbol(expression.clone())),
        },
        Expression::Quote(symbols) => {
            let mut result = Value::Nil;

            for symbol in symbols.iter().rev() {
                result = Value::Cons(Box::new(Value::Symbol(symbol.clone())), Box::new(result));
            }

            Ok(result)
        }
        Expression::If(cond, then_branch, else_branch) => {
            let cond_value = evaluate_with_env(cond, env.clone(), defs)?;
            match cond_value {
                Value::Nil => evaluate_with_env(else_branch, env.clone(), defs),
                _ => evaluate_with_env(then_branch, env, defs),
            }
        }
        Expression::Lambda(param, body) => Ok(Value::Closure {
            param: param.clone(),
            body: body.clone(),
            env,
        }),
        Expression::Application(func_expr, args_exprs) => {
            let mut func = evaluate_with_env(func_expr, env.clone(), defs)?;

            for arg_expr in args_exprs {
                let arg = evaluate_with_env(arg_expr, env.clone(), defs)?;
                func = apply(func, arg, defs)?;
            }

            Ok(func)
        }
    }
}

pub fn apply(func: Value, arg: Value, defs: &Definitions) -> Result<Value, EvalError> {
    match func {
        Value::Closure { param, body, env } => {
            let mut frame = Environment::new().with_parent(env);
            frame.set(param.to_string(), arg);
            evaluate_with_env(&body, Rc::new(frame), defs)
        }
        _ => Err(EvalError::NotFunction(func)),
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use super::*;

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
        let expr = Expression::Number(42);
        let result = evaluate(&expr, &Definitions::new()).unwrap();

        assert_eq!(result, Value::Number(42));
    }

    #[test]
    fn nil_should_return_nil_value() {
        let expr = Expression::Nil;
        let result = evaluate(&expr, &Definitions::new()).unwrap();

        assert_eq!(result, Value::Nil);
    }

    #[test]
    fn true_should_return_true_value() {
        let expr = Expression::True;
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
    fn quote_should_return_list_of_symbols() {
        let expr = Expression::Quote(vec!["x".to_owned(), "y".to_string()]);
        let result = evaluate(&expr, &Definitions::new()).unwrap();

        assert_eq!(
            result,
            cons_v(
                Value::Symbol("x".to_owned()),
                cons_v(Value::Symbol("y".to_owned()), Value::Nil)
            )
        );
    }

    #[test]
    fn if_should_return_true_branch() {
        let expr = Expression::If(
            Box::new(Expression::True),
            Box::new(Expression::Number(1)),
            Box::new(Expression::Number(2)),
        );
        let result = evaluate(&expr, &Definitions::new()).unwrap();

        assert_eq!(result, Value::Number(1));
    }

    #[test]
    fn if_should_return_false_branch() {
        let expr = Expression::If(
            Box::new(Expression::Nil),
            Box::new(Expression::Number(1)),
            Box::new(Expression::Number(2)),
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
            vec![Expression::Number(42)],
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
            vec![Expression::Number(1), Expression::Number(2)],
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
                vec![Expression::Number(1)],
            )),
            vec![Expression::Number(2)],
        );

        let result = evaluate(&expr, &Definitions::new()).unwrap();

        assert_eq!(result, Value::Number(1));
    }

    #[test]
    fn applying_non_function_should_error() {
        let expr =
            Expression::Application(Box::new(Expression::Number(1)), vec![Expression::Number(2)]);

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

        let expr = Expression::Application(Box::new(identity()), vec![Expression::Number(2)]);
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
        defs.set("f".to_owned(), f);
        defs.set("g".to_owned(), evaluate(&identity(), &defs).unwrap());

        let expr = Expression::Application(
            Box::new(Expression::Symbol("f".to_owned())),
            vec![Expression::Number(7)],
        );
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
}
