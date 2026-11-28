use crate::modules::support::{
    array_items, as_usize, compare_values, expect_array, values_equal,
};
#[cfg(test)]
use crate::modules::support::call_native;
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
            flatten_into(items, &mut result, 0)?;
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
        "first".to_string(),
        native("collections.first", |args| {
            let items = array_items(
                args.get(0).unwrap_or(&Value::Null),
                "for collections.first",
            )?;
            items
                .into_iter()
                .next()
                .ok_or_else(|| "collections.first expects a non-empty array".to_string())
        }),
    );

    fields.insert(
        "last".to_string(),
        native("collections.last", |args| {
            let items = array_items(
                args.get(0).unwrap_or(&Value::Null),
                "for collections.last",
            )?;
            items
                .into_iter()
                .last()
                .ok_or_else(|| "collections.last expects a non-empty array".to_string())
        }),
    );

    fields.insert(
        "min".to_string(),
        native("collections.min", |args| {
            let items = array_items(
                args.get(0).unwrap_or(&Value::Null),
                "for collections.min",
            )?;
            items
                .into_iter()
                .min_by(compare_values)
                .ok_or_else(|| "collections.min expects a non-empty array".to_string())
        }),
    );

    fields.insert(
        "max".to_string(),
        native("collections.max", |args| {
            let items = array_items(
                args.get(0).unwrap_or(&Value::Null),
                "for collections.max",
            )?;
            items
                .into_iter()
                .max_by(compare_values)
                .ok_or_else(|| "collections.max expects a non-empty array".to_string())
        }),
    );

    fields.insert(
        "product".to_string(),
        native("collections.product", |args| {
            let items = array_items(
                args.get(0).unwrap_or(&Value::Null),
                "for collections.product",
            )?;
            let mut total = 1.0;
            for item in items {
                total *= crate::runtime::expect_number(&item, "in the array for collections.product")?;
            }
            Ok(Value::Number(total))
        }),
    );

    fields.insert(
        "has".to_string(),
        native("collections.has", |args| {
            let Value::Object(fields) = args.get(0).unwrap_or(&Value::Null) else {
                return Err("collections.has expects an object".to_string());
            };
            let key = crate::runtime::expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "as the key for collections.has",
            )?;
            Ok(Value::Bool(fields.borrow().contains_key(&key)))
        }),
    );

    fields.insert(
        "get".to_string(),
        native("collections.get", |args| {
            let Value::Object(fields) = args.get(0).unwrap_or(&Value::Null) else {
                return Err("collections.get expects an object".to_string());
            };
            let key = crate::runtime::expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "as the key for collections.get",
            )?;
            Ok(fields
                .borrow()
                .get(&key)
                .cloned()
                .unwrap_or_else(|| args.get(2).cloned().unwrap_or(Value::Null)))
        }),
    );

    fields.insert(
        "set".to_string(),
        native("collections.set", |args| {
            let Value::Object(fields) = args.get(0).unwrap_or(&Value::Null) else {
                return Err("collections.set expects an object".to_string());
            };
            let key = crate::runtime::expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "as the key for collections.set",
            )?;
            let value = args.get(2).cloned().unwrap_or(Value::Null);
            fields.borrow_mut().insert(key, value);
            Ok(args.get(0).unwrap_or(&Value::Null).clone())
        }),
    );

    fields.insert(
        "delete".to_string(),
        native("collections.delete", |args| {
            let Value::Object(fields) = args.get(0).unwrap_or(&Value::Null) else {
                return Err("collections.delete expects an object".to_string());
            };
            let key = crate::runtime::expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "as the key for collections.delete",
            )?;
            Ok(Value::Bool(fields.borrow_mut().remove(&key).is_some()))
        }),
    );

    fields.insert(
        "keys".to_string(),
        native("collections.keys", |args| {
            let Value::Object(fields) = args.get(0).unwrap_or(&Value::Null) else {
                return Err("collections.keys expects an object".to_string());
            };
            let keys = fields
                .borrow()
                .keys()
                .map(|key| Value::String(key.clone()))
                .collect();
            Ok(Value::new_array(keys))
        }),
    );

    fields.insert(
        "values".to_string(),
        native("collections.values", |args| {
            let Value::Object(fields) = args.get(0).unwrap_or(&Value::Null) else {
                return Err("collections.values expects an object".to_string());
            };
            Ok(Value::new_array(fields.borrow().values().cloned().collect()))
        }),
    );

    fields.insert(
        "merge".to_string(),
        native("collections.merge", |args| {
            let mut merged = std::collections::BTreeMap::new();
            for arg in args {
                let Value::Object(fields) = arg else {
                    return Err("collections.merge expects objects".to_string());
                };
                for (key, value) in fields.borrow().iter() {
                    merged.insert(key.clone(), copy_value(value));
                }
            }
            Ok(Value::new_object(merged))
        }),
    );

    fields.insert(
        "copy".to_string(),
        native("collections.copy", |args| {
            Ok(copy_value(args.get(0).unwrap_or(&Value::Null)))
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

/// Deep-copy arrays and objects so `merge` and `copy` do not alias the input.
fn copy_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::new_array(
            items
                .borrow()
                .iter()
                .map(copy_value)
                .collect(),
        ),
        Value::Object(fields) => Value::new_object(
            fields
                .borrow()
                .iter()
                .map(|(key, value)| (key.clone(), copy_value(value)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn flatten_into(items: Vec<Value>, out: &mut Vec<Value>, depth: usize) -> Result<(), String> {
    if depth > 100 {
        return Err("collections.flatten nesting is too deep".to_string());
    }
    for item in items {
        match item {
            Value::Array(nested) => flatten_into(nested.borrow().clone(), out, depth + 1)?,
            other => out.push(other),
        }
    }
    Ok(())
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

    #[test]
    fn endpoints_extremes_and_product() {
        let items = numbers(&[3.0, 1.0, 4.0]);
        assert!(matches!(call("first", &[items.clone()]).unwrap(), Value::Number(n) if n == 3.0));
        assert!(matches!(call("last", &[items.clone()]).unwrap(), Value::Number(n) if n == 4.0));
        assert!(matches!(call("min", &[items.clone()]).unwrap(), Value::Number(n) if n == 1.0));
        assert!(matches!(call("max", &[items.clone()]).unwrap(), Value::Number(n) if n == 4.0));
        assert!(matches!(call("product", &[items.clone()]).unwrap(), Value::Number(n) if n == 12.0));
        assert!(call("first", &[Value::new_array(vec![])]).is_err());
        assert!(call("min", &[Value::new_array(vec![])]).is_err());
    }

    #[test]
    fn object_helpers() {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("a".to_string(), Value::Number(1.0));
        fields.insert("b".to_string(), Value::Number(2.0));
        let object = Value::new_object(fields);
        assert!(matches!(
            call("has", &[object.clone(), Value::String("a".to_string())]).unwrap(),
            Value::Bool(true)
        ));
        assert!(matches!(
            call("get", &[object.clone(), Value::String("z".to_string())]).unwrap(),
            Value::Null
        ));
        assert!(matches!(
            call(
                "get",
                &[
                    object.clone(),
                    Value::String("z".to_string()),
                    Value::String("fallback".to_string())
                ]
            )
            .unwrap(),
            Value::String(s) if s == "fallback"
        ));
        call(
            "set",
            &[
                object.clone(),
                Value::String("c".to_string()),
                Value::Number(3.0),
            ],
        )
        .unwrap();
        assert_eq!(call("keys", &[object.clone()]).unwrap().display(), "[\"a\", \"b\", \"c\"]");
        assert_eq!(call("values", &[object.clone()]).unwrap().display(), "[1, 2, 3]");
        assert!(matches!(
            call("delete", &[object.clone(), Value::String("a".to_string())]).unwrap(),
            Value::Bool(true)
        ));
        assert!(matches!(
            call("delete", &[object.clone(), Value::String("a".to_string())]).unwrap(),
            Value::Bool(false)
        ));

        let mut more = std::collections::BTreeMap::new();
        more.insert("b".to_string(), Value::Number(9.0));
        more.insert("d".to_string(), Value::Number(4.0));
        let merged = call("merge", &[object, Value::new_object(more)]).unwrap();
        assert_eq!(
            call("get", &[merged.clone(), Value::String("b".to_string())])
                .unwrap()
                .display(),
            "9"
        );
        let copied = call("copy", &[merged.clone()]).unwrap();
        assert_eq!(copied.display(), merged.display());
        assert!(call("merge", &[Value::Number(1.0)]).is_err());
    }

    #[test]
    fn flatten_rejects_cyclic_arrays() {
        let cycle = Value::new_array(vec![]);
        let Value::Array(items) = &cycle else { unreachable!() };
        items.borrow_mut().push(cycle.clone());

        let result = call("flatten", &[cycle]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("nesting is too deep"));
    }
}
