//! Built-in functions available in every Englishmen scope.
//!
//! Both execution backends create their global environment through
//! [`crate::environment::Environment::new`], so registering a native function
//! here makes it available to the interpreter, the VM compiled programs and
//! the REPL at once.

use crate::value::Value;
use std::collections::HashMap;

/// The global builtin table. Seeded into every new root environment.
pub fn globals() -> HashMap<String, Value> {
    HashMap::new()
}
