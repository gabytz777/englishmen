use crate::runtime::{expect_number, native};
use crate::value::Value;
use std::collections::BTreeMap;

pub fn module() -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "sqrt".to_string(),
        native("math.sqrt", |args| {
            let n = expect_number(args.get(0).unwrap_or(&Value::Null), "for math.sqrt")?;
            if n < 0.0 {
                return Err("Cannot take the square root of a negative number".to_string());
            }
            Ok(Value::Number(n.sqrt()))
        }),
    );

    fields.insert(
        "abs".to_string(),
        native("math.abs", |args| {
            let n = expect_number(args.get(0).unwrap_or(&Value::Null), "for math.abs")?;
            Ok(Value::Number(n.abs()))
        }),
    );

    fields.insert(
        "floor".to_string(),
        native("math.floor", |args| {
            let n = expect_number(args.get(0).unwrap_or(&Value::Null), "for math.floor")?;
            Ok(Value::Number(n.floor()))
        }),
    );

    fields.insert(
        "ceil".to_string(),
        native("math.ceil", |args| {
            let n = expect_number(args.get(0).unwrap_or(&Value::Null), "for math.ceil")?;
            Ok(Value::Number(n.ceil()))
        }),
    );

    fields.insert(
        "round".to_string(),
        native("math.round", |args| {
            let n = expect_number(args.get(0).unwrap_or(&Value::Null), "for math.round")?;
            Ok(Value::Number(n.round()))
        }),
    );

    fields.insert(
        "pow".to_string(),
        native("math.pow", |args| {
            let base = expect_number(
                args.get(0).unwrap_or(&Value::Null),
                "as the base for math.pow",
            )?;
            let exp = expect_number(
                args.get(1).unwrap_or(&Value::Null),
                "as the exponent for math.pow",
            )?;
            Ok(Value::Number(base.powf(exp)))
        }),
    );

    fields.insert(
        "min".to_string(),
        native("math.min", |args| {
            let a = expect_number(args.get(0).unwrap_or(&Value::Null), "for math.min")?;
            let b = expect_number(args.get(1).unwrap_or(&Value::Null), "for math.min")?;
            Ok(Value::Number(a.min(b)))
        }),
    );

    fields.insert(
        "max".to_string(),
        native("math.max", |args| {
            let a = expect_number(args.get(0).unwrap_or(&Value::Null), "for math.max")?;
            let b = expect_number(args.get(1).unwrap_or(&Value::Null), "for math.max")?;
            Ok(Value::Number(a.max(b)))
        }),
    );

    fields.insert("pi".to_string(), Value::Number(std::f64::consts::PI));
    fields.insert("e".to_string(), Value::Number(std::f64::consts::E));

    Value::new_object(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: &[Value]) -> Value {
        let module = module();
        let Value::Object(obj) = module else {
            unreachable!()
        };
        let func = obj.borrow().get(name).cloned().unwrap();
        let Value::Native(native) = func else {
            unreachable!()
        };
        (native.func)(args).unwrap()
    }

    #[test]
    fn sqrt_abs_floor_ceil_round() {
        assert!(matches!(call("sqrt", &[Value::Number(25.0)]), Value::Number(n) if n == 5.0));
        assert!(matches!(call("abs", &[Value::Number(-10.0)]), Value::Number(n) if n == 10.0));
        assert!(matches!(call("floor", &[Value::Number(3.7)]), Value::Number(n) if n == 3.0));
        assert!(matches!(call("ceil", &[Value::Number(3.2)]), Value::Number(n) if n == 4.0));
        assert!(matches!(call("round", &[Value::Number(3.5)]), Value::Number(n) if n == 4.0));
    }

    #[test]
    fn pow() {
        assert!(
            matches!(call("pow", &[Value::Number(2.0), Value::Number(8.0)]), Value::Number(n) if n == 256.0)
        );
    }

    #[test]
    fn constants() {
        let Value::Object(obj) = module() else {
            unreachable!()
        };
        let pi = obj.borrow().get("pi").cloned().unwrap();
        assert!(matches!(pi, Value::Number(n) if (n - std::f64::consts::PI).abs() < 1e-9));
    }

    #[test]
    fn sqrt_of_negative_errors() {
        let module = module();
        let Value::Object(obj) = module else {
            unreachable!()
        };
        let func = obj.borrow().get("sqrt").cloned().unwrap();
        let Value::Native(native) = func else {
            unreachable!()
        };
        assert!((native.func)(&[Value::Number(-1.0)]).is_err());
    }
}
