use std::collections::HashMap;

use crate::expr::{Type, Value};

#[derive(Debug, Clone)]
pub struct Definition {
    pub def_type: Type,
    pub value: Value,
}

/// The global definitions table.
#[derive(Debug, Default)]
pub struct Definitions {
    bindings: HashMap<String, Definition>,
}

impl Definitions {
    pub fn new() -> Self {
        Definitions::default()
    }

    pub fn get(&self, key: &str) -> Option<&Definition> {
        self.bindings.get(key)
    }

    pub fn set(&mut self, key: String, definition: Definition) {
        self.bindings.insert(key, definition);
    }

    pub fn remove(&mut self, key: &str) -> Option<Definition> {
        self.bindings.remove(key)
    }
}
