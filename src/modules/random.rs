//! Deterministic-when-seeded pseudo-random numbers, using only the standard
//! library. The generator is an xorshift64* state kept per thread.

use crate::modules::support::array_items;
#[cfg(test)]
use crate::modules::support::call_native;
use crate::runtime::{expect_number, native};
use crate::value::Value;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

thread_local! {
    static STATE: Cell<u64> = Cell::new(initial_seed());
}

fn initial_seed() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(DEFAULT_SEED);
    let seed = nanos ^ ((std::process::id() as u64) << 32);
    if seed == 0 { DEFAULT_SEED } else { seed }
}

fn next_u64() -> u64 {
    STATE.with(|state| {
        // xorshift64* (Marsaglia), with a nonzero fallback for an explicit 0 seed.
        let mut x = state.get();
        if x == 0 {
            x = DEFAULT_SEED;
        }
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        state.set(x);
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    })
}

/// Uniform value in [0, 1).
fn next_float() -> f64 {
    (next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// Uniform integer in `min..=max`, inclusive.
fn next_int(min: i64, max: i64) -> i64 {
    let span = (max as i128 - min as i128 + 1) as u128;
    let offset = ((next_u64() as u128).wrapping_mul(span)) >> 64;
    // Work in i128 so the full i64 range (including i64::MIN) cannot overflow.
    (min as i128 + offset as i128) as i64
}

pub fn module() -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "float".to_string(),
        native("random.float", |_args| Ok(Value::Number(next_float()))),
    );

    fields.insert(
        "int".to_string(),
        native("random.int", |args| {
            let (min, max) = if args.len() >= 2 {
                (
                    whole_number(args.get(0).unwrap_or(&Value::Null), "as the minimum for random.int")?,
                    whole_number(args.get(1).unwrap_or(&Value::Null), "as the maximum for random.int")?,
                )
            } else {
                (
                    0,
                    whole_number(
                        args.get(0).unwrap_or(&Value::Null),
                        "as the upper bound for random.int",
                    )?,
                )
            };
            if min > max {
                return Err("random.int expects the minimum to be no greater than the maximum".to_string());
            }
            Ok(Value::Number(next_int(min, max) as f64))
        }),
    );

    fields.insert(
        "choice".to_string(),
        native("random.choice", |args| {
            let items = array_items(
                args.get(0).unwrap_or(&Value::Null),
                "for random.choice",
            )?;
            if items.is_empty() {
                return Err("random.choice cannot choose from an empty array".to_string());
            }
            Ok(items[next_int(0, items.len() as i64 - 1) as usize].clone())
        }),
    );

    fields.insert(
        "shuffle".to_string(),
        native("random.shuffle", |args| {
            let mut items = array_items(
                args.get(0).unwrap_or(&Value::Null),
                "for random.shuffle",
            )?;
            for i in (1..items.len()).rev() {
                let j = next_int(0, i as i64) as usize;
                items.swap(i, j);
            }
            Ok(Value::new_array(items))
        }),
    );

    fields.insert(
        "bool".to_string(),
        native("random.bool", |_args| Ok(Value::Bool(next_float() < 0.5))),
    );

    fields.insert(
        "range".to_string(),
        native("random.range", |args| {
            let min = expect_number(
                args.get(0).unwrap_or(&Value::Null),
                "as the minimum for random.range",
            )?;
            let max = expect_number(
                args.get(1).unwrap_or(&Value::Null),
                "as the maximum for random.range",
            )?;
            if !min.is_finite() || !max.is_finite() || min >= max {
                return Err("random.range expects finite numbers with min < max".to_string());
            }
            Ok(Value::Number(min + next_float() * (max - min)))
        }),
    );

    fields.insert(
        "sample".to_string(),
        native("random.sample", |args| {
            let items = array_items(
                args.get(0).unwrap_or(&Value::Null),
                "for random.sample",
            )?;
            let count = whole_number(
                args.get(1).unwrap_or(&Value::Null),
                "as the count for random.sample",
            )?;
            if count < 0 || count as usize > items.len() {
                return Err("random.sample count must be between 0 and the array length".to_string());
            }
            let mut pool = items;
            for i in 0..count as usize {
                let j = i as i64 + next_int(0, (pool.len() - i) as i64 - 1);
                pool.swap(i, j as usize);
            }
            Ok(Value::new_array(pool[..count as usize].to_vec()))
        }),
    );

    fields.insert(
        "seed".to_string(),
        native("random.seed", |args| {
            let value = expect_number(
                args.get(0).unwrap_or(&Value::Null),
                "for random.seed",
            )?;
            if !value.is_finite() {
                return Err("random.seed expects a finite number".to_string());
            }
            STATE.with(|state| state.set((value as i64 as u64) ^ DEFAULT_SEED));
            Ok(Value::Null)
        }),
    );

    fields.insert(
        "uuid".to_string(),
        native("random.uuid", |_args| {
            // UUIDv4 must not be reproducible via random.seed, so prefer OS
            // entropy and only fall back to the PRNG where no device exists.
            let mut bytes = os_random_bytes();
            bytes[6] = (bytes[6] & 0x0f) | 0x40; // UUID version 4
            bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
            Ok(Value::String(format_uuid(&bytes)))
        }),
    );

    Value::new_object(fields)
}

fn whole_number(value: &Value, context: &str) -> Result<i64, String> {
    let n = expect_number(value, context)?;
    if !n.is_finite() || n.fract() != 0.0 || n < i64::MIN as f64 || n > i64::MAX as f64 {
        return Err(format!("Expected a whole number {context}, found {n}"));
    }
    Ok(n as i64)
}

fn os_random_bytes() -> [u8; 16] {
    #[cfg(unix)]
    {
        let mut bytes = [0u8; 16];
        if let Ok(mut source) = std::fs::File::open("/dev/urandom") {
            use std::io::Read;
            if source.read_exact(&mut bytes).is_ok() {
                return bytes;
            }
        }
    }
    let mut bytes = [0u8; 16];
    for chunk in bytes.chunks_mut(8) {
        chunk.copy_from_slice(&next_u64().to_le_bytes());
    }
    bytes
}

fn format_uuid(bytes: &[u8; 16]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: &[Value]) -> Result<Value, String> {
        call_native(&module(), name, args)
    }

    #[test]
    fn seeded_values_are_repeatable_and_in_range() {
        call("seed", &[Value::Number(42.0)]).unwrap();
        let first: Vec<Value> = (0..4)
            .map(|_| call("float", &[]).unwrap())
            .collect();
        call("seed", &[Value::Number(42.0)]).unwrap();
        let second: Vec<Value> = (0..4)
            .map(|_| call("float", &[]).unwrap())
            .collect();
        let displays: Vec<String> = first.iter().map(Value::display).collect();
        let repeats: Vec<String> = second.iter().map(Value::display).collect();
        assert_eq!(displays, repeats);
        for value in first {
            let Value::Number(n) = value else {
                panic!("float should return a number");
            };
            assert!((0.0..1.0).contains(&n));
        }
    }

    #[test]
    fn int_choice_and_shuffle() {
        call("seed", &[Value::Number(7.0)]).unwrap();
        for _ in 0..50 {
            let value = call("int", &[Value::Number(1.0), Value::Number(6.0)]).unwrap();
            let Value::Number(n) = value else {
                panic!("int should return a number");
            };
            assert!((1.0..=6.0).contains(&n));
        }
        let items = Value::new_array(
            (0..5).map(|i| Value::Number(i as f64)).collect(),
        );
        assert!(matches!(call("choice", &[items.clone()]).unwrap(), Value::Number(_)));
        let shuffled = call("shuffle", &[items.clone()]).unwrap();
        let Value::Array(original) = items else {
            unreachable!()
        };
        let Value::Array(shuffled) = shuffled else {
            unreachable!()
        };
        assert_eq!(original.borrow().len(), shuffled.borrow().len());
        let mut sorted: Vec<f64> = shuffled
            .borrow()
            .iter()
            .map(|v| match v {
                Value::Number(n) => *n,
                _ => panic!("unexpected item"),
            })
            .collect();
        sorted.sort_by(f64::total_cmp);
        assert_eq!(sorted, vec![0.0, 1.0, 2.0, 3.0, 4.0]);
        assert!(call("choice", &[Value::new_array(vec![])]).is_err());
        assert!(call("int", &[Value::Number(5.0), Value::Number(1.0)]).is_err());
    }

    #[test]
    fn bool_range_and_sample() {
        call("seed", &[Value::Number(99.0)]).unwrap();
        assert!(matches!(call("bool", &[]).unwrap(), Value::Bool(_)));
        for _ in 0..20 {
            let value = call("range", &[Value::Number(2.0), Value::Number(5.0)]).unwrap();
            let Value::Number(n) = value else {
                panic!("range should return a number");
            };
            assert!((2.0..5.0).contains(&n));
        }
        let items = Value::new_array((0..6).map(|i| Value::Number(i as f64)).collect());
        let sampled = call("sample", &[items, Value::Number(3.0)]).unwrap();
        let Value::Array(values) = sampled else {
            panic!("sample should return an array");
        };
        assert_eq!(values.borrow().len(), 3);
        let mut numbers: Vec<f64> = values
            .borrow()
            .iter()
            .map(|value| match value {
                Value::Number(n) => *n,
                _ => panic!("unexpected item"),
            })
            .collect();
        numbers.sort_by(f64::total_cmp);
        assert!(numbers.windows(2).all(|pair| pair[0] != pair[1]));
        assert!(call("range", &[Value::Number(1.0), Value::Number(1.0)]).is_err());
        assert!(call("sample", &[Value::new_array(vec![]), Value::Number(1.0)]).is_err());
    }

    #[test]
    fn uuid_has_version_and_variant_bits() {
        call("seed", &[Value::Number(11.0)]).unwrap();
        let uuid = call("uuid", &[]).unwrap().display();
        let bytes = uuid.as_bytes();
        assert_eq!(uuid.len(), 36);
        assert_eq!(uuid.matches('-').count(), 4);
        assert_eq!(bytes[14], b'4');
        assert!(matches!(bytes[19], b'8' | b'9' | b'a' | b'b'));

        let second = call("uuid", &[]).unwrap().display();
        assert_ne!(uuid, second, "UUIDs must not repeat even after seeding");
    }
}
