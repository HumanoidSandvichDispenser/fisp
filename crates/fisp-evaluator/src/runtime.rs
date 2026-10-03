use crate::{
    defs::{Definition, Definitions},
    eval::{self, EvalError},
    expr::{Type, Value},
    parser::{self, ParseError},
};

#[derive(Debug)]
pub enum RuntimeError {
    DefinitionError(String),
    ParseError(ParseError),
    EvalError(EvalError),
}

pub struct Runtime {
    defs: Definitions,
}

impl Runtime {
    pub fn new() -> Self {
        Runtime {
            defs: Definitions::new(),
        }
    }

    pub fn define(&mut self, name: &str, source: &str) -> Result<(), RuntimeError> {
        // TODO: leading lambdas miss definitions whose `if` branches return lambdas
        let expected = (0..leading_lambdas(source)).fold(Type::Value, |ty, _| {
            Type::Function(Box::new(ty))
        });

        let previous = self.defs.remove(name);
        self.defs.set(
            name.to_string(),
            Definition {
                def_type: expected.clone(),
                value: Value::Nil,
            },
        );

        let result = self.evaluate(source).and_then(|definition| {
            if definition.def_type == expected {
                Ok(definition)
            } else {
                Err(RuntimeError::DefinitionError(format!(
                    "{name} has type {:?}, expected {expected:?}",
                    definition.def_type
                )))
            }
        });

        match result {
            Ok(definition) => {
                self.defs.set(name.to_string(), definition);
                Ok(())
            }
            Err(error) => {
                match previous {
                    Some(definition) => self.defs.set(name.to_string(), definition),
                    None => {
                        self.defs.remove(name);
                    }
                }
                Err(error)
            }
        }
    }

    pub fn evaluate(&self, source: &str) -> Result<Definition, RuntimeError> {
        let source: Vec<&str> = source.split("/").collect();

        // parse rhs then evaluate it and store the result in defs
        let ast = parser::parse(&source, &self.defs).map_err(RuntimeError::ParseError)?;
        let res = eval::evaluate(&ast.0, &self.defs).map_err(RuntimeError::EvalError)?;

        Ok(Definition {
            def_type: ast.1,
            value: res,
        })
    }
}

// HACK: right now we use this to check arity at definition time, although this can be solved with
// a linear system. will change later.
fn leading_lambdas(source: &str) -> usize {
    let mut segments = source.split('/').filter(|s| !s.is_empty() && *s != ".");

    std::iter::from_fn(|| match segments.next() {
        Some("lambda" | "λ") => segments.next(),
        _ => None,
    })
    .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn define_should_store_definition() {
        let mut runtime = Runtime::new();

        let name = "x";
        let source = "42";

        runtime.define(name, source).unwrap();

        let definition = runtime.defs.get(name).unwrap();
        assert_eq!(definition.value, 42.into());
    }

    #[test]
    fn define_should_overwrite_existing_definition() {
        let mut runtime = Runtime::new();

        let name = "x";
        let source1 = "42";
        let source2 = "100";

        runtime.define(name, source1).unwrap();
        runtime.define(name, source2).unwrap();

        let definition = runtime.defs.get(name).unwrap();
        assert_eq!(definition.value, 100.into());
    }

    #[test]
    fn evaluate_should_return_error_on_parse_failure() {
        let mut runtime = Runtime::new();

        let name = "x";
        let source = "invalid source code";

        let result = runtime.define(name, source);
        assert!(matches!(result, Err(RuntimeError::ParseError(_))));
    }

    #[test]
    fn evaluate_should_return_error_on_eval_failure() {
        let mut runtime = Runtime::new();

        let name = "x";
        let source = "div/1/0";

        let result = runtime.define(name, source);
        assert!(matches!(result, Err(RuntimeError::EvalError(_))));
    }

    #[test]
    fn runtime_should_run_fibonacci_program() {
        let mut runtime = Runtime::new();

        let name = "fib";
        let source = "lambda/n/if/</n/2/n/+/call/@fib/-/n/1/call/@fib/-/n/2";

        runtime.define(name, source).unwrap();

        let result = runtime.evaluate("fib/4").unwrap();

        assert_eq!(result.value, 3.into());
    }

    #[test]
    fn runtime_should_run_factorial_program() {
        let mut runtime = Runtime::new();

        let name = "fact";
        let source = "lambda/n/if/=/n/0/1/*/n/call/@fact/-/n/1";

        runtime.define(name, source).unwrap();

        let result = runtime.evaluate("fact/5").unwrap();

        assert_eq!(result.value, 120.into());
    }
}
