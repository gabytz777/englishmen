use crate::runtime::{expect_number, expect_string, native};
use crate::value::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// Escape HTML special characters so text is safe to embed in a page.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escape a value for use as HTML text. Null becomes an empty string.
fn html_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        other => escape(&other.display()),
    }
}

/// Content passed to `html.page`, `html.tag`, and `html.element` may be a
/// string or an array of strings. Arrays are joined with newlines so generated
/// pages stay readable.
fn raw_content(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Array(items) => items
            .borrow()
            .iter()
            .map(raw_content)
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.display(),
    }
}

fn validate_name(name: &str, what: &str) -> Result<(), String> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':'))
    {
        return Err(format!("Invalid {what} '{name}'"));
    }
    Ok(())
}

fn attributes(value: &Value) -> Result<String, String> {
    let attrs = match value {
        Value::Null => return Ok(String::new()),
        Value::Object(attrs) => attrs,
        other => {
            return Err(format!(
                "Type mismatch: expected an object of HTML attributes, found {}",
                other.type_name()
            ))
        }
    };

    let mut out = String::new();
    for (name, value) in attrs.borrow().iter() {
        validate_name(name, "HTML attribute name")?;
        match value {
            Value::Null | Value::Bool(false) => {}
            Value::Bool(true) => {
                out.push(' ');
                out.push_str(name);
            }
            other => {
                out.push(' ');
                out.push_str(name);
                out.push_str("=\"");
                out.push_str(&escape(&other.display()));
                out.push('"');
            }
        }
    }
    Ok(out)
}

fn is_void(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn render_element(name: &str, attrs: &str, content: &str) -> String {
    if is_void(name) {
        format!("<{name}{attrs}>")
    } else {
        format!("<{name}{attrs}>{content}</{name}>")
    }
}

fn render_list(name: &str, value: &Value) -> Result<String, String> {
    let Value::Array(items) = value else {
        return Err(format!(
            "Type mismatch: expected an array of list items, found {}",
            value.type_name()
        ));
    };

    let mut out = format!("<{name}>\n");
    for item in items.borrow().iter() {
        out.push_str("  <li>");
        out.push_str(&html_text(item));
        out.push_str("</li>\n");
    }
    out.push_str(&format!("</{name}>"));
    Ok(out)
}

pub fn module() -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "escape".to_string(),
        native("html.escape", |args| {
            let text = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "as the text to escape",
            )?;
            Ok(Value::String(escape(&text)))
        }),
    );

    fields.insert(
        "page".to_string(),
        native("html.page", |args| {
            let title = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "as the page title",
            )?;
            let body = raw_content(args.get(1).unwrap_or(&Value::Null));
            let body = body.trim_end_matches('\n');
            Ok(Value::String(format!(
                concat!(
                    "<!DOCTYPE html>\n",
                    "<html lang=\"en\">\n",
                    "<head>\n",
                    "  <meta charset=\"utf-8\">\n",
                    "  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n",
                    "  <title>{title}</title>\n",
                    "</head>\n",
                    "<body>\n",
                    "{body}\n",
                    "</body>\n",
                    "</html>\n"
                ),
                title = escape(&title),
                body = body
            )))
        }),
    );

    fields.insert(
        "tag".to_string(),
        native("html.tag", |args| {
            let name = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "as the HTML tag name",
            )?;
            validate_name(&name, "HTML tag name")?;
            let content = raw_content(args.get(1).unwrap_or(&Value::Null));
            Ok(Value::String(render_element(&name, "", &content)))
        }),
    );

    fields.insert(
        "element".to_string(),
        native("html.element", |args| {
            let name = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "as the HTML tag name",
            )?;
            validate_name(&name, "HTML tag name")?;
            let attrs = attributes(args.get(1).unwrap_or(&Value::Null))?;
            let content = raw_content(args.get(2).unwrap_or(&Value::Null));
            Ok(Value::String(render_element(&name, &attrs, &content)))
        }),
    );

    fields.insert(
        "heading".to_string(),
        native("html.heading", |args| {
            let level = expect_number(
                args.get(0).unwrap_or(&Value::Null),
                "as the heading level",
            )?;
            if level.fract() != 0.0 || !(1.0..=6.0).contains(&level) {
                return Err("Heading level must be a whole number from 1 to 6".to_string());
            }
            let text = html_text(args.get(1).unwrap_or(&Value::Null));
            Ok(Value::String(render_element(
                &format!("h{}", level as u8),
                "",
                &text,
            )))
        }),
    );

    for level in 1..=6 {
        let tag = format!("h{level}");
        let field = format!("h{level}");
        let native_name = format!("html.{field}");
        fields.insert(
            field,
            native(&native_name, move |args| {
                let text = html_text(args.get(0).unwrap_or(&Value::Null));
                Ok(Value::String(render_element(&tag, "", &text)))
            }),
        );
    }

    for tag in ["p", "div", "section", "span"] {
        fields.insert(
            tag.to_string(),
            native(&format!("html.{tag}"), move |args| {
                let text = html_text(args.get(0).unwrap_or(&Value::Null));
                Ok(Value::String(render_element(tag, "", &text)))
            }),
        );
    }

    fields.insert(
        "link".to_string(),
        native("html.link", |args| {
            let text = html_text(args.get(0).unwrap_or(&Value::Null));
            let url = expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "as the link URL",
            )?;
            let attrs = format!(" href=\"{}\"", escape(&url));
            Ok(Value::String(render_element("a", &attrs, &text)))
        }),
    );

    fields.insert(
        "image".to_string(),
        native("html.image", |args| {
            let src = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "as the image source",
            )?;
            let alt = html_text(args.get(1).unwrap_or(&Value::Null));
            let attrs = format!(" src=\"{}\" alt=\"{}\"", escape(&src), alt);
            Ok(Value::String(render_element("img", &attrs, "")))
        }),
    );

    fields.insert(
        "list".to_string(),
        native("html.list", |args| {
            let list = render_list("ul", args.get(0).unwrap_or(&Value::Null))?;
            Ok(Value::String(list))
        }),
    );

    fields.insert(
        "ol".to_string(),
        native("html.ol", |args| {
            let list = render_list("ol", args.get(0).unwrap_or(&Value::Null))?;
            Ok(Value::String(list))
        }),
    );

    fields.insert(
        "save".to_string(),
        native("html.save", |args| {
            let path = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "as the HTML output path",
            )?;
            let content = raw_content(args.get(1).unwrap_or(&Value::Null));

            if let Some(parent) = Path::new(&path)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Could not create '{}': {e}", parent.display()))?;
            }
            std::fs::write(&path, content)
                .map_err(|e| format!("Could not save HTML to '{path}': {e}"))?;
            Ok(Value::Null)
        }),
    );

    Value::new_object(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: &[Value]) -> Result<Value, String> {
        let module = module();
        let Value::Object(obj) = module else {
            unreachable!()
        };
        let func = obj.borrow().get(name).cloned().unwrap();
        let Value::Native(native) = func else {
            unreachable!()
        };
        (native.func)(args)
    }

    fn text(value: Value) -> String {
        match value {
            Value::String(text) => text,
            other => panic!("expected a string, got {other:?}"),
        }
    }

    #[test]
    fn escape_handles_html_special_characters() {
        assert_eq!(
            escape("<a href=\"x\">&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;&lt;/a&gt;"
        );
    }

    #[test]
    fn page_escapes_the_title_and_joins_body_parts() {
        let page = text(
            call(
                "page",
                &[
                    Value::String("Tom & Jerry".to_string()),
                    Value::new_array(vec![
                        Value::String("<h1>Hi</h1>".to_string()),
                        Value::String("<p>Welcome</p>".to_string()),
                    ]),
                ],
            )
            .unwrap(),
        );
        assert!(page.starts_with("<!DOCTYPE html>"));
        assert!(page.contains("<title>Tom &amp; Jerry</title>"));
        assert!(page.contains("<h1>Hi</h1>\n<p>Welcome</p>"));
        assert!(page.ends_with("</html>\n"));
    }

    #[test]
    fn element_renders_attributes_and_skips_false_flags() {
        let mut attrs = BTreeMap::new();
        attrs.insert("class".to_string(), Value::String("hero".to_string()));
        attrs.insert("hidden".to_string(), Value::Bool(true));
        attrs.insert("disabled".to_string(), Value::Bool(false));

        let html = text(
            call(
                "element",
                &[
                    Value::String("div".to_string()),
                    Value::new_object(attrs),
                    Value::String("<strong>Hi</strong>".to_string()),
                ],
            )
            .unwrap(),
        );
        assert_eq!(
            html,
            "<div class=\"hero\" hidden><strong>Hi</strong></div>"
        );
    }

    #[test]
    fn link_and_image_escape_their_arguments() {
        let link = text(
            call(
                "link",
                &[
                    Value::String("Tom & Jerry".to_string()),
                    Value::String("/a?x=1&y=2".to_string()),
                ],
            )
            .unwrap(),
        );
        assert_eq!(link, "<a href=\"/a?x=1&amp;y=2\">Tom &amp; Jerry</a>");

        let image = text(
            call(
                "image",
                &[
                    Value::String("cat.png".to_string()),
                    Value::String("\"Tom\"".to_string()),
                ],
            )
            .unwrap(),
        );
        assert_eq!(image, "<img src=\"cat.png\" alt=\"&quot;Tom&quot;\">");
    }

    #[test]
    fn list_escapes_items() {
        let list = text(
            call(
                "list",
                &[Value::new_array(vec![
                    Value::String("Home".to_string()),
                    Value::String("<About>".to_string()),
                ])],
            )
            .unwrap(),
        );
        assert_eq!(
            list,
            "<ul>\n  <li>Home</li>\n  <li>&lt;About&gt;</li>\n</ul>"
        );
    }

    #[test]
    fn heading_checks_the_level() {
        let heading = text(
            call(
                "heading",
                &[Value::Number(2.0), Value::String("Hi".to_string())],
            )
            .unwrap(),
        );
        assert_eq!(heading, "<h2>Hi</h2>");
        assert_eq!(
            text(call("h3", &[Value::String("Alias".to_string())]).unwrap()),
            "<h3>Alias</h3>"
        );
        assert!(call(
            "heading",
            &[Value::Number(7.0), Value::String("Hi".to_string())]
        )
        .is_err());
        assert!(call(
            "heading",
            &[Value::Number(1.5), Value::String("Hi".to_string())]
        )
        .is_err());
    }

    #[test]
    fn tag_names_are_validated() {
        assert!(call(
            "tag",
            &[
                Value::String("div onclick=alert(1)".to_string()),
                Value::String("".to_string())
            ]
        )
        .is_err());
    }

    #[test]
    fn save_creates_parent_directories() {
        let dir = std::env::temp_dir().join(format!("em_html_test_{}", std::process::id()));
        let path = dir.join("site").join("index.html");
        let path_str = path.display().to_string();

        call(
            "save",
            &[
                Value::String(path_str.clone()),
                Value::String("<h1>Hello</h1>".to_string()),
            ],
        )
        .unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "<h1>Hello</h1>");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
