//! Stable JSON rendering of the parsed AST.
//!
//! `json.encode` sorts object keys through `BTreeMap`, which is excellent for
//! configuration but wrong for a compiler interchange format: the schema's
//! field order is part of the contract. This module therefore writes JSON
//! directly, in the order documented in `docs/selfhost/AST_JSON.md`, while
//! reusing the `json` module's string escaping for every leaf string.

use crate::ast::{Assignable, BinOp, Expr, Stmt, UnOp};
use crate::error::Span;
use crate::modules::json;
use crate::value::{format_number, Value};

/// Render `statements` as a single-line JSON value.
///
/// Englishmen objects are `BTreeMap`-backed, so `json.encode` writes object
/// keys alphabetically. The compiled and self-hosted paths both use that
/// serializer, so the canonical AST JSON form is alphabetical key order, not
/// the order the schema lists fields in.
pub fn to_json_string(statements: &[Stmt]) -> Result<String, String> {
    let schema_ordered = to_schema_json_string(statements)?;
    json::encode(&json::decode(&schema_ordered)?)
}

/// Render `statements` as runtime `Value`s.
pub fn to_json(statements: &[Stmt]) -> Result<Value, String> {
    json::decode(&to_json_string(statements)?)
}

/// Internal schema-ordered rendering. Re-encoding through `json` canonicalizes
/// the field order to match every other Englishmen JSON producer.
fn to_schema_json_string(statements: &[Stmt]) -> Result<String, String> {
    let body = statements
        .iter()
        .map(stmt_json)
        .collect::<Result<Vec<_>, _>>()?;
    object(vec![
        ("version", number(1.0)?),
        ("statements", array(body)),
    ])
}

fn string(value: &str) -> Result<String, String> {
    json::encode(&Value::String(value.to_string()))
}

fn number(value: f64) -> Result<String, String> {
    if !value.is_finite() {
        return Err("Cannot encode a non-finite AST number as JSON".to_string());
    }
    Ok(format_number(value))
}

fn boolean(value: bool) -> String {
    if value { "true" } else { "false" }.to_string()
}

fn object(fields: Vec<(&str, String)>) -> Result<String, String> {
    let mut out = String::from("{");
    for (index, (key, value)) in fields.into_iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&string(key)?);
        out.push(':');
        out.push_str(&value);
    }
    out.push('}');
    Ok(out)
}

fn array(items: Vec<String>) -> String {
    format!("[{}]", items.join(","))
}

fn span_json(span: Span) -> Result<String, String> {
    object(vec![
        ("line", number(span.line as f64)?),
        ("col", number(span.col as f64)?),
    ])
}

fn bin_op(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::FloorDiv => "//",
        BinOp::Pow => "**",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Eq => "==",
        BinOp::NotEq => "!=",
        BinOp::Gt => ">",
        BinOp::Lt => "<",
        BinOp::GtEq => ">=",
        BinOp::LtEq => "<=",
        BinOp::And => "and",
        BinOp::Or => "or",
        BinOp::Coalesce => "??",
    }
}

fn un_op(op: UnOp) -> &'static str {
    match op {
        UnOp::Neg => "-",
        UnOp::Not => "not",
        UnOp::BitNot => "~",
    }
}

fn assignable_json(target: &Assignable) -> Result<String, String> {
    match target {
        Assignable::Variable(name) => {
            object(vec![("kind", string("Variable")?), ("name", string(name)?)])
        }
        Assignable::Property(base, name) => object(vec![
            ("kind", string("Property")?),
            ("base", expr_json(base)?),
            ("name", string(name)?),
        ]),
        Assignable::Index(base, index) => object(vec![
            ("kind", string("Index")?),
            ("base", expr_json(base)?),
            ("index", expr_json(index)?),
        ]),
    }
}

fn expr_json(expr: &Expr) -> Result<String, String> {
    match expr {
        Expr::Number(value) => object(vec![
            ("kind", string("Number")?),
            ("value", number(*value)?),
        ]),
        Expr::String(value) => object(vec![
            ("kind", string("String")?),
            ("value", string(value)?),
        ]),
        Expr::Bool(value) => object(vec![
            ("kind", string("Bool")?),
            ("value", boolean(*value)),
        ]),
        Expr::Null => object(vec![("kind", string("Null")?)]),
        Expr::Variable(name) => {
            object(vec![("kind", string("Variable")?), ("name", string(name)?)])
        }
        Expr::PropertyAccess(base, name) => object(vec![
            ("kind", string("PropertyAccess")?),
            ("base", expr_json(base)?),
            ("name", string(name)?),
        ]),
        Expr::Index(base, index) => object(vec![
            ("kind", string("Index")?),
            ("base", expr_json(base)?),
            ("index", expr_json(index)?),
        ]),
        Expr::Array(items) => {
            let items = items
                .iter()
                .map(expr_json)
                .collect::<Result<Vec<_>, _>>()?;
            object(vec![("kind", string("Array")?), ("items", array(items))])
        }
        Expr::Object(fields) => {
            let fields = fields
                .iter()
                .map(|(name, value)| {
                    Ok(object(vec![
                        ("name", string(name)?),
                        ("value", expr_json(value)?),
                    ])?)
                })
                .collect::<Result<Vec<_>, String>>()?;
            object(vec![
                ("kind", string("Object")?),
                ("fields", array(fields)),
            ])
        }
        Expr::Call(callee, args) => {
            let args = args.iter().map(expr_json).collect::<Result<Vec<_>, _>>()?;
            object(vec![
                ("kind", string("Call")?),
                ("callee", expr_json(callee)?),
                ("args", array(args)),
            ])
        }
        Expr::MethodCall(receiver, name, args) => {
            let args = args.iter().map(expr_json).collect::<Result<Vec<_>, _>>()?;
            object(vec![
                ("kind", string("MethodCall")?),
                ("receiver", expr_json(receiver)?),
                ("name", string(name)?),
                ("args", array(args)),
            ])
        }
        Expr::Binary(left, op, right, span) => object(vec![
            ("kind", string("Binary")?),
            ("op", string(bin_op(*op))?),
            ("left", expr_json(left)?),
            ("right", expr_json(right)?),
            ("span", span_json(*span)?),
        ]),
        Expr::Unary(op, operand, span) => object(vec![
            ("kind", string("Unary")?),
            ("op", string(un_op(*op))?),
            ("operand", expr_json(operand)?),
            ("span", span_json(*span)?),
        ]),
        Expr::Ternary(condition, when_true, when_false, span) => object(vec![
            ("kind", string("Ternary")?),
            ("condition", expr_json(condition)?),
            ("whenTrue", expr_json(when_true)?),
            ("whenFalse", expr_json(when_false)?),
            ("span", span_json(*span)?),
        ]),
        Expr::Postfix(target, increment, span) => object(vec![
            ("kind", string("Postfix")?),
            ("op", string(if *increment { "++" } else { "--" })?),
            ("target", expr_json(target)?),
            ("span", span_json(*span)?),
        ]),
    }
}

fn stmt_json(stmt: &Stmt) -> Result<String, String> {
    match stmt {
        Stmt::Assign(target, value, span) => object(vec![
            ("kind", string("Assign")?),
            ("span", span_json(*span)?),
            ("target", assignable_json(target)?),
            ("value", expr_json(value)?),
        ]),
        Stmt::CompoundAssign(target, op, value, span) => object(vec![
            ("kind", string("CompoundAssign")?),
            ("span", span_json(*span)?),
            ("target", assignable_json(target)?),
            ("op", string(bin_op(*op))?),
            ("value", expr_json(value)?),
        ]),
        Stmt::ExprStmt(expr, span) => object(vec![
            ("kind", string("ExprStmt")?),
            ("span", span_json(*span)?),
            ("expr", expr_json(expr)?),
        ]),
        Stmt::If(branches, otherwise, span) => {
            let branches = branches
                .iter()
                .map(|(condition, body)| {
                    let body = body
                        .iter()
                        .map(stmt_json)
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(object(vec![
                        ("condition", expr_json(condition)?),
                        ("body", array(body)),
                    ])?)
                })
                .collect::<Result<Vec<_>, String>>()?;
            let otherwise = match otherwise {
                Some(body) => {
                    let body = body
                        .iter()
                        .map(stmt_json)
                        .collect::<Result<Vec<_>, _>>()?;
                    array(body)
                }
                None => "null".to_string(),
            };
            object(vec![
                ("kind", string("If")?),
                ("span", span_json(*span)?),
                ("branches", array(branches)),
                ("else", otherwise),
            ])
        }
        Stmt::FunctionDef(name, params, body, span) => {
            let params = params
                .iter()
                .map(|param| string(param))
                .collect::<Result<Vec<_>, _>>()?;
            let body = body
                .iter()
                .map(stmt_json)
                .collect::<Result<Vec<_>, _>>()?;
            object(vec![
                ("kind", string("FunctionDef")?),
                ("span", span_json(*span)?),
                ("name", string(name)?),
                ("params", array(params)),
                ("body", array(body)),
            ])
        }
        Stmt::Return(value, span) => object(vec![
            ("kind", string("Return")?),
            ("span", span_json(*span)?),
            (
                "value",
                match value {
                    Some(expr) => expr_json(expr)?,
                    None => "null".to_string(),
                },
            ),
        ]),
        Stmt::Repeat(count, body, span) => {
            let body = body
                .iter()
                .map(stmt_json)
                .collect::<Result<Vec<_>, _>>()?;
            object(vec![
                ("kind", string("Repeat")?),
                ("span", span_json(*span)?),
                ("count", expr_json(count)?),
                ("body", array(body)),
            ])
        }
        Stmt::While(condition, body, span) => {
            let body = body
                .iter()
                .map(stmt_json)
                .collect::<Result<Vec<_>, _>>()?;
            object(vec![
                ("kind", string("While")?),
                ("span", span_json(*span)?),
                ("condition", expr_json(condition)?),
                ("body", array(body)),
            ])
        }
        Stmt::ForIn(name, iterable, body, span) => {
            let body = body
                .iter()
                .map(stmt_json)
                .collect::<Result<Vec<_>, _>>()?;
            object(vec![
                ("kind", string("ForIn")?),
                ("span", span_json(*span)?),
                ("name", string(name)?),
                ("iterable", expr_json(iterable)?),
                ("body", array(body)),
            ])
        }
        Stmt::Break(span) => object(vec![
            ("kind", string("Break")?),
            ("span", span_json(*span)?),
        ]),
        Stmt::Continue(span) => object(vec![
            ("kind", string("Continue")?),
            ("span", span_json(*span)?),
        ]),
        Stmt::Import(name, span) => object(vec![
            ("kind", string("Import")?),
            ("span", span_json(*span)?),
            ("name", string(name)?),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn program(source: &str) -> Vec<Stmt> {
        parse(tokenize(source).expect("source should tokenize")).expect("source should parse")
    }

    #[test]
    fn fields_are_emitted_in_canonical_json_order() {
        let text = to_json_string(&program("say(1 + 2)\n")).expect("AST should serialize");
        assert!(text.starts_with("{\"statements\":["), "root keys must be sorted");
        assert!(text.ends_with(",\"version\":1}"), "root keys must be sorted");
        assert!(text.contains("\"kind\":\"Binary\",\"left\":{\"kind\":\"Number\",\"value\":1},\"op\":\"+\""));
        // Canonical form is exactly what json.encode produces for the same tree.
        assert_eq!(text, json::encode(&json::decode(&text).unwrap()).unwrap());
    }

    #[test]
    fn every_statement_shape_serializes() {
        let source = r#"import json
function f(a)
    if a > 1
        return a
    else
        return 0
    end
    repeat 2
        while a < 5
            a += 1
            if a == 3
                continue
            end
        end
    end
    for x in [1, 2]
        say(x)
    end
    obj = {a: 1}
    obj.a++
    return null
end
f(2)
"#;
        let json = to_json_string(&program(source)).expect("AST should serialize");
        for kind in [
            "Import",
            "FunctionDef",
            "If",
            "Return",
            "Repeat",
            "While",
            "CompoundAssign",
            "Continue",
            "ForIn",
            "Assign",
            "Postfix",
            "ExprStmt",
        ] {
            assert!(json.contains(&format!("\"kind\":\"{kind}\"")), "missing {kind}");
        }
        assert!(json::decode(&json).is_ok());
    }

    #[test]
    fn output_is_stable_across_calls() {
        let program = program("x = [1, true, null, \"a\\nb\"]\n");
        assert_eq!(
            to_json_string(&program).unwrap(),
            to_json_string(&program).unwrap()
        );
    }

    #[test]
    fn to_json_returns_the_same_tree_as_runtime_values() {
        let program = program("say(1)\n");
        let text = to_json_string(&program).unwrap();
        let value = to_json(&program).unwrap();
        // json::encode sorts object keys, so compare canonical sorted
        // serializations rather than the schema-ordered text.
        assert_eq!(
            json::encode(&value).unwrap(),
            json::encode(&json::decode(&text).unwrap()).unwrap()
        );
    }
}
