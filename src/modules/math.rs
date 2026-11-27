use crate::runtime::{expect_number, native};
use crate::value::Value;
use std::collections::BTreeMap;
#[cfg(test)]
use crate::modules::support::call_native;

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

    fields.insert(
        "log".to_string(),
        native("math.log", |args| {
            let value = expect_number(
                args.get(0).unwrap_or(&Value::Null),
                "for math.log",
            )?;
            let base = expect_number(
                args.get(1).unwrap_or(&Value::Null),
                "as the base for math.log",
            )?;
            if value <= 0.0 || base <= 0.0 || base == 1.0 {
                return Err("math.log requires a positive value and a positive base other than 1".to_string());
            }
            Ok(Value::Number(value.log(base)))
        }),
    );

    fields.insert(
        "ln".to_string(),
        native("math.ln", |args| {
            let n = expect_number(args.get(0).unwrap_or(&Value::Null), "for math.ln")?;
            if n <= 0.0 {
                return Err("math.ln requires a positive number".to_string());
            }
            Ok(Value::Number(n.ln()))
        }),
    );

    fields.insert(
        "exp".to_string(),
        native("math.exp", |args| {
            let n = expect_number(args.get(0).unwrap_or(&Value::Null), "for math.exp")?;
            Ok(Value::Number(n.exp()))
        }),
    );

    for (method, name) in [("sin", "math.sin"), ("cos", "math.cos"), ("tan", "math.tan")] {
        fields.insert(
            method.to_string(),
            native(name, move |args| {
                let n = expect_number(
                    args.get(0).unwrap_or(&Value::Null),
                    "for trigonometry",
                )?;
                Ok(Value::Number(match method {
                    "sin" => n.sin(),
                    "cos" => n.cos(),
                    _ => n.tan(),
                }))
            }),
        );
    }

    fields.insert(
        "hypot".to_string(),
        native("math.hypot", |args| {
            let a = expect_number(
                args.get(0).unwrap_or(&Value::Null),
                "for math.hypot",
            )?;
            let b = expect_number(
                args.get(1).unwrap_or(&Value::Null),
                "for math.hypot",
            )?;
            Ok(Value::Number(a.hypot(b)))
        }),
    );

    fields.insert(
        "sign".to_string(),
        native("math.sign", |args| {
            let n = expect_number(args.get(0).unwrap_or(&Value::Null), "for math.sign")?;
            if n.is_nan() {
                return Err("math.sign cannot be applied to NaN".to_string());
            }
            Ok(Value::Number(if n > 0.0 { 1.0 } else if n < 0.0 { -1.0 } else { 0.0 }))
        }),
    );

    fields.insert(
        "gcd".to_string(),
        native("math.gcd", |args| {
            let a = whole_number(
                args.get(0).unwrap_or(&Value::Null),
                "for math.gcd",
            )?
            .unsigned_abs();
            let b = whole_number(
                args.get(1).unwrap_or(&Value::Null),
                "for math.gcd",
            )?
            .unsigned_abs();
            let mut x = a as u128;
            let mut y = b as u128;
            while y != 0 {
                (x, y) = (y, x % y);
            }
            Ok(Value::Number(x as f64))
        }),
    );

    fields.insert(
        "lcm".to_string(),
        native("math.lcm", |args| {
            let a = whole_number(
                args.get(0).unwrap_or(&Value::Null),
                "for math.lcm",
            )?
            .unsigned_abs() as u128;
            let b = whole_number(
                args.get(1).unwrap_or(&Value::Null),
                "for math.lcm",
            )?
            .unsigned_abs() as u128;
            if a == 0 || b == 0 {
                return Ok(Value::Number(0.0));
            }
            let mut x = a;
            let mut y = b;
            let product = a * b;
            while y != 0 {
                (x, y) = (y, x % y);
            }
            Ok(Value::Number((product / x) as f64))
        }),
    );

    fields.insert(
        "clamp".to_string(),
        native("math.clamp", |args| {
            let value = expect_number(
                args.get(0).unwrap_or(&Value::Null),
                "for math.clamp",
            )?;
            let min = expect_number(
                args.get(1).unwrap_or(&Value::Null),
                "as the lower bound for math.clamp",
            )?;
            let max = expect_number(
                args.get(2).unwrap_or(&Value::Null),
                "as the upper bound for math.clamp",
            )?;
            if min > max {
                return Err("math.clamp lower bound cannot exceed the upper bound".to_string());
            }
            Ok(Value::Number(value.clamp(min, max)))
        }),
    );

    fields.insert(
        "degrees".to_string(),
        native("math.degrees", |args| {
            let radians = expect_number(
                args.get(0).unwrap_or(&Value::Null),
                "for math.degrees",
            )?;
            Ok(Value::Number(radians.to_degrees()))
        }),
    );

    fields.insert(
        "radians".to_string(),
        native("math.radians", |args| {
            let degrees = expect_number(
                args.get(0).unwrap_or(&Value::Null),
                "for math.radians",
            )?;
            Ok(Value::Number(degrees.to_radians()))
        }),
    );

    fields.insert(
        "isPrime".to_string(),
        native("math.isPrime", |args| {
            let n = whole_number(
                args.get(0).unwrap_or(&Value::Null),
                "for math.isPrime",
            )?;
            if n < 2 {
                return Ok(Value::Bool(false));
            }
            if n == 2 {
                return Ok(Value::Bool(true));
            }
            if n % 2 == 0 {
                return Ok(Value::Bool(false));
            }
            let mut divisor = 3i64;
            let mut prime = true;
            while divisor
                .checked_mul(divisor)
                .is_some_and(|square| square <= n)
            {
                if n % divisor == 0 {
                    prime = false;
                    break;
                }
                divisor += 2;
            }
            Ok(Value::Bool(prime))
        }),
    );

    fields.insert(
        "trunc".to_string(),
        native("math.trunc", |args| {
            let n = expect_number(args.get(0).unwrap_or(&Value::Null), "for math.trunc")?;
            Ok(Value::Number(n.trunc()))
        }),
    );

    fields.insert(
        "roundTo".to_string(),
        native("math.roundTo", |args| {
            let value = expect_number(
                args.get(0).unwrap_or(&Value::Null),
                "for math.roundTo",
            )?;
            let places = whole_number(
                args.get(1).unwrap_or(&Value::Null),
                "as the number of decimal places for math.roundTo",
            )?;
            if !(-12..=12).contains(&places) {
                return Err("math.roundTo expects between -12 and 12 decimal places".to_string());
            }
            let factor = 10_f64.powi(places as i32);
            Ok(Value::Number((value * factor).round() / factor))
        }),
    );

    fields.insert(
        "factorial".to_string(),
        native("math.factorial", |args| {
            let n = whole_number(
                args.get(0).unwrap_or(&Value::Null),
                "for math.factorial",
            )?;
            if n < 0 || n > 170 {
                return Err("math.factorial expects a whole number between 0 and 170".to_string());
            }
            let mut result = 1.0;
            for i in 2..=n {
                result *= i as f64;
            }
            Ok(Value::Number(result))
        }),
    );

    fields.insert(
        "toHex".to_string(),
        native("math.toHex", |args| {
            let n = whole_number(args.get(0).unwrap_or(&Value::Null), "for math.toHex")?;
            Ok(Value::String(format!("{n:x}")))
        }),
    );

    fields.insert(
        "toBinary".to_string(),
        native("math.toBinary", |args| {
            let n = whole_number(
                args.get(0).unwrap_or(&Value::Null),
                "for math.toBinary",
            )?;
            Ok(Value::String(format!("{n:b}")))
        }),
    );

    fields.insert("pi".to_string(), Value::Number(std::f64::consts::PI));
    fields.insert("e".to_string(), Value::Number(std::f64::consts::E));

    Value::new_object(fields)
}

fn whole_number(value: &Value, context: &str) -> Result<i64, String> {
    let n = crate::runtime::expect_number(value, context)?;
    if !n.is_finite() || n.fract() != 0.0 || n < i64::MIN as f64 || n > i64::MAX as f64 {
        return Err(format!("Expected a whole number {context}, found {n}"));
    }
    Ok(n as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn try_call(name: &str, args: &[Value]) -> Result<Value, String> {
        call_native(&module(), name, args)
    }

    fn call(name: &str, args: &[Value]) -> Value {
        try_call(name, args).unwrap()
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

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn logarithms_and_exponentials() {
        assert!(matches!(call("log", &[Value::Number(8.0), Value::Number(2.0)]), Value::Number(n) if close(n, 3.0)));
        assert!(matches!(call("ln", &[Value::Number(std::f64::consts::E)]), Value::Number(n) if close(n, 1.0)));
        assert!(matches!(call("exp", &[Value::Number(0.0)]), Value::Number(n) if close(n, 1.0)));
        assert!(try_call("log", &[Value::Number(-1.0), Value::Number(2.0)]).is_err());
        assert!(try_call("ln", &[Value::Number(0.0)]).is_err());
    }

    #[test]
    fn trigonometry_and_hypot() {
        assert!(matches!(call("sin", &[Value::Number(0.0)]), Value::Number(n) if close(n, 0.0)));
        assert!(matches!(call("cos", &[Value::Number(0.0)]), Value::Number(n) if close(n, 1.0)));
        assert!(matches!(call("tan", &[Value::Number(0.0)]), Value::Number(n) if close(n, 0.0)));
        assert!(matches!(call("hypot", &[Value::Number(3.0), Value::Number(4.0)]), Value::Number(n) if close(n, 5.0)));
        assert!(matches!(call("degrees", &[Value::Number(std::f64::consts::PI)]), Value::Number(n) if close(n, 180.0)));
        assert!(matches!(call("radians", &[Value::Number(180.0)]), Value::Number(n) if close(n, std::f64::consts::PI)));
    }

    #[test]
    fn integer_helpers_and_clamp() {
        assert!(matches!(call("sign", &[Value::Number(-3.0)]), Value::Number(n) if n == -1.0));
        assert!(matches!(call("sign", &[Value::Number(0.0)]), Value::Number(n) if n == 0.0));
        assert!(matches!(call("gcd", &[Value::Number(12.0), Value::Number(18.0)]), Value::Number(n) if n == 6.0));
        assert!(matches!(call("lcm", &[Value::Number(4.0), Value::Number(6.0)]), Value::Number(n) if n == 12.0));
        assert!(matches!(
            call("clamp", &[Value::Number(15.0), Value::Number(0.0), Value::Number(10.0)]),
            Value::Number(n) if n == 10.0
        ));
        assert!(try_call("clamp", &[Value::Number(1.0), Value::Number(3.0), Value::Number(2.0)]).is_err());
        assert!(try_call("gcd", &[Value::Number(1.5), Value::Number(2.0)]).is_err());
    }

    #[test]
    fn rounding_bases_and_factorials() {
        assert!(matches!(call("trunc", &[Value::Number(-2.9)]), Value::Number(n) if n == -2.0));
        assert!(matches!(
            call("roundTo", &[Value::Number(1.23456), Value::Number(2.0)]),
            Value::Number(n) if close(n, 1.23)
        ));
        assert!(matches!(
            call("factorial", &[Value::Number(5.0)]),
            Value::Number(n) if n == 120.0
        ));
        assert!(matches!(
            call("factorial", &[Value::Number(0.0)]),
            Value::Number(n) if n == 1.0
        ));
        assert_eq!(call("toHex", &[Value::Number(255.0)]).display(), "ff");
        assert_eq!(call("toBinary", &[Value::Number(10.0)]).display(), "1010");
        assert!(try_call("factorial", &[Value::Number(-1.0)]).is_err());
        assert!(try_call("roundTo", &[Value::Number(1.0), Value::Number(20.0)]).is_err());
    }

    #[test]
    fn primality() {
        for prime in [2.0, 3.0, 7.0, 97.0] {
            assert!(matches!(call("isPrime", &[Value::Number(prime)]), Value::Bool(true)));
        }
        for composite in [-3.0, 0.0, 1.0, 4.0, 9.0, 100.0] {
            assert!(matches!(call("isPrime", &[Value::Number(composite)]), Value::Bool(false)));
        }
        assert!(try_call("isPrime", &[Value::Number(2.5)]).is_err());
    }
}
