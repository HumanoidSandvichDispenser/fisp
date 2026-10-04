use std::collections::HashMap;

use crate::expr::Value;

/// The global definitions table.
#[derive(Debug, Default)]
pub struct Definitions {
    pub(crate) bindings: HashMap<String, Value>,
}

impl Definitions {
    pub fn new() -> Self {
        Definitions::default()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.bindings.get(key)
    }

    pub fn set(&mut self, key: String, value: Value) {
        self.bindings.insert(key, value);
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.bindings.remove(key)
    }
}
