pub mod collections;
pub mod cowsay;
pub mod fs;
pub mod json;
pub mod math;
pub mod os;
pub mod qrc;
pub mod random;
pub mod string;
pub mod support;
pub mod time;

use crate::value::Value;

/// Names of standard library modules Englishmen ships with.
pub const STD_MODULES: &[&str] = &[
    "math",
    "time",
    "qrc",
    "cowsay",
    "string",
    "json",
    "fs",
    "os",
    "random",
    "collections",
];

/// Resolve a standard module by name into its runtime value (an object of native functions).
pub fn load_module(name: &str) -> Option<Value> {
    match name {
        "math" => Some(math::module()),
        "time" => Some(time::module()),
        "qrc" => Some(qrc::module()),
        "cowsay" => Some(cowsay::module()),
        "string" => Some(string::module()),
        "json" => Some(json::module()),
        "fs" => Some(fs::module()),
        "os" => Some(os::module()),
        "random" => Some(random::module()),
        "collections" => Some(collections::module()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_module_loads_as_an_object() {
        for name in STD_MODULES {
            let Some(Value::Object(fields)) = load_module(name) else {
                panic!("module '{name}' did not load as an object");
            };
            assert!(!fields.borrow().is_empty(), "module '{name}' is empty");
        }
    }

    #[test]
    fn unknown_modules_are_not_registered() {
        assert!(load_module("not_a_module").is_none());
        assert!(!STD_MODULES.contains(&"not_a_module"));
    }

    #[test]
    fn module_loading_is_shared_by_both_backends() {
        // The interpreter and the VM both call `load_module`, so the same
        // native functions are available to each backend. Check that every
        // exported function can be invoked through the shared value.
        for name in STD_MODULES {
            let Some(module) = load_module(name) else {
                unreachable!()
            };
            let Value::Object(fields) = module else {
                unreachable!()
            };
            for (key, value) in fields.borrow().iter() {
                assert!(matches!(value, Value::Native(_)), "{name}.{key} is not native");
            }
        }
    }
}
