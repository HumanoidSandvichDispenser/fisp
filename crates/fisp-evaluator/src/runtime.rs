use std::{collections::HashMap, rc::Rc};

use crate::{
    builtins,
    defs::Definitions,
    env::Environment,
    eval::{self, EvalError},
    expr::{Expression, Value},
    parser::{self, ParseError},
};

#[derive(Debug)]
pub enum RuntimeError {
    DefinitionError(String),
    ParseError(ParseError),
    EvalError(EvalError),
}

#[derive(Debug)]
pub struct Runtime {
    defs: Definitions,
    /// The source each definition was evaluated from, by name.
    sources: HashMap<String, String>,
}

impl Runtime {
    pub fn new() -> Self {
        Runtime {
            defs: Definitions::new(),
            sources: HashMap::new(),
        }
    }

    pub fn define(&mut self, name: &str, source: &str) -> Result<(), RuntimeError> {
        if builtins::arity(name).is_some() {
            return Err(RuntimeError::DefinitionError(format!(
                "{name} is a builtin"
            )));
        }

        // TODO: refactor
        if name == "in" {
            return Err(RuntimeError::DefinitionError(
                "in is a reserved symbol".to_string(),
            ));
        }

        let value = self.evaluate(source)?;
        self.defs.set(name.to_string(), value);
        self.sources.insert(name.to_string(), source.to_string());

        Ok(())
    }

    pub fn undefine(&mut self, name: &str) -> Result<(), RuntimeError> {
        if builtins::arity(name).is_some() {
            return Err(RuntimeError::DefinitionError(format!(
                "{name} is a builtin"
            )));
        }

        // TODO: refactor and add new error for reserved symbols, which should return EPERM
        if name == "in" {
            return Err(RuntimeError::DefinitionError(
                "in is a reserved symbol".to_string(),
            ));
        }

        self.defs.remove(name);
        self.sources.remove(name);

        Ok(())
    }

    pub fn definitions(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.defs.bindings.iter()
    }

    /// Parse a source string into an AST expression without evaluating it.
    pub fn parse(&self, source: &str) -> Result<Expression, ParseError> {
        let segments: Vec<&str> = source.trim().split('/').collect();
        parser::parse(&segments, &self.defs)
    }

    /// Evaluate a source string into a value, using the current definitions.
    pub fn evaluate(&self, source: &str) -> Result<Value, RuntimeError> {
        let ast = self.parse(source).map_err(RuntimeError::ParseError)?;
        eval::evaluate(&ast, &self.defs).map_err(RuntimeError::EvalError)
    }

    /// Evaluate an AST expression into a value, using the current definitions.
    pub fn evaluate_ast(&self, expr: &Expression) -> Result<Value, RuntimeError> {
        eval::evaluate(expr, &self.defs).map_err(RuntimeError::EvalError)
    }

    /// Evaluate an AST expression into a value, with extra top-level bindings.
    pub fn evaluate_ast_with(
        &self,
        expr: &Expression,
        bindings: impl IntoIterator<Item = (String, Value)>,
    ) -> Result<Value, RuntimeError> {
        let mut env = Environment::new();
        for (name, value) in bindings {
            env.set(name, value);
        }

        eval::evaluate_with_env(expr, Rc::new(env), &self.defs).map_err(RuntimeError::EvalError)
    }

    pub fn source(&self, name: &str) -> Option<String> {
        self.sources.get(name).cloned()
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

        let show = |source| runtime.evaluate(source).unwrap().sexp().to_string();

        assert_eq!(show("preorder/λ/x/mul/x/10/tree"), "(10 20 40 50 30)");
        assert_eq!(show("preorder/^show/tree"), r#"("1" "2" "4" "5" "3")"#);
        assert_eq!(show("preorder/λ/x/x/nil"), "nil");
    }
}
