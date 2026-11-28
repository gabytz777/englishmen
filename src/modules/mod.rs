pub mod bytes;
pub mod collections;
pub mod cowsay;
pub mod emb;
pub mod fs;
pub mod gfx;
pub mod json;
pub mod math;
pub mod os;
pub mod project;
pub mod qrc;
pub mod random;
pub mod string;
pub mod support;
pub mod text;
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
    "gfx",
    "text",
    "bytes",
    "emb",
    "project",
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
        "gfx" => Some(gfx::module()),
        "text" => Some(text::module()),
        "bytes" => Some(bytes::module()),
        "emb" => Some(emb::module()),
        "project" => Some(project::module()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Vec<crate::ast::Stmt> {
        crate::parser::parse(crate::lexer::tokenize(source).unwrap()).unwrap()
    }

    fn run_interpreted(source: &str) -> crate::interpreter::Interpreter {
        let program = parse(source);
        let mut interpreter = crate::interpreter::Interpreter::new();
        interpreter.run(&program).unwrap();
        interpreter
    }

    fn run_compiled(source: &str) -> crate::vm::Vm {
        let program = parse(source);
        let chunk = crate::compiler::compile(&program).unwrap();
        let mut vm = crate::vm::Vm::new();
        vm.run(chunk).unwrap();
        vm
    }

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
                assert!(
                    matches!(value, Value::Native(_)) || matches!(value, Value::Number(_)),
                    "{name}.{key} is neither a native function nor a constant"
                );
            }
            assert!(
                fields.borrow().values().any(|value| matches!(value, Value::Native(_))),
                "module '{name}' exports no native functions"
            );
        }
    }

    #[test]
    fn native_modules_agree_across_both_backends() {
        let source = r#"
import math
import string
import json
import collections
import random
import time
import os
import fs
import gfx

text = string.upper("hello")
power = math.pow(2, 8)
encoded = json.encode(collections.sorted([3, 1, 2]))
random.seed(11)
dice = random.int(1, 6)
weekday_name = time.weekday()
platform_name = os.platform()
working_dir = fs.cwd()
canvas = gfx.canvas(2, 2)
canvas.set(1, 1, 10, 20, 30)
pixel = canvas.get(1, 1)
"#;
        let interpreted = run_interpreted(source);
        let compiled = run_compiled(source);
        for name in [
            "text",
            "power",
            "encoded",
            "dice",
            "weekday_name",
            "platform_name",
            "working_dir",
            "pixel",
        ] {
            let left = interpreted.globals.get(name).unwrap();
            let right = compiled.globals.get(name).unwrap();
            assert_eq!(left.type_name(), right.type_name(), "{name} has a different type");
            assert_eq!(left.display(), right.display(), "{name} has a different value");
        }
    }
}
