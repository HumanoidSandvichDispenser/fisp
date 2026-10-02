use std::rc::Rc;
use crate::env::Environment;

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
        param: String,
        body: Box<Expression>,
        env: Rc<Environment>,

        // The number of parameters the closure expects. This is used to check if the closure is
        // called with the correct number of arguments and for static and runtime typechecking.
        //arity: usize,
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

#[derive(Debug, PartialEq, Clone)]
pub enum Expression {
    Nil,
    True,
    Number(i64),
    String(String),
    Symbol(String),
    Quote(Vec<String>),
    If(Box<Expression>, Box<Expression>, Box<Expression>),
    Lambda(Box<Expression>, Box<Expression>),
    Application(Box<Expression>, Vec<Expression>),
}
