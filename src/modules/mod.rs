pub mod cowsay;
pub mod html;
pub mod math;
pub mod qrc;
pub mod time;

use crate::value::Value;

/// Names of standard library modules Englishmen ships with.
pub const STD_MODULES: &[&str] = &["math", "time", "qrc", "cowsay", "html"];

/// Resolve a standard module by name into its runtime value (an object of native functions).
pub fn load_module(name: &str) -> Option<Value> {
    match name {
        "math" => Some(math::module()),
        "time" => Some(time::module()),
        "qrc" => Some(qrc::module()),
        "cowsay" => Some(cowsay::module()),
        "html" => Some(html::module()),
        _ => None,
    }
}
