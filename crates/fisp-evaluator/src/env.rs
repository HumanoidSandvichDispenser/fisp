use std::{collections::HashMap, rc::Rc};

use crate::expr::Value;

#[derive(Debug)]
pub struct Environment {
    pub parent: Option<Rc<Environment>>,
    pub bindings: HashMap<String, Value>,
}

impl Environment {
    pub fn new() -> Self {
        Environment {
            parent: None,
            bindings: HashMap::new(),
        }
    }

    pub fn with_parent(mut self: Self, parent: Rc<Environment>) -> Self {
        self.parent = Some(parent);
        self
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self.bindings.get(key) {
            Some(value) => Some(value),
            None => match &self.parent {
                Some(parent) => parent.get(key),
                None => None,
            },
        }
    }

    pub fn set(&mut self, key: String, value: Value) {
        self.bindings.insert(key, value);
    }
}
