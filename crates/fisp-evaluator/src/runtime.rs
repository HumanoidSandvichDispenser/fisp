use crate::{
    builtins,
    defs::Definitions,
    eval::{self, EvalError},
    expr::Value,
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
        if builtins::arity(name).is_some() {
            return Err(RuntimeError::DefinitionError(format!(
                "{name} is a builtin"
            )));
        }

        let value = self.evaluate(source)?;
        self.defs.set(name.to_string(), value);

        Ok(())
    }

    pub fn evaluate(&self, source: &str) -> Result<Value, RuntimeError> {
        let source: Vec<&str> = source.split("/").collect();

        let ast = parser::parse(&source, &self.defs).map_err(RuntimeError::ParseError)?;
        eval::evaluate(&ast, &self.defs).map_err(RuntimeError::EvalError)
    }
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

        assert_eq!(runtime.defs.get(name), Some(&42.into()));
    }

    #[test]
    fn define_should_overwrite_existing_definition() {
        let mut runtime = Runtime::new();

        let name = "x";
        let source1 = "42";
        let source2 = "100";

        runtime.define(name, source1).unwrap();
        runtime.define(name, source2).unwrap();

        assert_eq!(runtime.defs.get(name), Some(&100.into()));
    }

    #[test]
    fn evaluate_should_return_error_on_parse_failure() {
        let mut runtime = Runtime::new();

        let name = "x";
        let source = "+/1";

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
        let source = "lambda/n/if/</n/2/n/+/@fib/-/n/1/@fib/-/n/2";

        runtime.define(name, source).unwrap();

        let result = runtime.evaluate("fib/4").unwrap();

        assert_eq!(result, 3.into());
    }

    #[test]
    fn runtime_should_run_factorial_program() {
        let mut runtime = Runtime::new();

        let name = "fact";
        let source = "lambda/n/if/=/n/0/1/*/n/@fact/-/n/1";

        runtime.define(name, source).unwrap();

        let result = runtime.evaluate("fact/5").unwrap();

        assert_eq!(result, 120.into());
    }

    #[test]
    fn runtime_should_run_mutually_recursive_programs() {
        let mut runtime = Runtime::new();

        runtime.define("even", "λ/n/if/=/n/0/t/@odd/-/n/1").unwrap();
        runtime
            .define("odd", "λ/n/if/=/n/0/nil/@even/-/n/1")
            .unwrap();

        assert_eq!(runtime.evaluate("even/10").unwrap(), Value::True);
        assert_eq!(runtime.evaluate("odd/10").unwrap(), Value::Nil);
    }

    #[test]
    fn failed_define_should_keep_previous_definition() {
        let mut runtime = Runtime::new();

        runtime.define("x", "42").unwrap();
        assert!(runtime.define("x", "div/1/0").is_err());

        assert_eq!(runtime.defs.get("x"), Some(&42.into()));
    }

    #[test]
    fn define_should_reject_builtin_name() {
        let mut runtime = Runtime::new();

        let result = runtime.define("car", "1");
        assert!(matches!(result, Err(RuntimeError::DefinitionError(_))));
    }

    #[test]
    fn runtime_should_map_binary_tree_in_preorder() {
        let mut runtime = Runtime::new();

        // A tree is (value left right), and nil is the empty tree.
        let definitions = [
            (
                "walk",
                "λ/f/λ/node/λ/acc/if/node/cons/@f/car/node/@@@walk/f/car/cdr/node/@@@walk/f/car/cdr/cdr/node/acc/acc",
            ),
            ("preorder", "λ/f/λ/node/@@@walk/f/node/nil"),
            (
                "tree",
                "cons/1/cons/cons/2/cons/4,nil,nil/cons/5,nil,nil/nil/cons/3,nil,nil/nil",
            ),
        ];

        for (name, source) in definitions {
            runtime.define(name, source).unwrap();
        }

        let show = |source| runtime.evaluate(source).unwrap().to_string();

        assert_eq!(show("preorder/λ/x/mul/x/10/tree"), "(10 20 40 50 30)");
        assert_eq!(show("preorder/^show/tree"), "(1 2 4 5 3)");
        assert_eq!(show("preorder/λ/x/x/nil"), "nil");
    }
}
