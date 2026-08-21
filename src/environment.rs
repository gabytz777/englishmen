use crate::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub struct Environment {
    vars: RefCell<HashMap<String, Value>>,
    parent: Option<Rc<Environment>>,
}

impl Environment {
    pub fn new() -> Rc<Environment> {
        Rc::new(Environment {
            vars: RefCell::new(HashMap::new()),
            parent: None,
        })
    }

    pub fn child(parent: &Rc<Environment>) -> Rc<Environment> {
        Rc::new(Environment {
            vars: RefCell::new(HashMap::new()),
            parent: Some(Rc::clone(parent)),
        })
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(v) = self.vars.borrow().get(name) {
            return Some(v.clone());
        }
        self.parent.as_ref().and_then(|p| p.get(name))
    }

    /// Define or overwrite a variable in the *current* scope.
    pub fn define(&self, name: &str, value: Value) {
        self.vars.borrow_mut().insert(name.to_string(), value);
    }

    /// Assign to an existing variable, searching up the scope chain.
    /// If it does not exist anywhere, defines it in the current (innermost) scope,
    /// matching Englishmen's "assignment creates variables" semantics.
    pub fn assign(&self, name: &str, value: Value) {
        if self.vars.borrow().contains_key(name) {
            self.vars.borrow_mut().insert(name.to_string(), value);
            return;
        }
        if let Some(parent) = &self.parent {
            if parent.has(name) {
                parent.assign(name, value);
                return;
            }
        }
        self.vars.borrow_mut().insert(name.to_string(), value);
    }

    pub fn has(&self, name: &str) -> bool {
        if self.vars.borrow().contains_key(name) {
            return true;
        }
        self.parent.as_ref().map(|p| p.has(name)).unwrap_or(false)
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.vars.borrow().keys().cloned().collect();
        if let Some(parent) = &self.parent {
            names.extend(parent.names());
        }
        names
    }
}
