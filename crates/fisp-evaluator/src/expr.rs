use std::{fmt, rc::Rc};

use crate::{builtins::Op, env::Environment};

#[derive(Debug, PartialEq, Clone)]
pub enum Type {
    Value,
    Function(Box<Type>),
}

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
    }
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

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::Nil => write!(f, "nil"),
            Value::True => write!(f, "t"),
            Value::Number(n) => write!(f, "{n}"),
            Value::String(s) | Value::Symbol(s) => write!(f, "{s}"),
            Value::Cons(car, cdr) => {
                write!(f, "({car}")?;
                let mut rest = cdr.as_ref();
                loop {
                    match rest {
                        Value::Nil => break,
                        Value::Cons(car, cdr) => {
                            write!(f, " {car}")?;
                            rest = cdr;
                        }
                        other => {
                            write!(f, " . {other}")?;
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

#[derive(Debug, PartialEq, Clone)]
pub enum Expression {
    Nil,
    True,
    Number(i64),
    String(String),
    Symbol(String),
    Quote(Vec<String>),
    If(Box<Expression>, Box<Expression>, Box<Expression>),
    Lambda(Rc<str>, Rc<Expression>),
    Application(Box<Expression>, Vec<Expression>),
    /// Body of a built-in closure.
    Primitive(Op),
}
