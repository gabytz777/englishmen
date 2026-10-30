use crate::runtime::{expect_number, native};
use crate::value::Value;
use chrono::{Datelike, Local, Timelike};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn module() -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "now".to_string(),
        native("time.now", |_args| {
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_secs_f64();
            Ok(Value::Number(secs))
        }),
    );

    fields.insert(
        "sleep".to_string(),
        native("time.sleep", |args| {
            let secs = expect_number(args.get(0).unwrap_or(&Value::Null), "for time.sleep")?;
            // NaN and infinity compare false against `< 0.0`, and
            // `Duration::from_secs_f64` panics on them (as well as on
            // durations that overflow), so reject them before converting.
            if !secs.is_finite() || secs < 0.0 {
                return Err(
                    "time.sleep expects a finite, non-negative number of seconds".to_string()
                );
            }
            let duration = Duration::try_from_secs_f64(secs).map_err(|_| {
                "time.sleep expects a finite, non-negative number of seconds".to_string()
            })?;
            std::thread::sleep(duration);
            Ok(Value::Null)
        }),
    );

    fields.insert(
        "year".to_string(),
        native("time.year", |_args| {
            Ok(Value::Number(Local::now().year() as f64))
        }),
    );
    fields.insert(
        "month".to_string(),
        native("time.month", |_args| {
            Ok(Value::Number(Local::now().month() as f64))
        }),
    );
    fields.insert(
        "day".to_string(),
        native("time.day", |_args| {
            Ok(Value::Number(Local::now().day() as f64))
        }),
    );
    fields.insert(
        "hour".to_string(),
        native("time.hour", |_args| {
            Ok(Value::Number(Local::now().hour() as f64))
        }),
    );
    fields.insert(
        "minute".to_string(),
        native("time.minute", |_args| {
            Ok(Value::Number(Local::now().minute() as f64))
        }),
    );
    fields.insert(
        "second".to_string(),
        native("time.second", |_args| {
            Ok(Value::Number(Local::now().second() as f64))
        }),
    );

    Value::new_object(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_returns_a_positive_number() {
        let Value::Object(obj) = module() else {
            unreachable!()
        };
        let func = obj.borrow().get("now").cloned().unwrap();
        let Value::Native(native) = func else {
            unreachable!()
        };
        let result = (native.func)(&[]).unwrap();
        assert!(matches!(result, Value::Number(n) if n > 0.0));
    }

    #[test]
    fn sleep_rejects_values_that_are_not_a_duration() {
        let Value::Object(obj) = module() else {
            unreachable!()
        };
        let func = obj.borrow().get("sleep").cloned().unwrap();
        let Value::Native(native) = func else {
            unreachable!()
        };
        // Before the fix these panicked inside Duration::from_secs_f64.
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 1e300] {
            assert!(
                (native.func)(&[Value::Number(bad)]).is_err(),
                "time.sleep({bad}) should fail with an error, not panic"
            );
        }
    }

    #[test]
    fn year_returns_a_plausible_value() {
        let Value::Object(obj) = module() else {
            unreachable!()
        };
        let func = obj.borrow().get("year").cloned().unwrap();
        let Value::Native(native) = func else {
            unreachable!()
        };
        let result = (native.func)(&[]).unwrap();
        assert!(matches!(result, Value::Number(n) if n >= 2024.0));
    }
}
