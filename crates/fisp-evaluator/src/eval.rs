use std::rc::Rc;

use crate::{
    env::Environment,
    expr::{Expression, Value},
};

#[derive(Debug)]
pub enum EvalError {
    UnresolvedSymbol(Expression),
}

pub fn evaluate(expression: &Expression) -> Result<Value, EvalError> {
    let environment = Rc::new(Environment::new());
    evaluate_with_env(expression, environment)
}

pub fn evaluate_with_env(
    expression: &Expression,
    env: Rc<Environment>,
) -> Result<Value, EvalError> {
    match expression {
        Expression::Nil => Ok(Value::Nil),
        Expression::True => Ok(Value::True),
        Expression::Number(n) => Ok(Value::Number(*n)),
        Expression::String(s) => Ok(Value::String(s.clone())),
        Expression::Symbol(s) => match env.get(s).cloned() {
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
            let cond_value = evaluate_with_env(cond, env.clone())?;
            match cond_value {
                Value::Nil => evaluate_with_env(else_branch, env.clone()),
                _ => evaluate_with_env(then_branch, env),
            }
        }
        Expression::Lambda(param, body) => {
            todo!()
        },
        Expression::Application(func_expr, args_exprs) => {
            todo!()
        }
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
        let result = evaluate(&expr).unwrap();

        assert_eq!(result, Value::String("H".to_owned()));
    }

    #[test]
    fn number_should_return_number_value() {
        let expr = Expression::Number(42);
        let result = evaluate(&expr).unwrap();

        assert_eq!(result, Value::Number(42));
    }

    #[test]
    fn nil_should_return_nil_value() {
        let expr = Expression::Nil;
        let result = evaluate(&expr).unwrap();

        assert_eq!(result, Value::Nil);
    }

    #[test]
    fn true_should_return_true_value() {
        let expr = Expression::True;
        let result = evaluate(&expr).unwrap();

        assert_eq!(result, Value::True);
    }

    #[test]
    fn unresolved_symbol_should_error() {
        let expr = Expression::Symbol("x".to_owned());
        let result = evaluate(&expr);

        assert!(result.is_err());
    }

    #[test]
    fn quote_should_return_list_of_symbols() {
        let expr = Expression::Quote(vec!["x".to_owned(), "y".to_string()]);
        let result = evaluate(&expr).unwrap();

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
        let result = evaluate(&expr).unwrap();

        assert_eq!(result, Value::Number(1));
    }

    #[test]
    fn if_should_return_false_branch() {
        let expr = Expression::If(
            Box::new(Expression::Nil),
            Box::new(Expression::Number(1)),
            Box::new(Expression::Number(2)),
        );
        let result = evaluate(&expr).unwrap();

        assert_eq!(result, Value::Number(2));
    }

    #[test]
    fn lambda_should_return_closure() {
        let expr = Expression::Lambda(
            Box::new(Expression::Symbol("x".to_owned())),
            Box::new(Expression::Symbol("x".to_owned())),
        );

        let result = evaluate(&expr).unwrap();

        match result {
            Value::Closure {
                param,
                body,
                env: _,
            } => {
                let param = param.deref();
                let body = body.deref();

                assert_eq!(param.clone(), "x".to_owned());
                assert_eq!(body.clone(), Expression::Symbol("x".to_owned()));
            }
            _ => panic!("Expected closure"),
        }
    }

    #[test]
    fn application_should_return_result_of_function() {
        let expr = Expression::Application(
            Box::new(Expression::Lambda(
                Box::new(Expression::Symbol("x".to_owned())),
                Box::new(Expression::Symbol("x".to_owned())),
            )),
            vec![Expression::Number(42)],
        );

        let mut env = Rc::new(Environment::new());
        let env_mut = Rc::get_mut(&mut env).unwrap();
        env_mut.set("x".to_owned(), Value::Number(42));

        let result = evaluate_with_env(&expr, env.clone()).unwrap();

        assert_eq!(result, Value::Number(42));
    }
}
