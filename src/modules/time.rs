use crate::runtime::{expect_number, expect_string, native};
use crate::value::Value;
use chrono::{Datelike, Local, TimeZone, Timelike, Utc, Weekday};
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

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

    fields.insert(
        "today".to_string(),
        native("time.today", |_args| {
            Ok(Value::String(Local::now().format("%Y-%m-%d").to_string()))
        }),
    );

    fields.insert(
        "iso".to_string(),
        native("time.iso", |_args| {
            Ok(Value::String(Local::now().to_rfc3339()))
        }),
    );

    fields.insert(
        "weekday".to_string(),
        native("time.weekday", |_args| {
            let name = match Local::now().weekday() {
                Weekday::Mon => "Monday",
                Weekday::Tue => "Tuesday",
                Weekday::Wed => "Wednesday",
                Weekday::Thu => "Thursday",
                Weekday::Fri => "Friday",
                Weekday::Sat => "Saturday",
                Weekday::Sun => "Sunday",
            };
            Ok(Value::String(name.to_string()))
        }),
    );

    fields.insert(
        "elapsed".to_string(),
        native("time.elapsed", |args| {
            if let Some(value) = args.get(0) {
                let since = expect_number(value, "as a timestamp for time.elapsed")?;
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| e.to_string())?
                    .as_secs_f64();
                Ok(Value::Number(now - since))
            } else {
                let start = PROCESS_START.get_or_init(Instant::now);
                Ok(Value::Number(start.elapsed().as_secs_f64()))
            }
        }),
    );

    fields.insert(
        "format".to_string(),
        native("time.format", |args| {
            let pattern = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "as the format for time.format",
            )?;
            let moment = if let Some(value) = args.get(1) {
                let secs = expect_number(value, "as a timestamp for time.format")?;
                if !secs.is_finite() {
                    return Err("time.format expects a finite timestamp".to_string());
                }
                Local
                    .timestamp_opt(secs as i64, 0)
                    .single()
                    .ok_or_else(|| "time.format timestamp is out of range".to_string())?
            } else {
                Local::now()
            };
            Ok(Value::String(moment.format(&pattern).to_string()))
        }),
    );

    fields.insert(
        "isoUtc".to_string(),
        native("time.isoUtc", |_args| {
            Ok(Value::String(Utc::now().to_rfc3339()))
        }),
    );

    fields.insert(
        "millis".to_string(),
        native("time.millis", |_args| {
            let millis = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_millis();
            Ok(Value::Number(millis as f64))
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

    fn call(name: &str, args: &[Value]) -> Result<Value, String> {
        let Value::Object(obj) = module() else {
            unreachable!()
        };
        let func = obj.borrow().get(name).cloned().unwrap();
        let Value::Native(native) = func else {
            unreachable!()
        };
        (native.func)(args)
    }

    #[test]
    fn today_iso_and_weekday() {
        let today = call("today", &[]).unwrap().display();
        assert_eq!(today.len(), 10);
        assert_eq!(today.as_bytes()[4], b'-');
        let iso = call("iso", &[]).unwrap().display();
        assert!(iso.contains('T'));
        let utc = call("isoUtc", &[]).unwrap().display();
        assert!(utc.contains('T') && utc.contains("+00:00"));
        let weekday = call("weekday", &[]).unwrap().display();
        assert!(matches!(
            weekday.as_str(),
            "Monday" | "Tuesday" | "Wednesday" | "Thursday" | "Friday" | "Saturday" | "Sunday"
        ));
    }

    #[test]
    fn elapsed_millis_and_format() {
        let millis = call("millis", &[]).unwrap();
        assert!(matches!(millis, Value::Number(n) if n > 1_000_000_000_000.0));
        let elapsed = call("elapsed", &[Value::Number(0.0)]).unwrap();
        assert!(matches!(elapsed, Value::Number(n) if n > 0.0));
        let since_start = call("elapsed", &[]).unwrap();
        assert!(matches!(since_start, Value::Number(n) if n >= 0.0));
        let formatted = call(
            "format",
            &[
                Value::String("%Y-%m-%d".to_string()),
                Value::Number(0.0),
            ],
        )
        .unwrap();
        assert_eq!(formatted.display().len(), 10);
        assert!(call("format", &[Value::Number(1.0)]).is_err());
        assert!(call("elapsed", &[Value::String("now".to_string())]).is_err());
    }
}
