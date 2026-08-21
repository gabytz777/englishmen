use crate::runtime::{expect_string, native};
use crate::value::Value;
use std::collections::BTreeMap;

pub fn module() -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "say".to_string(),
        native("cowsay.say", |args| {
            let text = expect_string(args.get(0).unwrap_or(&Value::Null), "for cowsay.say")?;
            println!("{}", render(&text));
            Ok(Value::Null)
        }),
    );

    Value::new_object(fields)
}

/// Build cowsay-style speech bubble + cow art entirely in Rust (no external binary).
pub fn render(message: &str) -> String {
    const WIDTH: usize = 40;
    let words = message.split_whitespace();
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in words {
        if current.is_empty() {
            current.push_str(word);
        } else if current.len() + 1 + word.len() <= WIDTH {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }

    let max_len = lines.iter().map(|l| l.len()).max().unwrap_or(0);
    let border_len = max_len + 2;

    let mut out = String::new();
    out.push_str(&format!(" {}\n", "_".repeat(border_len)));
    let count = lines.len();
    for (i, line) in lines.iter().enumerate() {
        let padded = format!("{:<width$}", line, width = max_len);
        let (l, r) = if count == 1 {
            ("<", ">")
        } else if i == 0 {
            ("/", "\\")
        } else if i == count - 1 {
            ("\\", "/")
        } else {
            ("|", "|")
        };
        out.push_str(&format!("{} {} {}\n", l, padded, r));
    }
    out.push_str(&format!(" {}\n", "-".repeat(border_len)));
    out.push_str(
        r#"        \   ^__^
         \  (oo)\_______
            (__)\       )\/\
                ||----w |
                ||     ||
"#,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_contains_message_and_cow() {
        let out = render("Hello");
        assert!(out.contains("Hello"));
        assert!(out.contains("^__^"));
    }

    #[test]
    fn render_wraps_long_messages() {
        let out = render("this is a fairly long message that should wrap across more than one line of the bubble");
        assert!(out.lines().count() > 5);
    }
}
