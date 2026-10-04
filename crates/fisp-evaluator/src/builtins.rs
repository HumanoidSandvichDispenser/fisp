use std::{
    fmt::{self, Display},
    rc::Rc,
};

use crate::{
    env::Environment,
    eval::EvalError,
    expr::{Expression, Value},
};

const PARAMS: [&str; 2] = ["a", "b"];

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    NumEq,
    Lt,
    Gt,
    Car,
    Cdr,
    Cons,
    Atom,
    Eq,
    Show,
}

impl Op {
    pub fn from_name(name: &str) -> Option<Op> {
        match name {
            "+" => Some(Op::Add),
            "-" => Some(Op::Sub),
            "*" | "mul" => Some(Op::Mul),
            "div" => Some(Op::Div),
            "mod" => Some(Op::Mod),
            "=" => Some(Op::NumEq),
            "<" => Some(Op::Lt),
            ">" => Some(Op::Gt),
            "car" => Some(Op::Car),
            "cdr" => Some(Op::Cdr),
            "cons" => Some(Op::Cons),
            "atom" => Some(Op::Atom),
            "eq" => Some(Op::Eq),
            "show" => Some(Op::Show),
            _ => None,
        }
    }

    pub fn arity(self) -> usize {
        match self {
            Op::Car | Op::Cdr | Op::Atom | Op::Show => 1,
            _ => 2,
        }
    }
}

impl Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "*",
            Op::Div => "div",
            Op::Mod => "mod",
            Op::NumEq => "=",
            Op::Lt => "<",
            Op::Gt => ">",
            Op::Car => "car",
            Op::Cdr => "cdr",
            Op::Cons => "cons",
            Op::Atom => "atom",
            Op::Eq => "eq",
            Op::Show => "show",
        };
        write!(f, "{name}")
    }
}

pub fn lookup(name: &str) -> Option<Value> {
    if name == "call" {
        // (lambda (f) (lambda (x) (f x)))
        let body = Expression::Application(
            Box::new(Expression::Symbol("f".to_owned())),
            vec![Expression::Symbol("x".to_owned())],
        );
        return Some(curry(&["f", "x"], body));
    }

    let op = Op::from_name(name)?;
    Some(curry(&PARAMS[..op.arity()], Expression::Primitive(op)))
}

fn curry(params: &[&str], body: Expression) -> Value {
    let body = params[1..].iter().rev().fold(body, |body, param| {
        Expression::Lambda((*param).into(), Rc::new(body))
    });

    Value::Closure {
        param: params[0].into(),
        body: Rc::new(body),
        env: Rc::new(Environment::new()),
    }
}

/// Returns how many arguments a builtin takes, or `None` if the name is not a builtin.
pub fn arity(name: &str) -> Option<usize> {
    match name {
        "call" => Some(2),
        _ => Op::from_name(name).map(Op::arity),
    }
}

pub fn run(op: Op, env: &Environment) -> Result<Value, EvalError> {
    let arg = |i: usize| {
        env.get(PARAMS[i])
            .cloned()
            .ok_or_else(|| EvalError::UnresolvedSymbol(Expression::Symbol(PARAMS[i].to_owned())))
    };

    match op {
        Op::Add => arithmetic(arg(0)?, arg(1)?, i64::checked_add),
        Op::Sub => arithmetic(arg(0)?, arg(1)?, i64::checked_sub),
        Op::Mul => arithmetic(arg(0)?, arg(1)?, i64::checked_mul),
        Op::Div => division(arg(0)?, arg(1)?, i64::checked_div),
        Op::Mod => division(arg(0)?, arg(1)?, i64::checked_rem),
        Op::NumEq => comparison(arg(0)?, arg(1)?, |a, b| a == b),
        Op::Lt => comparison(arg(0)?, arg(1)?, |a, b| a < b),
        Op::Gt => comparison(arg(0)?, arg(1)?, |a, b| a > b),
        Op::Car => match arg(0)? {
            Value::Cons(car, _) => Ok(*car),
            value => Err(type_error("cons", value)),
        },
        Op::Cdr => match arg(0)? {
            Value::Cons(_, cdr) => Ok(*cdr),
            value => Err(type_error("cons", value)),
        },
        Op::Cons => Ok(Value::Cons(Box::new(arg(0)?), Box::new(arg(1)?))),
        Op::Atom => Ok(Value::from(!matches!(arg(0)?, Value::Cons(..)))),
        Op::Eq => Ok(Value::from(arg(0)? == arg(1)?)),
        Op::Show => Ok(Value::String(arg(0)?.to_string())),
    }
}

fn number(value: Value) -> Result<i64, EvalError> {
    match value {
        Value::Number(n) => Ok(n),
        value => Err(type_error("number", value)),
    }
}

fn arithmetic(a: Value, b: Value, op: fn(i64, i64) -> Option<i64>) -> Result<Value, EvalError> {
    op(number(a)?, number(b)?)
        .map(Value::Number)
        .ok_or(EvalError::Overflow)
}

fn division(a: Value, b: Value, op: fn(i64, i64) -> Option<i64>) -> Result<Value, EvalError> {
    let (a, b) = (number(a)?, number(b)?);
    if b == 0 {
        return Err(EvalError::DivisionByZero);
    }
    op(a, b).map(Value::Number).ok_or(EvalError::Overflow)
}

fn comparison(a: Value, b: Value, op: fn(i64, i64) -> bool) -> Result<Value, EvalError> {
    Ok(Value::from(op(number(a)?, number(b)?)))
}

fn type_error(expected: &'static str, found: Value) -> EvalError {
    EvalError::Type { expected, found }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{defs::Definitions, eval::evaluate};

    fn sym(name: &str) -> Expression {
        Expression::Symbol(name.to_owned())
    }

    fn lit(value: impl Into<Value>) -> Expression {
        Expression::Literal(value.into())
    }

    fn app(func: Expression, args: Vec<Expression>) -> Expression {
        Expression::Application(Box::new(func), args)
    }

    fn eval(expr: Expression) -> Result<Value, EvalError> {
        evaluate(&expr, &Definitions::new())
    }

    fn show(expr: Expression) -> Value {
        eval(app(sym("show"), vec![expr])).unwrap()
    }

    #[test]
    fn add_should_add_numbers() {
        let result = eval(app(sym("+"), vec![lit(1), lit(2)]));

        assert_eq!(result.unwrap(), Value::Number(3));
    }

    #[test]
    fn builtin_should_be_partially_applicable() {
        let inc = app(sym("+"), vec![lit(1)]);
        let result = eval(app(inc, vec![lit(2)]));

        assert_eq!(result.unwrap(), Value::Number(3));
    }

    #[test]
    fn partial_builtin_should_print_as_closure() {
        let inc = app(sym("+"), vec![lit(1)]);

        assert_eq!(show(inc), Value::String("#<lambda b>".to_owned()));
    }

    #[test]
    fn mul_should_alias_star() {
        let result = eval(app(sym("mul"), vec![lit(6), lit(7)]));

        assert_eq!(result.unwrap(), Value::Number(42));
    }

    #[test]
    fn overflow_should_error() {
        let result = eval(app(sym("+"), vec![lit(i64::MAX), lit(1)]));

        assert!(matches!(result, Err(EvalError::Overflow)));
    }

    #[test]
    fn division_by_zero_should_error() {
        let result = eval(app(sym("div"), vec![lit(1), lit(0)]));

        assert!(matches!(result, Err(EvalError::DivisionByZero)));
    }

    #[test]
    fn comparison_should_return_t_or_nil() {
        let lt = eval(app(sym("<"), vec![lit(1), lit(2)]));
        let gt = eval(app(sym(">"), vec![lit(1), lit(2)]));

        assert_eq!(lt.unwrap(), Value::True);
        assert_eq!(gt.unwrap(), Value::Nil);
    }

    #[test]
    fn car_of_non_cons_should_be_type_error() {
        let result = eval(app(sym("car"), vec![lit(Value::Nil)]));

        assert!(matches!(
            result,
            Err(EvalError::Type {
                expected: "cons",
                ..
            })
        ));
    }

    #[test]
    fn arithmetic_on_non_number_should_be_type_error() {
        let result = eval(app(sym("+"), vec![lit(Value::True), lit(1)]));

        assert!(matches!(
            result,
            Err(EvalError::Type {
                expected: "number",
                ..
            })
        ));
    }

    #[test]
    fn call_should_apply_function_value() {
        let result = eval(app(sym("call"), vec![sym("+"), lit(1), lit(2)]));

        assert_eq!(result.unwrap(), Value::Number(3));
    }

    #[test]
    fn atom_should_be_t_unless_cons() {
        let pair = app(sym("cons"), vec![lit(1), lit(2)]);

        assert_eq!(eval(app(sym("atom"), vec![lit(1)])).unwrap(), Value::True);
        assert_eq!(eval(app(sym("atom"), vec![pair])).unwrap(), Value::Nil);
    }

    #[test]
    fn show_should_print_lists() {
        let list = app(
            sym("cons"),
            vec![lit(1), app(sym("cons"), vec![lit(2), lit(Value::Nil)])],
        );
        let pair = app(sym("cons"), vec![lit(1), lit(2)]);

        assert_eq!(show(list), Value::String("(1 2)".to_owned()));
        assert_eq!(show(pair), Value::String("(1 . 2)".to_owned()));
    }

    #[test]
    fn definition_should_shadow_builtin() {
        let mut defs = Definitions::new();
        defs.set("car".to_owned(), Value::Number(1));

        let result = evaluate(&sym("car"), &defs);

        assert_eq!(result.unwrap(), Value::Number(1));
    }
}
