pub mod cowsay;
pub mod math;
pub mod qrc;
pub mod time;

use crate::value::Value;

/// Names of standard library modules Englishmen ships with.
pub const STD_MODULES: &[&str] = &["math", "time", "qrc", "cowsay"];

/// Resolve a standard module by name into its runtime value (an object of native functions).
pub fn load_module(name: &str) -> Option<Value> {
    match name {
        "math" => Some(math::module()),
        "time" => Some(time::module()),
        "qrc" => Some(qrc::module()),
        "cowsay" => Some(cowsay::module()),
        _ => None,
    }
}
