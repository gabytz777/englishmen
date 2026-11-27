use crate::modules::support::{
    array_items, as_usize, call_native, compare_values, expect_array, values_equal,
};
use crate::runtime::native;
use crate::value::Value;
use std::collections::BTreeMap;

pub fn module() -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "sorted".to_string(),
        native("collections.sorted", |args| {
            let mut items = array_items(
                args.get(0).unwrap_or(&Value::Null),
                "for collections.sorted",
            )?;
            items.sort_by(compare_values);
            Ok(Value::new_array(items))
        }),
    );

    fields.insert(
        "reversed".to_string(),
        native("collections.reversed", |args| {
            let value = args.get(0).unwrap_or(&Value::Null);
            match value {
                Value::String(text) => Ok(Value::String(text.chars().rev().collect())),
                Value::Array(_) => {
                    let mut items = array_items(value, "for collections.reversed")?;
                    items.reverse();
                    Ok(Value::new_array(items))
                }
                other => Err(format!(
                    "collections.reversed expects an array or string, found {}",
                    other.type_name()
                )),
            }
        }),
    );

    fields.insert(
        "unique".to_string(),
        native("collections.unique", |args| {
            let items = array_items(
                args.get(0).unwrap_or(&Value::Null),
                "for collections.unique",
            )?;
            let mut result: Vec<Value> = Vec::new();
            for item in items {
                if !result.iter().any(|existing| values_equal(existing, &item)) {
                    result.push(item);
                }
            }
            Ok(Value::new_array(result))
        }),
    );

    fields.insert(
        "flatten".to_string(),
        native("collections.flatten", |args| {
            let items = array_items(
                args.get(0).unwrap_or(&Value::Null),
                "for collections.flatten",
            )?;
            let mut result = Vec::new();
            flatten_into(items, &mut result);
            Ok(Value::new_array(result))
        }),
    );

    fields.insert(
        "zip".to_string(),
        native("collections.zip", |args| {
            let arrays: Vec<Vec<Value>> = args
                .iter()
                .map(|arg| array_items(arg, "for collections.zip"))
                .collect::<Result<_, _>>()?;
            let len = arrays.iter().map(Vec::len).min().unwrap_or(0);
            let rows: Vec<Value> = (0..len)
                .map(|i| {
                    Value::new_array(
                        arrays
                            .iter()
                            .map(|array| array[i].clone())
                            .collect(),
                    )
                })
                .collect();
            Ok(Value::new_array(rows))
        }),
    );

    fields.insert(
        "enumerate".to_string(),
        native("collections.enumerate", |args| {
            let items = array_items(
                args.get(0).unwrap_or(&Value::Null),
                "for collections.enumerate",
            )?;
            let pairs = items
                .into_iter()
                .enumerate()
                .map(|(i, item)| {
                    Value::new_array(vec![Value::Number(i as f64), item])
                })
                .collect();
            Ok(Value::new_array(pairs))
        }),
    );

    fields.insert(
        "chunk".to_string(),
        native("collections.chunk", |args| {
            let items = expect_array(
                args.get(0).unwrap_or(&Value::Null),
                "for collections.chunk",
            )?;
            let size = as_usize(
                args.get(1).unwrap_or(&Value::Null),
                "as the chunk size for collections.chunk",
            )?;
            if size == 0 {
                return Err("collections.chunk size must be greater than zero".to_string());
            }
            let items = items.borrow();
            let chunks = items
                .chunks(size)
                .map(|chunk| Value::new_array(chunk.to_vec()))
                .collect();
            Ok(Value::new_array(chunks))
        }),
    );

    fields.insert(
        "range".to_string(),
        native("collections.range", |args| {
            if args.is_empty() || args.len() > 3 {
                return Err("collections.range expects one, two or three arguments".to_string());
            }
            let number = |index: usize, label: &str| -> Result<f64, String> {
                let value = crate::runtime::expect_number(
                    args.get(index).unwrap_or(&Value::Null),
                    label,
                )?;
                if !value.is_finite() {
                    return Err("collections.range expects finite numbers".to_string());
                }
                Ok(value)
            };
            let (start, end, step) = match args.len() {
                1 => (0.0, number(0, "as the end for collections.range")?, 1.0),
                2 => (
                    number(0, "as the start for collections.range")?,
                    number(1, "as the end for collections.range")?,
                    1.0,
                ),
                _ => (
                    number(0, "as the start for collections.range")?,
                    number(1, "as the end for collections.range")?,
                    number(2, "as the step for collections.range")?,
                ),
            };
            if step == 0.0 {
                return Err("collections.range step cannot be zero".to_string());
            }
            let mut values = Vec::new();
            let mut current = start;
            while (step > 0.0 && current < end) || (step < 0.0 && current > end) {
                values.push(Value::Number(current));
                if values.len() >= 100_000 {
                    return Err("collections.range would produce too many values".to_string());
                }
                current += step;
            }
            Ok(Value::new_array(values))
        }),
    );

    fields.insert(
        "sum".to_string(),
        native("collections.sum", |args| {
            let items = array_items(
                args.get(0).unwrap_or(&Value::Null),
                "for collections.sum",
            )?;
            let mut total = 0.0;
            for item in items {
                total += crate::runtime::expect_number(&item, "in the array for collections.sum")?;
            }
            Ok(Value::Number(total))
        }),
    );

    fields.insert(
        "average".to_string(),
        native("collections.average", |args| {
            let items = array_items(
                args.get(0).unwrap_or(&Value::Null),
                "for collections.average",
            )?;
            if items.is_empty() {
                return Err("collections.average cannot average an empty array".to_string());
            }
            let mut total = 0.0;
            for item in &items {
                total += crate::runtime::expect_number(item, "in the array for collections.average")?;
            }
            Ok(Value::Number(total / items.len() as f64))
        }),
    );

    Value::new_object(fields)
}

fn flatten_into(items: Vec<Value>, out: &mut Vec<Value>) {
    for item in items {
        match item {
            Value::Array(nested) => flatten_into(nested.borrow().clone(), out),
            other => out.push(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: &[Value]) -> Result<Value, String> {
        call_native(&module(), name, args)
    }

    fn numbers(values: &[f64]) -> Value {
        Value::new_array(values.iter().map(|n| Value::Number(*n)).collect())
    }

    #[test]
    fn sorted_reversed_and_unique() {
        assert_eq!(
            call("sorted", &[numbers(&[3.0, 1.0, 2.0])])
                .unwrap()
                .display(),
            "[1, 2, 3]"
        );
        assert_eq!(
            call("reversed", &[Value::String("abc".to_string())])
                .unwrap()
                .display(),
            "cba"
        );
        assert_eq!(
            call("reversed", &[numbers(&[1.0, 2.0])]).unwrap().display(),
            "[2, 1]"
        );
        assert_eq!(
            call("unique", &[numbers(&[1.0, 2.0, 1.0])])
                .unwrap()
                .display(),
            "[1, 2]"
        );
        let nested = Value::new_array(vec![Value::Number(1.0)]);
        let duplicates = Value::new_array(vec![nested.clone(), nested]);
        assert_eq!(call("unique", &[duplicates]).unwrap().display(), "[[1]]");
    }

    #[test]
    fn flatten_zip_enumerate_and_chunk() {
        let nested = Value::new_array(vec![
            Value::Number(1.0),
            Value::new_array(vec![Value::Number(2.0), Value::Number(3.0)]),
        ]);
        assert_eq!(
            call("flatten", &[nested]).unwrap().display(),
            "[1, 2, 3]"
        );
        assert_eq!(
            call(
                "zip",
                &[numbers(&[1.0, 2.0]), numbers(&[3.0, 4.0, 5.0])]
            )
            .unwrap()
            .display(),
            "[[1, 3], [2, 4]]"
        );
        assert_eq!(
            call(
                "enumerate",
                &[Value::new_array(vec![Value::String("a".to_string())])]
            )
            .unwrap()
            .display(),
            "[[0, \"a\"]]"
        );
        assert_eq!(
            call("chunk", &[numbers(&[1.0, 2.0, 3.0]), Value::Number(2.0)])
                .unwrap()
                .display(),
            "[[1, 2], [3]]"
        );
        assert!(call("chunk", &[numbers(&[1.0]), Value::Number(0.0)]).is_err());
    }

    #[test]
    fn range_sum_and_average() {
        assert_eq!(call("range", &[Value::Number(4.0)]).unwrap().display(), "[0, 1, 2, 3]");
        assert_eq!(
            call(
                "range",
                &[
                    Value::Number(5.0),
                    Value::Number(0.0),
                    Value::Number(-2.0)
                ]
            )
            .unwrap()
            .display(),
            "[5, 3, 1]"
        );
        assert_eq!(
            call("sum", &[numbers(&[1.0, 2.0, 3.0])])
                .unwrap()
                .display(),
            "6"
        );
        assert_eq!(
            call("average", &[numbers(&[1.0, 2.0, 6.0])])
                .unwrap()
                .display(),
            "3"
        );
        assert!(call("average", &[Value::new_array(vec![])]).is_err());
        assert!(call("range", &[Value::Number(1.0), Value::Number(2.0), Value::Number(0.0)]).is_err());
        assert!(call("sum", &[Value::new_array(vec![Value::String("x".to_string())])]).is_err());
    }
}
